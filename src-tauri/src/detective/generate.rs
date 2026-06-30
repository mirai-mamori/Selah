//! The three-pass AI generation pipeline + campaign bible + knowledge extraction + AI input building.
use super::*;
use crate::db::Database;
use serde::Deserialize;
#[allow(unused_imports)]
use std::collections::{HashMap, HashSet};

/// Build an empty case shell. The AI is responsible for filling every
/// content-bearing field including the evidence cards themselves.
pub(crate) fn build_case(course: &DetectiveCourse) -> DetectiveCase {
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
        case_logic: CaseLogic::default(),
        meta_beat: String::new(),
    }
}

/// Extract a short, meaningful content snippet from an evidence excerpt.
/// Skips metadata lines and bullet markers; returns 18-70 char chunks.
pub(crate) fn content_snippet(excerpt: &str) -> Option<String> {
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

pub(crate) fn looks_like_metadata(text: &str) -> bool {
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

/// Run one AI turn that must return a single JSON object, and extract it.
/// Shared by the three chapter-generation passes.
pub(crate) async fn detective_ai_json(
    provider: &crate::agent_provider::AgentProvider,
    max_tokens: u32,
    system: &str,
    user: String,
    temperature: f32,
    think_budget_pct: u32,
    tag: &str,
) -> Result<String, String> {
    let messages = vec![
        crate::ai::ChatMessage {
            role: "system".to_string(),
            content: system.to_string(),
            images: Vec::new(),
        },
        crate::ai::ChatMessage {
            role: "user".to_string(),
            content: user,
            images: Vec::new(),
        },
    ];
    let raw = provider
        .plan(messages, max_tokens, temperature, "", think_budget_pct, tag)
        .await
        .map_err(|e| format!("AI call failed: {e}"))?;
    eprintln!("[detective] {tag} raw BEGIN ===\n{}\n=== END", raw);
    extract_json_object(&raw).ok_or_else(|| {
        let preview = truncate_chars(raw.trim(), 300);
        if raw.trim().is_empty() {
            "AI returned an empty response (the model may have refused or been blocked)."
                .to_string()
        } else {
            format!("AI did not return JSON. The response begins with: \"{preview}\"")
        }
    })
}

/// Generate a Detective chapter via a three-pass AI pipeline (outline → draft →
/// editor). There is no silent fallback to a worse case: the outline + draft
/// passes are required and surface `Err` on failure; the editor pass only ever
/// *improves* the draft and never regresses it.
///   Pass A — outline: the 推理 spine (truth/culprit/motive/red herrings/
///     deduction chain) + act plan + 暗线 beat. Logic only, no prose.
///   Pass B — draft: the full chapter written to conform to the outline.
///   Pass C — editor: a consistency critique that may return a repaired draft.
/// The hard structural + content + coverage gate (`apply_ai_case_draft`) runs
/// on whichever draft we keep.
pub(crate) async fn generate_case_with_ai(
    case: DetectiveCase,
    input: Vec<EvidenceInputEntry>,
    memory: DetectiveMemory,
    campaign: Option<DetectiveCampaign>,
    syllabus: Vec<PlannedSession>,
    knowledge: Vec<KnowledgePoint>,
    plan: ChapterPlan,
) -> Result<DetectiveCase, String> {
    let cfg = crate::ai::load_ai_config();
    eprintln!(
        "[detective] generate_case_with_ai (3-pass): course={} ai_enabled={} input_sources={}",
        case.course_name,
        cfg.ai_enabled,
        input.len()
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
        format!("AI provider unavailable: {e}")
    })?;

    // Scale the chapter's structure targets to how much testable content this
    // Live note actually yielded — thin notes get a tighter (but real) chapter.
    let targets = gen_targets(knowledge.len());
    eprintln!(
        "[detective] targets: {} knowledge pts → acts {}–{}, coverage {}, lies {}",
        knowledge.len(),
        targets.acts_min,
        targets.acts_max,
        targets.coverage_min,
        targets.lies_min
    );

    eprintln!(
        "[detective] plan: arc_focus={:?}/{} archetype={}",
        plan.arc_focus, plan.arc_total, plan.archetype
    );

    // ── Pass A — outline (推理 & 暗线 skeleton) ─────────────────────────────
    let outline_user = detective_outline_user_prompt(
        &case,
        &input,
        &memory,
        campaign.as_ref(),
        &syllabus,
        &knowledge,
        &targets,
        &plan,
    );
    eprintln!("[detective] PASS A (outline) dispatching…");
    let outline_json = detective_ai_json(
        &provider,
        cfg.max_tokens,
        detective_outline_system_prompt(),
        outline_user,
        0.4,
        25,
        &format!("detective-outline:{}", case.id),
    )
    .await
    .map_err(|e| format!("案件の推理プロットの生成に失敗しました（Pass A）: {e}"))?;
    let outline: CaseOutlineDraft = serde_json::from_str(&outline_json)
        .map_err(|e| format!("Outline JSON parse failed (Pass A): {e}"))?;

    // ── Pass B — draft (full chapter prose conforming to the outline) ──────
    let draft_user = format!(
        "{base}\n\n═══ 承認済みプロット（このスケルトンに厳密に従う） ═══\n{outline}\n\n上のプロットが合意済みの推理スパイン＋幕構成です。本章を執筆する際は必ず: (1) 幕の数と種類が `actPlan` と一致する。(2) 各 testimony 幕で仕込む唯一の嘘は、その幕の `lieAbout` が指す“既に教えた事実”を歪める。(3) `coveragePlan` の知識点 id をすべて被覆する。(4) `caseLogic`（実際に書いた内容に合わせて微調整可）と `metaBeat` をトップレベルで返す。(5) CAMPAIGN WORLD・世界の正典・投下済みの伏線とすべて整合させる。",
        base = detective_ai_user_prompt(&case, &input, &memory, campaign.as_ref(), &syllabus, &knowledge, &targets, &plan),
        outline = outline_json,
    );
    eprintln!(
        "[detective] PASS B (draft) dispatching ({} chars)…",
        draft_user.chars().count()
    );
    let draft_json = detective_ai_json(
        &provider,
        cfg.max_tokens,
        detective_ai_system_prompt(),
        draft_user,
        0.2,
        20,
        &format!("detective:{}", case.id),
    )
    .await
    .map_err(|e| format!("案件の本文生成に失敗しました（Pass B）: {e}"))?;

    // Parse + apply + hard-validate a draft JSON onto a fresh case shell.
    let parse_apply = |json: &str, base: DetectiveCase| -> Result<DetectiveCase, String> {
        let d: DetectiveAiCaseDraft =
            serde_json::from_str(json).map_err(|e| format!("draft JSON parse failed: {e}"))?;
        apply_ai_case_draft(base, d, &input, &knowledge, &targets)
    };
    let applied = parse_apply(&draft_json, case.clone())?;
    eprintln!(
        "[detective] PASS B accepted: acts={} evidence={} testimony={}",
        applied.acts.len(),
        applied.evidence.len(),
        applied.testimony.len()
    );

    // ── Pass C — editor critique / repair (best-effort, never regresses) ───
    let mut final_case = applied;
    match detective_editor_pass(
        &provider,
        &cfg,
        &draft_json,
        &outline_json,
        campaign.as_ref(),
        &knowledge,
        plan.arc_focus,
        &case.id,
    )
    .await
    {
        Ok(Some(patched_json)) => match parse_apply(&patched_json, case.clone()) {
            Ok(better) => {
                eprintln!("[detective] PASS C: editor repair applied + revalidated");
                final_case = better;
            }
            Err(e) => eprintln!("[detective] PASS C: repair rejected ({e}); keeping Pass B draft"),
        },
        Ok(None) => eprintln!("[detective] PASS C: editor reports no changes needed"),
        Err(e) => eprintln!("[detective] PASS C skipped (non-fatal): {e}"),
    }

    // Backfill the logic spine / 暗线 beat / session number from the outline if
    // the draft didn't echo them.
    if final_case.case_logic.truth.trim().is_empty() {
        if let Some(logic) = outline.case_logic {
            final_case.case_logic = resolve_case_logic(logic);
        }
    }
    if final_case.meta_beat.trim().is_empty() {
        if let Some(beat) = clean_ai_text(outline.meta_beat, 300)
            .filter(|t| !looks_like_metadata_leak(t) && !looks_like_platitude(t))
        {
            final_case.meta_beat = beat;
        }
    }
    if final_case.session_num == 0 {
        final_case.session_num = outline.session_num.unwrap_or(0).clamp(0, 99) as u8;
    }

    eprintln!(
        "[detective] case ACCEPTED: acts={} evidence={} culprit={:?} chain={}",
        final_case.acts.len(),
        final_case.evidence.len(),
        final_case.case_logic.culprit,
        final_case.case_logic.deduction_chain.len()
    );
    Ok(final_case)
}

/// Pass C: ask an editor model to critique the draft for logical consistency,
/// motive traceability, fair-play clueing, 暗线-stage fit, and canon consistency.
/// Returns `Ok(Some(json))` with a fully-repaired draft when it found problems,
/// `Ok(None)` when the draft is already clean, or `Err` on AI failure.
pub(crate) async fn detective_editor_pass(
    provider: &crate::agent_provider::AgentProvider,
    cfg: &crate::ai::AiConfig,
    draft_json: &str,
    outline_json: &str,
    campaign: Option<&DetectiveCampaign>,
    knowledge: &[KnowledgePoint],
    arc_focus: Option<u8>,
    case_id: &str,
) -> Result<Option<String>, String> {
    let user =
        detective_editor_user_prompt(draft_json, outline_json, campaign, knowledge, arc_focus);
    let json = detective_ai_json(
        provider,
        cfg.max_tokens,
        detective_editor_system_prompt(),
        user,
        0.1,
        15,
        &format!("detective-editor:{case_id}"),
    )
    .await?;
    #[derive(Deserialize)]
    struct Env {
        ok: Option<bool>,
    }
    if let Ok(env) = serde_json::from_str::<Env>(&json) {
        if env.ok == Some(true) {
            return Ok(None);
        }
    }
    Ok(Some(json))
}

/// Generate the campaign bible (世界観 layer) for one course. AI-required.
/// The world/era is DERIVED FROM the lecture subject matter — an American
/// Revolution course yields an 18th-century colonial setting, a statistics
/// course yields a "probability-ruled" world, etc. Generated once per course,
/// then read + advanced by each session.
pub(crate) async fn generate_campaign_bible(
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
                voice: c.voice.unwrap_or_default().trim().to_string(),
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
                setup: a.setup.unwrap_or_default().trim().to_string(),
                misdirection: a.misdirection.unwrap_or_default().trim().to_string(),
                session_band: a.session_band.unwrap_or_default().trim().to_string(),
            }
        })
        .collect();

    let relationships: Vec<CampaignRelationship> = draft
        .relationships
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| {
            let from = r.from.unwrap_or_default().trim().to_string();
            let to = r.to.unwrap_or_default().trim().to_string();
            let relation = r.relation.unwrap_or_default().trim().to_string();
            if from.is_empty() || to.is_empty() || relation.is_empty() {
                return None;
            }
            Some(CampaignRelationship {
                from,
                to,
                relation,
                tension: r.tension.unwrap_or_default().trim().to_string(),
            })
        })
        .take(5)
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
        meta_arc,
        finale,
        chapters: Vec::new(),
        relationships,
        canon: CampaignCanon::default(),
        created_at: now,
        updated_at: now,
    })
}

/// Load the cached campaign for a course, or generate + persist a fresh one.
pub(crate) async fn ensure_campaign(
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
/// only then augment from prose. The prompt still asks for 12–20 points; the
/// hard floor is `KNOWLEDGE_FLOOR`, and `gen_targets` scales a thinner chapter
/// down to fit whatever the note actually yielded.
pub(crate) async fn extract_knowledge_points(
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
    if out.len() < KNOWLEDGE_FLOOR {
        return Err(format!(
            "Knowledge extraction yielded only {} usable points; need at least {KNOWLEDGE_FLOOR}",
            out.len(),
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
pub(crate) async fn ensure_knowledge_points(
    db: &Database,
    course: &DetectiveCourse,
    live_id: &str,
    topic_hint: &str,
) -> Result<Vec<KnowledgePoint>, String> {
    if let Some(existing) = load_knowledge_points(db, &course.key, live_id) {
        if existing.len() >= KNOWLEDGE_FLOOR {
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

/// One raw source unit fed to the AI. Transient — these never appear in the
/// final `DetectiveCase`. The AI reads these and emits its own short
/// evidence cards (one paragraph of distilled information per card).
#[derive(Debug, Clone)]
pub(crate) struct EvidenceInputEntry {
    pub(crate) alias: String, // l1, s1, d1
    pub(crate) source_type: String,
    pub(crate) source: String,
    pub(crate) source_id: String,
    pub(crate) raw_title: String,
    pub(crate) raw_content: String,
    pub(crate) source_path: String,
    pub(crate) source_url: String,
    pub(crate) information_type: String,
    pub(crate) person_category_cd: String,
    pub(crate) category_cd: String,
}

/// Assemble the input package handed to the AI: full Live note bodies +
/// signal titles + doubt notes, each tagged with a short alias (l1/s1/d1).
pub(crate) fn build_evidence_input(course: &DetectiveCourse) -> Vec<EvidenceInputEntry> {
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
pub(crate) fn build_chapter_input(
    course: &DetectiveCourse,
    live_id: &str,
) -> Option<Vec<EvidenceInputEntry>> {
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
