use crate::db::{AiScheduleItem, Database};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const DETECTIVE_DOUBTS_KEY: &str = "detective_doubts_v1";
const DETECTIVE_RESULTS_KEY: &str = "detective_case_results_v1";
const DETECTIVE_INCLUDED_KEY: &str = "detective_included_courses_v1";
const DETECTIVE_MEMORY_KEY: &str = "detective_memory_v1";
/// Per-course campaign bible key prefix (+ course_key).
const DETECTIVE_CAMPAIGN_PREFIX: &str = "detective_campaign_v1:";
/// Per-chapter cached case key prefix (+ "{course_key}:{live_id}"). Each Live
/// note is one chapter; its generated case is cached so replays are instant and
/// new lectures only add NEW chapters (existing ones are never regenerated).
const DETECTIVE_CHAPTER_PREFIX: &str = "detective_chapter_v1:";
/// Per-course alignment cache (+ course_key): maps a Live note id to the 授業計画
/// 第N回 it was matched to BY CONTENT. Survives world rebuilds (alignment is
/// about what the lecture covered, not the campaign dressing).
const DETECTIVE_ALIGN_PREFIX: &str = "detective_align_v1:";
/// Per-chapter extracted knowledge-point checklist (+ "{course_key}:{live_id}").
/// Drives the chapter generator + the coverage check. Built once per Live note
/// (cheap AI call) and cached; survives world rebuilds.
const DETECTIVE_KNOWLEDGE_PREFIX: &str = "detective_knowledge_v1:";

/// A chapter MUST cover at least this many knowledge points across its
/// evidence cards + testimony statements, on top of covering every
/// `must_cover: true` point.
const COVERAGE_MIN_POINTS: usize = 10;
const LIVE_EXCERPT_MAX_CHARS: usize = 8000;
const LIVE_EXCERPT_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Minimum number of lies (one per testimony act) across a whole chapter.
/// Bumped from 2 → 3 so a chapter forces enough testimony beats to cover the
/// lecture's surface area properly.
const SESSION_LIES_MIN: usize = 3;

/// A chapter (1 case) is split into mixed investigation / testimony acts.
/// 6–8 acts so one lecture's content actually gets the room to breathe; an
/// investigation act bears 2–4 evidence cards and a testimony act 3–5
/// statements (see the prompt).
const ACTS_MIN: usize = 6;
const ACTS_MAX: usize = 8;
/// Each investigation act must reveal at least this many evidence cards.
const INVESTIGATION_EV_MIN: usize = 2;
/// Each testimony act must contain at least this many statements (still ONE lie).
const TESTIMONY_STMT_MIN: usize = 3;

const EXAM_KEYWORDS: &[&str] = &[
    "試験",
    "テスト",
    "小テスト",
    "中間",
    "期末",
    "レポート試験",
    "範囲",
    "持ち込み",
    "exam",
    "quiz",
    "midterm",
    "final",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveContext {
    pub courses: Vec<DetectiveCourse>,
    pub review_queue: Vec<DetectiveReviewItem>,
    pub recent_results: Vec<DetectiveCaseResult>,
    pub generated_at: i64,
    /// Course keys the user has explicitly opted into Detective. Empty by
    /// default — when empty the frontend shows the selection screen.
    pub included_course_keys: Vec<String>,
    /// Cross-session memory shown in the Detective HQ home — busted topics,
    /// pending doubts, recently-used evidence.
    pub memory: DetectiveMemory,
    /// Per-course campaign bibles (世界観 layer) that have already been
    /// generated and cached. Empty entries are omitted — the title screen
    /// uses these to show the world/tagline/meta-progress for selected courses.
    #[serde(default)]
    pub campaigns: Vec<DetectiveCampaign>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveCourse {
    pub name: String,
    pub key: String,
    pub live_records: Vec<DetectiveLiveRecord>,
    pub exam_signals: Vec<DetectiveSignal>,
    pub schedule_items: Vec<AiScheduleItem>,
    pub latest_at: i64,
    pub doubts: Vec<DetectiveDoubt>,
    pub recent_results: Vec<DetectiveCaseResult>,
    pub case_type: String,
    pub readiness: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveCase {
    pub id: String,
    pub course_key: String,
    pub course_name: String,
    pub title: String,
    pub case_type: String,
    pub difficulty: u8,
    pub briefing: String,
    pub evidence: Vec<DetectiveCaseEvidence>,
    pub final_question: String,
    #[serde(default)]
    pub testimony: Vec<DetectiveTestimony>,
    /// Narrative prologue (3–5 sentences) that sets up *why* the case is being
    /// investigated and *who* is being cross-examined. Drawn from course content.
    #[serde(default)]
    pub scenario: String,
    /// Witness name as written by the AI (e.g. 「ミナミ」). Japanese only.
    #[serde(default)]
    pub witness_name: String,
    /// Witness's role / relation (e.g. 「同級生」「先輩」「ゼミ仲間」).
    #[serde(default)]
    pub witness_role: String,
    #[serde(default)]
    pub generation_mode: String,
    #[serde(default)]
    pub generation_note: String,
    /// The 授業計画 第N回 this chapter's Live note was matched to BY CONTENT
    /// (0 = could not be determined). Drives chapter numbering + finale.
    #[serde(default)]
    pub session_num: u8,
    /// The knowledge-point checklist this chapter was built to cover.
    #[serde(default)]
    pub knowledge_points: Vec<KnowledgePoint>,
    /// Where each covered knowledge point landed (evidence id or testimony id).
    /// Validated against `knowledge_points` so the player is guaranteed to be
    /// exposed to every must-cover point.
    #[serde(default)]
    pub coverage: Vec<CoverageEntry>,
    /// The chapter's acts (幕) — a mix of investigation and testimony beats
    /// that the player walks through in order. The primary play structure;
    /// `evidence` is the shared Court Record pool referenced by these acts.
    #[serde(default)]
    pub acts: Vec<DetectiveAct>,
}

/// One act (幕) of a chapter. Either an INVESTIGATION beat (the player reads
/// evidence cards revealed here — the teaching moment) or a TESTIMONY beat (a
/// witness testifies with one planted lie — the testing moment). Story prose
/// in `narrative` advances the chapter's main plot between beats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveAct {
    pub id: String,
    /// 1-based 幕番号.
    pub index: u8,
    /// "investigation" | "testimony".
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub location: String,
    /// Story beat shown when the act opens (advances the chapter's main plot).
    #[serde(default)]
    pub narrative: String,
    /// Whether this act subtly seeds the campaign's overarching 暗线.
    #[serde(default)]
    pub seeds_meta: bool,
    /// Investigation acts: the ids (into `case.evidence`) revealed here.
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    /// Testimony acts: who is on the stand for this beat.
    #[serde(default)]
    pub witness_name: String,
    #[serde(default)]
    pub witness_role: String,
    /// Testimony acts: the witness's statements (one planted lie).
    #[serde(default)]
    pub testimony: Vec<DetectiveTestimony>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveTestimony {
    pub id: String,
    pub text: String,
    pub is_false: bool,
    pub key_evidence_id: String,
    /// Substrings inside `text` that should be highlighted in the UI — the
    /// terms most likely to either prove or break the statement.
    #[serde(default)]
    pub highlights: Vec<String>,
    /// What the witness says when the player presses (ゆさぶる) this statement.
    /// Used to teach: for TRUE statements it elaborates the concept; for the
    /// FALSE statement it nudges without giving the answer away.
    #[serde(default)]
    pub press_response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveCaseEvidence {
    pub id: String,
    pub source_id: String,
    pub source_type: String,
    pub source: String,
    pub title: String,
    pub date: String,
    pub excerpt: String,
    pub source_path: String,
    pub source_url: String,
    pub information_type: String,
    pub person_category_cd: String,
    pub category_cd: String,
}

/// One distilled knowledge point extracted from a Live note. Drives chapter
/// generation (the AI must place each must-cover point somewhere in the case)
/// + coverage validation. Cached per Live note so it's built once.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgePoint {
    pub id: String,
    /// Short Japanese label (8–30 chars) — the topic headline.
    pub label: String,
    /// One-sentence gist — what the learner must actually know.
    #[serde(default)]
    pub gist: String,
    /// True for load-bearing concepts the teacher emphasised — must be covered.
    #[serde(default)]
    pub must_cover: bool,
}

/// Records which evidence card or testimony statement carries a given
/// knowledge point. `placement` is either an evidence id (e.g. "e3") or a
/// testimony id (e.g. "a4t2").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageEntry {
    pub point_id: String,
    pub placement: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveLiveRecord {
    pub id: String,
    pub filename: String,
    pub path: String,
    pub course_name: String,
    pub downloaded_at: i64,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveSignal {
    pub id: String,
    pub source_id: String,
    pub title: String,
    pub date: String,
    pub category: String,
    pub source: String,
    pub course_info: String,
    pub source_url: String,
    pub information_type: String,
    pub person_category_cd: String,
    pub category_cd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveDoubt {
    pub id: String,
    pub course_name: String,
    pub note: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    pub created_at: i64,
    pub due_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveCaseResult {
    pub id: String,
    pub case_id: String,
    pub course_key: String,
    pub course_name: String,
    pub case_title: String,
    pub case_type: String,
    #[serde(default)]
    pub selected_evidence_ids: Vec<String>,
    pub relation: String,
    pub deduction: String,
    pub closed_at: i64,
    pub confidence: u8,
}

/// One chapter of a course's campaign — backed by a single Live note.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveChapterInfo {
    pub live_id: String,
    /// 1-based chapter number (oldest lecture = chapter 1).
    pub index: u8,
    /// Chapter title — from the cached case if generated, else 「第N章」.
    pub title: String,
    /// True when this chapter's case has already been generated + cached.
    pub generated: bool,
    /// True when the player has an archived result for this chapter.
    pub played: bool,
    /// Best (latest) confidence 1–5, or 0 if never played.
    pub best_confidence: u8,
    pub played_at: i64,
    /// True for a PLANNED-but-not-yet-delivered lecture (per 授業計画) that has
    /// no Live note yet — shown locked so the player sees the full arc length.
    #[serde(default)]
    pub locked: bool,
    /// True when `index` is a content-confirmed 第N回 (vs. a not-yet-aligned note).
    #[serde(default)]
    pub aligned: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveReviewItem {
    pub id: String,
    pub course_key: String,
    pub course_name: String,
    pub reason: String,
    pub priority: u8,
    pub due_at: i64,
}

/// Cross-session memory: drives content continuity across Detective sessions.
/// Persisted as JSON under DETECTIVE_MEMORY_KEY.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveMemory {
    /// Topics the player has consistently busted — AI should de-emphasise.
    #[serde(default)]
    pub mastered: Vec<MemoryItem>,
    /// Topics the player failed on — AI should re-emphasise next session.
    #[serde(default)]
    pub mistakes: Vec<MemoryItem>,
    /// Evidence ids used in the last several sessions; AI tries to vary picks.
    #[serde(default)]
    pub recent_evidence_titles: Vec<String>,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryItem {
    /// A short tag describing the topic — derived from the busted/failed lie text.
    pub topic: String,
    /// Course this came from (best-effort).
    #[serde(default)]
    pub course_name: String,
    pub at: i64,
}

/// The narrative "bible" for one course. Persisted per course under
/// `DETECTIVE_CAMPAIGN_PREFIX + course_key`. This is the long-running story
/// layer: a world (derived from the lecture subject matter), a recurring cast,
/// and an overarching hidden mystery that every chapter (= one live note)
/// advances. Generated once, then read + nudged forward by each case.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectiveCampaign {
    pub course_key: String,
    pub course_name: String,
    /// Short era/genre label, derived from the course content
    /// (e.g. 「18世紀アメリカ独立戦争」「確率が支配する街」).
    #[serde(default)]
    pub world_label: String,
    /// 世界観 — 2–4 sentence premise that recasts the course as a mystery world.
    #[serde(default)]
    pub setting: String,
    /// One-line hook for the campaign.
    #[serde(default)]
    pub tagline: String,
    /// Recurring cast tied to the meta-mystery.
    #[serde(default)]
    pub cast: Vec<CampaignCharacter>,
    /// The overarching hidden thread spanning all chapters.
    #[serde(default)]
    pub meta_mystery: String,
    /// How far the meta-plot has been revealed (0–100).
    #[serde(default)]
    pub meta_progress: u8,
    /// Planted / paid narrative threads.
    #[serde(default)]
    pub threads: Vec<CampaignThread>,
    /// Staged reveals of the overarching 暗线 — unlocked as meta_progress rises.
    #[serde(default)]
    pub meta_arc: Vec<CampaignRevelation>,
    /// The grand epilogue — shown once the campaign reaches 100% (all chapters
    /// cleared). Conclusively resolves the meta-mystery.
    #[serde(default)]
    pub finale: String,
    /// Chapters (live notes) already turned into cases.
    #[serde(default)]
    pub chapters: Vec<CampaignChapter>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignCharacter {
    pub name: String,
    /// 役回り — e.g. ライバル, 相棒, 黒幕候補, 証人.
    pub role: String,
    /// Relationship to the protagonist / the meta-mystery.
    #[serde(default)]
    pub bond: String,
    /// 2–3 sentences of concrete background — where they came from, what
    /// they've already done, what shaped them. NOT a label, an actual mini-bio.
    #[serde(default)]
    pub background: String,
    /// What this character WANTS right now (in-fiction goal). Every action they
    /// take in the campaign should be traceable to this.
    #[serde(default)]
    pub motivation: String,
    /// What they stand to LOSE if the truth comes out / things go sideways.
    /// Drives why they help, evade, or lie.
    #[serde(default)]
    pub stake: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignThread {
    pub id: String,
    pub summary: String,
    /// "planted" | "paid".
    pub status: String,
}

/// One staged reveal of the campaign's overarching 暗线. The bible defines an
/// ordered arc (faint hint → deepening → twist → final truth); each entry
/// unlocks once meta_progress reaches its threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignRevelation {
    /// 1-based stage in the arc.
    pub stage: u8,
    /// meta_progress (0–100) at which this reveal unlocks.
    pub threshold: u8,
    pub title: String,
    pub reveal: String,
    #[serde(default)]
    pub unlocked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignChapter {
    /// Live note id / path used as the chapter source.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub played_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CampaignBibleDraft {
    world_label: Option<String>,
    setting: Option<String>,
    tagline: Option<String>,
    meta_mystery: Option<String>,
    cast: Option<Vec<CampaignCharacterDraft>>,
    meta_arc: Option<Vec<CampaignArcDraft>>,
    finale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CampaignArcDraft {
    title: Option<String>,
    reveal: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CampaignCharacterDraft {
    name: Option<String>,
    role: Option<String>,
    bond: Option<String>,
    background: Option<String>,
    motivation: Option<String>,
    stake: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetectiveAiCaseDraft {
    title: Option<String>,
    case_type: Option<String>,
    difficulty: Option<u8>,
    briefing: Option<String>,
    scenario: Option<String>,
    witness_name: Option<String>,
    witness_role: Option<String>,
    final_question: Option<String>,
    /// Which 授業計画 第N回 this Live note matches (content alignment); 0/absent
    /// when the model can't tell.
    session_num: Option<i32>,
    acts: Option<Vec<DetectiveAiActDraft>>,
    /// Where each knowledge point landed. Hard-validated against the must-cover
    /// list + COVERAGE_MIN_POINTS in `apply_ai_case_draft`.
    coverage: Option<Vec<CoverageDraft>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoverageDraft {
    point_id: Option<String>,
    placement: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetectiveAiActDraft {
    id: Option<String>,
    kind: Option<String>,
    title: Option<String>,
    location: Option<String>,
    narrative: Option<String>,
    seeds_meta: Option<bool>,
    /// Investigation acts: the evidence cards discovered in this beat.
    evidence: Option<Vec<DetectiveAiEvidenceDraft>>,
    /// Testimony acts: who testifies + their statements.
    witness_name: Option<String>,
    witness_role: Option<String>,
    testimony: Option<Vec<DetectiveAiTestimonyDraft>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetectiveAiTestimonyDraft {
    id: Option<String>,
    text: Option<String>,
    is_false: Option<bool>,
    key_evidence_id: Option<String>,
    #[serde(default)]
    highlights: Option<Vec<String>>,
    press_response: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetectiveAiEvidenceDraft {
    id: String,
    title: Option<String>,
    body: Option<String>,
    source_ref: Option<String>,
}

#[derive(Default)]
struct CourseBuilder {
    name: String,
    key: String,
    live_records: Vec<DetectiveLiveRecord>,
    exam_signals: Vec<DetectiveSignal>,
    schedule_items: Vec<AiScheduleItem>,
    doubts: Vec<DetectiveDoubt>,
    recent_results: Vec<DetectiveCaseResult>,
    latest_at: i64,
}

#[tauri::command]
pub fn detective_get_context(db: tauri::State<'_, Database>) -> Result<DetectiveContext, String> {
    build_context(&db)
}

/// Generate (or fetch the cached) campaign bible for a single course. With
/// `force: true` the world is rebuilt from scratch (e.g. to backfill the
/// meta-arc / finale onto an older campaign) while carrying over the player's
/// meta_progress and played chapters; reveals are re-unlocked to match.
#[tauri::command]
pub async fn detective_generate_campaign(
    db: tauri::State<'_, Database>,
    course_key: String,
    force: Option<bool>,
) -> Result<DetectiveCampaign, String> {
    let context = build_context(&db)?;
    let course = context
        .courses
        .iter()
        .find(|course| course.key == course_key)
        .ok_or_else(|| "選んだ科目にはライブメモ／通知が見つかりませんでした。".to_string())?;

    if !force.unwrap_or(false) {
        return ensure_campaign(&db, course).await;
    }

    let prev = load_campaign(&db, &course.key);
    let input = build_evidence_input(course);
    let mut fresh = generate_campaign_bible(course.key.clone(), course.name.clone(), input).await?;
    if let Some(prev) = prev {
        fresh.meta_progress = prev.meta_progress;
        fresh.chapters = prev.chapters;
        fresh.created_at = prev.created_at;
        for rev in fresh.meta_arc.iter_mut() {
            if rev.threshold <= fresh.meta_progress {
                rev.unlocked = true;
            }
        }
    }
    save_campaign(&db, &fresh);
    // The world changed — drop cached chapter cases so they regenerate inside
    // the new world on next play. Progress/clear status survive (they live in
    // case results + the campaign, keyed by the stable case_id, not the case).
    for record in &course.live_records {
        let _ = db.delete_data_cache(&chapter_case_key(&course.key, &record.id));
    }
    Ok(fresh)
}

/// List a course's chapters — one per Live note, oldest lecture first. Each
/// chapter reports whether it has been generated and/or played. New lectures
/// surface here automatically as fresh (ungenerated) chapters.
#[tauri::command]
pub fn detective_get_chapters(
    db: tauri::State<'_, Database>,
    course_key: String,
) -> Result<Vec<DetectiveChapterInfo>, String> {
    let context = build_context(&db)?;
    let course = context
        .courses
        .iter()
        .find(|course| course.key == course_key)
        .ok_or_else(|| "選んだ科目にはライブメモが見つかりませんでした。".to_string())?;
    let results = load_case_results(&db);
    let align = load_align(&db, &course_key);

    let mut records: Vec<&DetectiveLiveRecord> = course.live_records.iter().collect();
    records.sort_by(|a, b| a.downloaded_at.cmp(&b.downloaded_at)); // download order

    // Build one row per captured Live note, numbered by its content-aligned
    // 第N回 (not by capture order — robust to missing/online lectures).
    let mut rows: Vec<DetectiveChapterInfo> = Vec::new();
    for record in &records {
        let case_id = format!("detective:chapter:{course_key}:{}", record.id);
        let cached = load_chapter_case(&db, &course_key, &record.id);
        let result = results.iter().find(|r| r.case_id == case_id);
        let session_num = align.get(&record.id).copied().unwrap_or(0).max(0) as u8;
        let aligned = session_num > 0;
        let title = cached
            .as_ref()
            .map(|c| c.title.clone())
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| {
                if aligned {
                    format!("第{session_num}章")
                } else {
                    "未整理の記録".to_string()
                }
            });
        rows.push(DetectiveChapterInfo {
            live_id: record.id.clone(),
            index: session_num,
            title,
            generated: cached.is_some(),
            played: result.is_some(),
            best_confidence: result.map(|r| r.confidence).unwrap_or(0),
            played_at: result.map(|r| r.closed_at).unwrap_or(0),
            locked: false,
            aligned,
        });
    }
    // Aligned rows first (by 回), then not-yet-aligned notes (by capture order).
    rows.sort_by(|a, b| match (a.aligned, b.aligned) {
        (true, true) => a.index.cmp(&b.index),
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => std::cmp::Ordering::Equal,
    });

    // Append OFFLINE 授業計画 回 that no captured note covers, as locked 未配信
    // rows — only the future tail, so existing-but-unaligned notes aren't
    // double-counted. Online 回 are intentionally omitted (they bear no Live).
    let offline = offline_session_nums(&db, &course_key);
    if !offline.is_empty() {
        let covered: std::collections::HashSet<i32> = rows
            .iter()
            .filter(|r| r.aligned)
            .map(|r| r.index as i32)
            .collect();
        let uncovered: Vec<i32> = offline
            .iter()
            .copied()
            .filter(|n| !covered.contains(n))
            .collect();
        let show = offline.len().saturating_sub(records.len());
        let start = uncovered.len().saturating_sub(show);
        for &n in &uncovered[start..] {
            rows.push(DetectiveChapterInfo {
                live_id: String::new(),
                index: n.max(0) as u8,
                title: format!("第{n}章（未配信）"),
                generated: false,
                played: false,
                best_confidence: 0,
                played_at: 0,
                locked: true,
                aligned: true,
            });
        }
    }
    Ok(rows)
}

/// Generate (or fetch the cached) case for one chapter = one Live note. The
/// case is set inside the course's campaign world. Cached after first build so
/// replays are instant; pass `force: true` to regenerate from scratch.
#[tauri::command]
pub async fn detective_generate_chapter(
    db: tauri::State<'_, Database>,
    course_key: String,
    live_id: String,
    force: Option<bool>,
) -> Result<DetectiveCase, String> {
    if !force.unwrap_or(false) {
        if let Some(cached) = load_chapter_case(&db, &course_key, &live_id) {
            return Ok(cached);
        }
    }
    let context = build_context(&db)?;
    let course = context
        .courses
        .iter()
        .find(|course| course.key == course_key)
        .ok_or_else(|| "選んだ科目にはライブメモが見つかりませんでした。".to_string())?;
    let input = build_chapter_input(course, &live_id)
        .ok_or_else(|| "そのライブメモが見つかりませんでした。".to_string())?;

    let mut case = build_case(course);
    case.id = format!("detective:chapter:{course_key}:{live_id}");
    case.course_key = course_key.clone();

    let memory = load_memory(&db);
    let campaign = ensure_campaign(&db, course).await.ok();
    let syllabus = planned_sessions(&db, &course_key);
    // Pre-extract a knowledge-point checklist from this Live note (cached per
    // live_id) so chapter generation is driven by — and validated against —
    // the actual concepts the lecture covered. The Live note's own structure
    // (箇条書き / 「今日のポイント」 / 「まとめ」) is the primary signal; no
    // separate topic hint needed here.
    let knowledge = ensure_knowledge_points(&db, course, &live_id, "").await?;
    let case = generate_case_with_ai(case, input, memory, campaign, syllabus, knowledge).await?;
    save_chapter_case(&db, &course_key, &live_id, &case);
    // Persist the content-derived 回 alignment so the chapter list can number
    // chapters by their真の第N回 (robust to missing/online lectures).
    if case.session_num > 0 {
        let mut align = load_align(&db, &course_key);
        align.insert(live_id.clone(), case.session_num as i32);
        save_align(&db, &course_key, &align);
    }
    Ok(case)
}

#[tauri::command]
pub fn detective_save_doubts(
    db: tauri::State<'_, Database>,
    doubts: Vec<DetectiveDoubt>,
) -> Result<(), String> {
    let json = serde_json::to_string(&doubts).map_err(|e| format!("Detective doubts JSON: {e}"))?;
    db.save_data_cache(DETECTIVE_DOUBTS_KEY, &json)
}

#[tauri::command]
pub fn detective_save_included_courses(
    db: tauri::State<'_, Database>,
    included: Vec<String>,
) -> Result<(), String> {
    let mut clean: Vec<String> = included
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    clean.sort();
    clean.dedup();
    let json =
        serde_json::to_string(&clean).map_err(|e| format!("Detective included JSON: {e}"))?;
    db.save_data_cache(DETECTIVE_INCLUDED_KEY, &json)
}

#[tauri::command]
pub fn detective_save_case_result(
    db: tauri::State<'_, Database>,
    result: DetectiveCaseResult,
) -> Result<Vec<DetectiveCaseResult>, String> {
    let anchor_key = result
        .course_key
        .split(',')
        .next()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(|s| s.to_string());

    // Persist the result FIRST so progress recompute below counts this chapter.
    let mut results = load_case_results(&db);
    results.retain(|item| item.id != result.id);
    let result_for_campaign = result.clone();
    results.insert(0, result);
    results.sort_by(|a, b| b.closed_at.cmp(&a.closed_at));
    results.truncate(80);
    let json =
        serde_json::to_string(&results).map_err(|e| format!("Detective case result JSON: {e}"))?;
    db.save_data_cache(DETECTIVE_RESULTS_KEY, &json)?;

    // Advance the campaign meta-plot. Progress = fraction of the course's
    // OFFLINE 授業計画 回 (the ones that actually produce a Live note) that have
    // an aligned, cleared chapter — so 100% fires only when the final offline
    // 回 is done. Online/on-demand 回 are excluded. Dedup chapters by case_id.
    if let Some(anchor_key) = anchor_key {
        if let Some(mut campaign) = load_campaign(&db, &anchor_key) {
            let is_new = !campaign
                .chapters
                .iter()
                .any(|ch| ch.id == result_for_campaign.case_id);
            if is_new {
                let title = if result_for_campaign.case_title.trim().is_empty() {
                    format!("第{}章", campaign.chapters.len() + 1)
                } else {
                    result_for_campaign.case_title.trim().to_string()
                };
                campaign.chapters.push(CampaignChapter {
                    id: result_for_campaign.case_id.clone(),
                    title,
                    summary: truncate_chars(result_for_campaign.deduction.trim(), 160),
                    played_at: result_for_campaign.closed_at,
                });
            }
            // Recompute against the 授業計画 (offline 回). Fall back to a coarse
            // chapters-cleared / captured-Live ratio when no syllabus exists.
            campaign.meta_progress = campaign_progress_pct(&db, &anchor_key)
                .or_else(|| {
                    build_context(&db).ok().and_then(|ctx| {
                        ctx.courses
                            .iter()
                            .find(|c| c.key == anchor_key)
                            .map(|c| c.live_records.len())
                            .filter(|n| *n > 0)
                            .map(|total| {
                                let cleared = campaign.chapters.len().min(total);
                                ((cleared as f32 / total as f32) * 100.0).round() as u8
                            })
                    })
                })
                .unwrap_or_else(|| campaign.meta_progress.saturating_add(12))
                .min(100);
            // Unlock any reveal whose threshold the player has now reached.
            for rev in campaign.meta_arc.iter_mut() {
                if !rev.unlocked && rev.threshold <= campaign.meta_progress {
                    rev.unlocked = true;
                }
            }
            campaign.updated_at = crate::db::epoch_secs();
            save_campaign(&db, &campaign);
        }
    }

    Ok(results)
}

/// Record per-session outcomes that drive cross-session continuity. The
/// frontend calls this once the player closes a session (win or loss).
#[tauri::command]
pub fn detective_save_memory_outcome(
    db: tauri::State<'_, Database>,
    busted_topics: Vec<String>,
    missed_topics: Vec<String>,
    course_name: String,
    evidence_titles: Vec<String>,
) -> Result<(), String> {
    let mut memory = load_memory(&db);
    let now = crate::db::epoch_secs();
    let course = course_name.trim().to_string();

    for topic in busted_topics {
        let t = topic.trim();
        if t.is_empty() {
            continue;
        }
        // Drop from mistakes if it was there (player has now resolved it).
        memory.mistakes.retain(|m| m.topic != t);
        memory.mastered.push(MemoryItem {
            topic: t.to_string(),
            course_name: course.clone(),
            at: now,
        });
    }
    for topic in missed_topics {
        let t = topic.trim();
        if t.is_empty() {
            continue;
        }
        // Remove any older mastered claim — clearly not mastered now.
        memory.mastered.retain(|m| m.topic != t);
        memory.mistakes.push(MemoryItem {
            topic: t.to_string(),
            course_name: course.clone(),
            at: now,
        });
    }
    for title in evidence_titles {
        let t = title.trim();
        if !t.is_empty() {
            memory.recent_evidence_titles.push(t.to_string());
        }
    }

    // Sliding windows: keep recency, drop ancient items.
    if memory.mastered.len() > 40 {
        let cut = memory.mastered.len() - 40;
        memory.mastered.drain(0..cut);
    }
    if memory.mistakes.len() > 24 {
        let cut = memory.mistakes.len() - 24;
        memory.mistakes.drain(0..cut);
    }
    if memory.recent_evidence_titles.len() > 60 {
        let cut = memory.recent_evidence_titles.len() - 60;
        memory.recent_evidence_titles.drain(0..cut);
    }
    memory.updated_at = now;
    save_memory(&db, &memory);
    Ok(())
}

fn build_context(db: &Database) -> Result<DetectiveContext, String> {
    let doubts = load_doubts(db);
    let results = load_case_results(db);
    let mut courses: HashMap<String, CourseBuilder> = HashMap::new();

    let mut schedule_items = Vec::new();
    if let Ok(Some((result, _))) = db.get_ai_schedule_cache() {
        schedule_items.extend(result.current_week);
        schedule_items.extend(result.next_week);
    }
    for item in schedule_items {
        if item.course_name.trim().is_empty() {
            continue;
        }
        let course = ensure_course(&mut courses, &item.course_name);
        course.schedule_items.push(item);
    }

    if let Ok(Some(snap)) = db.get_snapshot_state() {
        if let Ok(raw) = db.build_raw_data(
            &snap.current_week_label,
            &snap.next_week_label,
            snap.luna_communities,
        ) {
            for row in raw
                .kgc_entries_current
                .iter()
                .chain(raw.kgc_entries_next.iter())
            {
                if !row.name.trim().is_empty() {
                    ensure_course(&mut courses, &row.name);
                }
            }
            for row in raw.luna_courses.iter() {
                if !row.name.trim().is_empty() {
                    ensure_course(&mut courses, &row.name);
                }
            }
        }
    }

    let mut records = crate::commands::list_downloads();
    if !records.iter().any(is_live_record) {
        records = crate::commands::scan_download_dir();
    }
    for record in records
        .into_iter()
        .filter(|record| record.file_exists && is_live_record(record))
    {
        let course_name = if record.course_name.trim().is_empty() {
            infer_course_name_from_record(&record)
        } else {
            record.course_name.clone()
        };
        let course = ensure_course(&mut courses, &course_name);
        course.latest_at = course.latest_at.max(record.downloaded_at);
        course.live_records.push(DetectiveLiveRecord {
            id: record.id,
            filename: record.filename,
            path: record.path.clone(),
            course_name: course.name.clone(),
            downloaded_at: record.downloaded_at,
            excerpt: live_excerpt(&record.path),
        });
    }

    let signals = collect_exam_signals(db);
    for signal in signals {
        if let Some(key) = match_signal_course_key(&signal, &courses) {
            if let Some(course) = courses.get_mut(&key) {
                course.latest_at = course.latest_at.max(date_score(&signal.date));
                course.exam_signals.push(signal);
            }
        } else if !signal.course_info.trim().is_empty() {
            let course = ensure_course(&mut courses, &signal.course_info);
            course.exam_signals.push(signal);
        }
    }

    for doubt in doubts {
        let course = ensure_course(&mut courses, &doubt.course_name);
        course.doubts.push(doubt);
    }

    for result in results.iter().cloned() {
        let course = ensure_course(&mut courses, &result.course_name);
        course.latest_at = course.latest_at.max(result.closed_at);
        course.recent_results.push(result);
    }

    let mut out: Vec<DetectiveCourse> = courses
        .into_values()
        .filter(|course| {
            !course.live_records.is_empty()
                || !course.exam_signals.is_empty()
                || !course.doubts.is_empty()
                || !course.recent_results.is_empty()
        })
        .map(|mut course| {
            course
                .live_records
                .sort_by(|a, b| b.downloaded_at.cmp(&a.downloaded_at));
            course.exam_signals = dedupe_signals(course.exam_signals);
            course
                .recent_results
                .sort_by(|a, b| b.closed_at.cmp(&a.closed_at));
            course.recent_results.truncate(5);
            let case_type = course_case_type(&course).to_string();
            let readiness = readiness_text(&course);
            DetectiveCourse {
                name: course.name,
                key: course.key,
                live_records: course.live_records,
                exam_signals: course.exam_signals,
                schedule_items: course.schedule_items,
                latest_at: course.latest_at,
                doubts: course.doubts,
                recent_results: course.recent_results,
                case_type,
                readiness,
            }
        })
        .collect();

    out.sort_by(|a, b| course_score(b).total_cmp(&course_score(a)));

    let review_queue = build_review_queue(&out);

    let included_course_keys = load_included_courses(db);
    let memory = load_memory(db);

    // Surface any campaign bibles already cached for the visible courses.
    let campaigns: Vec<DetectiveCampaign> = out
        .iter()
        .filter_map(|course| load_campaign(db, &course.key))
        .collect();

    Ok(DetectiveContext {
        courses: out,
        review_queue,
        recent_results: results.into_iter().take(12).collect(),
        generated_at: crate::db::epoch_secs(),
        included_course_keys,
        memory,
        campaigns,
    })
}

/// Build an empty case shell. The AI is responsible for filling every
/// content-bearing field including the evidence cards themselves.
fn build_case(course: &DetectiveCourse) -> DetectiveCase {
    DetectiveCase {
        id: format!("detective:{}:{}", course.key, crate::db::epoch_secs()),
        course_key: course.key.clone(),
        course_name: course.name.clone(),
        title: String::new(),
        case_type: String::new(),
        difficulty: 0,
        briefing: String::new(),
        evidence: Vec::new(),
        final_question: String::new(),
        testimony: Vec::new(),
        scenario: String::new(),
        witness_name: String::new(),
        witness_role: String::new(),
        generation_mode: "ai".to_string(),
        generation_note: String::new(),
        session_num: 0,
        knowledge_points: Vec::new(),
        coverage: Vec::new(),
        acts: Vec::new(),
    }
}

/// Extract a short, meaningful content snippet from an evidence excerpt.
/// Skips metadata lines and bullet markers; returns 18-70 char chunks.
fn content_snippet(excerpt: &str) -> Option<String> {
    let cleaned = excerpt
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('#')
                .trim()
                .trim_start_matches('-')
                .trim_start_matches('*')
                .trim()
                .trim_start_matches('・')
                .trim()
        })
        .filter(|line| !line.is_empty());

    // Try sentence-level splits first.
    for line in cleaned.clone() {
        for sentence in line.split(['。', '．', '！', '？']) {
            let s = sentence.trim();
            let len = s.chars().count();
            if (18..=70).contains(&len) && !looks_like_metadata(s) {
                return Some(s.to_string());
            }
        }
    }
    // Fall back to first line of decent length.
    for line in cleaned {
        let len = line.chars().count();
        if (12..=70).contains(&len) && !looks_like_metadata(line) {
            return Some(line.to_string());
        }
    }
    None
}

fn looks_like_metadata(text: &str) -> bool {
    matches!(
        text,
        "区間ごとの要約" | "全文転写" | "授業コード" | "教員" | "教室" | "時間帯"
    ) || text.starts_with("http")
        || text.starts_with("授業コード:")
        || text.starts_with("教員:")
        || text.starts_with("教室:")
        || text.starts_with("開始:")
        || text.starts_with("終了:")
}

/// Generate a Detective case via the configured AI provider. There is no
/// fallback: every failure mode returns `Err`, which the frontend surfaces.
/// Each step is logged to both `log::` and `eprintln!` so a developer can
/// confirm in `cargo tauri dev` output that the AI was actually called.
async fn generate_case_with_ai(
    mut case: DetectiveCase,
    input: Vec<EvidenceInputEntry>,
    memory: DetectiveMemory,
    campaign: Option<DetectiveCampaign>,
    syllabus: Vec<PlannedSession>,
    knowledge: Vec<KnowledgePoint>,
) -> Result<DetectiveCase, String> {
    let cfg = crate::ai::load_ai_config();
    eprintln!(
        "[detective] generate_case_with_ai: course={} ai_enabled={} input_sources={}",
        case.course_name,
        cfg.ai_enabled,
        input.len()
    );
    log::info!(
        "[detective] generate_case_with_ai start: course={} ai_enabled={}",
        case.course_name,
        cfg.ai_enabled
    );

    if !cfg.ai_enabled {
        return Err(
            "AI is disabled. Detective cases require an AI provider — enable AI in Selah settings."
                .to_string(),
        );
    }

    if input.is_empty() {
        return Err(
            "No Live notes or exam signals are available for this course. Capture a Live session or wait for notifications, then try again.".to_string(),
        );
    }

    let provider = crate::agent_provider::AgentProvider::resolve().map_err(|e| {
        eprintln!("[detective] provider resolve FAILED: {}", e);
        log::warn!("[detective] provider resolve failed: {}", e);
        format!("AI provider unavailable: {e}")
    })?;
    eprintln!("[detective] AI provider resolved");
    log::info!("[detective] AI provider resolved");

    let user_prompt = detective_ai_user_prompt(
        &case,
        &input,
        &memory,
        campaign.as_ref(),
        &syllabus,
        &knowledge,
    );
    let prompt_chars = user_prompt.chars().count();
    eprintln!(
        "[detective] AI prompt ready ({} chars, {} input sources)",
        prompt_chars,
        input.len()
    );
    log::info!("[detective] AI prompt ready ({} chars)", prompt_chars);

    let messages = vec![
        crate::ai::ChatMessage {
            role: "system".to_string(),
            content: detective_ai_system_prompt().to_string(),
            images: Vec::new(),
        },
        crate::ai::ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
            images: Vec::new(),
        },
    ];

    // Use the user-configured max_tokens. `0` means "let provider pick default";
    // providers fall back to 8192 / 32768 — plenty for our JSON shape.
    eprintln!(
        "[detective] AI plan() call dispatching (max_tokens={})",
        cfg.max_tokens
    );
    let raw = provider
        .plan(
            messages,
            cfg.max_tokens,
            0.2,
            "",
            20,
            &format!("detective:{}", case.id),
        )
        .await
        .map_err(|e| {
            eprintln!("[detective] AI plan() FAILED: {}", e);
            log::warn!("[detective] AI plan failed: {}", e);
            format!("AI call failed: {e}")
        })?;

    let raw_chars = raw.chars().count();
    eprintln!("[detective] AI returned {} chars", raw_chars);
    log::info!("[detective] AI raw response: {} chars", raw_chars);
    // Always log the full raw response so the developer terminal shows exactly
    // what the AI produced.
    eprintln!(
        "[detective] AI raw response BEGIN ===\n{}\n=== END raw response",
        raw
    );
    log::debug!("[detective] AI raw: {}", raw);

    let json = extract_json_object(&raw).ok_or_else(|| {
        let preview = truncate_chars(raw.trim(), 300);
        eprintln!("[detective] AI returned non-JSON output. Preview: {}", preview);
        log::warn!("[detective] AI returned non-JSON output. Preview: {}", preview);
        if raw.trim().is_empty() {
            "AI returned an empty response. The model may have refused or the request may have been blocked. Check the terminal log for details.".to_string()
        } else {
            format!(
                "AI did not return JSON. The response begins with: \"{preview}\". Try again, or check that the model supports JSON output."
            )
        }
    })?;

    let draft: DetectiveAiCaseDraft = serde_json::from_str(&json).map_err(|e| {
        let preview = truncate_chars(json.trim(), 300);
        eprintln!(
            "[detective] AI JSON parse FAILED: {}. JSON preview: {}",
            e, preview
        );
        log::warn!("[detective] AI JSON parse failed: {}", e);
        format!("AI JSON parse failed: {e}. JSON begins with: \"{preview}\"")
    })?;
    eprintln!("[detective] AI JSON parsed OK");
    log::info!("[detective] AI JSON parsed");

    case = apply_ai_case_draft(case, draft, &input, &knowledge)?;
    eprintln!(
        "[detective] AI case ACCEPTED: testimony={} evidence={} briefing={}chars",
        case.testimony.len(),
        case.evidence.len(),
        case.briefing.chars().count()
    );
    log::info!(
        "[detective] AI case accepted: testimony={} evidence={}",
        case.testimony.len(),
        case.evidence.len()
    );
    Ok(case)
}

/// Generate the campaign bible (世界観 layer) for one course. AI-required.
/// The world/era is DERIVED FROM the lecture subject matter — an American
/// Revolution course yields an 18th-century colonial setting, a statistics
/// course yields a "probability-ruled" world, etc. Generated once per course,
/// then read + advanced by each session.
async fn generate_campaign_bible(
    course_key: String,
    course_name: String,
    input: Vec<EvidenceInputEntry>,
) -> Result<DetectiveCampaign, String> {
    let cfg = crate::ai::load_ai_config();
    eprintln!(
        "[detective] generate_campaign_bible: course={} ai_enabled={} input_sources={}",
        course_name,
        cfg.ai_enabled,
        input.len()
    );
    if !cfg.ai_enabled {
        return Err(
            "AI is disabled. Campaign worlds require an AI provider — enable AI in Selah settings."
                .to_string(),
        );
    }
    if input.is_empty() {
        return Err(
            "この科目にはまだライブメモがありません。先にライブを記録してから世界観を生成してください。"
                .to_string(),
        );
    }

    let provider = crate::agent_provider::AgentProvider::resolve()
        .map_err(|e| format!("AI provider unavailable: {e}"))?;

    let user_prompt = campaign_bible_user_prompt(&course_name, &input);
    eprintln!(
        "[detective] campaign bible prompt ready ({} chars)",
        user_prompt.chars().count()
    );
    let messages = vec![
        crate::ai::ChatMessage {
            role: "system".to_string(),
            content: campaign_bible_system_prompt().to_string(),
            images: Vec::new(),
        },
        crate::ai::ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
            images: Vec::new(),
        },
    ];
    eprintln!(
        "[detective] campaign bible plan() dispatching (max_tokens={})",
        cfg.max_tokens
    );
    let raw = provider
        .plan(
            messages,
            cfg.max_tokens,
            0.5,
            "",
            20,
            &format!("detective-bible:{course_key}"),
        )
        .await
        .map_err(|e| {
            eprintln!("[detective] campaign bible plan() FAILED: {}", e);
            format!("AI call failed: {e}")
        })?;
    eprintln!(
        "[detective] campaign bible raw BEGIN ===\n{}\n=== END raw response",
        raw
    );

    let json = extract_json_object(&raw).ok_or_else(|| {
        let preview = truncate_chars(raw.trim(), 300);
        format!("AI did not return JSON for the campaign bible. Begins with: \"{preview}\"")
    })?;
    let draft: CampaignBibleDraft = serde_json::from_str(&json).map_err(|e| {
        let preview = truncate_chars(json.trim(), 300);
        format!("Campaign bible JSON parse failed: {e}. JSON begins with: \"{preview}\"")
    })?;

    let world_label = draft.world_label.unwrap_or_default().trim().to_string();
    let setting = draft.setting.unwrap_or_default().trim().to_string();
    let tagline = draft.tagline.unwrap_or_default().trim().to_string();
    let meta_mystery = draft.meta_mystery.unwrap_or_default().trim().to_string();
    if world_label.is_empty() || setting.is_empty() || meta_mystery.is_empty() {
        return Err(
            "Campaign bible was missing worldLabel/setting/metaMystery — retry generation."
                .to_string(),
        );
    }
    let cast: Vec<CampaignCharacter> = draft
        .cast
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| {
            let name = c.name.unwrap_or_default().trim().to_string();
            let role = c.role.unwrap_or_default().trim().to_string();
            if name.is_empty() || role.is_empty() {
                return None;
            }
            Some(CampaignCharacter {
                name,
                role,
                bond: c.bond.unwrap_or_default().trim().to_string(),
                background: c.background.unwrap_or_default().trim().to_string(),
                motivation: c.motivation.unwrap_or_default().trim().to_string(),
                stake: c.stake.unwrap_or_default().trim().to_string(),
            })
        })
        .take(3)
        .collect();

    // Build the staged reveal arc: distribute thresholds evenly across 0–100,
    // so the final stage lands at 100 (full payoff).
    let arc_drafts: Vec<CampaignArcDraft> = draft
        .meta_arc
        .unwrap_or_default()
        .into_iter()
        .filter(|a| {
            a.reveal
                .as_deref()
                .map(|r| !r.trim().is_empty())
                .unwrap_or(false)
        })
        .take(4)
        .collect();
    let arc_len = arc_drafts.len().max(1);
    let meta_arc: Vec<CampaignRevelation> = arc_drafts
        .into_iter()
        .enumerate()
        .map(|(i, a)| {
            let stage = (i + 1) as u8;
            let threshold = (((i + 1) as f32 / arc_len as f32) * 100.0).round() as u8;
            CampaignRevelation {
                stage,
                threshold: threshold.min(100),
                title: a
                    .title
                    .unwrap_or_default()
                    .trim()
                    .chars()
                    .take(40)
                    .collect(),
                reveal: a.reveal.unwrap_or_default().trim().to_string(),
                unlocked: false,
            }
        })
        .collect();

    let finale = draft.finale.unwrap_or_default().trim().to_string();

    let now = crate::db::epoch_secs();
    eprintln!(
        "[detective] campaign bible ACCEPTED: world={} cast={} arc={}",
        world_label,
        cast.len(),
        meta_arc.len()
    );
    Ok(DetectiveCampaign {
        course_key,
        course_name,
        world_label,
        setting,
        tagline,
        cast,
        meta_mystery,
        meta_progress: 0,
        threads: Vec::new(),
        meta_arc,
        finale,
        chapters: Vec::new(),
        created_at: now,
        updated_at: now,
    })
}

/// Load the cached campaign for a course, or generate + persist a fresh one.
async fn ensure_campaign(
    db: &Database,
    course: &DetectiveCourse,
) -> Result<DetectiveCampaign, String> {
    if let Some(existing) = load_campaign(db, &course.key) {
        return Ok(existing);
    }
    let input = build_evidence_input(course);
    let campaign = generate_campaign_bible(course.key.clone(), course.name.clone(), input).await?;
    save_campaign(db, &campaign);
    Ok(campaign)
}

/// Extract a knowledge-point checklist from one Live note (cheap AI pass).
/// Live notes typically already contain list-like structures (「今日のポイント」/
/// 「まとめ」/「要点」/箇条書き) — the prompt is wired to USE THEM FIRST and
/// only then augment from prose. Returns ≥12 points so the chapter generator
/// has slack to cover the mandatory 10.
async fn extract_knowledge_points(
    course_key: &str,
    course_name: &str,
    live_topic_hint: &str,
    live_content: &str,
) -> Result<Vec<KnowledgePoint>, String> {
    let cfg = crate::ai::load_ai_config();
    if !cfg.ai_enabled {
        return Err(
            "AI is disabled. Knowledge-point extraction requires an AI provider.".to_string(),
        );
    }
    let provider = crate::agent_provider::AgentProvider::resolve()
        .map_err(|e| format!("AI provider unavailable: {e}"))?;

    let user_prompt = format!(
        r#"Course: {course}
{topic_line}
═══ LIVE LECTURE NOTE (full body) ═══
{content}

═══ TASK ═══
この講義から testable な知識点を網羅的に抽出してください。

抽出の手順:
1. 本文中に既に「今日のポイント」「まとめ」「要点」「ねらい」「キーワード」のような節、または箇条書き（・/-/●/1./2./など）がある場合、それらを **最優先のヒント** として拾う。教員自身が抽出してくれた知識点リストだから。
2. その後、本文の散文部分から、定義・例・対比・分類・数値・固有概念・名前付きプロセス・教員が強調した説明をさらに加える。
3. 事務連絡（提出方法・教室・出欠など）は除外。

各知識点には:
- `id`: "k1", "k2", ... と連番。
- `label`: 8–30字の日本語で「何の論点か」を端的に。
- `gist`: 1文で「学習者が必ず分かるべき核心」を述べる。
- `mustCover`: 教員が中心として扱った概念・定義・分類・代表例 → true。周辺・補助・余談的なら false。

合計 **12〜20件** 出してください（章生成側は 10件以上の被覆を要求する仕様なので、必ず余裕を持って）。
出力は単一の JSON のみ、コードフェンスや前置きなし:

{{
  "points": [
    {{ "id": "k1", "label": "…", "gist": "…", "mustCover": true }},
    {{ "id": "k2", "label": "…", "gist": "…", "mustCover": false }}
  ]
}}"#,
        course = course_name,
        topic_line = if live_topic_hint.is_empty() {
            String::new()
        } else {
            format!("授業計画該当回の topic: {live_topic_hint}\n")
        },
        content = live_content,
    );
    let messages = vec![
        crate::ai::ChatMessage {
            role: "system".to_string(),
            content: "あなたは大学講義のライブメモから testable 知識点を抽出する専門家。出力は単一の JSON オブジェクトのみ。前置き・コードフェンス・余分なテキスト禁止。"
                .to_string(),
            images: Vec::new(),
        },
        crate::ai::ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
            images: Vec::new(),
        },
    ];
    eprintln!(
        "[detective] knowledge-point extract dispatching (max_tokens={})",
        cfg.max_tokens
    );
    let raw = provider
        .plan(
            messages,
            cfg.max_tokens,
            0.3,
            "",
            10,
            &format!("detective-knowledge:{course_key}"),
        )
        .await
        .map_err(|e| {
            eprintln!("[detective] knowledge extract FAILED: {}", e);
            format!("AI call failed: {e}")
        })?;
    eprintln!("[detective] knowledge extract raw ===\n{}\n=== END", raw);

    #[derive(Deserialize)]
    struct PointsEnvelope {
        points: Option<Vec<KnowledgePointDraft>>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct KnowledgePointDraft {
        id: Option<String>,
        label: Option<String>,
        gist: Option<String>,
        must_cover: Option<bool>,
    }

    let json = extract_json_object(&raw).ok_or_else(|| {
        let preview = truncate_chars(raw.trim(), 300);
        format!("Knowledge extraction returned non-JSON. Begins with: \"{preview}\"")
    })?;
    let envelope: PointsEnvelope = serde_json::from_str(&json)
        .map_err(|e| format!("Knowledge points JSON parse failed: {e}"))?;
    let drafts = envelope.points.unwrap_or_default();

    let mut out: Vec<KnowledgePoint> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    for (i, d) in drafts.into_iter().enumerate() {
        let label = d.label.unwrap_or_default().trim().to_string();
        if label.is_empty()
            || looks_like_metadata_leak(&label)
            || looks_like_admin_trivia(&label)
            || looks_like_generic_label(&label)
        {
            continue;
        }
        let id =
            d.id.filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("k{}", i + 1));
        if !seen_ids.insert(id.clone()) {
            continue;
        }
        let gist = d
            .gist
            .unwrap_or_default()
            .trim()
            .chars()
            .take(160)
            .collect::<String>();
        out.push(KnowledgePoint {
            id,
            label: label.chars().take(60).collect(),
            gist,
            must_cover: d.must_cover.unwrap_or(false),
        });
    }
    if out.len() < COVERAGE_MIN_POINTS + 2 {
        return Err(format!(
            "Knowledge extraction yielded only {} usable points; need at least {}",
            out.len(),
            COVERAGE_MIN_POINTS + 2
        ));
    }
    eprintln!(
        "[detective] knowledge extract ACCEPTED: {} points ({} must-cover)",
        out.len(),
        out.iter().filter(|p| p.must_cover).count()
    );
    Ok(out)
}

/// Load or build (cache-through) the knowledge-point checklist for one Live note.
async fn ensure_knowledge_points(
    db: &Database,
    course: &DetectiveCourse,
    live_id: &str,
    topic_hint: &str,
) -> Result<Vec<KnowledgePoint>, String> {
    if let Some(existing) = load_knowledge_points(db, &course.key, live_id) {
        if existing.len() >= COVERAGE_MIN_POINTS {
            return Ok(existing);
        }
    }
    let record = course
        .live_records
        .iter()
        .find(|r| r.id == live_id)
        .ok_or_else(|| "そのライブメモが見つかりませんでした。".to_string())?;
    let content = if record.excerpt.trim().is_empty() {
        "(本文未抽出)".to_string()
    } else {
        record.excerpt.clone()
    };
    let pts = extract_knowledge_points(&course.key, &course.name, topic_hint, &content).await?;
    save_knowledge_points(db, &course.key, live_id, &pts);
    Ok(pts)
}

fn detective_ai_system_prompt() -> &'static str {
    "You author a Japanese Ace-Attorney-style cross-examination case that helps the player STUDY THE TESTABLE LECTURE CONTENT from the supplied Live notes. The case is review study — not a slice-of-life game.\n\nOUTPUT FORMAT — MANDATORY:\n- Output a single JSON object and nothing else.\n- Start your response with `{` and end with `}`.\n- Do NOT wrap in markdown code fences (```).\n- Do NOT add prose, preamble, explanation, comments, or trailing text.\n- Do NOT use a tool call wrapper; just emit raw JSON.\n\nINPUT KINDS:\n- LIVE notes (sourceType=live): the body text is what the teacher actually taught. THIS IS THE PRIMARY SOURCE. Extract testable knowledge from here: concepts, definitions, taxonomies, examples, formulas, theorems, classifications, historical facts, named processes, instructor explanations of meaning.\n- SIGNAL notifications (sourceType=signal): use ONLY for exam-CONTEXT (exam date, exam range, format, allowed materials, number of questions, weighting). Use them to anchor the urgency, NEVER as a topic.\n- DOUBT items (sourceType=doubt): pull the player's previously-flagged knowledge gaps as priority topics.\n\nABSOLUTE FOCUS RULE — every evidence card body, every testimony statement, every press response must be about TESTABLE COURSE KNOWLEDGE. The following CONTENT IS FORBIDDEN regardless of whether it appears in the supplied notes:\n- Administrative trivia: 学籍番号 / ネームカード / レポート提出方法 / 用紙の色・サイズ / 出欠の取り方 / 教室の場所 / 持参物 (pen, USB) / 提出期限の机械的記述 / ファイル命名規則 / Word vs PDF / その他「事務連絡」\n- Class logistics: 休講連絡, 補講日程, 教員の余談, アイスブレイクの内容, 自己紹介, 出席確認のやり方\n- General-knowledge questions that don't tie back to a specific concept the teacher explained\n- Filenames (.md, _live), ISO dates (YYYY-MM-DD), course codes, instructor names, classroom numbers\n- Generic placeholder labels (ライブメモ, 授業ノート, 講義メモ, 本講義の記録)\n- Empty platitudes (重要な内容がある, 記録が残っている, 資料を確認できる)\n- Any fact, number, date, or chapter not literally present in the supplied content\n\nIf the supplied Live notes are MOSTLY administrative and contain little testable content, prefer producing FEWER evidence cards and FEWER lies — quality over quantity. Never invent topics to fill a quota.\n\nCHAPTER SCOPE — a chapter is the WHOLE lecture turned into a play. AIM HIGH: pull as many distinct testable points from the supplied Live note as it supports — definitions, examples, contrasts, numerical claims, classifications, named processes, instructor explanations. A thin chapter (few cards, few statements, single-line teaching) is a FAILURE; depth and breadth are mandatory.\n\nCHAPTER STRUCTURE — you write ONE chapter told in 6–8 ACTS (幕), like a 逆転裁判 episode. Acts ALTERNATE between two kinds:\n- INVESTIGATION act (kind=\"investigation\"): the teaching beat. A `narrative` (2–4 Japanese sentences) advances the plot, and 2–4 `evidence` cards reveal distilled facts from the content. The first act MUST be investigation.\n- TESTIMONY act (kind=\"testimony\"): the testing beat. A `narrative` brings a witness to the stand, and 3–5 `testimony` statements follow with EXACTLY ONE lie (`isFalse: true`). The TRUE statements are NOT filler — each is its own testable concept the player should learn.\n\nKEY CONSTRAINT — teaching before testing: a lie's `keyEvidenceId` MUST point at an evidence card revealed in an EARLIER investigation act. Never test a fact the player has not yet been shown. Provide at least 3 investigation acts and at least 3 testimony acts.\n\n- `scenario` (4–6 Japanese sentences): chapter prologue / hook — situation, witnesses, stakes — anchored in real concepts from the Live notes.\n- Per testimony act: `witnessName` (2–6 Japanese chars, an invented given name e.g. ミナミ/ジュン/ハル — NEVER an instructor name) and `witnessRole` (4–14 chars). Vary witnesses across testimony acts when natural.\n- All testimony in plain Japanese witness speech (〜だ / 〜である / 〜のはず), each statement under 140 characters. The true statements must reference DIFFERENT testable points (not paraphrases of each other).\n- Each evidence `body` is 2–5 sentences: state the fact, then add ONE concrete grounding element (example / counter-example / value / contrast / named instance the teacher used).\n\nFor every testimony statement, also provide:\n- `highlights`: 1–3 keywords COPIED VERBATIM from `text` — the concept name, value, or term to scrutinise.\n- `pressResponse`: 2–3 Japanese sentences. For TRUE statements, USE THIS TO TEACH — start from the concept and drill deeper (definition → concrete example → contrast / common confusion). For the FALSE statement, the witness doubles down for 2–3 sentences (deflect, change the subject, cite an unrelated 'fact'), but never reveals the lie.\n\nNARRATIVE & MOTIVATION: each act's `narrative` is a beat of the chapter's main plot, set in the campaign world's specific historical locus (era / named place / community). **Every character who speaks or acts must have a stated or inferable motive — what they want, what they protect**. The bible's cast comes with 背景/動機/利害; honour those. Witnesses you newly invent for this chapter need a one-sentence backstory + a reason for being on the stand, established in the testimony act's `narrative` before they speak. A witness whose lie has no plausible motive (cover an ally, save reputation, conceal involvement, defend a payoff) is a failure — make the motive shape their tone in `pressResponse`, without ever stating 「私は嘘をついている」. Seed the overarching hidden thread (暗线) with ONE subtle hint in a single early act (mark that act `seedsMeta: true`). Story is the vehicle that makes the testable content stick — never invent testable facts to serve story, and never invent story so thin that characters feel like quiz props."
}

/// Build the AI user prompt. AI reads the raw source content and distills
/// it into N short evidence cards (one-paragraph facts), each tagged with a
/// `sourceRef` pointing back to an input alias (l1/s1/d1) so we can attach
/// the original file path or URL afterwards.
fn detective_ai_user_prompt(
    case: &DetectiveCase,
    input: &[EvidenceInputEntry],
    memory: &DetectiveMemory,
    campaign: Option<&DetectiveCampaign>,
    syllabus: &[PlannedSession],
    knowledge: &[KnowledgePoint],
) -> String {
    fn render(entry: &EvidenceInputEntry, max_chars: Option<usize>) -> String {
        let body = entry.raw_content.replace('\r', "");
        let body = body.trim();
        let content = match max_chars {
            Some(cap) => truncate_chars(body, cap),
            None => body.to_string(),
        };
        let title_line = if entry.source_type == "signal" && !entry.raw_title.trim().is_empty() {
            format!("\n  title: {}", truncate_chars(&entry.raw_title, 120))
        } else {
            String::new()
        };
        format!(
            "- id: {alias}{title_line}\n  content:\n    {content}",
            alias = entry.alias,
            content = content.replace('\n', "\n    ")
        )
    }

    let live_section = collect_section(input, "live", None);
    let signal_section = collect_section(input, "signal", Some(400));
    let doubt_section = collect_section(input, "doubt", Some(280));

    let syllabus_section = if syllabus.is_empty() {
        "(この科目の授業計画は未取得。sessionNum は 0 とすること)".to_string()
    } else {
        syllabus
            .iter()
            .map(|s| {
                let mode = if s.online {
                    "（オンライン）"
                } else {
                    ""
                };
                format!(
                    "- 第{}回{}: {}",
                    s.num,
                    mode,
                    truncate_chars(s.topic.trim(), 90)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let knowledge_section = if knowledge.is_empty() {
        "(知識点リスト未取得)".to_string()
    } else {
        knowledge
            .iter()
            .map(|p| {
                let star = if p.must_cover { "★" } else { " " };
                format!(
                    "- {star} `{}` {} — {}",
                    p.id,
                    p.label,
                    truncate_chars(p.gist.trim(), 90)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let must_cover_ids: Vec<&str> = knowledge
        .iter()
        .filter(|p| p.must_cover)
        .map(|p| p.id.as_str())
        .collect();
    let must_cover_list = if must_cover_ids.is_empty() {
        "(なし)".to_string()
    } else {
        must_cover_ids.join(", ")
    };

    fn collect_section(
        input: &[EvidenceInputEntry],
        source_type: &str,
        max_chars: Option<usize>,
    ) -> String {
        let blocks: Vec<String> = input
            .iter()
            .filter(|e| e.source_type == source_type)
            .map(|e| render(e, max_chars))
            .collect();
        if blocks.is_empty() {
            "(none)".to_string()
        } else {
            blocks.join("\n")
        }
    }

    format!(
        r#"Course: {course}

═══ INPUT SOURCES — read these in full and extract concrete facts ═══

Live lecture notes (PRIMARY — body is what the teacher actually said):
{live}

Notifications (exam format / scope intel):
{signals}

Student's unresolved doubts:
{doubts}

═══ 授業計画 (lecture plan — match THIS lecture to its 第N回) ═══
{syllabus}

═══ KNOWLEDGE POINTS (★ = must-cover; you MUST place each ★ point somewhere in the chapter, and at least {coverage_min} total) ═══
{knowledge}
must-cover ids: {must_cover_list}

═══ CAMPAIGN WORLD (世界観 — this session is a chapter inside it) ═══
{campaign}

═══ MEMORY (player history — drive continuity) ═══
{memory}

═══ TASK ═══
Write ONE CHAPTER of the campaign — a substantial case told in {acts_min}–{acts_max} ACTS (幕). This is the WHOLE lecture turned into a play; aim for breadth + depth, NOT a quick quiz. The chapter plays like a 逆転裁判 episode: the detective ALTERNATES between INVESTIGATION acts (teach concrete facts) and TESTIMONY acts (catch a witness in a lie). Cover every NOTABLE testable point from the Live note — definitions, examples, contrasts, classifications, named processes, numerical claims — spread across the acts. Do not under-deliver.

ACT RHYTHM (mandatory):
- The FIRST act MUST be an investigation act (the player needs evidence before anyone can be cross-examined).
- ALTERNATE: investigation → testimony → investigation → testimony … A testimony act's lie may ONLY be busted with a card revealed in an EARLIER investigation act.
- Provide at least 3 investigation acts and at least 3 testimony acts.

INVESTIGATION ACT — the teaching beat:
- `narrative`: 2–4 Japanese sentences advancing the chapter's main plot (a scene in the campaign world: where the detective goes, who they meet, what they find).
- `evidence`: 2–4 cards (4 preferred when the lecture has the material). Each card = ONE meaty fact, 2–5 Japanese sentences. State the concept, then ground it with a concrete detail from the source: an example, a counter-example, a value, a contrast, or a named instance the teacher actually used. NEVER paste raw source text; rewrite as a clean fact paragraph (e.g. "教員はピジン言語の例として太平洋戦争中のヤシ語を挙げ、語彙は主に英語由来だが文法は太平洋諸語の影響を強く受けると説明した。さらに、ピジンが世代を超えて母語化したものがクレオールであり、両者は安定性で区別されると述べた。"). `title` = 6–30 char topic headline. `sourceRef` = the input alias (l1/s1/d1/...).

TESTIMONY ACT — the testing beat:
- `narrative`: 2–3 Japanese sentences moving the plot to the confrontation. MUST establish: (a) who this witness is (one-sentence background — origin/role in the locus), (b) why they are here / what they want from this encounter, (c) the moment they take the stand. The witness's motive must be inferable from this — it will drive their tone in testimony.
- `witnessName` (2–6 Japanese chars, invented unless reusing a bible cast member) + `witnessRole` (4–14 chars, fits the world's cast / community). Vary witnesses across acts when natural; reuse a bible character when it makes sense for their motivation.
- `testimony`: 3–5 statements spoken by that witness, of which EXACTLY ONE is a lie (`isFalse: true`). The lie CONTRADICTS one already-revealed evidence card; its `keyEvidenceId` is that card's id. The other (true) statements should ALSO be content-anchored — each is a different testable point the player should learn (not filler). All statements in plain Japanese witness speech, each under 140 chars.

CONTINUITY (MEMORY section): RE-EMPHASIZE recently-failed topics — make at least one lie touch one if present. DE-EMPHASIZE recently-mastered topics. Avoid reusing `recent_evidence_titles` verbatim.

WORLD & MOTIVATION (CAMPAIGN WORLD section, if present): every act's `narrative` happens INSIDE that world (its era/place/named locale/cast). The bible above lists each cast member's `背景`/`動機`/`利害` — when you reuse them, their behaviour in this chapter MUST flow from those. Even brand-new witnesses you invent for THIS chapter need a stated motive: in the testimony act's `narrative`, establish WHO this witness is (background sketch in 1 sentence), WHY they are at the scene / why they care, and WHAT they want from the encounter. A witness whose lie has no traceable motive is a failure — the lie should plausibly serve their interests (cover for an ally, protect reputation, conceal involvement, protect a payoff). The motive does NOT need to be revealed to the player as text — it just needs to shape the tone of their testimony and their `pressResponse`. Across the chapter, SEED the overarching hidden thread (暗线) with ONE subtle early hint — set `seedsMeta: true` on the single act that drops it, and keep it a passing detail (not resolved this chapter). Testable content stays exactly as rigorous; the world is dressing for the knowledge, never an excuse to invent facts. If NO world is given, use a neutral study framing — but characters still need motivation.

SESSION ALIGNMENT (授業計画): the Live note above is ONE lecture. Compare its actual content to the 授業計画 list and decide which 第N回 it corresponds to BY CONTENT (topic match), NOT by order. Output that number as top-level `sessionNum`. If the plan is empty or you genuinely cannot tell, use 0. Never guess by position.

COVERAGE (mandatory): for EVERY knowledge point listed above, ensure that the concept actually appears in some evidence card body OR in some testimony statement text. Then emit a `coverage` array mapping each covered point to where it landed. RULES:
- Every must-cover (★) point MUST appear, no exception. A chapter missing even one ★ point is rejected and the player has to retry.
- Total distinct points covered (★ + non-★ combined) MUST be ≥ {coverage_min}. So plan the chapter to fit at least {coverage_min} of the listed points.
- `placement` is either an evidence id ("e3") or a testimony id ("a4t3" — = act `a4`, statement `t3` in its testimony list).
- One point per coverage entry; you may cover one piece of content with multiple cards/statements but each coverage entry references a single placement.

Produce exactly this JSON shape (no comments, no extra fields):

{{
  "caseType": one of ["Exam Signal Case", "Concept Web Case", "Doubt Repair Case", "Contradiction Case", "Missing Link Case"],
  "difficulty": integer 1..5 (PREFER 2–3),
  "sessionNum": integer (the 第N回 this lecture matches by content; 0 if unsure),
  "briefing": "Japanese paragraph 60–220 chars summarising what this chapter investigates and what concepts it spans",
  "scenario": "4–6 Japanese sentences of narrative prologue: the chapter's hook, the situation, who's involved, what's at stake — set in the campaign world and rooted in the lecture content",
  "finalQuestion": "one specific Japanese question whose answer lies in the chapter's evidence",
  "acts": [
    {{ "id": "a1", "kind": "investigation", "title": "幕タイトル(〜16字)", "location": "舞台(任意)", "narrative": "2–4文の物語…", "seedsMeta": false,
       "evidence": [
         {{ "id": "e1", "title": "6–30字の見出し", "body": "2–5文の事実。概念→具体例/反例/数値/分類などで肉付けする。", "sourceRef": "l1" }},
         {{ "id": "e2", "title": "別の論点", "body": "2–5文の事実…", "sourceRef": "l1" }}
       ] }},
    {{ "id": "a2", "kind": "testimony", "title": "幕タイトル", "narrative": "2–3文で対決の場へ…", "seedsMeta": false,
       "witnessName": "ミナミ", "witnessRole": "ゼミ仲間",
       "testimony": [
         {{ "id": "a2t1", "text": "真実の証言（別の論点）…", "isFalse": false, "keyEvidenceId": "", "highlights": ["…逐語…"], "pressResponse": "2–3文で概念を教える。定義→具体例→対比、の順で踏み込む。" }},
         {{ "id": "a2t2", "text": "別の真実の証言…", "isFalse": false, "keyEvidenceId": "", "highlights": ["…"], "pressResponse": "2–3文で深掘り。" }},
         {{ "id": "a2t3", "text": "e1 と矛盾する嘘…", "isFalse": true, "keyEvidenceId": "e1", "highlights": ["…誤った語…"], "pressResponse": "2–3文で白を切る。論点をすり替えたり別の例を持ち出すが、嘘そのものは明かさない。" }}
       ] }},
    {{ "id": "a3", "kind": "investigation", "title": "…", "narrative": "…", "evidence": [ /* 2–4 cards, 2–5文 each */ ] }},
    {{ "id": "a4", "kind": "testimony", "title": "…", "witnessName": "…", "witnessRole": "…", "testimony": [ /* 3–5 statements, exactly 1 lie keyed to some earlier evidence */ ] }},
    {{ "id": "a5", "kind": "investigation", "title": "…", "narrative": "…", "evidence": [ /* … */ ] }},
    {{ "id": "a6", "kind": "testimony", "title": "…", "witnessName": "…", "witnessRole": "…", "testimony": [ /* … */ ] }}
  ],
  "coverage": [
    {{ "pointId": "k1", "placement": "e1" }},
    {{ "pointId": "k2", "placement": "a2t1" }},
    {{ "pointId": "k3", "placement": "e3" }}
    /* …continue until every ★ point and at least {coverage_min} total are listed */
  ]
}}

`sourceRef` MUST be one of the input aliases above (l1/l2/.../s1/.../d1/...). Every evidence `id` is unique across the whole chapter. Each lie's `keyEvidenceId` MUST be an evidence id revealed in an EARLIER investigation act. Each `highlights` entry MUST be a verbatim substring of its `text`. Every text field MUST follow the CONTENT-ONLY RULE in the system message."#,
        course = case.course_name,
        live = live_section,
        signals = signal_section,
        doubts = doubt_section,
        syllabus = syllabus_section,
        knowledge = knowledge_section,
        must_cover_list = must_cover_list,
        campaign = format_campaign_section(campaign),
        memory = format_memory_section(memory),
        acts_min = ACTS_MIN,
        acts_max = ACTS_MAX,
        coverage_min = COVERAGE_MIN_POINTS,
    )
}

/// Render the campaign bible into a compact prompt section. When no campaign
/// exists yet (None), the case is generated world-free.
fn format_campaign_section(campaign: Option<&DetectiveCampaign>) -> String {
    let Some(c) = campaign else {
        return "(この科目にはまだ世界観が設定されていない。中立的な学習シーンで構成すること)"
            .to_string();
    };
    let cast = if c.cast.is_empty() {
        "(未設定)".to_string()
    } else {
        c.cast
            .iter()
            .map(|m| {
                let mut block = format!("- {}（{}）", m.name, m.role);
                if !m.bond.trim().is_empty() {
                    block.push_str(&format!(" — {}", m.bond.trim()));
                }
                if !m.background.trim().is_empty() {
                    block.push_str(&format!("\n    背景: {}", m.background.trim()));
                }
                if !m.motivation.trim().is_empty() {
                    block.push_str(&format!("\n    動機: {}", m.motivation.trim()));
                }
                if !m.stake.trim().is_empty() {
                    block.push_str(&format!("\n    利害: {}", m.stake.trim()));
                }
                block
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let played = if c.chapters.is_empty() {
        "(まだ章は進んでいない — 序章にあたる)".to_string()
    } else {
        c.chapters
            .iter()
            .rev()
            .take(5)
            .map(|ch| format!("- {}", ch.title))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "世界観ラベル: {label}\n舞台設定: {setting}\nキャッチコピー: {tagline}\n登場人物（再登場させてよい）:\n{cast}\n\n大きな暗线（meta-mystery — 全章を貫く隠された真相。今回は早期の伏線として “匂わせる” だけ。解決しない）:\n{meta}\n進行度: {progress}/100\nこれまでの章:\n{played}",
        label = c.world_label,
        setting = c.setting,
        tagline = if c.tagline.trim().is_empty() { "(なし)" } else { c.tagline.trim() },
        cast = cast,
        meta = c.meta_mystery,
        progress = c.meta_progress,
        played = played,
    )
}

/// Render the persisted memory into a compact prompt section. Keeps only
/// recent entries so the AI isn't drowned.
fn format_memory_section(memory: &DetectiveMemory) -> String {
    if memory.mistakes.is_empty()
        && memory.mastered.is_empty()
        && memory.recent_evidence_titles.is_empty()
    {
        return "(まだ過去のセッション記録はない)".to_string();
    }
    let mistakes = memory
        .mistakes
        .iter()
        .rev()
        .take(6)
        .map(|m| format!("- {}（{}）", m.topic, m.course_name))
        .collect::<Vec<_>>()
        .join("\n");
    let mastered = memory
        .mastered
        .iter()
        .rev()
        .take(8)
        .map(|m| format!("- {}（{}）", m.topic, m.course_name))
        .collect::<Vec<_>>()
        .join("\n");
    let recent = memory
        .recent_evidence_titles
        .iter()
        .rev()
        .take(10)
        .map(|s| format!("- {s}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mistakes_block = if mistakes.is_empty() {
        "(なし)".to_string()
    } else {
        mistakes
    };
    let mastered_block = if mastered.is_empty() {
        "(なし)".to_string()
    } else {
        mastered
    };
    let recent_block = if recent.is_empty() {
        "(なし)".to_string()
    } else {
        recent
    };
    format!(
        "Recently failed topics (RE-EMPHASIZE — at least one lie should touch these):\n{}\n\nRecently mastered topics (DE-EMPHASIZE — don't repeat as the lie):\n{}\n\nRecently used evidence titles (find new angles, don't reuse verbatim):\n{}",
        mistakes_block, mastered_block, recent_block,
    )
}

fn campaign_bible_system_prompt() -> &'static str {
    r#"You are the lead writer for a long-running courtroom-mystery game in the spirit of 逆転裁判 (Ace Attorney). You are designing the CAMPAIGN BIBLE — the persistent world (世界観) that every future chapter of one university course lives inside.

═══ THE GOLDEN RULE — anchor to a SIGNATURE HISTORICAL LOCUS ═══

The world is NOT "a vaguely-themed setting matching the subject". It is **the most iconic, historically/culturally representative time-and-place that the subject is associated with** — the specific moment a textbook would cite as the topic's center of gravity. Reach for the concrete locus, not the generic theme.

Good vs. bad anchoring:
- 米国黒人英語(AAVE) → ❌「19世紀のアメリカ」 / ✅「1960年代公民権運動下のミシシッピ・デルタの綿花町」or「1920年代ハーレム・ルネサンス期のニューヨーク」
- 確率論 → ❌「賭博の街」 / ✅「17世紀パスカルとフェルマーが文通した賭博問題のパリ・サロン」
- 量子力学 → ❌「物理学の街」 / ✅「1927年ソルベイ会議直前のコペンハーゲン、ボーア研究所」
- 古代中国法制 → ❌「中華風の都」 / ✅「商鞅変法下の戦国秦・咸陽」
- ピジン/クレオール言語 → ❌「多言語の港」 / ✅「19世紀末ハワイの製糖プランテーションと寄せ集めの労働者集落」

Specificity is non-negotiable. Name the **decade or specific historical event window** + **a real-feeling named locale** + **a concrete community/social structure**. If you cannot identify a signature locus, look harder — every academic subject has one.

═══ CHARACTERS NEED BACKGROUND, MOTIVATION, AND STAKES ═══

Every cast member is a person, not a label. For each, write:
- `background` (2–3 sentences): where they came from, what they've done before the campaign starts, what shaped them. NOT a role tag — a concrete mini-bio.
- `motivation` (1 sentence): what they WANT right now in this world. Every action they take in any chapter should be traceable here.
- `stake` (1 sentence): what they LOSE if the truth comes out / things go wrong. This is why they may lie, evade, or push back when cross-examined.
- `bond` (1 short phrase): their relation to the protagonist or to the meta-mystery.

═══ THE META-MYSTERY NEEDS A FACE ═══

The 暗线 is not just "a hidden truth". Name a **concrete antagonist force** — a person, a society, a guild, an institution — and give IT its own motivation (why are they hiding the truth? what do THEY want?). This makes the conspiracy feel real and the meta-arc reveals concrete.

═══ OUTPUT — return ONE JSON object, nothing else ═══
{
  "worldLabel": "8–22 char Japanese label naming the SIGNATURE LOCUS (era + place), e.g. 「1965年・ミシシッピ綿花町」「1927年・コペンハーゲン」",
  "setting": "3–5 Japanese sentences establishing the locus: the specific historical moment, the named place, the community structure, and WHY mysteries happen here. Anchor in real history that connects to the course subject.",
  "tagline": "one short Japanese hook line, under 28 chars — should evoke the SPECIFIC locus, not be generic",
  "metaMystery": "3–5 Japanese sentences. Name the antagonist force (who/what is hiding the truth), what they are concealing, and WHY they are concealing it (their motivation). The conspiracy must be thematically tied to the course's core ideas, and must be seedable in early chapters and payable in the finale.",
  "cast": [
    {
      "name": "2–6 char Japanese name (fits the locus's culture)",
      "role": "役回り e.g. 相棒/ライバル/黒幕候補/語り部/情報屋",
      "bond": "one phrase: their relation to the protagonist or the meta-mystery",
      "background": "2–3 Japanese sentences — concrete past, where they came from, what shaped them",
      "motivation": "1 Japanese sentence — what they WANT right now",
      "stake": "1 Japanese sentence — what they LOSE if the truth comes out"
    }
  ],
  "metaArc": [
    { "title": "短い見出し(〜12字)", "reveal": "1–2 Japanese sentences — this STAGE of unveiling the 暗线" }
  ],
  "finale": "3–5 Japanese sentences — the grand epilogue shown when the campaign is 100% complete. The conclusive resolution of the metaMystery: who/what was behind it, what they wanted, how the detective resolves it, the closing image."
}

Provide 2–3 cast members; one of them should plausibly be the antagonist force's local agent or someone whose stake aligns with the meta-mystery. `metaArc` = EXACTLY 4 ordered entries: (1) faint hint, (2) deepening clue, (3) twist that recontextualises earlier chapters, (4) full payoff revealing the antagonist's identity + motivation. `finale` lands AFTER stage 4. Keep everything in Japanese. Names, places, dates, and references must FEEL historically grounded — avoid totally invented place names when a real locus exists. Never invent facts that contradict the discipline."#
}

fn campaign_bible_user_prompt(course_name: &str, input: &[EvidenceInputEntry]) -> String {
    let live: Vec<String> = input
        .iter()
        .filter(|e| e.source_type == "live")
        .map(|e| truncate_chars(e.raw_content.replace('\r', "").trim(), 1200))
        .collect();
    let signals: Vec<String> = input
        .iter()
        .filter(|e| e.source_type == "signal")
        .map(|e| truncate_chars(e.raw_content.trim(), 160))
        .collect();
    let live_block = if live.is_empty() {
        "(none)".to_string()
    } else {
        live.iter()
            .enumerate()
            .map(|(i, b)| format!("[{}]\n{}", i + 1, b))
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let signal_block = if signals.is_empty() {
        "(none)".to_string()
    } else {
        signals.join("\n")
    };
    format!(
        r#"Course: {course}

═══ COURSE CONTENT (read this to infer the SUBJECT, then build a world around it) ═══

Live lecture notes (what the teacher actually taught):
{live}

Notifications (exam scope / format intel):
{signals}

═══ TASK ═══
Infer the academic SUBJECT of this course from the content above (history? statistics? linguistics? chemistry? law? …). Then design the CAMPAIGN BIBLE: a mystery world whose era/place/genre is DERIVED FROM that subject (see the system rules — American Revolution → 1770s colonial town, etc.). Establish a recurring cast and an overarching hidden conspiracy (暗线) that future chapters will unravel. Return the JSON object exactly as specified in the system message — Japanese, no extra fields, no prose."#,
        course = course_name,
        live = live_block,
        signals = signal_block,
    )
}

/// Clean + validate one evidence card from an investigation act. `seen_ids`
/// guards against duplicate ids across the whole chapter pool.
fn clean_evidence_card(
    ev: DetectiveAiEvidenceDraft,
    input_by_alias: &HashMap<&str, &EvidenceInputEntry>,
    seen_ids: &mut HashSet<String>,
) -> Result<DetectiveCaseEvidence, String> {
    let id = ev.id.trim().to_string();
    if id.is_empty() {
        return Err("an evidence card has an empty id".to_string());
    }
    if !seen_ids.insert(id.clone()) {
        return Err(format!("evidence id '{id}' is duplicated"));
    }
    let title = clean_ai_text(ev.title, 80)
        .filter(|t| {
            !looks_like_metadata_leak(t)
                && !looks_like_generic_label(t)
                && !looks_like_admin_trivia(t)
        })
        .ok_or_else(|| format!("evidence '{id}' has no content-clean title"))?;
    let body = clean_ai_text(ev.body, 700)
        .filter(|t| {
            !looks_like_metadata_leak(t) && !looks_like_platitude(t) && !looks_like_admin_trivia(t)
        })
        .ok_or_else(|| format!("evidence '{id}' has admin/non-testable body"))?;
    let source = ev
        .source_ref
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| input_by_alias.get(s).copied());
    Ok(DetectiveCaseEvidence {
        id,
        source_id: source.map(|s| s.source_id.clone()).unwrap_or_default(),
        source_type: source
            .map(|s| s.source_type.clone())
            .unwrap_or_else(|| "note".to_string()),
        source: source.map(|s| s.source.clone()).unwrap_or_default(),
        title,
        date: String::new(),
        excerpt: body,
        source_path: source.map(|s| s.source_path.clone()).unwrap_or_default(),
        source_url: source.map(|s| s.source_url.clone()).unwrap_or_default(),
        information_type: source
            .map(|s| s.information_type.clone())
            .unwrap_or_default(),
        person_category_cd: source
            .map(|s| s.person_category_cd.clone())
            .unwrap_or_default(),
        category_cd: source.map(|s| s.category_cd.clone()).unwrap_or_default(),
    })
}

/// Clean + validate one testimony statement. `revealed_ids` is the set of
/// evidence ids discovered in EARLIER investigation acts — a lie may only
/// point at a card the player has already seen (teaching before testing).
fn clean_testimony_statement(
    item: DetectiveAiTestimonyDraft,
    fallback_id: String,
    revealed_ids: &HashSet<String>,
) -> Result<DetectiveTestimony, String> {
    let Some(text) = clean_ai_text(item.text, 320) else {
        return Err("a testimony statement is empty".to_string());
    };
    if looks_like_metadata_leak(&text) {
        return Err(format!("testimony leaked metadata: {text}"));
    }
    if looks_like_platitude(&text) {
        return Err(format!("testimony is a platitude: {text}"));
    }
    if looks_like_admin_trivia(&text) {
        return Err(format!("testimony is administrative trivia: {text}"));
    }
    let is_false = item.is_false.unwrap_or(false);
    let key_id = item.key_evidence_id.unwrap_or_default().trim().to_string();
    let key_id = if is_false {
        if !revealed_ids.contains(&key_id) {
            return Err(format!(
                "a lie points at evidence '{key_id}' not revealed in an earlier investigation act"
            ));
        }
        key_id
    } else {
        String::new()
    };
    let id = item
        .id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(fallback_id);
    let highlights: Vec<String> = item
        .highlights
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.chars().count() <= 30 && text.contains(s.as_str()))
        .filter(|s| !looks_like_metadata_leak(s))
        .take(4)
        .collect();
    let press_response = clean_ai_text(item.press_response, 480)
        .filter(|t| !looks_like_metadata_leak(t) && !looks_like_platitude(t))
        .unwrap_or_default();
    Ok(DetectiveTestimony {
        id,
        text,
        is_false,
        key_evidence_id: key_id,
        highlights,
        press_response,
    })
}

/// Validate + apply the AI draft. The AI is required to:
/// 1. Produce a valid case header (caseType, briefing, finalQuestion).
/// 2. Produce 4–6 acts (幕) mixing investigation beats (which reveal evidence
///    cards) and testimony beats (each with exactly one lie keyed to an
///    already-revealed card).
///
/// Every text field is content-clean (no metadata leaks, no platitudes).
fn apply_ai_case_draft(
    mut case: DetectiveCase,
    draft: DetectiveAiCaseDraft,
    input: &[EvidenceInputEntry],
    knowledge: &[KnowledgePoint],
) -> Result<DetectiveCase, String> {
    let case_type = clean_ai_text(draft.case_type, 48)
        .filter(|t| is_allowed_case_type(t))
        .ok_or_else(|| "AI did not provide a valid caseType".to_string())?;
    case.case_type = case_type;

    case.difficulty = draft.difficulty.unwrap_or(2).clamp(1, 5);

    let briefing = clean_ai_text(draft.briefing, 480)
        .filter(|t| !looks_like_metadata_leak(t) && !looks_like_platitude(t))
        .ok_or_else(|| "AI did not provide a content-clean briefing".to_string())?;
    case.briefing = briefing;

    let scenario = clean_ai_text(draft.scenario, 800)
        .filter(|t| !looks_like_metadata_leak(t) && !looks_like_platitude(t))
        .ok_or_else(|| "AI did not provide a content-clean scenario".to_string())?;
    case.scenario = scenario;

    case.witness_name = clean_ai_text(draft.witness_name, 24)
        .filter(|t| !looks_like_metadata_leak(t))
        .unwrap_or_else(|| "証人".to_string());
    case.witness_role = clean_ai_text(draft.witness_role, 40)
        .filter(|t| !looks_like_metadata_leak(t))
        .unwrap_or_default();

    let question = clean_ai_text(draft.final_question, 260)
        .filter(|t| !looks_like_metadata_leak(t) && !looks_like_platitude(t))
        .ok_or_else(|| "AI did not provide a content-clean finalQuestion".to_string())?;
    case.final_question = question;

    // Acts (幕) — the chapter is a sequence of investigation + testimony beats.
    // Evidence cards live inside investigation acts; we flatten them into a
    // shared Court Record pool and let testimony lies reference only cards the
    // player has already discovered in an EARLIER investigation act.
    let input_by_alias: HashMap<&str, &EvidenceInputEntry> =
        input.iter().map(|e| (e.alias.as_str(), e)).collect();

    let Some(act_drafts) = draft.acts else {
        return Err("AI did not provide any acts (幕)".to_string());
    };
    if act_drafts.len() < ACTS_MIN || act_drafts.len() > ACTS_MAX {
        return Err(format!(
            "AI produced {} acts; a chapter needs {ACTS_MIN}–{ACTS_MAX} acts",
            act_drafts.len()
        ));
    }

    let mut pool: Vec<DetectiveCaseEvidence> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut revealed_ids: HashSet<String> = HashSet::new();
    let mut acts: Vec<DetectiveAct> = Vec::new();
    let mut flat_testimony: Vec<DetectiveTestimony> = Vec::new();
    let mut investigation_count = 0usize;
    let mut testimony_count = 0usize;
    let mut total_lies = 0usize;

    for (ai, act) in act_drafts.into_iter().enumerate() {
        let index = (ai + 1) as u8;
        let kind = act.kind.unwrap_or_default().trim().to_lowercase();
        let title = clean_ai_text(act.title, 80)
            .filter(|t| !looks_like_metadata_leak(t))
            .unwrap_or_else(|| format!("第{index}幕"));
        let narrative = clean_ai_text(act.narrative, 600)
            .filter(|t| !looks_like_metadata_leak(t))
            .unwrap_or_default();
        let location = clean_ai_text(act.location, 60)
            .filter(|t| !looks_like_metadata_leak(t))
            .unwrap_or_default();
        let seeds_meta = act.seeds_meta.unwrap_or(false);
        let act_id = act
            .id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("act{index}"));

        match kind.as_str() {
            "investigation" => {
                let Some(ev_drafts) = act.evidence else {
                    return Err(format!("investigation act {index} has no evidence cards"));
                };
                let mut ids: Vec<String> = Vec::new();
                for ev in ev_drafts {
                    let card = clean_evidence_card(ev, &input_by_alias, &mut seen_ids)
                        .map_err(|e| format!("act {index}: {e}"))?;
                    revealed_ids.insert(card.id.clone());
                    ids.push(card.id.clone());
                    pool.push(card);
                }
                if ids.len() < INVESTIGATION_EV_MIN {
                    return Err(format!(
                        "investigation act {index} produced only {} evidence cards; need at least {INVESTIGATION_EV_MIN}",
                        ids.len()
                    ));
                }
                investigation_count += 1;
                acts.push(DetectiveAct {
                    id: act_id,
                    index,
                    kind: "investigation".to_string(),
                    title,
                    location,
                    narrative,
                    seeds_meta,
                    evidence_ids: ids,
                    witness_name: String::new(),
                    witness_role: String::new(),
                    testimony: Vec::new(),
                });
            }
            "testimony" => {
                let Some(stmt_drafts) = act.testimony else {
                    return Err(format!("testimony act {index} has no statements"));
                };
                let mut stmts: Vec<DetectiveTestimony> = Vec::new();
                let mut lies_here = 0usize;
                for (si, item) in stmt_drafts.into_iter().enumerate() {
                    let stmt = clean_testimony_statement(
                        item,
                        format!("{act_id}_t{}", si + 1),
                        &revealed_ids,
                    )
                    .map_err(|e| format!("act {index}: {e}"))?;
                    if stmt.is_false {
                        lies_here += 1;
                    }
                    stmts.push(stmt);
                }
                if stmts.len() < TESTIMONY_STMT_MIN {
                    return Err(format!(
                        "testimony act {index} has only {} statements; need at least {TESTIMONY_STMT_MIN}",
                        stmts.len()
                    ));
                }
                if lies_here != 1 {
                    return Err(format!(
                        "testimony act {index} has {lies_here} lies; each testimony act needs exactly 1"
                    ));
                }
                testimony_count += 1;
                total_lies += lies_here;
                let witness_name = clean_ai_text(act.witness_name, 24)
                    .filter(|t| !looks_like_metadata_leak(t))
                    .unwrap_or_else(|| {
                        if case.witness_name.is_empty() {
                            "証人".to_string()
                        } else {
                            case.witness_name.clone()
                        }
                    });
                let witness_role = clean_ai_text(act.witness_role, 40)
                    .filter(|t| !looks_like_metadata_leak(t))
                    .unwrap_or_else(|| case.witness_role.clone());
                flat_testimony.extend(stmts.iter().cloned());
                acts.push(DetectiveAct {
                    id: act_id,
                    index,
                    kind: "testimony".to_string(),
                    title,
                    location,
                    narrative,
                    seeds_meta,
                    evidence_ids: Vec::new(),
                    witness_name,
                    witness_role,
                    testimony: stmts,
                });
            }
            other => {
                return Err(format!(
                    "act {index} has unknown kind '{other}' (expected investigation|testimony)"
                ));
            }
        }
    }

    if pool.len() < 2 {
        return Err(format!(
            "chapter produced only {} evidence cards; need at least 2",
            pool.len()
        ));
    }
    if investigation_count == 0 {
        return Err("chapter has no investigation act".to_string());
    }
    if testimony_count == 0 {
        return Err("chapter has no testimony act".to_string());
    }
    if total_lies < SESSION_LIES_MIN {
        return Err(format!(
            "chapter has only {total_lies} lies across its testimony acts; need at least {SESSION_LIES_MIN}"
        ));
    }

    case.evidence = pool;
    case.acts = acts;
    case.testimony = flat_testimony;

    // ─── Knowledge-point coverage validation ───────────────────────────
    // Build the set of valid placement targets: every evidence id, plus every
    // testimony id of the form "{actId}t{n}" (e.g. "a4t3"). The AI must show
    // that every must-cover point and at least COVERAGE_MIN_POINTS total were
    // placed into one of these targets.
    if !knowledge.is_empty() {
        let known_point_ids: HashSet<&str> = knowledge.iter().map(|p| p.id.as_str()).collect();
        let evidence_ids: HashSet<String> = case.evidence.iter().map(|e| e.id.clone()).collect();
        let mut testimony_ids: HashSet<String> = HashSet::new();
        for act in &case.acts {
            if act.kind == "testimony" {
                for (i, t) in act.testimony.iter().enumerate() {
                    // Accept either the AI-provided testimony id verbatim, or
                    // the canonical "{actId}t{n}" addressing scheme.
                    testimony_ids.insert(t.id.clone());
                    testimony_ids.insert(format!("{}t{}", act.id, i + 1));
                }
            }
        }
        let raw_coverage = draft.coverage.unwrap_or_default();
        let mut cleaned: Vec<CoverageEntry> = Vec::new();
        let mut covered_points: HashSet<String> = HashSet::new();
        for entry in raw_coverage {
            let Some(pid) = entry
                .point_id
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let Some(placement) = entry
                .placement
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            if !known_point_ids.contains(pid.as_str()) {
                return Err(format!(
                    "coverage references unknown pointId '{pid}' (not in the supplied knowledge list)"
                ));
            }
            if !evidence_ids.contains(&placement) && !testimony_ids.contains(&placement) {
                return Err(format!(
                    "coverage entry for '{pid}' points at '{placement}', which is neither an evidence id nor a testimony id"
                ));
            }
            covered_points.insert(pid.clone());
            cleaned.push(CoverageEntry {
                point_id: pid,
                placement,
            });
        }
        let missing_must: Vec<&str> = knowledge
            .iter()
            .filter(|p| p.must_cover && !covered_points.contains(&p.id))
            .map(|p| p.label.as_str())
            .collect();
        if !missing_must.is_empty() {
            return Err(format!(
                "{} must-cover knowledge point(s) not covered in this chapter: {}",
                missing_must.len(),
                missing_must.join(" / ")
            ));
        }
        if covered_points.len() < COVERAGE_MIN_POINTS {
            return Err(format!(
                "chapter covered only {} knowledge points; need at least {COVERAGE_MIN_POINTS}",
                covered_points.len()
            ));
        }
        case.knowledge_points = knowledge.to_vec();
        case.coverage = cleaned;
        eprintln!(
            "[detective] coverage OK: {} / {} points covered ({} must-cover)",
            covered_points.len(),
            knowledge.len(),
            knowledge.iter().filter(|p| p.must_cover).count()
        );
    }

    let title = clean_ai_text(draft.title, 140)
        .filter(|t| !looks_like_metadata_leak(t))
        .unwrap_or_else(|| {
            content_snippet(&case.briefing)
                .map(|s| truncate_chars(&s, 80))
                .unwrap_or_else(|| case.course_name.clone())
        });
    case.title = title;

    case.session_num = draft.session_num.unwrap_or(0).max(0).min(99) as u8;

    case.generation_mode = "ai".to_string();
    Ok(case)
}

/// Reject text that includes metadata patterns (filenames, ISO dates) that
/// must NEVER appear in a content-derived Detective case.
fn looks_like_metadata_leak(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains(".md") || lower.contains("_live") || lower.contains("_Live") {
        return true;
    }
    if contains_iso_date(text) {
        return true;
    }
    false
}

/// Reject "generic placeholder" titles that don't name an actual topic.
fn looks_like_generic_label(text: &str) -> bool {
    let t = text.trim();
    matches!(
        t,
        "ライブメモ"
            | "Liveメモ"
            | "授業ノート"
            | "本講義の記録"
            | "本日の講義"
            | "今回の講義"
            | "講義メモ"
            | "Live note"
            | "Live Note"
    )
}

/// Reject empty-content platitudes — testimony must reference a concrete fact.
fn looks_like_platitude(text: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "重要な内容がある",
        "記録が残っている",
        "資料を確認できる",
        "手がかりが残されている",
        "調査は続いている",
    ];
    for p in PATTERNS {
        if text.contains(p) {
            return true;
        }
    }
    false
}

/// Reject testimony / evidence text that is about administrative course
/// logistics rather than testable lecture content. Detective is review study,
/// not classroom management trivia.
fn looks_like_admin_trivia(text: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "学籍番号",
        "ネームカード",
        "出席確認",
        "出席カード",
        "出席を取",
        "提出方法",
        "提出期限",
        "用紙の色",
        "用紙のサイズ",
        "ファイル名",
        "ファイル形式",
        "Word で",
        "PDF で",
        "USB",
        "持参物",
        "持参する",
        "持参して",
        "持ち込み可",
        "持ち込み不可",
        "休講",
        "補講",
        "教室の場所",
        "教室は",
        "自己紹介",
        "アイスブレイク",
        "余談",
        "雑談",
    ];
    for p in PATTERNS {
        if text.contains(p) {
            return true;
        }
    }
    false
}

fn contains_iso_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    let n = bytes.len();
    if n < 10 {
        return false;
    }
    for i in 0..=n - 10 {
        let w = &bytes[i..i + 10];
        let dashes = (w[4] == b'-' || w[4] == b'/') && (w[7] == b'-' || w[7] == b'/');
        let digits = w[..4].iter().all(|b| b.is_ascii_digit())
            && w[5..7].iter().all(|b| b.is_ascii_digit())
            && w[8..10].iter().all(|b| b.is_ascii_digit());
        if dashes && digits {
            return true;
        }
    }
    false
}

fn clean_ai_text(value: Option<String>, max_chars: usize) -> Option<String> {
    let text = value?;
    let text = text.trim().trim_matches('"').trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(truncate_chars(&text, max_chars))
    }
}

fn is_allowed_case_type(value: &str) -> bool {
    matches!(
        value,
        "Exam Signal Case"
            | "Concept Web Case"
            | "Doubt Repair Case"
            | "Contradiction Case"
            | "Missing Link Case"
    )
}

/// Find a balanced JSON object inside the AI response. Tolerates:
/// - markdown code fences (```json ... ```)
/// - prose before/after the object
/// - stray `{` `}` inside strings
fn extract_json_object(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            i += 1;
            continue;
        }
        // Try to walk a balanced object from this position.
        let mut depth = 0i32;
        let mut in_str = false;
        let mut escape = false;
        let mut j = i;
        while j < bytes.len() {
            let c = bytes[j];
            if in_str {
                if escape {
                    escape = false;
                } else if c == b'\\' {
                    escape = true;
                } else if c == b'"' {
                    in_str = false;
                }
            } else if c == b'"' {
                in_str = true;
            } else if c == b'{' {
                depth += 1;
            } else if c == b'}' {
                depth -= 1;
                if depth == 0 {
                    return Some(text[i..=j].to_string());
                }
            }
            j += 1;
        }
        // Unbalanced from this position; keep scanning for another `{`.
        i += 1;
    }
    None
}

/// One raw source unit fed to the AI. Transient — these never appear in the
/// final `DetectiveCase`. The AI reads these and emits its own short
/// evidence cards (one paragraph of distilled information per card).
#[derive(Debug, Clone)]
struct EvidenceInputEntry {
    alias: String, // l1, s1, d1
    source_type: String,
    source: String,
    source_id: String,
    raw_title: String,
    raw_content: String,
    source_path: String,
    source_url: String,
    information_type: String,
    person_category_cd: String,
    category_cd: String,
}

/// Assemble the input package handed to the AI: full Live note bodies +
/// signal titles + doubt notes, each tagged with a short alias (l1/s1/d1).
fn build_evidence_input(course: &DetectiveCourse) -> Vec<EvidenceInputEntry> {
    let mut out = Vec::new();

    let mut li = 0u32;
    for record in course.live_records.iter().take(6) {
        li += 1;
        let content = if record.excerpt.trim().is_empty() {
            "(本文未抽出)".to_string()
        } else {
            record.excerpt.clone()
        };
        out.push(EvidenceInputEntry {
            alias: format!("l{li}"),
            source_type: "live".to_string(),
            source: "live".to_string(),
            source_id: record.id.clone(),
            raw_title: String::new(), // filename — deliberately not shown to AI
            raw_content: content,
            source_path: record.path.clone(),
            source_url: String::new(),
            information_type: String::new(),
            person_category_cd: String::new(),
            category_cd: String::new(),
        });
    }

    let mut si = 0u32;
    for signal in course.exam_signals.iter().take(6) {
        si += 1;
        let content = if signal.category.is_empty() {
            signal.title.clone()
        } else {
            format!("{}: {}", signal.category, signal.title)
        };
        out.push(EvidenceInputEntry {
            alias: format!("s{si}"),
            source_type: "signal".to_string(),
            source: signal.source.clone(),
            source_id: signal.source_id.clone(),
            raw_title: signal.title.clone(),
            raw_content: content,
            source_path: String::new(),
            source_url: signal.source_url.clone(),
            information_type: signal.information_type.clone(),
            person_category_cd: signal.person_category_cd.clone(),
            category_cd: signal.category_cd.clone(),
        });
    }

    let mut di = 0u32;
    for doubt in course.doubts.iter().take(3) {
        di += 1;
        out.push(EvidenceInputEntry {
            alias: format!("d{di}"),
            source_type: "doubt".to_string(),
            source: "doubt".to_string(),
            source_id: doubt.id.clone(),
            raw_title: String::new(),
            raw_content: doubt.note.clone(),
            source_path: String::new(),
            source_url: String::new(),
            information_type: String::new(),
            person_category_cd: String::new(),
            category_cd: String::new(),
        });
    }

    out
}

/// Assemble the AI input for ONE chapter: a single Live note as the primary
/// source (l1), plus the course's exam signals (s*) and doubts (d*) for
/// exam-context. Returns `None` if the live note can't be found.
fn build_chapter_input(course: &DetectiveCourse, live_id: &str) -> Option<Vec<EvidenceInputEntry>> {
    let record = course.live_records.iter().find(|r| r.id == live_id)?;
    let mut out = Vec::new();
    let content = if record.excerpt.trim().is_empty() {
        "(本文未抽出)".to_string()
    } else {
        record.excerpt.clone()
    };
    out.push(EvidenceInputEntry {
        alias: "l1".to_string(),
        source_type: "live".to_string(),
        source: "live".to_string(),
        source_id: record.id.clone(),
        raw_title: String::new(),
        raw_content: content,
        source_path: record.path.clone(),
        source_url: String::new(),
        information_type: String::new(),
        person_category_cd: String::new(),
        category_cd: String::new(),
    });

    let mut si = 0u32;
    for signal in course.exam_signals.iter().take(4) {
        si += 1;
        let content = if signal.category.is_empty() {
            signal.title.clone()
        } else {
            format!("{}: {}", signal.category, signal.title)
        };
        out.push(EvidenceInputEntry {
            alias: format!("s{si}"),
            source_type: "signal".to_string(),
            source: signal.source.clone(),
            source_id: signal.source_id.clone(),
            raw_title: signal.title.clone(),
            raw_content: content,
            source_path: String::new(),
            source_url: signal.source_url.clone(),
            information_type: signal.information_type.clone(),
            person_category_cd: signal.person_category_cd.clone(),
            category_cd: signal.category_cd.clone(),
        });
    }

    let mut di = 0u32;
    for doubt in course.doubts.iter().take(2) {
        di += 1;
        out.push(EvidenceInputEntry {
            alias: format!("d{di}"),
            source_type: "doubt".to_string(),
            source: "doubt".to_string(),
            source_id: doubt.id.clone(),
            raw_title: String::new(),
            raw_content: doubt.note.clone(),
            source_path: String::new(),
            source_url: String::new(),
            information_type: String::new(),
            person_category_cd: String::new(),
            category_cd: String::new(),
        });
    }

    Some(out)
}

/// Decide a course's preview `caseType` for the picker list. The AI will
/// choose the final caseType when the case is opened.
fn course_case_type(course: &CourseBuilder) -> &'static str {
    let has_live = !course.live_records.is_empty();
    let has_signal = !course.exam_signals.is_empty();
    let has_doubts = !course.doubts.is_empty();
    if has_signal && has_live {
        "Exam Signal Case"
    } else if has_signal {
        "Contradiction Case"
    } else if has_doubts {
        "Doubt Repair Case"
    } else if course.live_records.len() >= 2 {
        "Missing Link Case"
    } else {
        "Concept Web Case"
    }
}

fn ensure_course<'a>(
    courses: &'a mut HashMap<String, CourseBuilder>,
    name: &str,
) -> &'a mut CourseBuilder {
    let clean = clean_course_name(name);
    let key = normalize_course_key(&clean);
    courses.entry(key.clone()).or_insert_with(|| CourseBuilder {
        name: clean,
        key,
        ..Default::default()
    })
}

fn clean_course_name(name: &str) -> String {
    let simplified = crate::commands::simplify_course_name(name);
    let trimmed = simplified.trim();
    if trimmed.is_empty() {
        "Detective".to_string()
    } else {
        trimmed.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

fn normalize_course_key(name: &str) -> String {
    crate::commands::simplify_course_name(name)
        .chars()
        .flat_map(|ch| ch.to_lowercase())
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

fn is_live_record(record: &crate::commands::DownloadRecord) -> bool {
    let source = record.source.to_ascii_lowercase();
    let filename = record.filename.to_ascii_lowercase();
    let path = record.path.to_ascii_lowercase();
    source == "live" || filename.contains("_live") || path.contains("_live")
}

fn infer_course_name_from_record(record: &crate::commands::DownloadRecord) -> String {
    let from_parent = Path::new(&record.path)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|name| name.to_str())
        .map(clean_course_name);
    if let Some(parent) = from_parent.filter(|name| !name.is_empty()) {
        return parent;
    }
    record
        .filename
        .replace("_live", "")
        .trim_end_matches(".md")
        .trim_end_matches(".markdown")
        .to_string()
}

fn live_excerpt(path: &str) -> String {
    let path = Path::new(path);
    let Ok(metadata) = std::fs::metadata(path) else {
        return String::new();
    };
    if !metadata.is_file() || metadata.len() > LIVE_EXCERPT_MAX_BYTES {
        return String::new();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return String::new();
    };
    compact_live_note_markdown(&String::from_utf8_lossy(&bytes), LIVE_EXCERPT_MAX_CHARS)
}

fn compact_live_note_markdown(markdown: &str, max_chars: usize) -> String {
    let head = markdown.split("\n## 全文転写").next().unwrap_or(markdown);
    let mut lines = Vec::new();
    for line in head.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let cleaned = trimmed
            .trim_start_matches('#')
            .trim()
            .trim_start_matches('-')
            .trim();
        if cleaned.is_empty()
            || cleaned == "区間ごとの要約"
            || cleaned == "全文転写"
            || matches!(
                cleaned,
                "開始:" | "終了:" | "授業コード:" | "教員:" | "教室:" | "時間帯:"
            )
        {
            continue;
        }
        lines.push(cleaned.to_string());
    }
    truncate_chars(&lines.join("\n"), max_chars)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}...")
}

fn collect_exam_signals(db: &Database) -> Vec<DetectiveSignal> {
    let mut out = Vec::new();

    if let Some(notifications) =
        load_cache_json::<crate::parser::NotificationsData>(db, "notifications")
    {
        for notif in notifications.entries {
            if !is_exam_signal(&format!("{} {}", notif.title, notif.category)) {
                continue;
            }
            out.push(DetectiveSignal {
                id: format!("kgc:{}:{}", notif.id, notif.date),
                source_id: notif.id,
                title: notif.title,
                date: notif.date,
                category: notif.category,
                source: "kgc".to_string(),
                course_info: String::new(),
                source_url: notif.url,
                information_type: String::new(),
                person_category_cd: String::new(),
                category_cd: String::new(),
            });
        }
    }

    if let Some(luna_updates) =
        load_cache_json::<Vec<crate::luna_parser::LunaNotification>>(db, "luna_updates")
    {
        for notif in luna_updates {
            let text = format!("{} {} {}", notif.content, notif.module, notif.course_info);
            if !is_exam_signal(&text) {
                continue;
            }
            let source_id = if notif.idnumber.is_empty() {
                notif.url.clone()
            } else {
                notif.idnumber.clone()
            };
            out.push(DetectiveSignal {
                id: format!(
                    "luna:{}:{}",
                    if notif.url.is_empty() {
                        &notif.idnumber
                    } else {
                        &notif.url
                    },
                    notif.date
                ),
                source_id,
                title: notif.content,
                date: notif.date,
                category: if notif.module.is_empty() {
                    notif.course_info.clone()
                } else {
                    notif.module
                },
                source: "luna".to_string(),
                course_info: notif.course_info,
                source_url: notif.url,
                information_type: String::new(),
                person_category_cd: String::new(),
                category_cd: String::new(),
            });
        }
    }

    if let Some(home) = load_cache_json::<crate::kwic_commands::KwicPortalHome>(db, "kwic_home") {
        for section in home.sections {
            if section.title == "メインリンク" || section.title == "注目コンテンツ" {
                continue;
            }
            for item in section.items {
                let text = format!("{} {} {}", item.title, item.category, section.title);
                if !is_exam_signal(&text) {
                    continue;
                }
                out.push(DetectiveSignal {
                    id: format!("kwic:{}:{}", item.id, item.date),
                    source_id: item.id,
                    title: item.title,
                    date: item.date,
                    category: if item.category.is_empty() {
                        section.title.clone()
                    } else {
                        item.category
                    },
                    source: "kwic".to_string(),
                    course_info: String::new(),
                    source_url: item.url,
                    information_type: item.information_type,
                    person_category_cd: item.person_category_cd,
                    category_cd: item.category_cd,
                });
            }
        }
    }

    if let Ok(Some((result, _))) = db.get_ai_schedule_cache() {
        for item in result
            .current_week
            .into_iter()
            .chain(result.next_week.into_iter())
        {
            for signal in item
                .exams
                .iter()
                .chain(item.notifications.iter().filter(|s| is_exam_signal(s)))
            {
                out.push(DetectiveSignal {
                    id: format!("schedule:{}:{}", item.course_name, signal),
                    source_id: item.course_name.clone(),
                    title: signal.clone(),
                    date: String::new(),
                    category: "時間割".to_string(),
                    source: "schedule".to_string(),
                    course_info: item.course_name.clone(),
                    source_url: String::new(),
                    information_type: String::new(),
                    person_category_cd: String::new(),
                    category_cd: String::new(),
                });
            }
        }
    }

    dedupe_signals(out)
}

fn load_cache_json<T: DeserializeOwned>(db: &Database, key: &str) -> Option<T> {
    let (json, _) = db.get_data_cache(key).ok().flatten()?;
    serde_json::from_str(&json).ok()
}

fn load_doubts(db: &Database) -> Vec<DetectiveDoubt> {
    load_cache_json(db, DETECTIVE_DOUBTS_KEY).unwrap_or_default()
}

fn load_memory(db: &Database) -> DetectiveMemory {
    load_cache_json(db, DETECTIVE_MEMORY_KEY).unwrap_or_default()
}

fn save_memory(db: &Database, memory: &DetectiveMemory) {
    if let Ok(json) = serde_json::to_string(memory) {
        let _ = db.save_data_cache(DETECTIVE_MEMORY_KEY, &json);
    }
}

fn campaign_key(course_key: &str) -> String {
    format!("{DETECTIVE_CAMPAIGN_PREFIX}{course_key}")
}

fn load_campaign(db: &Database, course_key: &str) -> Option<DetectiveCampaign> {
    load_cache_json(db, &campaign_key(course_key))
}

fn save_campaign(db: &Database, campaign: &DetectiveCampaign) {
    if let Ok(json) = serde_json::to_string(campaign) {
        let _ = db.save_data_cache(&campaign_key(&campaign.course_key), &json);
    }
}

fn chapter_case_key(course_key: &str, live_id: &str) -> String {
    format!("{DETECTIVE_CHAPTER_PREFIX}{course_key}:{live_id}")
}

fn load_chapter_case(db: &Database, course_key: &str, live_id: &str) -> Option<DetectiveCase> {
    load_cache_json(db, &chapter_case_key(course_key, live_id))
}

fn save_chapter_case(db: &Database, course_key: &str, live_id: &str, case: &DetectiveCase) {
    if let Ok(json) = serde_json::to_string(case) {
        let _ = db.save_data_cache(&chapter_case_key(course_key, live_id), &json);
    }
}

fn knowledge_key(course_key: &str, live_id: &str) -> String {
    format!("{DETECTIVE_KNOWLEDGE_PREFIX}{course_key}:{live_id}")
}

fn load_knowledge_points(
    db: &Database,
    course_key: &str,
    live_id: &str,
) -> Option<Vec<KnowledgePoint>> {
    load_cache_json(db, &knowledge_key(course_key, live_id))
}

fn save_knowledge_points(db: &Database, course_key: &str, live_id: &str, pts: &[KnowledgePoint]) {
    if let Ok(json) = serde_json::to_string(pts) {
        let _ = db.save_data_cache(&knowledge_key(course_key, live_id), &json);
    }
}

/// One planned lecture from the 授業計画.
struct PlannedSession {
    num: i32,
    topic: String,
    /// True for online / on-demand 回 that never produce a Live recording.
    online: bool,
}

/// Classify a 授業計画 delivery_mode string. Anything that clearly says online /
/// on-demand / remote counts as online (no Live); everything else (対面,
/// ハイブリッド, blank, …) is treated as offline / Live-bearing.
fn is_online_mode(mode: &str) -> bool {
    const JP: &[&str] = &["オンデマンド", "オンライン", "遠隔", "非対面", "配信"];
    const EN: &[&str] = &["on-demand", "ondemand", "online", "remote"];
    let lower = mode.to_lowercase();
    JP.iter().any(|k| mode.contains(k)) || EN.iter().any(|k| lower.contains(k))
}

/// The course's 授業計画 sessions (content-ordered), or empty if none parsed.
fn planned_sessions(db: &Database, course_key: &str) -> Vec<PlannedSession> {
    db.get_planned_sessions_by_name()
        .ok()
        .into_iter()
        .flatten()
        .find(|(name, _)| normalize_course_key(name) == course_key)
        .map(|(_, rows)| {
            rows.into_iter()
                .map(|(num, topic, mode)| PlannedSession {
                    num,
                    topic,
                    online: is_online_mode(&mode),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Offline (Live-bearing) session numbers — the chapters that can actually be
/// played. The finale is the highest of these.
fn offline_session_nums(db: &Database, course_key: &str) -> Vec<i32> {
    let mut nums: Vec<i32> = planned_sessions(db, course_key)
        .into_iter()
        .filter(|s| !s.online)
        .map(|s| s.num)
        .collect();
    nums.sort_unstable();
    nums.dedup();
    nums
}

fn align_key(course_key: &str) -> String {
    format!("{DETECTIVE_ALIGN_PREFIX}{course_key}")
}

/// Map of Live note id → matched 第N回 (content alignment).
fn load_align(db: &Database, course_key: &str) -> std::collections::HashMap<String, i32> {
    load_cache_json(db, &align_key(course_key)).unwrap_or_default()
}

fn save_align(db: &Database, course_key: &str, map: &std::collections::HashMap<String, i32>) {
    if let Ok(json) = serde_json::to_string(map) {
        let _ = db.save_data_cache(&align_key(course_key), &json);
    }
}

/// Campaign completion %, based on how many OFFLINE 授業計画 回 have an aligned,
/// cleared Live note. 100% ⇒ the final offline 回 is done ⇒ finale. Returns
/// `None` when the course has no usable 授業計画 (caller falls back).
fn campaign_progress_pct(db: &Database, course_key: &str) -> Option<u8> {
    let offline = offline_session_nums(db, course_key);
    if offline.is_empty() {
        return None;
    }
    let align = load_align(db, course_key);
    // No alignment data yet (older chapters, or alignment failed) — let the
    // caller fall back rather than freezing progress at 0%.
    if align.is_empty() {
        return None;
    }
    let offline_set: std::collections::HashSet<i32> = offline.iter().copied().collect();
    let prefix = format!("detective:chapter:{course_key}:");
    let mut cleared: std::collections::HashSet<i32> = std::collections::HashSet::new();
    for r in load_case_results(db) {
        if let Some(live_id) = r.case_id.strip_prefix(&prefix) {
            if let Some(&num) = align.get(live_id) {
                if offline_set.contains(&num) {
                    cleared.insert(num);
                }
            }
        }
    }
    let pct = ((cleared.len().min(offline.len()) as f32 / offline.len() as f32) * 100.0).round();
    Some((pct as u8).min(100))
}

fn load_included_courses(db: &Database) -> Vec<String> {
    load_cache_json(db, DETECTIVE_INCLUDED_KEY).unwrap_or_default()
}

fn load_case_results(db: &Database) -> Vec<DetectiveCaseResult> {
    let mut results: Vec<DetectiveCaseResult> =
        load_cache_json(db, DETECTIVE_RESULTS_KEY).unwrap_or_default();
    results.sort_by(|a, b| b.closed_at.cmp(&a.closed_at));
    results
}

fn build_review_queue(courses: &[DetectiveCourse]) -> Vec<DetectiveReviewItem> {
    let now_ms = crate::db::epoch_secs() * 1000;
    let mut out = Vec::new();

    for course in courses {
        for doubt in course.doubts.iter().take(3) {
            let overdue = doubt.due_at > 0 && doubt.due_at <= now_ms;
            out.push(DetectiveReviewItem {
                id: format!("doubt:{}", doubt.id),
                course_key: course.key.clone(),
                course_name: course.name.clone(),
                reason: if overdue {
                    format!(
                        "Unresolved doubt is due: {}",
                        truncate_chars(&doubt.note, 90)
                    )
                } else {
                    format!("Unresolved doubt: {}", truncate_chars(&doubt.note, 90))
                },
                priority: if overdue { 5 } else { 4 },
                due_at: doubt.due_at,
            });
        }

        if let Some(result) = course
            .recent_results
            .iter()
            .find(|item| item.confidence <= 2)
        {
            out.push(DetectiveReviewItem {
                id: format!("confidence:{}", result.id),
                course_key: course.key.clone(),
                course_name: course.name.clone(),
                reason: format!(
                    "Last closed case had low confidence ({}/5): {}",
                    result.confidence,
                    truncate_chars(&result.case_title, 80)
                ),
                priority: 3,
                due_at: result.closed_at + 24 * 60 * 60 * 1000,
            });
        }

        if course.live_records.is_empty() && !course.exam_signals.is_empty() {
            out.push(DetectiveReviewItem {
                id: format!("source-gap:{}", course.key),
                course_key: course.key.clone(),
                course_name: course.name.clone(),
                reason: "Exam signal exists, but Detective has no Live note evidence yet."
                    .to_string(),
                priority: 2,
                due_at: 0,
            });
        }
    }

    out.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| nonzero_or_max(a.due_at).cmp(&nonzero_or_max(b.due_at)))
    });
    out.truncate(8);
    out
}

fn nonzero_or_max(value: i64) -> i64 {
    if value > 0 {
        value
    } else {
        i64::MAX
    }
}

fn is_exam_signal(text: &str) -> bool {
    let lower = text.to_lowercase();
    EXAM_KEYWORDS
        .iter()
        .any(|keyword| lower.contains(&keyword.to_lowercase()))
}

fn match_signal_course_key(
    signal: &DetectiveSignal,
    courses: &HashMap<String, CourseBuilder>,
) -> Option<String> {
    if !signal.course_info.trim().is_empty() {
        let key = normalize_course_key(&signal.course_info);
        if courses.contains_key(&key) {
            return Some(key);
        }
    }
    let hay = normalize_course_key(&format!(
        "{} {} {}",
        signal.title, signal.category, signal.course_info
    ));
    let mut best: Option<String> = None;
    let mut best_len = 0usize;
    for key in courses.keys() {
        if key.len() < 3 {
            continue;
        }
        if (hay.contains(key) || key.contains(&hay)) && key.len() > best_len {
            best = Some(key.clone());
            best_len = key.len();
        }
    }
    best
}

fn date_score(date: &str) -> i64 {
    date.chars()
        .filter(|c| c.is_ascii_digit())
        .take(8)
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn dedupe_signals(signals: Vec<DetectiveSignal>) -> Vec<DetectiveSignal> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for signal in signals {
        let key = format!(
            "{}:{}:{}:{}",
            signal.source, signal.title, signal.date, signal.category
        );
        if seen.insert(key) {
            out.push(signal);
        }
    }
    out
}

fn readiness_text(course: &CourseBuilder) -> String {
    match (
        course.live_records.is_empty(),
        course.exam_signals.is_empty(),
        course.doubts.is_empty(),
    ) {
        (false, false, _) => "Live notes and exam signals ready".to_string(),
        (false, true, false) => "Live-note investigation with unresolved doubts".to_string(),
        (false, true, true) => "Live-note investigation ready".to_string(),
        (true, false, _) => "Exam-signal investigation ready".to_string(),
        (true, true, false) => "Unresolved doubt case ready".to_string(),
        _ => "Waiting for course evidence".to_string(),
    }
}

fn course_score(course: &DetectiveCourse) -> f64 {
    course.exam_signals.len() as f64 * 8.0
        + course.doubts.len() as f64 * 5.0
        + course.live_records.len() as f64 * 2.0
        + course.latest_at as f64 / 1_000_000_000_000.0
}
