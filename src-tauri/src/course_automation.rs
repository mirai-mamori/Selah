use crate::agent_provider::AgentProvider;
use crate::ai::ChatMessage;
use crate::db::{epoch_secs, Database};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

mod context;

const CONFIG_PREFIX: &str = "course_automation:config:";
const STATUS_PREFIX: &str = "course_automation:status:";
const DEFAULT_INTERVAL_MINUTES: u32 = 30;
const CHECK_INTERVAL_SECS: u64 = 5 * 60;
const STARTUP_DELAY_SECS: u64 = 75;
const MAX_FILE_TEXT_CHARS: usize = 16_000;
const FULL_SUMMARY_NEW_ITEM_THRESHOLD: usize = 4;
const PRINT_CONFIDENCE_THRESHOLD: f32 = 0.8;
const PLUS_AI_ATTEMPTS: usize = 2;
const PLUS_AI_TIMEOUT_SECS: u64 = 180;
/// Whole-run watchdog. A hung download / print / render must never hold the
/// global run lock forever; incremental status saves let a timed-out run resume
/// on the next cycle, so this can be generous without losing work.
const RUN_TIMEOUT_SECS: u64 = 600;
const PLUS_DOCUMENT_MAX_TOKENS: u32 = 4096;
/// Internal sentinel placed on `AnalysisDocument.load_error` when a PDF yields
/// neither a text layer nor extractable images: it is skipped as a terminal,
/// non-retryable outcome rather than counted as a failure.
const DOC_SKIP_MARKER: &str = "__doc_skip__";

/// One unit of SenseA work. Every operation that issues SenseA AI requests —
/// the scheduled/manual full cycle, "re-analyze all", and single-document
/// re-analysis — is expressed as a Job and processed through the one queue, so
/// there is a single serialised mechanism rather than per-operation locks.
enum JobKind {
    Cycle { force_all: bool },
    ReanalyzeDoc { document_id: String },
    RebuildMemory,
    ConfirmPrint { category: String },
}

struct Job {
    luna_id: String,
    course_name: String,
    trigger: String,
    kind: JobKind,
    /// Present for command-triggered jobs that await the result; absent for the
    /// scheduler's fire-and-forget cycles.
    respond: Option<tokio::sync::oneshot::Sender<Result<CourseAutomationView, String>>>,
}

/// How many queued jobs may run at once (queue + bounded parallelism).
const JOB_PARALLELISM: usize = 3;

pub struct CourseAutomationState {
    /// Sender into the unified job queue. All SenseA AI work goes through here.
    job_tx: tokio::sync::mpsc::UnboundedSender<Job>,
    /// Receiver, taken once by the dispatcher in `start_course_automation_loop`.
    job_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Job>>>,
    /// Coordinates the parallel jobs safely: a full cycle takes the write side
    /// (exclusive — it rewrites the whole status), single-document jobs take the
    /// read side (they run their AI in parallel with each other).
    cycle_lock: tokio::sync::RwLock<()>,
    /// Serialises the brief status read-modify-write of concurrent document
    /// jobs so their upserts merge instead of clobbering.
    status_write: tokio::sync::Mutex<()>,
}

impl CourseAutomationState {
    pub fn new() -> Self {
        let (job_tx, job_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            job_tx,
            job_rx: Mutex::new(Some(job_rx)),
            cycle_lock: tokio::sync::RwLock::new(()),
            status_write: tokio::sync::Mutex::new(()),
        }
    }
}

impl Default for CourseAutomationState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseAutomationConfig {
    pub luna_id: String,
    pub course_name: String,
    pub enabled: bool,
    pub interval_minutes: u32,
    pub monitor_materials: bool,
    pub monitor_announcements: bool,
    pub monitor_assignments: bool,
    pub analyze_all: bool,
    /// Master switch for printing. When on, printable candidates are gated by
    /// per-category approval: the first file of a type waits for the user to
    /// confirm, after which that category prints automatically.
    pub auto_print: bool,
    /// Print categories the user has approved. A candidate whose category is
    /// listed prints without asking; others wait as `needs_confirmation`.
    #[serde(default)]
    pub approved_print_categories: Vec<String>,
    pub notify_seat_changes: bool,
}

impl CourseAutomationConfig {
    fn new(luna_id: String, course_name: String) -> Self {
        Self {
            luna_id,
            course_name,
            enabled: false,
            interval_minutes: DEFAULT_INTERVAL_MINUTES,
            monitor_materials: true,
            monitor_announcements: true,
            monitor_assignments: true,
            analyze_all: true,
            auto_print: true,
            approved_print_categories: Vec::new(),
            notify_seat_changes: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SeatConclusion {
    #[serde(default)]
    pub assignment: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrintCandidate {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub confidence: f32,
    /// A short type name grouping similar printables (例: ワークシート / 小テスト /
    /// 出席カード). Per-category user approval decides whether this prints
    /// automatically.
    #[serde(default)]
    pub category: String,
}

/// A continuing memory item that is explicitly tied to this course and may
/// carry an expiry date. `expires_at` is an ISO date (`YYYY-MM-DD`) after which
/// the item is no longer relevant; an empty value means a standing rule with no
/// fixed expiry. Items whose expiry has passed are pruned deterministically in
/// code (see `prune_and_normalize_standing`), independent of the model.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StandingMemory {
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub expires_at: String,
    /// True when this memory is time-critical / action-required (a near deadline,
    /// schedule change, exam, seat change, …). Surfaced first in the UI so the
    /// student sees what needs attention now; the kinds of change that set this
    /// are also what make an incoming document trigger an immediate refresh.
    #[serde(default, skip_serializing_if = "is_false")]
    pub urgent: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

// Accept both the legacy bare-string form and the new object so already-persisted
// memories deserialize without migration.
impl<'de> Deserialize<'de> for StandingMemory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Text(String),
            Object {
                #[serde(default)]
                text: String,
                #[serde(default, rename = "expiresAt")]
                expires_at: String,
                #[serde(default)]
                urgent: bool,
            },
        }
        Ok(match Repr::deserialize(deserializer)? {
            Repr::Text(text) => StandingMemory {
                text,
                expires_at: String::new(),
                urgent: false,
            },
            Repr::Object {
                text,
                expires_at,
                urgent,
            } => StandingMemory {
                text,
                expires_at,
                urgent,
            },
        })
    }
}

/// A 確認事項 item the student should check. Carries an optional expiry (auto-
/// dropped once past, like memory) and an `action` flag marking items the student
/// must act on — those are pushed into the main TODO page.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub action: bool,
}

// Accept the legacy bare-string form so already-persisted findings load without
// migration.
impl<'de> Deserialize<'de> for Finding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Text(String),
            Object {
                #[serde(default)]
                text: String,
                #[serde(default, rename = "expiresAt")]
                expires_at: String,
                #[serde(default)]
                action: bool,
            },
        }
        Ok(match Repr::deserialize(deserializer)? {
            Repr::Text(text) => Finding {
                text,
                expires_at: String::new(),
                action: false,
            },
            Repr::Object {
                text,
                expires_at,
                action,
            } => Finding {
                text,
                expires_at,
                action,
            },
        })
    }
}

/// A consolidated cluster of past (expired) memories under one short heading,
/// e.g. label「完了した課題」with items for each finished assignment.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedGroup {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentCourseAnalysis {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub standing_context: Vec<StandingMemory>,
    /// Past memory, consolidated. When a standing memory expires it is not kept
    /// verbatim — it is folded into a short labeled group here (e.g. a finished
    /// assignment becomes one sub-item under「完了した課題」), so old context stays
    /// available to relate to future material without the list growing forever.
    /// The model maintains and condenses these groups each round.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archived_context: Vec<ArchivedGroup>,
    #[serde(default)]
    pub seat: SeatConclusion,
    #[serde(default)]
    pub print_candidates: Vec<PrintCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DocumentAnalysis {
    pub id: String,
    pub fingerprint: String,
    #[serde(default)]
    pub source_fingerprint: String,
    pub kind: String,
    pub title: String,
    pub filename: String,
    pub path: String,
    pub status: String,
    /// The text that was analysed, kept only for fileless documents
    /// (announcements have no downloaded file to re-read), so a later
    /// single-document re-analysis can run without re-fetching.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<String>,
    #[serde(default)]
    pub seat_evidence: Vec<String>,
    #[serde(default)]
    pub print_instruction: String,
    #[serde(default)]
    pub trigger_decision: String,
    #[serde(default)]
    pub observation_context: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CourseArtifactRecord {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub filename: String,
    pub path: String,
    pub source_fingerprint: String,
    pub status: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PrintResult {
    pub filename: String,
    pub path: String,
    pub status: String,
    #[serde(default)]
    pub detail: String,
    /// The candidate's print category, so the confirm UI can approve the whole
    /// type at once. Empty for legacy records.
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CourseAutomationStatus {
    pub luna_id: String,
    pub course_name: String,
    pub running: bool,
    #[serde(default)]
    pub stage: String,
    pub last_run: Option<i64>,
    pub last_ok: Option<bool>,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub trigger: String,
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub downloaded_files: Vec<String>,
    #[serde(default)]
    pub external_links: Vec<String>,
    #[serde(default)]
    pub total_documents: usize,
    #[serde(default)]
    pub processed_documents: usize,
    #[serde(default)]
    pub current_document: String,
    #[serde(default)]
    pub document_analyses: Vec<DocumentAnalysis>,
    #[serde(default)]
    pub artifacts: Vec<CourseArtifactRecord>,
    #[serde(default)]
    pub pending_summary_ids: Vec<String>,
    #[serde(default)]
    pub last_summary_document_ids: Vec<String>,
    #[serde(default)]
    pub pending_notification_ids: Vec<String>,
    #[serde(default)]
    pub notified_document_ids: Vec<String>,
    #[serde(default)]
    pub pending_seat_notification: bool,
    #[serde(default)]
    pub last_notified_seat_assignment: String,
    #[serde(default)]
    pub analysis: AgentCourseAnalysis,
    #[serde(default)]
    pub print_results: Vec<PrintResult>,
    /// 確認事項 the user has marked known/done. Filtered out of the displayed
    /// findings and fed to the summary model so it won't re-list them.
    #[serde(default)]
    pub acknowledged_findings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseAutomationView {
    pub config: CourseAutomationConfig,
    pub status: CourseAutomationStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisDocument {
    kind: String,
    title: String,
    filename: String,
    path: String,
    content: String,
    source_fingerprint: String,
    load_error: String,
    // Page images for a scanned PDF with no text layer; sent to the vision
    // model instead of text. Never serialized into the prompt JSON.
    #[serde(skip)]
    images: Vec<crate::ai::ImagePart>,
}

pub fn start_course_automation_loop(app: &AppHandle) {
    let app = app.clone();

    // Crash/restart recovery: a status left with running=true (e.g. the app
    // quit mid-run) would otherwise wedge the UI with every button disabled.
    reset_stale_running_flags(&app);

    // The unified queue dispatcher. It pulls every Job (and therefore every
    // SenseA AI request) and runs it on a bounded pool, so up to
    // JOB_PARALLELISM jobs proceed at once while the rest queue. Per-job safety
    // (cycle exclusivity, document upsert serialisation) is handled in
    // process_job via the state's locks.
    if let Some(mut rx) = app
        .state::<CourseAutomationState>()
        .job_rx
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
    {
        let dispatch_app = app.clone();
        let permits = std::sync::Arc::new(tokio::sync::Semaphore::new(JOB_PARALLELISM));
        tauri::async_runtime::spawn(async move {
            while let Some(job) = rx.recv().await {
                let Ok(permit) = permits.clone().acquire_owned().await else {
                    break;
                };
                let job_app = dispatch_app.clone();
                tauri::async_runtime::spawn(async move {
                    let _permit = permit;
                    let result = process_job(&job_app, &job).await;
                    if let Some(tx) = job.respond {
                        let _ = tx.send(result);
                    } else if let Err(error) = result {
                        log::warn!(
                            "[course_automation] course '{}' failed: {}",
                            job.course_name,
                            error
                        );
                    }
                });
            }
        });
    }

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;
        loop {
            if let Err(error) = run_due_courses(&app).await {
                log::warn!("[course_automation] scheduled check failed: {}", error);
            }
            tokio::time::sleep(Duration::from_secs(CHECK_INTERVAL_SECS)).await;
        }
    });
}

/// Runs one queued Job to completion. Each branch carries a watchdog so a hung
/// AI request can never tie up a worker slot (and thus stall the queue).
async fn process_job(app: &AppHandle, job: &Job) -> Result<CourseAutomationView, String> {
    let state = app.state::<CourseAutomationState>();
    let db = app.state::<Database>();
    match &job.kind {
        JobKind::Cycle { force_all } => {
            // A full cycle rewrites the whole status from a snapshot, so it runs
            // exclusively (write side): no document job writes underneath it.
            let _exclusive = state.cycle_lock.write().await;
            // run_course has its own timeout + status cleanup.
            run_course(app, &job.luna_id, &job.course_name, &job.trigger, *force_all).await?;
            Ok(load_view(&db, &job.luna_id, &job.course_name))
        }
        JobKind::ReanalyzeDoc { document_id } => {
            // Document jobs share the read side, so several re-analyse in
            // parallel, but none overlaps a full cycle.
            let _shared = state.cycle_lock.read().await;
            match tokio::time::timeout(
                Duration::from_secs(RUN_TIMEOUT_SECS),
                reanalyze_one_document(app, &job.luna_id, &job.course_name, document_id),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err("再分析がタイムアウトしました".into()),
            }
        }
        JobKind::RebuildMemory => {
            // Rebuilding rewrites the whole working memory from the existing
            // per-document analyses, so it runs exclusively like a cycle.
            let _exclusive = state.cycle_lock.write().await;
            match tokio::time::timeout(
                Duration::from_secs(RUN_TIMEOUT_SECS),
                rebuild_memory(app, &job.luna_id, &job.course_name),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err("記憶の再構築がタイムアウトしました".into()),
            }
        }
        JobKind::ConfirmPrint { category } => {
            // Printing a pending file is independent of cycles; share the read
            // side so it never overlaps a full cycle's status rewrite.
            let _shared = state.cycle_lock.read().await;
            confirm_print_category(app, &job.luna_id, &job.course_name, category).await
        }
    }
}

/// Enqueues a job and awaits its result — used by the user-triggered commands.
async fn enqueue_job(
    app: &AppHandle,
    luna_id: String,
    course_name: String,
    trigger: &str,
    kind: JobKind,
) -> Result<CourseAutomationView, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let job = Job {
        luna_id,
        course_name,
        trigger: trigger.to_string(),
        kind,
        respond: Some(tx),
    };
    app.state::<CourseAutomationState>()
        .job_tx
        .send(job)
        .map_err(|_| "処理キューに送信できません".to_string())?;
    rx.await
        .map_err(|_| "処理が中断されました".to_string())?
}

#[tauri::command]
pub fn course_automation_get(
    db: State<'_, Database>,
    luna_id: String,
    course_name: String,
) -> Result<CourseAutomationView, String> {
    Ok(load_view(&db, &luna_id, &course_name))
}

#[tauri::command]
pub fn course_automation_set_enabled(
    db: State<'_, Database>,
    luna_id: String,
    course_name: String,
    enabled: bool,
) -> Result<CourseAutomationView, String> {
    let mut config = load_config(&db, &luna_id, &course_name);
    config.enabled = enabled;
    if !course_name.trim().is_empty() {
        config.course_name = course_name;
    }
    save_json(&db, &config_key(&luna_id), &config)?;
    Ok(load_view(&db, &luna_id, &config.course_name))
}

#[tauri::command]
pub async fn course_automation_run_now(
    app: AppHandle,
    luna_id: String,
    course_name: String,
) -> Result<CourseAutomationView, String> {
    if !load_config(&app.state::<Database>(), &luna_id, &course_name).enabled {
        return Err("このコースの SenseA を先に有効にしてください".into());
    }
    enqueue_job(&app, luna_id, course_name, "manual", JobKind::Cycle { force_all: false }).await
}

#[tauri::command]
pub async fn course_automation_reanalyze_all(
    app: AppHandle,
    luna_id: String,
    course_name: String,
) -> Result<CourseAutomationView, String> {
    if !load_config(&app.state::<Database>(), &luna_id, &course_name).enabled {
        return Err("このコースの SenseA を先に有効にしてください".into());
    }
    enqueue_job(&app, luna_id, course_name, "manual", JobKind::Cycle { force_all: true }).await
}

#[tauri::command]
pub async fn course_automation_rebuild_memory(
    app: AppHandle,
    luna_id: String,
    course_name: String,
) -> Result<CourseAutomationView, String> {
    if !load_config(&app.state::<Database>(), &luna_id, &course_name).enabled {
        return Err("このコースの SenseA を先に有効にしてください".into());
    }
    enqueue_job(&app, luna_id, course_name, "manual", JobKind::RebuildMemory).await
}

#[tauri::command]
pub async fn course_automation_acknowledge_finding(
    app: AppHandle,
    luna_id: String,
    course_name: String,
    text: String,
    done: bool,
) -> Result<CourseAutomationView, String> {
    let db = app.state::<Database>();
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("確認事項が指定されていません".into());
    }
    let state = app.state::<CourseAutomationState>();
    // Share the read side so this never overlaps a full cycle's status rewrite.
    let _shared = state.cycle_lock.read().await;
    let _write = state.status_write.lock().await;
    let mut status = load_status(&db, &luna_id, &course_name);
    let exists = status.acknowledged_findings.iter().any(|item| item == &text);
    if done && !exists {
        status.acknowledged_findings.push(text);
    } else if !done {
        status.acknowledged_findings.retain(|item| item != &text);
    }
    save_status_and_emit(&app, &db, &status)?;
    Ok(load_view(&db, &luna_id, &course_name))
}

#[tauri::command]
pub async fn course_automation_confirm_print(
    app: AppHandle,
    luna_id: String,
    course_name: String,
    category: String,
) -> Result<CourseAutomationView, String> {
    if !load_config(&app.state::<Database>(), &luna_id, &course_name).enabled {
        return Err("このコースの SenseA を先に有効にしてください".into());
    }
    enqueue_job(
        &app,
        luna_id,
        course_name,
        "manual",
        JobKind::ConfirmPrint { category },
    )
    .await
}

#[tauri::command]
pub async fn course_automation_reanalyze_document(
    app: AppHandle,
    luna_id: String,
    course_name: String,
    document_id: String,
) -> Result<CourseAutomationView, String> {
    if !load_config(&app.state::<Database>(), &luna_id, &course_name).enabled {
        return Err("このコースの SenseA を先に有効にしてください".into());
    }
    enqueue_job(
        &app,
        luna_id,
        course_name,
        "manual",
        JobKind::ReanalyzeDoc { document_id },
    )
    .await
}

/// Re-analyses a single already-downloaded document. Runs as a queue Job, so it
/// is naturally serialised with every other SenseA operation — no extra locks.
async fn reanalyze_one_document(
    app: &AppHandle,
    luna_id: &str,
    course_name: &str,
    document_id: &str,
) -> Result<CourseAutomationView, String> {
    let db = app.state::<Database>();
    let status_snapshot = load_status(&db, luna_id, course_name);
    let existing = status_snapshot
        .document_analyses
        .iter()
        .find(|item| item.id == document_id)
        .cloned()
        .ok_or_else(|| "対象の資料が見つかりません".to_string())?;

    // Rebuild the document for re-analysis. File-backed documents are re-read
    // from disk (picking up any change); fileless ones (announcements) reuse
    // the text captured when they were first analysed.
    let mut document = AnalysisDocument {
        kind: existing.kind.clone(),
        title: existing.title.clone(),
        filename: existing.filename.clone(),
        path: existing.path.clone(),
        content: String::new(),
        source_fingerprint: existing.source_fingerprint.clone(),
        load_error: String::new(),
        images: Vec::new(),
    };
    if existing.path.is_empty() {
        if existing.content.is_empty() {
            return Err("この資料は本文が保存されていないため、「全て再分析」で再取得してください".into());
        }
        document.content = existing.content.clone();
    } else {
        let path = PathBuf::from(&existing.path);
        match crate::agent_tools::read_downloaded_text(&path) {
            Ok(text) => document.content = truncate_chars(&text, MAX_FILE_TEXT_CHARS),
            Err(error) => load_document_images_or_error(&path, &mut document, &error),
        }
    }

    let fingerprint = document_fingerprint(&document)?;
    let mut analysis = if document.load_error == DOC_SKIP_MARKER {
        DocumentAnalysis {
            id: document_id.to_string(),
            fingerprint,
            source_fingerprint: document.source_fingerprint.clone(),
            kind: document.kind.clone(),
            title: document.title.clone(),
            filename: document.filename.clone(),
            path: document.path.clone(),
            status: "skipped".into(),
            error: "本文・画像とも抽出できないためスキップしました".into(),
            ..Default::default()
        }
    } else if !document.load_error.is_empty() {
        return Err(document.load_error);
    } else {
        let student = load_student_profile(&db);
        analyze_document_with_agent(luna_id, course_name, &document, &student).await?
    };
    // Preserve the text on the record so fileless docs stay re-analysable.
    if existing.path.is_empty() && !document.content.is_empty() {
        analysis.content = document.content.clone();
    }
    let was_done = analysis.status == "done";

    // Brief critical section (serialised across concurrent document jobs):
    // reload the latest status, upsert this one document, queue it for the next
    // synthesis. The 分析 card reflects the new per-document summary at once;
    // the overall synthesis refreshes on the next cycle.
    let state = app.state::<CourseAutomationState>();
    let _write = state.status_write.lock().await;
    let mut status = load_status(&db, luna_id, course_name);
    upsert_document_analysis(&mut status.document_analyses, analysis);
    if was_done && !status.pending_summary_ids.iter().any(|id| id == document_id) {
        status.pending_summary_ids.push(document_id.to_string());
    }
    save_status_and_emit(app, &db, &status)?;

    Ok(load_view(&db, luna_id, course_name))
}

/// Rebuilds the whole working memory (summary / findings / 記憶 / seat / print)
/// from a clean slate, re-running the synthesis over every already-analysed
/// document. Re-uses the stored per-document analyses — no re-download or
/// per-document AI — so it only re-derives the consolidated memory. Used when
/// the accumulated memory has drifted and the user wants it reconstructed.
async fn rebuild_memory(
    app: &AppHandle,
    luna_id: &str,
    course_name: &str,
) -> Result<CourseAutomationView, String> {
    let db = app.state::<Database>();
    let status_snapshot = load_status(&db, luna_id, course_name);
    let analysed = status_snapshot
        .document_analyses
        .iter()
        .filter(|item| item.status == "done")
        .cloned()
        .collect::<Vec<_>>();
    if analysed.is_empty() {
        return Err("分析済みの資料がありません。先に確認を実行してください".into());
    }

    let student = load_student_profile(&db);
    // Clean slate: no previous analysis, so the memory is derived purely from
    // the documents themselves rather than carried forward.
    let analysis = summarize_with_agent(
        luna_id,
        &status_snapshot.course_name,
        &analysed,
        &student,
        &AgentCourseAnalysis::default(),
    )
    .await?;

    let analysed_ids = analysed.iter().map(|item| item.id.clone()).collect();

    let state = app.state::<CourseAutomationState>();
    let _write = state.status_write.lock().await;
    // Reload to fold the rebuilt memory onto the latest persisted status without
    // clobbering anything a concurrent change touched.
    let mut status = load_status(&db, luna_id, course_name);
    status.analysis = analysis;
    prune_acknowledged_findings(&mut status);
    status.pending_summary_ids.clear();
    status.last_summary_document_ids = analysed_ids;
    save_status_and_emit(app, &db, &status)?;

    Ok(load_view(&db, luna_id, course_name))
}

/// Approves a print category and prints the files currently waiting under it.
/// Approval is remembered in the config, so subsequent same-category files
/// print automatically without asking again.
async fn confirm_print_category(
    app: &AppHandle,
    luna_id: &str,
    course_name: &str,
    category: &str,
) -> Result<CourseAutomationView, String> {
    let db = app.state::<Database>();
    let category = category.trim().to_string();
    if category.is_empty() {
        return Err("印刷タイプが指定されていません".into());
    }

    // Remember the approval for future runs.
    let mut config = load_config(&db, luna_id, course_name);
    if !config
        .approved_print_categories
        .iter()
        .any(|item| item.trim() == category)
    {
        config.approved_print_categories.push(category.clone());
        save_json(&db, &config_key(luna_id), &config)?;
    }

    // Print every file currently waiting under this category. Done before taking
    // the status lock so the (slow) print isn't holding it.
    let pending: Vec<PrintResult> = load_status(&db, luna_id, course_name)
        .print_results
        .into_iter()
        .filter(|item| item.status == "needs_confirmation" && item.category.trim() == category)
        .collect();
    if pending.is_empty() {
        return Ok(load_view(&db, luna_id, course_name));
    }
    let mut printed = Vec::new();
    for item in pending {
        let path = PathBuf::from(&item.path);
        printed.push(print_one(&item.filename, &path, item.path.clone(), item.category).await);
    }

    let state = app.state::<CourseAutomationState>();
    let _write = state.status_write.lock().await;
    let mut status = load_status(&db, luna_id, course_name);
    status.print_results = merge_print_results(&status.print_results, printed);
    save_status_and_emit(app, &db, &status)?;
    Ok(load_view(&db, luna_id, course_name))
}

/// Clears any persisted `running` flag at startup. A run can only be active
/// after this point via the live queue, so a stored `true` is always stale.
fn reset_stale_running_flags(app: &AppHandle) {
    let db = app.state::<Database>();
    let Ok(rows) = db.list_data_cache_prefix(STATUS_PREFIX) else {
        return;
    };
    for (_, raw, _) in rows {
        let Ok(mut status) = serde_json::from_str::<CourseAutomationStatus>(&raw) else {
            continue;
        };
        if status.running {
            status.running = false;
            status.stage = String::new();
            let _ = save_status_and_emit(app, &db, &status);
        }
    }
}

async fn run_due_courses(app: &AppHandle) -> Result<(), String> {
    let db = app.state::<Database>();
    let configs = db.list_data_cache_prefix(CONFIG_PREFIX)?;
    let now = epoch_secs();
    for (_, raw, _) in configs {
        let Ok(config) = serde_json::from_str::<CourseAutomationConfig>(&raw) else {
            continue;
        };
        if !config.enabled {
            continue;
        }
        let status = load_status(&db, &config.luna_id, &config.course_name);
        let due_after = i64::from(config.interval_minutes.max(5)) * 60;
        if status
            .last_run
            .is_some_and(|last_run| now.saturating_sub(last_run) < due_after)
        {
            continue;
        }
        // Hand the due course to the unified queue; the worker logs failures.
        let _ = app.state::<CourseAutomationState>().job_tx.send(Job {
            luna_id: config.luna_id,
            course_name: config.course_name,
            trigger: "scheduled".to_string(),
            kind: JobKind::Cycle { force_all: false },
            respond: None,
        });
    }
    Ok(())
}

async fn run_course(
    app: &AppHandle,
    luna_id: &str,
    course_name_hint: &str,
    trigger: &str,
    force_all: bool,
) -> Result<(), String> {
    // Exclusivity is provided by the single queue worker; here we only keep a
    // watchdog so a hung run can't wedge the queue.
    let outcome = match tokio::time::timeout(
        Duration::from_secs(RUN_TIMEOUT_SECS),
        run_course_inner(app, luna_id, course_name_hint, trigger, force_all),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err("実行がタイムアウトしました・次回再試行します".to_string()),
    };
    // Any failure (timeout OR an early error return from the inner run) must
    // clear the running flag — otherwise the persisted status stays running and
    // the UI wedges with every button disabled.
    if let Err(error) = &outcome {
        let db = app.state::<Database>();
        let mut status = load_status(&db, luna_id, course_name_hint);
        status.running = false;
        status.last_run = Some(epoch_secs());
        status.last_ok = Some(false);
        status.stage = "error".into();
        status.last_error = error.clone();
        let _ = save_status_and_emit(app, &db, &status);
    }
    outcome
}

async fn run_course_inner(
    app: &AppHandle,
    luna_id: &str,
    course_name_hint: &str,
    trigger: &str,
    force_all: bool,
) -> Result<(), String> {
    let db = app.state::<Database>();
    let config = load_config(&db, luna_id, course_name_hint);
    let mut previous = load_status(&db, luna_id, course_name_hint);
    migrate_legacy_artifacts(&mut previous);
    let mut status = CourseAutomationStatus {
        luna_id: luna_id.to_string(),
        course_name: course_name_hint.to_string(),
        running: true,
        stage: "checking".into(),
        trigger: trigger.into(),
        ..previous.clone()
    };
    status.last_error.clear();
    save_status_and_emit(app, &db, &status)?;

    let outcome: Result<(), String> = async {
        let contents = crate::agent_tools::fetch_luna_course_contents(app, luna_id).await?;
        if !contents.course_name.trim().is_empty() {
            status.course_name = contents.course_name.clone();
        }
        let mut source_snapshot = json!({
            "course": &contents.course_name,
            "materials": &contents.materials,
            "announcements": &contents.announcements,
            "reports": &contents.reports,
        });

        status.stage = "downloading".into();
        save_status_and_emit(app, &db, &status)?;
        let mut downloaded = Vec::<(String, String, String, PathBuf, String)>::new();
        let mut artifacts = previous.artifacts.clone();
        let mut external_links = Vec::new();
        let mut activity_documents = Vec::new();
        let mut seen_paths = HashSet::new();
        for material in contents.materials.iter().filter(|_| config.monitor_materials) {
            for file in &material.files {
                let filename = if file.file_name.trim().is_empty() {
                    file.display_name.trim()
                } else {
                    file.file_name.trim()
                };
                if filename.is_empty() {
                    continue;
                }
                let source_fingerprint = material_source_fingerprint(file)?;
                if let Some((path, persisted_source_fingerprint)) =
                    reusable_artifact_path(&artifacts, "material", &material.title, filename)
                {
                    let persisted_source_fingerprint = if persisted_source_fingerprint.is_empty() {
                        source_fingerprint.clone()
                    } else {
                        persisted_source_fingerprint
                    };
                    upsert_artifact(
                        &mut artifacts,
                        CourseArtifactRecord {
                            id: artifact_id("material", filename),
                            kind: "material".into(),
                            title: material.title.clone(),
                            filename: filename.to_string(),
                            path: path.to_string_lossy().to_string(),
                            source_fingerprint: persisted_source_fingerprint.clone(),
                            status: "downloaded".into(),
                            error: String::new(),
                        },
                    );
                    status.artifacts = artifacts.clone();
                    save_status_and_emit(app, &db, &status)?;
                    downloaded.push((
                        "material".into(),
                        material.title.clone(),
                        filename.to_string(),
                        path,
                        persisted_source_fingerprint,
                    ));
                    continue;
                }
                match crate::agent_tools::download_luna_course_material(app, luna_id, filename)
                    .await
                {
                    Ok(Some(value)) => {
                        collect_download_value(
                            &mut downloaded,
                            &mut external_links,
                            &mut seen_paths,
                            "material",
                            &material.title,
                            &source_fingerprint,
                            &value,
                        );
                        upsert_artifact(
                            &mut artifacts,
                            artifact_from_download_value(
                                "material",
                                &material.title,
                                filename,
                                &source_fingerprint,
                                &value,
                            ),
                        );
                        status.artifacts = artifacts.clone();
                        save_status_and_emit(app, &db, &status)?;
                    }
                    Ok(None) => {
                        upsert_artifact(
                            &mut artifacts,
                            failed_artifact(
                                "material",
                                &material.title,
                                filename,
                                &source_fingerprint,
                                "資料ファイルが見つかりません",
                            ),
                        );
                        status.artifacts = artifacts.clone();
                        save_status_and_emit(app, &db, &status)?;
                    }
                    Err(error) => {
                        log::warn!(
                            "[course_automation] material download failed '{}': {}",
                            filename,
                            error
                        );
                        upsert_artifact(
                            &mut artifacts,
                            failed_artifact(
                                "material",
                                &material.title,
                                filename,
                                &source_fingerprint,
                                &format!("ダウンロード失敗: {}", error),
                            ),
                        );
                        status.artifacts = artifacts.clone();
                        save_status_and_emit(app, &db, &status)?;
                    }
                }
            }
        }
        let reusable_activity_paths = reusable_activity_downloads(&artifacts);
        let mut activity_kinds = Vec::new();
        if config.monitor_announcements {
            activity_kinds.push("announcement");
        }
        if config.monitor_assignments {
            activity_kinds.push("report");
        }
        let activity_values = if activity_kinds.is_empty() {
            Vec::new()
        } else {
            crate::agent_tools::download_luna_activity_attachments(
                app,
                luna_id,
                &contents,
                &activity_kinds,
                &reusable_activity_paths,
            )
            .await?
        };
        source_snapshot["activityDetails"] = Value::Array(
            activity_values
                .iter()
                .filter(|value| {
                    matches!(
                        value.get("status").and_then(Value::as_str),
                        Some("detail" | "detail_error")
                    )
                })
                .cloned()
                .collect(),
        );
        let fingerprint = sha256_json(&source_snapshot)?;
        for value in activity_values {
            let kind = value
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("activity");
            let title = value.get("title").and_then(Value::as_str).unwrap_or("");
            if matches!(
                value.get("status").and_then(Value::as_str),
                Some("downloaded" | "reused" | "error")
            ) {
                let filename = value.get("filename").and_then(Value::as_str).unwrap_or("");
                let source_fingerprint = value
                    .get("source_fingerprint")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let artifact = if value.get("status").and_then(Value::as_str) == Some("error") {
                    failed_artifact(
                        kind,
                        title,
                        filename,
                        source_fingerprint,
                        value
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("ダウンロード失敗"),
                    )
                } else {
                    artifact_from_download_value(kind, title, filename, source_fingerprint, &value)
                };
                upsert_artifact(&mut artifacts, artifact);
                status.artifacts = artifacts.clone();
                save_status_and_emit(app, &db, &status)?;
            }
            if value.get("status").and_then(Value::as_str) == Some("detail") {
                let content = value.get("content").and_then(Value::as_str).unwrap_or("");
                let meta = value.get("meta").cloned().unwrap_or_else(|| json!([]));
                activity_documents.push(AnalysisDocument {
                    kind: kind.to_string(),
                    title: title.to_string(),
                    filename: String::new(),
                    path: String::new(),
                    content: truncate_chars(&format!("{}\n{}", content, meta), MAX_FILE_TEXT_CHARS),
                    source_fingerprint: value
                        .get("source_fingerprint")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    load_error: String::new(),
                    images: Vec::new(),
                });
                continue;
            }
            if matches!(
                value.get("status").and_then(Value::as_str),
                Some("detail_error" | "error")
            ) {
                if value.get("status").and_then(Value::as_str) == Some("detail_error") {
                    activity_documents.push(failed_analysis_document(
                        kind,
                        title,
                        "",
                        value
                            .get("source_fingerprint")
                            .and_then(Value::as_str)
                            .unwrap_or(""),
                        value
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("取得失敗"),
                    ));
                }
                continue;
            }
            collect_download_value(
                &mut downloaded,
                &mut external_links,
                &mut seen_paths,
                kind,
                title,
                value
                    .get("source_fingerprint")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                &value,
            );
        }
        status.downloaded_files = merge_unique_ids(
            &previous.downloaded_files,
            downloaded
                .iter()
                .map(|(_, _, _, path, _)| path.to_string_lossy().to_string()),
        );
        status.external_links = merge_unique_ids(&previous.external_links, external_links);
        status.stage = "analyzing".into();
        let mut documents = activity_documents;
        documents.extend(build_analysis_documents(&downloaded, &previous));
        // analyze_all = false: only touch new or changed items. Already-done
        // items keep their stored analysis (still in working/audit memory) and
        // are dropped from this run's set. Default (true) keeps the full sweep.
        // A manual "re-analyze all" overrides this and re-analyses everything.
        if !(config.analyze_all || force_all) {
            documents.retain(|document| {
                match previous_document_analysis(&previous, document) {
                    Some(previous_analysis) if previous_analysis.status == "done" => {
                        document_fingerprint(document)
                            .map(|fingerprint| fingerprint != previous_analysis.fingerprint)
                            .unwrap_or(true)
                    }
                    _ => true,
                }
            });
        }
        let student = load_student_profile(&db);
        status.total_documents = documents.len();
        status.processed_documents = 0;
        status.current_document.clear();
        save_status_and_emit(app, &db, &status)?;

        let mut document_analyses = previous.document_analyses.clone();
        let mut newly_analyzed_ids = Vec::new();
        for document in &documents {
            status.current_document = document_label(document);
            save_status_and_emit(app, &db, &status)?;
            let fingerprint = document_fingerprint(document)?;
            let previous_document = previous_document_analysis(&previous, document);
            let mut analysis = if document.load_error == DOC_SKIP_MARKER {
                DocumentAnalysis {
                    id: document_id(document),
                    fingerprint,
                    source_fingerprint: document.source_fingerprint.clone(),
                    kind: document.kind.clone(),
                    title: document.title.clone(),
                    filename: document.filename.clone(),
                    path: document.path.clone(),
                    status: "skipped".into(),
                    error: "本文・画像とも抽出できないためスキップしました".into(),
                    ..Default::default()
                }
            } else if !document.load_error.is_empty() {
                DocumentAnalysis {
                    id: document_id(document),
                    fingerprint,
                    source_fingerprint: document.source_fingerprint.clone(),
                    kind: document.kind.clone(),
                    title: document.title.clone(),
                    filename: document.filename.clone(),
                    path: document.path.clone(),
                    status: "error".into(),
                    error: document.load_error.clone(),
                    ..Default::default()
                }
            } else if previous_document.is_some_and(|item| item.status == "done") {
                migrate_successful_analysis(
                    previous_document.expect("checked above"),
                    document,
                    fingerprint,
                )
            } else {
                match analyze_document_with_agent(luna_id, &status.course_name, document, &student)
                    .await
                {
                    Ok(analysis) => analysis,
                    Err(error) => DocumentAnalysis {
                        id: document_id(document),
                        fingerprint,
                        source_fingerprint: document.source_fingerprint.clone(),
                        kind: document.kind.clone(),
                        title: document.title.clone(),
                        filename: document.filename.clone(),
                        path: document.path.clone(),
                        status: "error".into(),
                        error,
                        ..Default::default()
                    },
                }
            };
            // Fileless documents (announcements) have no file to re-read later,
            // so keep the analysed text on the record for single re-analysis.
            if document.path.is_empty() && !document.content.is_empty() {
                analysis.content = document.content.clone();
            }
            if analysis.status == "done"
                && previous_document.is_none_or(|item| item.status != "done")
            {
                newly_analyzed_ids.push(analysis.id.clone());
            }
            upsert_document_analysis(&mut document_analyses, analysis);
            status.processed_documents += 1;
            status.document_analyses = document_analyses.clone();
            save_status_and_emit(app, &db, &status)?;
        }
        status.current_document.clear();
        let current_document_ids = documents.iter().map(document_id).collect::<HashSet<_>>();
        let successful = document_analyses
            .iter()
            .filter(|analysis| {
                analysis.status == "done" && current_document_ids.contains(&analysis.id)
            })
            .count();
        let errored = document_analyses
            .iter()
            .filter(|analysis| {
                analysis.status == "error" && current_document_ids.contains(&analysis.id)
            })
            .count();
        // Abort only when documents genuinely failed; a run where everything was
        // skipped (e.g. unreadable scans) is not a failure.
        if successful == 0 && errored > 0 {
            return Err("全資料の分析に失敗しました".into());
        }

        let mut pending_summary_ids = previous.pending_summary_ids.to_vec();
        let mut pending_notification_ids = previous.pending_notification_ids.to_vec();
        for id in newly_analyzed_ids {
            if !pending_summary_ids.iter().any(|existing| existing == &id) {
                pending_summary_ids.push(id);
            }
        }
        for analysis in &document_analyses {
            let notification_key = document_notification_key(analysis);
            if analysis.status == "done"
                && analysis.trigger_decision == "immediate"
                && pending_summary_ids.iter().any(|id| id == &analysis.id)
                && !previous
                    .notified_document_ids
                    .iter()
                    .any(|id| id == &notification_key)
                && !pending_notification_ids
                    .iter()
                    .any(|id| id == &notification_key)
            {
                pending_notification_ids.push(notification_key);
            }
        }
        status.pending_notification_ids = pending_notification_ids;
        let agent_requests_immediate_summary = document_analyses.iter().any(|item| {
            pending_summary_ids.iter().any(|id| id == &item.id)
                && item.trigger_decision == "immediate"
        });
        let should_summarize = should_refresh_summary(
            &previous.analysis.summary,
            pending_summary_ids.len(),
            agent_requests_immediate_summary,
        );
        let analysis = if should_summarize {
            status.stage = "summarizing".into();
            status.pending_summary_ids = pending_summary_ids.clone();
            save_status_and_emit(app, &db, &status)?;
            let new_items = document_analyses
                .iter()
                .filter(|item| pending_summary_ids.iter().any(|id| id == &item.id))
                .cloned()
                .collect::<Vec<_>>();
            let mut result = summarize_with_agent(
                luna_id,
                &status.course_name,
                &new_items,
                &student,
                &previous.analysis,
            )
            .await?;
            result.print_candidates = merge_print_candidates(
                &previous.analysis.print_candidates,
                result.print_candidates,
            );
            status.pending_summary_ids.clear();
            status.last_summary_document_ids = merge_unique_ids(
                &previous.last_summary_document_ids,
                new_items.iter().map(|item| item.id.clone()),
            );
            result
        } else {
            status.pending_summary_ids = pending_summary_ids;
            previous.analysis.clone()
        };

        status.pending_seat_notification = config.notify_seat_changes
            && !analysis.seat.assignment.trim().is_empty()
            && analysis.seat.assignment != status.last_notified_seat_assignment;
        if status.pending_seat_notification {
            let body = format!(
                "{}\n根拠: {}",
                analysis.seat.assignment,
                analysis.seat.evidence.join(" / ")
            );
            match crate::ai::send_native_notification(
                app,
                &format!("{}: 座席情報が更新されました", status.course_name),
                &body,
            ) {
                Ok(_) => {
                    status.last_notified_seat_assignment = analysis.seat.assignment.clone();
                    status.pending_seat_notification = false;
                }
                Err(error) => {
                    log::warn!(
                        "[course_automation] seat notification failed; retrying next run: {}",
                        error
                    );
                }
            }
        }

        status.analysis = analysis;
        prune_acknowledged_findings(&mut status);
        if !status.pending_notification_ids.is_empty() && !status.analysis.summary.trim().is_empty()
        {
            let pending_ids = status.pending_notification_ids.clone();
            match crate::ai::send_native_notification(
                app,
                &format!("{}: SenseA から確認事項", status.course_name),
                &proactive_notification_body(&status.analysis),
            ) {
                Ok(_) => {
                    status.notified_document_ids =
                        merge_unique_ids(&previous.notified_document_ids, pending_ids);
                    status.pending_notification_ids.clear();
                }
                Err(error) => {
                    log::warn!(
                        "[course_automation] proactive notification failed; retrying next run: {}",
                        error
                    );
                }
            }
        }
        status.print_results = if config.auto_print
            && (should_summarize || has_retryable_print_failure(&previous.print_results))
        {
            status.stage = "printing".into();
            save_status_and_emit(app, &db, &status)?;
            merge_print_results(
                &previous.print_results,
                process_print_candidates(
                    &downloaded,
                    &status.analysis.print_candidates,
                    &previous,
                    &config.approved_print_categories,
                )
                .await,
            )
        } else if should_summarize {
            previous.print_results.clone()
        } else {
            status.stage = if status.pending_summary_ids.is_empty() {
                "unchanged".into()
            } else {
                "pending_summary".into()
            };
            previous.print_results.clone()
        };
        status.fingerprint = fingerprint;
        Ok(())
    }
    .await;

    let retryable_failure_count = retryable_failure_count(&status);
    status.running = false;
    status.last_run = Some(epoch_secs());
    status.last_ok = Some(outcome.is_ok() && retryable_failure_count == 0);
    // Print failures are surfaced on the 印刷 capsule (via print_results), not the
    // control capsule, so a successful run with only print failures is not an
    // error here — it does not touch last_error or the error stage.
    status.stage = if outcome.is_ok() {
        match status.stage.as_str() {
            "unchanged" | "pending_summary" => status.stage.clone(),
            _ => "done".into(),
        }
    } else {
        "error".into()
    };
    if let Err(error) = &outcome {
        status.last_error = error.clone();
    }
    save_status_and_emit(app, &db, &status)?;
    outcome
}

fn collect_download_value(
    downloaded: &mut Vec<(String, String, String, PathBuf, String)>,
    external_links: &mut Vec<String>,
    seen_paths: &mut HashSet<String>,
    kind: &str,
    title: &str,
    source_fingerprint: &str,
    value: &Value,
) {
    if let Some(url) = value.get("url").and_then(Value::as_str) {
        if !url.trim().is_empty() && !external_links.iter().any(|item| item == url) {
            external_links.push(url.to_string());
        }
    }
    let Some(path) = value.get("saved_path").and_then(Value::as_str) else {
        return;
    };
    if path.trim().is_empty() || !seen_paths.insert(path.to_string()) {
        return;
    }
    let filename = value
        .get("filename")
        .or_else(|| value.get("attachment_name"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
        });
    downloaded.push((
        kind.to_string(),
        title.to_string(),
        filename.to_string(),
        PathBuf::from(path),
        source_fingerprint.to_string(),
    ));
}

fn build_analysis_documents(
    downloaded: &[(String, String, String, PathBuf, String)],
    previous: &CourseAutomationStatus,
) -> Vec<AnalysisDocument> {
    let mut documents = Vec::new();
    for (kind, title, filename, path, source_fingerprint) in downloaded {
        let mut document = AnalysisDocument {
            kind: kind.clone(),
            title: title.clone(),
            filename: filename.clone(),
            path: path.to_string_lossy().to_string(),
            content: String::new(),
            source_fingerprint: source_fingerprint.clone(),
            load_error: String::new(),
            images: Vec::new(),
        };
        match previous_document_analysis(previous, &document).map(|item| item.status.as_str()) {
            // Reused on a later pass: a done analysis is migrated, a skipped one
            // stays skipped. Neither re-reads the file.
            Some("done") => {}
            Some("skipped") => document.load_error = DOC_SKIP_MARKER.into(),
            _ => match crate::agent_tools::read_downloaded_text(path) {
                Ok(text) => document.content = truncate_chars(&text, MAX_FILE_TEXT_CHARS),
                Err(error) => load_document_images_or_error(path, &mut document, &error),
            },
        }
        documents.push(document);
    }
    documents
}

/// When text extraction fails for a PDF, fall back to its embedded page images
/// so a vision model can still read it. If a PDF yields neither text nor images
/// it is skipped (terminal). Non-PDF failures keep the original error.
fn load_document_images_or_error(path: &Path, document: &mut AnalysisDocument, text_error: &str) {
    let is_pdf = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"));
    if is_pdf {
        // 1) Cheap path: pass through embedded JPEG images (scanned PDFs).
        // 2) Fallback: rasterize the pages (vector PDFs with no images).
        let images = crate::agent_tools::read_downloaded_images(path)
            .ok()
            .filter(|images| !images.is_empty())
            .or_else(|| {
                crate::agent_tools::render_pdf_images(path)
                    .map_err(|error| {
                        log::warn!("[course_automation] PDF render failed: {}", error)
                    })
                    .ok()
                    .filter(|images| !images.is_empty())
            });
        if let Some(images) = images {
            document.images = images;
            document.content =
                "（本文をテキスト抽出できないため、添付画像から読み取ってください）".into();
            return;
        }
        // No text, no images, no render: skip rather than retry forever.
        document.load_error = DOC_SKIP_MARKER.into();
        return;
    }
    document.load_error = format!("本文抽出失敗: {}", text_error);
}

fn failed_analysis_document(
    kind: &str,
    title: &str,
    filename: &str,
    source_fingerprint: &str,
    error: &str,
) -> AnalysisDocument {
    AnalysisDocument {
        kind: kind.to_string(),
        title: title.to_string(),
        filename: filename.to_string(),
        path: String::new(),
        content: String::new(),
        source_fingerprint: source_fingerprint.to_string(),
        load_error: error.to_string(),
        images: Vec::new(),
    }
}

fn reusable_artifact_path(
    artifacts: &[CourseArtifactRecord],
    kind: &str,
    _title: &str,
    filename: &str,
) -> Option<(PathBuf, String)> {
    artifacts
        .iter()
        .find(|artifact| {
            artifact.status == "downloaded"
                && (artifact.kind == kind || artifact.kind == "legacy")
                && artifact.filename == filename
                && !artifact.path.is_empty()
                && Path::new(&artifact.path).is_file()
        })
        .map(|artifact| {
            (
                PathBuf::from(&artifact.path),
                artifact.source_fingerprint.clone(),
            )
        })
}

fn reusable_activity_downloads(
    artifacts: &[CourseArtifactRecord],
) -> HashMap<String, crate::agent_tools::ReusableCourseDownload> {
    let mut paths = HashMap::new();
    for artifact in artifacts {
        if artifact.status != "downloaded"
            || !matches!(artifact.kind.as_str(), "announcement" | "report" | "legacy")
            || artifact.path.is_empty()
            || !Path::new(&artifact.path).is_file()
        {
            continue;
        }
        let reusable = crate::agent_tools::ReusableCourseDownload {
            path: artifact.path.clone(),
            source_fingerprint: artifact.source_fingerprint.clone(),
        };
        if !artifact.source_fingerprint.is_empty() {
            paths.insert(
                format!("fingerprint:{}", artifact.source_fingerprint),
                reusable.clone(),
            );
        }
        if artifact.kind == "legacy" {
            paths.insert(
                activity_download_identity("announcement", &artifact.filename),
                reusable.clone(),
            );
            paths.insert(
                activity_download_identity("report", &artifact.filename),
                reusable,
            );
        } else {
            paths.insert(
                activity_download_identity(&artifact.kind, &artifact.filename),
                reusable,
            );
        }
    }
    paths
}

fn artifact_id(kind: &str, filename: &str) -> String {
    format!("{:x}", Sha256::digest(format!("{}|{}", kind, filename)))
}

fn artifact_from_download_value(
    kind: &str,
    title: &str,
    filename: &str,
    source_fingerprint: &str,
    value: &Value,
) -> CourseArtifactRecord {
    let path = value
        .get("saved_path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    CourseArtifactRecord {
        id: artifact_id(kind, filename),
        kind: kind.to_string(),
        title: title.to_string(),
        filename: filename.to_string(),
        source_fingerprint: source_fingerprint.to_string(),
        status: if path.is_empty() {
            "external".into()
        } else {
            "downloaded".into()
        },
        path,
        error: String::new(),
    }
}

fn failed_artifact(
    kind: &str,
    title: &str,
    filename: &str,
    source_fingerprint: &str,
    error: &str,
) -> CourseArtifactRecord {
    CourseArtifactRecord {
        id: format!("{}:error", artifact_id(kind, filename)),
        kind: kind.to_string(),
        title: title.to_string(),
        filename: filename.to_string(),
        source_fingerprint: source_fingerprint.to_string(),
        status: "error".into(),
        error: error.to_string(),
        ..Default::default()
    }
}

fn upsert_artifact(artifacts: &mut Vec<CourseArtifactRecord>, artifact: CourseArtifactRecord) {
    if artifact.status == "downloaded" {
        artifacts.retain(|item| {
            item.status != "error"
                || item.kind != artifact.kind
                || item.filename != artifact.filename
        });
    }
    if let Some(existing) = artifacts.iter_mut().find(|item| item.id == artifact.id) {
        if existing.status != "downloaded" || artifact.status == "downloaded" {
            *existing = artifact;
        }
    } else {
        artifacts.push(artifact);
    }
}

fn migrate_legacy_artifacts(status: &mut CourseAutomationStatus) {
    for analysis in &status.document_analyses {
        if analysis.path.is_empty() || !Path::new(&analysis.path).is_file() {
            continue;
        }
        let artifact = CourseArtifactRecord {
            id: artifact_id(&analysis.kind, &analysis.filename),
            kind: analysis.kind.clone(),
            title: analysis.title.clone(),
            filename: analysis.filename.clone(),
            path: analysis.path.clone(),
            source_fingerprint: analysis.source_fingerprint.clone(),
            status: "downloaded".into(),
            error: String::new(),
        };
        if !status
            .artifacts
            .iter()
            .any(|existing| existing.id == artifact.id)
        {
            status.artifacts.push(artifact);
        }
    }
    for path in &status.downloaded_files {
        if !Path::new(path).is_file() {
            continue;
        }
        let filename = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if filename.is_empty()
            || status.artifacts.iter().any(|artifact| {
                artifact.status == "downloaded"
                    && artifact.filename == filename
                    && artifact.path == *path
            })
        {
            continue;
        }
        status.artifacts.push(CourseArtifactRecord {
            id: artifact_id("legacy", filename),
            kind: "legacy".into(),
            filename: filename.to_string(),
            path: path.clone(),
            status: "downloaded".into(),
            ..Default::default()
        });
    }
}

fn activity_download_identity(kind: &str, filename: &str) -> String {
    format!("identity:{}|{}", kind, filename)
}

fn migrate_successful_analysis(
    previous: &DocumentAnalysis,
    document: &AnalysisDocument,
    fingerprint: String,
) -> DocumentAnalysis {
    DocumentAnalysis {
        id: document_id(document),
        fingerprint,
        source_fingerprint: document.source_fingerprint.clone(),
        kind: document.kind.clone(),
        title: document.title.clone(),
        filename: document.filename.clone(),
        path: document.path.clone(),
        ..previous.clone()
    }
}

fn upsert_document_analysis(analyses: &mut Vec<DocumentAnalysis>, analysis: DocumentAnalysis) {
    if analysis.status == "done" {
        analyses.retain(|item| {
            item.status != "error"
                || item.kind != analysis.kind
                || item.title != analysis.title
                || item.filename != analysis.filename
        });
    }
    if let Some(existing) = analyses.iter_mut().find(|item| item.id == analysis.id) {
        if existing.status != "done" || analysis.status == "done" {
            *existing = analysis;
        }
    } else {
        analyses.push(analysis);
    }
}

fn merge_unique_ids(
    existing: &[String],
    additional: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut merged = existing.to_vec();
    for id in additional {
        if !merged.iter().any(|existing_id| existing_id == &id) {
            merged.push(id);
        }
    }
    merged
}

fn has_retryable_print_failure(results: &[PrintResult]) -> bool {
    results
        .iter()
        .any(|item| matches!(item.status.as_str(), "error" | "not_found"))
}

fn merge_print_results(existing: &[PrintResult], current: Vec<PrintResult>) -> Vec<PrintResult> {
    let mut merged = existing.to_vec();
    for result in current {
        let matching_index = merged.iter().position(|item| {
            (!result.path.is_empty() && item.path == result.path)
                || item.filename == result.filename
        });
        match matching_index {
            Some(index) if merged[index].status == "printed" => {}
            Some(index) => merged[index] = result,
            None => merged.push(result),
        }
    }
    merged
}

fn merge_print_candidates(
    existing: &[PrintCandidate],
    current: Vec<PrintCandidate>,
) -> Vec<PrintCandidate> {
    let mut merged = existing.to_vec();
    for candidate in current {
        if let Some(existing) = merged
            .iter_mut()
            .find(|item| item.filename == candidate.filename)
        {
            *existing = candidate;
        } else {
            merged.push(candidate);
        }
    }
    merged
}

fn retryable_failure_count(status: &CourseAutomationStatus) -> usize {
    status
        .artifacts
        .iter()
        .filter(|item| item.status == "error")
        .count()
        + status
            .document_analyses
            .iter()
            .filter(|item| item.status == "error")
            .count()
        + status
            .print_results
            .iter()
            .filter(|item| matches!(item.status.as_str(), "error" | "not_found"))
            .count()
        + usize::from(!status.pending_notification_ids.is_empty())
        + usize::from(status.pending_seat_notification)
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DocumentAgentOutput {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    findings: Vec<String>,
    #[serde(default)]
    seat_evidence: Vec<String>,
    #[serde(default)]
    print_instruction: String,
    #[serde(default)]
    trigger_decision: String,
    #[serde(default)]
    observation_context: String,
}

async fn analyze_document_with_agent(
    luna_id: &str,
    course_name: &str,
    document: &AnalysisDocument,
    student: &Value,
) -> Result<DocumentAnalysis, String> {
    let provider = AgentProvider::resolve().map_err(|error| error.to_string())?;
    let input = context::IndividualInput {
        course_id: luna_id,
        course_name,
        student,
        document: document.into(),
    };
    let label = format!("「{}」の個別まとめ", document_label(document));
    let output: DocumentAgentOutput = request_plus_json(
        &provider,
        context::INDIVIDUAL_SYSTEM_PROMPT,
        serde_json::to_string(&input).map_err(|error| error.to_string())?,
        document.images.clone(),
        PLUS_DOCUMENT_MAX_TOKENS,
        10,
        &format!("course-automation-{}-{}", luna_id, document_id(document)),
        &label,
    )
    .await?;
    let output = normalize_document_agent_output(output);
    Ok(DocumentAnalysis {
        id: document_id(document),
        fingerprint: document_fingerprint(document)?,
        source_fingerprint: document.source_fingerprint.clone(),
        kind: document.kind.clone(),
        title: document.title.clone(),
        filename: document.filename.clone(),
        path: document.path.clone(),
        status: "done".into(),
        content: String::new(),
        summary: output.summary,
        findings: output.findings,
        seat_evidence: output.seat_evidence,
        print_instruction: output.print_instruction,
        trigger_decision: normalize_trigger_decision(&output.trigger_decision),
        observation_context: output.observation_context,
        error: String::new(),
    })
}

async fn summarize_with_agent(
    luna_id: &str,
    course_name: &str,
    new_or_changed_documents: &[DocumentAnalysis],
    student: &Value,
    previous: &AgentCourseAnalysis,
) -> Result<AgentCourseAnalysis, String> {
    let provider = AgentProvider::resolve().map_err(|error| error.to_string())?;
    let configured_max_tokens = crate::ai::load_ai_config().max_tokens;
    let mut working_memory = previous.clone();
    for (batch_index, batch) in context::summary_batches(new_or_changed_documents)
        .into_iter()
        .enumerate()
    {
        let input = context::SummaryInput {
            current_local_time: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            course_id: luna_id,
            course_name,
            student,
            previous_course_analysis: &working_memory,
            new_or_changed_documents: batch
                .into_iter()
                .map(context::CompactAnalysis::from)
                .collect(),
        };
        let label = format!("最終まとめ batch {}", batch_index + 1);
        let result = request_plus_json(
            &provider,
            context::SUMMARY_SYSTEM_PROMPT,
            serde_json::to_string(&input).map_err(|error| error.to_string())?,
            Vec::new(),
            configured_max_tokens,
            20,
            &format!("course-automation-{}-summary-{}", luna_id, batch_index),
            &label,
        )
        .await?;
        working_memory =
            normalize_course_analysis(result, &chrono::Local::now().format("%Y-%m-%d").to_string());
    }
    Ok(working_memory)
}

async fn request_plus_json<T: DeserializeOwned>(
    provider: &AgentProvider,
    instructions: &str,
    input: String,
    images: Vec<crate::ai::ImagePart>,
    max_tokens: u32,
    think_budget_pct: u32,
    gen_id: &str,
    label: &str,
) -> Result<T, String> {
    context::log_request_size(label, instructions, &input);
    let mut last_error = String::new();
    for attempt in 1..=PLUS_AI_ATTEMPTS {
        let request = provider.plan(
            vec![
                ChatMessage {
                    role: "system".into(),
                    content: instructions.to_string(),
                    images: Vec::new(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: input.clone(),
                    images: images.clone(),
                },
            ],
            max_tokens,
            0.1,
            "",
            think_budget_pct,
            gen_id,
        );
        let response =
            match tokio::time::timeout(Duration::from_secs(PLUS_AI_TIMEOUT_SECS), request).await {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => {
                    last_error = error.to_string();
                    log::warn!(
                        "[course_automation] {} AI attempt {}/{} failed: {}",
                        label,
                        attempt,
                        PLUS_AI_ATTEMPTS,
                        last_error
                    );
                    if attempt < PLUS_AI_ATTEMPTS {
                        tokio::time::sleep(Duration::from_millis(750)).await;
                    }
                    continue;
                }
                Err(_) => {
                    last_error = format!(
                        "AI リクエストが {} 秒でタイムアウトしました",
                        PLUS_AI_TIMEOUT_SECS
                    );
                    log::warn!(
                        "[course_automation] {} AI attempt {}/{} timed out",
                        label,
                        attempt,
                        PLUS_AI_ATTEMPTS
                    );
                    if attempt < PLUS_AI_ATTEMPTS {
                        tokio::time::sleep(Duration::from_millis(750)).await;
                    }
                    continue;
                }
            };
        log::info!(
            "[course_automation] {} response size: {} tokens",
            label,
            context::estimate_tokens(&response)
        );
        let Some(json_text) = extract_json_object(&response) else {
            last_error = format!("{}の結果に JSON がありません", label);
            log::warn!(
                "[course_automation] {} AI attempt {}/{} returned no JSON",
                label,
                attempt,
                PLUS_AI_ATTEMPTS
            );
            if attempt < PLUS_AI_ATTEMPTS {
                tokio::time::sleep(Duration::from_millis(750)).await;
            }
            continue;
        };
        match serde_json::from_str(json_text) {
            Ok(output) => return Ok(output),
            Err(error) => {
                last_error = format!("{} JSON 解析失敗: {}", label, error);
                log::warn!(
                    "[course_automation] {} AI attempt {}/{} returned invalid JSON: {}",
                    label,
                    attempt,
                    PLUS_AI_ATTEMPTS,
                    error
                );
                if attempt < PLUS_AI_ATTEMPTS {
                    tokio::time::sleep(Duration::from_millis(750)).await;
                }
            }
        }
    }
    Err(last_error)
}

fn document_label(document: &AnalysisDocument) -> String {
    if document.filename.trim().is_empty() {
        document.title.clone()
    } else {
        document.filename.clone()
    }
}

fn document_id(document: &AnalysisDocument) -> String {
    if !document.source_fingerprint.is_empty() {
        return format!(
            "{:x}",
            Sha256::digest(format!(
                "{}|{}|{}",
                document.kind, document.filename, document.source_fingerprint
            ))
        );
    }
    format!(
        "{:x}",
        Sha256::digest(format!(
            "{}|{}|{}|{}",
            document.kind, document.title, document.filename, document.path
        ))
    )
}

fn document_fingerprint(document: &AnalysisDocument) -> Result<String, String> {
    sha256_json(&json!({
        "kind": document.kind,
        "title": document.title,
        "filename": document.filename,
        "path": document.path,
        "content": document.content,
        "sourceFingerprint": document.source_fingerprint,
    }))
}

fn previous_document_analysis<'a>(
    previous: &'a CourseAutomationStatus,
    document: &AnalysisDocument,
) -> Option<&'a DocumentAnalysis> {
    let id = document_id(document);
    previous
        .document_analyses
        .iter()
        .find(|analysis| analysis.id == id)
        .or_else(|| {
            previous.document_analyses.iter().find(|analysis| {
                analysis.status == "done"
                    && analysis.source_fingerprint.is_empty()
                    && analysis.kind == document.kind
                    && analysis.title == document.title
                    && analysis.filename == document.filename
            })
        })
}

fn normalize_trigger_decision(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "immediate" => "immediate".into(),
        "observe" => "observe".into(),
        _ => "routine".into(),
    }
}

fn normalize_document_agent_output(mut output: DocumentAgentOutput) -> DocumentAgentOutput {
    output.summary = truncate_chars(output.summary.trim(), 240);
    output.findings = normalize_short_list(output.findings, 3, 240);
    output.seat_evidence = normalize_short_list(output.seat_evidence, 6, 240);
    output.print_instruction = truncate_chars(output.print_instruction.trim(), 240);
    output.observation_context = truncate_chars(output.observation_context.trim(), 240);
    output.trigger_decision = normalize_trigger_decision(&output.trigger_decision);
    output
}

fn normalize_course_analysis(mut analysis: AgentCourseAnalysis, today: &str) -> AgentCourseAnalysis {
    analysis.summary = truncate_chars(analysis.summary.trim(), 240);
    analysis.findings = normalize_findings(std::mem::take(&mut analysis.findings), today);
    let (active, stale) = split_active_memory(std::mem::take(&mut analysis.standing_context), today);
    analysis.standing_context = active;
    analysis.archived_context =
        consolidate_archive(std::mem::take(&mut analysis.archived_context), stale);
    analysis.seat.assignment = truncate_chars(analysis.seat.assignment.trim(), 80);
    analysis.seat.evidence = normalize_short_list(analysis.seat.evidence, 6, 280);
    let mut seen_prints = HashSet::new();
    analysis.print_candidates.retain(|candidate| {
        !candidate.filename.trim().is_empty() && seen_prints.insert(candidate.filename.clone())
    });
    analysis.print_candidates.truncate(12);
    for candidate in &mut analysis.print_candidates {
        candidate.reason = truncate_chars(candidate.reason.trim(), 240);
    }
    analysis
}

/// Heading used for expired memories that reached the archive without the model
/// having grouped them yet. The model is asked to re-file them into proper
/// groups on the next round; until then they live here so nothing is lost.
const ARCHIVE_FALLBACK_LABEL: &str = "過去の項目";

/// Splits the model's continuing memories by expiry: active items (empty or
/// future `expires_at`) stay in the live list; items whose `expires_at` has
/// passed are returned as `stale` text to be folded into the consolidated
/// archive. Trims/dedupes; an unparseable expiry is treated as no expiry (kept
/// active). No item cap.
fn split_active_memory(
    produced: Vec<StandingMemory>,
    today: &str,
) -> (Vec<StandingMemory>, Vec<String>) {
    let today_date = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").ok();
    let is_expired = |item: &StandingMemory| {
        match (
            today_date,
            chrono::NaiveDate::parse_from_str(item.expires_at.get(..10).unwrap_or(""), "%Y-%m-%d")
                .ok(),
        ) {
            (Some(today), Some(expiry)) => expiry < today,
            _ => false,
        }
    };

    let mut active = Vec::new();
    let mut stale = Vec::new();
    let mut seen = HashSet::new();
    for mut item in produced {
        item.text = truncate_chars(item.text.trim(), 280);
        item.expires_at = item.expires_at.trim().to_string();
        if item.text.is_empty() || !seen.insert(item.text.clone()) {
            continue;
        }
        if is_expired(&item) {
            stale.push(item.text);
        } else {
            active.push(item);
        }
    }
    (active, stale)
}

/// Normalizes the model's consolidated past-memory groups and folds in any
/// freshly-expired `fallback_items` the model didn't group itself. Groups with
/// the same label are merged, items are trimmed and deduped within a group, and
/// empty groups are dropped. There is no cap — size is bounded by the model
/// consolidating related items into compact sub-entries rather than by a limit.
fn consolidate_archive(groups: Vec<ArchivedGroup>, fallback_items: Vec<String>) -> Vec<ArchivedGroup> {
    fn insert(result: &mut Vec<ArchivedGroup>, label: &str, item: String) {
        let item = truncate_chars(item.trim(), 280);
        if item.is_empty() {
            return;
        }
        if let Some(group) = result.iter_mut().find(|group| group.label == label) {
            if !group.items.iter().any(|existing| existing == &item) {
                group.items.push(item);
            }
        } else {
            result.push(ArchivedGroup {
                label: label.to_string(),
                items: vec![item],
            });
        }
    }

    let mut result: Vec<ArchivedGroup> = Vec::new();
    for group in groups {
        let label = truncate_chars(group.label.trim(), 80);
        let label = if label.is_empty() {
            ARCHIVE_FALLBACK_LABEL.to_string()
        } else {
            label
        };
        for item in group.items {
            insert(&mut result, &label, item);
        }
    }
    for item in fallback_items {
        insert(&mut result, ARCHIVE_FALLBACK_LABEL, item);
    }
    result.retain(|group| !group.items.is_empty());
    result
}

/// Trims/dedupes 確認事項 and drops any past their `expires_at` (auto-expire,
/// like memory). No item cap — the list is bounded by relevance, not a number.
fn normalize_findings(findings: Vec<Finding>, today: &str) -> Vec<Finding> {
    let today_date = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").ok();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for mut finding in findings {
        finding.text = truncate_chars(finding.text.trim(), 280);
        finding.expires_at = finding.expires_at.trim().to_string();
        if finding.text.is_empty() || !seen.insert(finding.text.clone()) {
            continue;
        }
        if let (Some(today), Some(expiry)) = (
            today_date,
            chrono::NaiveDate::parse_from_str(finding.expires_at.get(..10).unwrap_or(""), "%Y-%m-%d")
                .ok(),
        ) {
            if expiry < today {
                continue;
            }
        }
        out.push(finding);
    }
    out
}

/// Drops acknowledgements whose finding the model no longer produces, so the
/// set stays bounded. An ack persists exactly while its finding keeps appearing.
fn prune_acknowledged_findings(status: &mut CourseAutomationStatus) {
    let present: HashSet<String> = status
        .analysis
        .findings
        .iter()
        .map(|finding| finding.text.clone())
        .collect();
    status
        .acknowledged_findings
        .retain(|text| present.contains(text));
}

fn normalize_short_list(values: Vec<String>, max_items: usize, max_chars: usize) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let value = truncate_chars(value.trim(), max_chars);
        if value.is_empty() || !seen.insert(value.clone()) {
            continue;
        }
        normalized.push(value);
        if normalized.len() >= max_items {
            break;
        }
    }
    normalized
}

fn proactive_notification_body(analysis: &AgentCourseAnalysis) -> String {
    let mut lines = Vec::new();
    if !analysis.summary.trim().is_empty() {
        lines.push(analysis.summary.trim().to_string());
    }
    lines.extend(
        analysis
            .findings
            .iter()
            .take(3)
            .map(|item| format!("・{}", item.text)),
    );
    truncate_chars(&lines.join("\n"), 600)
}

fn document_notification_key(analysis: &DocumentAnalysis) -> String {
    format!("{}:{}", analysis.id, analysis.fingerprint)
}

fn material_source_fingerprint(
    file: &crate::luna_parser::LunaMaterialFile,
) -> Result<String, String> {
    sha256_json(&json!({
        "fileName": file.file_name,
        "displayName": file.display_name,
        "objectName": file.object_name,
        "resourceId": file.resource_id,
        "materialId": file.material_id,
        "fileType": file.file_type,
        "externalUrl": file.external_url,
    }))
}

fn should_refresh_summary(
    previous_summary: &str,
    pending_count: usize,
    agent_requests_immediate_summary: bool,
) -> bool {
    previous_summary.trim().is_empty()
        || agent_requests_immediate_summary
        || pending_count >= FULL_SUMMARY_NEW_ITEM_THRESHOLD
}

async fn process_print_candidates(
    downloaded: &[(String, String, String, PathBuf, String)],
    candidates: &[PrintCandidate],
    previous: &CourseAutomationStatus,
    approved_categories: &[String],
) -> Vec<PrintResult> {
    let mut results = Vec::new();
    for candidate in candidates {
        let category = candidate.category.trim().to_string();
        if candidate.confidence < PRINT_CONFIDENCE_THRESHOLD {
            results.push(PrintResult {
                filename: candidate.filename.clone(),
                status: "skipped_low_confidence".into(),
                detail: format!("confidence={:.2}", candidate.confidence),
                category,
                ..Default::default()
            });
            continue;
        }
        let Some((_, _, filename, path, _)) =
            downloaded.iter().find(|(_, _, filename, path, _)| {
                filename == &candidate.filename
                    || path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name == candidate.filename)
            })
        else {
            results.push(PrintResult {
                filename: candidate.filename.clone(),
                status: "not_found".into(),
                detail: "Agent が指定したダウンロード済みファイルを特定できません".into(),
                category,
                ..Default::default()
            });
            continue;
        };
        let path_text = path.to_string_lossy().to_string();
        if previous.print_results.iter().any(|item| {
            item.status == "printed"
                && (item.path == path_text || item.filename == candidate.filename)
        }) {
            results.push(PrintResult {
                filename: filename.clone(),
                path: path_text,
                status: "already_printed".into(),
                detail: "同じ SenseA 履歴で印刷済みです".into(),
                category,
            });
            continue;
        }
        // Gate by per-category approval: an un-approved type waits for the user
        // to confirm once; an approved type prints automatically from then on.
        let approved = !category.is_empty()
            && approved_categories
                .iter()
                .any(|item| item.trim() == category);
        if !approved {
            results.push(PrintResult {
                filename: filename.clone(),
                path: path_text,
                status: "needs_confirmation".into(),
                detail: "このタイプの印刷を許可すると、以降の同種ファイルは自動で印刷されます".into(),
                category,
            });
            continue;
        }
        results.push(print_one(filename, path, path_text, category).await);
    }
    results
}

/// Sends one already-located file to the printer and maps the outcome to a
/// `PrintResult`. Shared by the automatic path and the manual confirm command.
async fn print_one(filename: &str, path: &Path, path_text: String, category: String) -> PrintResult {
    let path = path.to_path_buf();
    let result = tauri::async_runtime::spawn_blocking(move || print_verified(&path)).await;
    match result {
        Ok(Ok(detail)) => PrintResult {
            filename: filename.to_string(),
            path: path_text,
            status: "printed".into(),
            detail,
            category,
        },
        Ok(Err(error)) => PrintResult {
            filename: filename.to_string(),
            path: path_text,
            status: "error".into(),
            detail: error,
            category,
        },
        Err(error) => PrintResult {
            filename: filename.to_string(),
            path: path_text,
            status: "error".into(),
            detail: format!("印刷タスク失敗: {}", error),
            category,
        },
    }
}

fn print_verified(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Err("印刷対象ファイルが存在しません".into());
    }
    let metadata =
        std::fs::metadata(path).map_err(|error| format!("ファイル検証失敗: {}", error))?;
    if metadata.len() == 0 {
        return Err("空のファイルは印刷できません".into());
    }
    #[cfg(target_os = "windows")]
    {
        let printer = Command::new("powershell")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg("(Get-CimInstance -ClassName Win32_Printer -Filter 'Default = $true').Name")
            .output()
            .map_err(|error| format!("既定プリンタの確認に失敗: {}", error))?;
        if !printer.status.success() {
            return Err(format!(
                "既定プリンタが確認できません: {}",
                String::from_utf8_lossy(&printer.stderr).trim()
            ));
        }
        let printer_name = String::from_utf8_lossy(&printer.stdout).trim().to_string();
        if printer_name.is_empty() {
            return Err("既定プリンタが設定されていません".into());
        }
        let path_arg = path.to_string_lossy().replace('\'', "''");
        let output = Command::new("powershell")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(format!(
                "Start-Process -FilePath '{}' -Verb Print -PassThru -Wait | Out-Null",
                path_arg
            ))
            .output()
            .map_err(|error| format!("印刷コマンド起動失敗: {}", error))?;
        if !output.status.success() {
            return Err(format!(
                "印刷受付失敗: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(format!("既定プリンタ {} に送信しました", printer_name))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let printer = Command::new("lpstat")
            .arg("-d")
            .output()
            .map_err(|error| format!("既定プリンタの確認に失敗: {}", error))?;
        if !printer.status.success() {
            return Err(format!(
                "既定プリンタが確認できません: {}",
                String::from_utf8_lossy(&printer.stderr).trim()
            ));
        }
        let output = Command::new("lp")
            .arg(path)
            .output()
            .map_err(|error| format!("印刷コマンド起動失敗: {}", error))?;
        if !output.status.success() {
            return Err(format!(
                "印刷受付失敗: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let response = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if response.is_empty() {
            return Err("印刷コマンドから受付確認を取得できませんでした".into());
        }
        Ok(response)
    }
}

fn load_student_profile(db: &Database) -> Value {
    let profile = db
        .get_data_cache("student_profile")
        .ok()
        .flatten()
        .and_then(|(raw, _)| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| json!({}));
    context::compact_student_profile(&profile)
}

fn load_view(db: &Database, luna_id: &str, course_name: &str) -> CourseAutomationView {
    CourseAutomationView {
        config: load_config(db, luna_id, course_name),
        status: load_status(db, luna_id, course_name),
    }
}

fn load_config(db: &Database, luna_id: &str, course_name: &str) -> CourseAutomationConfig {
    load_json(db, &config_key(luna_id)).unwrap_or_else(|| {
        CourseAutomationConfig::new(luna_id.to_string(), course_name.to_string())
    })
}

fn load_status(db: &Database, luna_id: &str, course_name: &str) -> CourseAutomationStatus {
    load_json(db, &status_key(luna_id)).unwrap_or_else(|| CourseAutomationStatus {
        luna_id: luna_id.to_string(),
        course_name: course_name.to_string(),
        ..Default::default()
    })
}

fn load_json<T: for<'de> Deserialize<'de>>(db: &Database, key: &str) -> Option<T> {
    db.get_data_cache(key)
        .ok()
        .flatten()
        .and_then(|(raw, _)| serde_json::from_str(&raw).ok())
}

fn save_json<T: Serialize>(db: &Database, key: &str, value: &T) -> Result<(), String> {
    let raw = serde_json::to_string(value).map_err(|error| error.to_string())?;
    db.save_data_cache(key, &raw)
}

fn save_status_and_emit(
    app: &AppHandle,
    db: &Database,
    status: &CourseAutomationStatus,
) -> Result<(), String> {
    save_json(db, &status_key(&status.luna_id), status)?;
    app.emit("course-automation-updated", status)
        .map_err(|error| error.to_string())
}

fn config_key(luna_id: &str) -> String {
    format!("{}{}", CONFIG_PREFIX, luna_id)
}

fn status_key(luna_id: &str) -> String {
    format!("{}{}", STATUS_PREFIX, luna_id)
}

fn sha256_json<T: Serialize>(value: &T) -> Result<String, String> {
    let raw = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(raw)))
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, character) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&text[start..start + offset + character.len_utf8()]);
                }
            }
            _ => {}
        }
    }
    None
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_fenced_response() {
        let raw = "```json\n{\"summary\":\"ok\"}\n```";
        assert_eq!(extract_json_object(raw), Some("{\"summary\":\"ok\"}"));
    }

    #[test]
    fn extracts_first_complete_json_without_trailing_model_text() {
        let raw = "{\"summary\":\"ok\",\"nested\":{\"text\":\"brace } in string\"}}\n追加説明";
        assert_eq!(
            extract_json_object(raw),
            Some("{\"summary\":\"ok\",\"nested\":{\"text\":\"brace } in string\"}}")
        );
    }

    #[test]
    fn config_defaults_to_disabled_and_full_monitoring() {
        let config = CourseAutomationConfig::new("C1".into(), "Course".into());
        assert!(!config.enabled);
        assert!(config.monitor_materials);
        assert!(config.monitor_announcements);
        assert!(config.monitor_assignments);
        assert!(config.analyze_all);
        assert!(config.auto_print);
    }

    #[test]
    fn document_fingerprint_changes_with_content() {
        let mut document = AnalysisDocument {
            kind: "material".into(),
            title: "座席表".into(),
            filename: "seats.xlsx".into(),
            path: "/tmp/seats.xlsx".into(),
            content: "A-1".into(),
            source_fingerprint: "remote-v1".into(),
            load_error: String::new(),
            images: Vec::new(),
        };
        let before = document_fingerprint(&document).expect("fingerprint");
        document.content = "A-2".into();
        let after = document_fingerprint(&document).expect("fingerprint");
        assert_ne!(before, after);
    }

    #[test]
    fn previous_document_analysis_matches_stable_identity() {
        let document = AnalysisDocument {
            kind: "announcement".into(),
            title: "座席変更".into(),
            filename: String::new(),
            path: String::new(),
            content: "new content".into(),
            source_fingerprint: "announcement-v1".into(),
            load_error: String::new(),
            images: Vec::new(),
        };
        let prior = DocumentAnalysis {
            id: document_id(&document),
            status: "done".into(),
            summary: "old summary".into(),
            ..Default::default()
        };
        let status = CourseAutomationStatus {
            document_analyses: vec![prior],
            ..Default::default()
        };
        assert_eq!(
            previous_document_analysis(&status, &document).map(|item| item.summary.as_str()),
            Some("old summary")
        );
    }

    #[test]
    fn unchanged_remote_source_reuses_existing_download() {
        let path = std::env::temp_dir().join(format!("course-plus-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"cached").expect("write cached file");
        let artifacts = vec![CourseArtifactRecord {
            id: artifact_id("material", "notes.pdf"),
            kind: "material".into(),
            title: "Week 1".into(),
            filename: "notes.pdf".into(),
            path: path.to_string_lossy().to_string(),
            status: "downloaded".into(),
            source_fingerprint: "remote-v1".into(),
            ..Default::default()
        }];

        assert_eq!(
            reusable_artifact_path(&artifacts, "material", "Week 1", "notes.pdf"),
            Some((path.clone(), "remote-v1".into()))
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn legacy_analysis_migrates_to_persistent_artifact_ledger() {
        let path = std::env::temp_dir().join(format!("course-plus-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"cached").expect("write cached file");
        let mut status = CourseAutomationStatus {
            document_analyses: vec![DocumentAnalysis {
                kind: "material".into(),
                title: "Week 1".into(),
                filename: "notes.pdf".into(),
                path: path.to_string_lossy().to_string(),
                status: "done".into(),
                source_fingerprint: "remote-v1".into(),
                ..Default::default()
            }],
            ..Default::default()
        };

        migrate_legacy_artifacts(&mut status);
        assert_eq!(status.artifacts.len(), 1);
        assert_eq!(status.artifacts[0].status, "downloaded");
        assert_eq!(status.artifacts[0].path, path.to_string_lossy());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn legacy_downloaded_file_path_migrates_and_is_reused() {
        let path = std::env::temp_dir().join(format!("course-plus-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"cached").expect("write cached file");
        let mut status = CourseAutomationStatus {
            downloaded_files: vec![path.to_string_lossy().to_string()],
            ..Default::default()
        };

        migrate_legacy_artifacts(&mut status);

        assert_eq!(status.artifacts.len(), 1);
        assert_eq!(status.artifacts[0].kind, "legacy");
        assert_eq!(
            reusable_artifact_path(
                &status.artifacts,
                "material",
                "Week 1",
                path.file_name().and_then(|name| name.to_str()).unwrap()
            )
            .map(|item| item.0),
            Some(path.clone())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn legacy_success_is_reused_and_migrated_without_retry() {
        let path = std::env::temp_dir().join(format!("course-plus-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"cached").expect("write cached file");
        let prior = DocumentAnalysis {
            id: "legacy-id".into(),
            kind: "material".into(),
            title: "Week 1".into(),
            filename: "notes.pdf".into(),
            path: path.to_string_lossy().to_string(),
            status: "done".into(),
            summary: "persisted".into(),
            ..Default::default()
        };
        let status = CourseAutomationStatus {
            document_analyses: vec![prior.clone()],
            ..Default::default()
        };
        let document = AnalysisDocument {
            kind: "material".into(),
            title: "Week 1".into(),
            filename: "notes.pdf".into(),
            path: path.to_string_lossy().to_string(),
            content: "cached".into(),
            source_fingerprint: "remote-v1".into(),
            load_error: String::new(),
            images: Vec::new(),
        };

        let matched = previous_document_analysis(&status, &document).expect("legacy match");
        let migrated =
            migrate_successful_analysis(matched, &document, "content-fingerprint".into());
        assert_eq!(migrated.summary, "persisted");
        assert_eq!(migrated.source_fingerprint, "remote-v1");
        assert_eq!(migrated.id, document_id(&document));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn new_source_version_is_appended_without_deleting_success_history() {
        let mut analyses = vec![DocumentAnalysis {
            id: "version-1".into(),
            status: "done".into(),
            summary: "old success".into(),
            ..Default::default()
        }];
        upsert_document_analysis(
            &mut analyses,
            DocumentAnalysis {
                id: "version-2".into(),
                status: "done".into(),
                summary: "new success".into(),
                ..Default::default()
            },
        );

        assert_eq!(analyses.len(), 2);
        assert!(analyses.iter().any(|item| item.summary == "old success"));
        assert!(analyses.iter().any(|item| item.summary == "new success"));
    }

    #[test]
    fn final_summary_input_uses_delta_and_excludes_ledger_metadata() {
        let previous = AgentCourseAnalysis {
            summary: "compressed working memory".into(),
            standing_context: vec![StandingMemory {
                text: "keep this context".into(),
                expires_at: String::new(),
                urgent: false,
            }],
            ..Default::default()
        };
        let delta = DocumentAnalysis {
            id: "new-success".into(),
            fingerprint: "must-not-be-sent".into(),
            path: "/must/not/be/sent.pdf".into(),
            status: "done".into(),
            summary: "new context".into(),
            ..Default::default()
        };
        let student = json!({"studentNumber": "1234"});
        let input = context::SummaryInput {
            current_local_time: "2026-06-15 10:00".into(),
            course_id: "course",
            course_name: "Course",
            student: &student,
            previous_course_analysis: &previous,
            new_or_changed_documents: vec![context::CompactAnalysis::from(&delta)],
        };
        let serialized = serde_json::to_string(&input).expect("serialize compact input");

        assert!(serialized.contains("compressed working memory"));
        assert!(serialized.contains("new-success"));
        assert!(!serialized.contains("must-not-be-sent"));
        assert!(!serialized.contains("/must/not/be/sent.pdf"));
    }

    #[test]
    fn normalizes_model_output_before_persisting_it() {
        let analysis = normalize_course_analysis(
            AgentCourseAnalysis {
                summary: "a".repeat(400),
                findings: [
                    "same",
                    "same",
                    &"b".repeat(400),
                    "third",
                    "fourth",
                    "fifth",
                    "sixth",
                    "seventh",
                    "eighth",
                ]
                .into_iter()
                .map(|text| Finding {
                    text: text.to_string(),
                    ..Default::default()
                })
                .collect(),
                standing_context: (0..20)
                    .map(|index| StandingMemory {
                        text: format!("context-{index}"),
                        expires_at: String::new(),
                        urgent: false,
                    })
                    .collect(),
                seat: SeatConclusion {
                    assignment: "s".repeat(100),
                    evidence: (0..10).map(|index| format!("evidence-{index}")).collect(),
                    confidence: 0.9,
                },
                ..Default::default()
            },
            "2026-06-17",
        );

        assert_eq!(analysis.summary.chars().count(), 240);
        // No cap any more: 8 unique survive ("same" deduped). Exceeding the old
        // limit of 6 proves there is no item cap; the 400-char one is kept too.
        assert_eq!(analysis.findings.len(), 8);
        assert_eq!(analysis.standing_context.len(), 20);
        assert_eq!(analysis.seat.assignment.chars().count(), 80);
        assert_eq!(analysis.seat.evidence.len(), 6);
    }

    #[test]
    fn split_active_memory_separates_expired_from_active() {
        let (active, stale) = split_active_memory(
            vec![
                StandingMemory {
                    text: "expired".into(),
                    expires_at: "2026-06-10".into(),
                    urgent: false,
                },
                StandingMemory {
                    text: "today still valid".into(),
                    expires_at: "2026-06-17".into(),
                    urgent: false,
                },
                StandingMemory {
                    text: "future".into(),
                    expires_at: "2026-12-01".into(),
                    urgent: false,
                },
                StandingMemory {
                    text: "standing rule".into(),
                    expires_at: String::new(),
                    urgent: false,
                },
            ],
            "2026-06-17",
        );

        let active_texts: Vec<&str> = active.iter().map(|item| item.text.as_str()).collect();
        assert_eq!(active_texts, vec!["today still valid", "future", "standing rule"]);
        assert_eq!(stale, vec!["expired"]);
    }

    #[test]
    fn consolidate_archive_merges_labels_and_folds_stale() {
        let groups = vec![
            ArchivedGroup {
                label: "完了した課題".into(),
                items: vec!["第1回レポート".into(), "第1回レポート".into()],
            },
            ArchivedGroup {
                label: "完了した課題".into(),
                items: vec!["第2回レポート".into()],
            },
            ArchivedGroup {
                label: "終了したイベント".into(),
                items: vec![],
            },
        ];
        let archive = consolidate_archive(groups, vec!["第3回レポート".into()]);

        // Same-label groups merge and items dedupe; the empty group is dropped.
        assert_eq!(archive.len(), 2);
        let assignments = &archive[0];
        assert_eq!(assignments.label, "完了した課題");
        assert_eq!(assignments.items, vec!["第1回レポート", "第2回レポート"]);
        // The freshly-expired memory lands under the fallback heading.
        assert_eq!(archive[1].label, ARCHIVE_FALLBACK_LABEL);
        assert_eq!(archive[1].items, vec!["第3回レポート"]);
    }

    #[test]
    fn standing_memory_accepts_legacy_string_form() {
        let parsed: Vec<StandingMemory> =
            serde_json::from_str(r#"["legacy", {"text": "new", "expiresAt": "2026-09-01"}]"#)
                .expect("deserialize mixed standing context");
        assert_eq!(parsed[0].text, "legacy");
        assert_eq!(parsed[0].expires_at, "");
        assert_eq!(parsed[1].text, "new");
        assert_eq!(parsed[1].expires_at, "2026-09-01");
    }

    #[test]
    fn successful_download_identity_is_reused_even_if_remote_metadata_changes() {
        let path = std::env::temp_dir().join(format!("course-plus-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"persisted").expect("write cached file");
        let artifacts = vec![CourseArtifactRecord {
            id: artifact_id("material", "notes.pdf"),
            kind: "material".into(),
            title: "Week 1".into(),
            filename: "notes.pdf".into(),
            path: path.to_string_lossy().to_string(),
            status: "downloaded".into(),
            source_fingerprint: "first-success".into(),
            ..Default::default()
        }];

        let reused = reusable_artifact_path(&artifacts, "material", "Renamed Week", "notes.pdf")
            .expect("successful download must persist");
        assert_eq!(reused, (path.clone(), "first-success".into()));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn failed_artifact_is_retried_and_replaced_by_success() {
        let mut artifacts = vec![failed_artifact(
            "material",
            "Week 1",
            "notes.pdf",
            "remote-v1",
            "temporary",
        )];
        assert!(reusable_artifact_path(&artifacts, "material", "Week 1", "notes.pdf").is_none());

        upsert_artifact(
            &mut artifacts,
            CourseArtifactRecord {
                id: artifact_id("material", "notes.pdf"),
                kind: "material".into(),
                title: "Week 1".into(),
                filename: "notes.pdf".into(),
                status: "downloaded".into(),
                ..Default::default()
            },
        );
        assert_eq!(artifacts.len(), 1);
        assert!(artifacts[0].error.is_empty());
    }

    #[test]
    fn retry_failure_does_not_delete_download_success_history() {
        let mut artifacts = vec![CourseArtifactRecord {
            id: artifact_id("material", "notes.pdf"),
            kind: "material".into(),
            title: "Week 1".into(),
            filename: "notes.pdf".into(),
            path: "/missing/notes.pdf".into(),
            status: "downloaded".into(),
            source_fingerprint: "first-success".into(),
            ..Default::default()
        }];

        upsert_artifact(
            &mut artifacts,
            failed_artifact(
                "material",
                "Week 1",
                "notes.pdf",
                "remote-v2",
                "temporary retry failure",
            ),
        );

        assert_eq!(artifacts.len(), 2);
        assert!(artifacts.iter().any(|item| item.status == "downloaded"));
        assert!(artifacts.iter().any(|item| item.status == "error"));
    }

    #[test]
    fn later_failure_cannot_overwrite_successful_analysis() {
        let mut analyses = vec![DocumentAnalysis {
            id: "same-version".into(),
            status: "done".into(),
            summary: "persisted success".into(),
            ..Default::default()
        }];
        upsert_document_analysis(
            &mut analyses,
            DocumentAnalysis {
                id: "same-version".into(),
                status: "error".into(),
                error: "temporary read failure".into(),
                ..Default::default()
            },
        );

        assert_eq!(analyses.len(), 1);
        assert_eq!(analyses[0].status, "done");
        assert_eq!(analyses[0].summary, "persisted success");
    }

    #[test]
    fn successful_analysis_does_not_read_file_again() {
        let path =
            std::env::temp_dir().join(format!("course-plus-{}.unsupported", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"already analyzed").expect("write cached file");
        let source_fingerprint = "persisted-source".to_string();
        let document = AnalysisDocument {
            kind: "material".into(),
            title: "Week 1".into(),
            filename: "notes.unsupported".into(),
            path: path.to_string_lossy().to_string(),
            content: String::new(),
            source_fingerprint: source_fingerprint.clone(),
            load_error: String::new(),
            images: Vec::new(),
        };
        let previous = CourseAutomationStatus {
            document_analyses: vec![DocumentAnalysis {
                id: document_id(&document),
                status: "done".into(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let documents = build_analysis_documents(
            &[(
                "material".into(),
                "Week 1".into(),
                "notes.unsupported".into(),
                path.clone(),
                source_fingerprint,
            )],
            &previous,
        );
        assert_eq!(documents.len(), 1);
        assert!(documents[0].load_error.is_empty());
        assert!(documents[0].content.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn failed_version_is_replaced_when_retry_succeeds() {
        let mut analyses = vec![DocumentAnalysis {
            id: "same-version".into(),
            status: "error".into(),
            error: "temporary".into(),
            ..Default::default()
        }];
        upsert_document_analysis(
            &mut analyses,
            DocumentAnalysis {
                id: "same-version".into(),
                status: "done".into(),
                summary: "recovered".into(),
                ..Default::default()
            },
        );

        assert_eq!(analyses.len(), 1);
        assert_eq!(analyses[0].status, "done");
        assert_eq!(analyses[0].summary, "recovered");
    }

    #[test]
    fn comprehensive_summary_waits_for_enough_new_items() {
        assert!(!should_refresh_summary("existing context", 3, false));
        assert!(should_refresh_summary("existing context", 4, false));
        assert!(should_refresh_summary("existing context", 1, true));
        assert!(should_refresh_summary("", 1, false));
    }

    #[test]
    fn normalizes_agent_trigger_decision() {
        assert_eq!(normalize_trigger_decision(" immediate "), "immediate");
        assert_eq!(normalize_trigger_decision("OBSERVE"), "observe");
        assert_eq!(normalize_trigger_decision("anything else"), "routine");
    }

    #[test]
    fn printed_results_are_persistent_and_only_failures_are_retryable() {
        let printed = PrintResult {
            filename: "handout.pdf".into(),
            path: "/tmp/handout.pdf".into(),
            status: "printed".into(),
            detail: "accepted".into(),
            ..Default::default()
        };
        let merged = merge_print_results(
            std::slice::from_ref(&printed),
            vec![PrintResult {
                filename: "handout.pdf".into(),
                path: "/tmp/handout.pdf".into(),
                status: "error".into(),
                detail: "later error".into(),
                ..Default::default()
            }],
        );
        assert_eq!(merged, vec![printed]);
        assert!(!has_retryable_print_failure(&merged));
        assert!(has_retryable_print_failure(&[PrintResult {
            status: "error".into(),
            ..Default::default()
        }]));
        assert!(!has_retryable_print_failure(&[PrintResult {
            status: "skipped_low_confidence".into(),
            ..Default::default()
        }]));
    }

    #[test]
    fn print_candidates_persist_across_new_summaries() {
        let merged = merge_print_candidates(
            &[PrintCandidate {
                filename: "old.pdf".into(),
                reason: "persist".into(),
                confidence: 0.9,
                ..Default::default()
            }],
            vec![PrintCandidate {
                filename: "new.pdf".into(),
                reason: "new".into(),
                confidence: 0.95,
                ..Default::default()
            }],
        );
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|item| item.filename == "old.pdf"));
        assert!(merged.iter().any(|item| item.filename == "new.pdf"));
    }

    #[test]
    fn retryable_failure_count_includes_only_current_failures() {
        let status = CourseAutomationStatus {
            artifacts: vec![
                CourseArtifactRecord {
                    status: "downloaded".into(),
                    ..Default::default()
                },
                CourseArtifactRecord {
                    status: "error".into(),
                    ..Default::default()
                },
            ],
            document_analyses: vec![
                DocumentAnalysis {
                    status: "done".into(),
                    ..Default::default()
                },
                DocumentAnalysis {
                    status: "error".into(),
                    ..Default::default()
                },
            ],
            print_results: vec![
                PrintResult {
                    status: "printed".into(),
                    ..Default::default()
                },
                PrintResult {
                    status: "not_found".into(),
                    ..Default::default()
                },
            ],
            pending_notification_ids: vec!["notify-retry".into()],
            pending_seat_notification: true,
            ..Default::default()
        };

        assert_eq!(retryable_failure_count(&status), 5);
    }

    #[test]
    fn proactive_notification_identity_changes_with_document_version() {
        let mut analysis = DocumentAnalysis {
            id: "announcement".into(),
            fingerprint: "v1".into(),
            ..Default::default()
        };
        assert_eq!(
            document_notification_key(&analysis),
            "announcement:v1".to_string()
        );

        analysis.fingerprint = "v2".into();
        assert_eq!(
            document_notification_key(&analysis),
            "announcement:v2".to_string()
        );
    }

    #[test]
    fn proactive_notification_body_is_compact_and_actionable() {
        let body = proactive_notification_body(&AgentCourseAnalysis {
            summary: "Act now".into(),
            findings: ["First", "Second", "Third", "Must not appear"]
                .into_iter()
                .map(|text| Finding {
                    text: text.to_string(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });

        assert!(body.starts_with("Act now\n・First"));
        assert!(body.contains("・Third"));
        assert!(!body.contains("Must not appear"));
        assert!(body.chars().count() <= 600);
    }
}
