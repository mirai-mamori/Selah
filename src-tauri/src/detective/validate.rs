//! Validate + apply an AI draft onto a case; content-cleanliness gates; JSON extraction.
use super::*;
use std::collections::{HashMap, HashSet};

/// Clean + validate one evidence card from an investigation act. `seen_ids`
/// guards against duplicate ids across the whole chapter pool.
pub(crate) fn clean_evidence_card(
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
pub(crate) fn clean_testimony_statement(
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
pub(crate) fn apply_ai_case_draft(
    mut case: DetectiveCase,
    draft: DetectiveAiCaseDraft,
    input: &[EvidenceInputEntry],
    knowledge: &[KnowledgePoint],
    targets: &GenTargets,
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
    if act_drafts.len() < targets.acts_min || act_drafts.len() > targets.acts_max {
        return Err(format!(
            "AI produced {} acts; a chapter needs {}–{} acts",
            act_drafts.len(),
            targets.acts_min,
            targets.acts_max
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
    if total_lies < targets.lies_min {
        return Err(format!(
            "chapter has only {total_lies} lies across its testimony acts; need at least {}",
            targets.lies_min
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
        if covered_points.len() < targets.coverage_min {
            return Err(format!(
                "chapter covered only {} knowledge points; need at least {}",
                covered_points.len(),
                targets.coverage_min
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

    case.session_num = draft.session_num.unwrap_or(0).clamp(0, 99) as u8;

    if let Some(logic) = draft.case_logic {
        case.case_logic = resolve_case_logic(logic);
    }
    if let Some(beat) = clean_ai_text(draft.meta_beat, 300)
        .filter(|t| !looks_like_metadata_leak(t) && !looks_like_platitude(t))
    {
        case.meta_beat = beat;
    }

    case.generation_mode = "ai".to_string();
    Ok(case)
}

/// Resolve an AI `CaseLogicDraft` into the stored `CaseLogic`, trimming and
/// bounding each field. Empty / metadata-leak fragments are dropped.
pub(crate) fn resolve_case_logic(draft: CaseLogicDraft) -> CaseLogic {
    let clean_one = |s: Option<String>, max: usize| -> String {
        clean_ai_text(s, max)
            .filter(|t| !looks_like_metadata_leak(t))
            .unwrap_or_default()
    };
    let clean_list = |v: Option<Vec<String>>, max: usize, cap: usize| -> Vec<String> {
        v.unwrap_or_default()
            .into_iter()
            .filter_map(|s| clean_ai_text(Some(s), max))
            .filter(|t| !looks_like_metadata_leak(t))
            .take(cap)
            .collect()
    };
    CaseLogic {
        truth: clean_one(draft.truth, 300),
        culprit: clean_one(draft.culprit, 60),
        motive: clean_one(draft.motive, 240),
        red_herrings: clean_list(draft.red_herrings, 160, 4),
        deduction_chain: clean_list(draft.deduction_chain, 200, 8),
    }
}

/// Reject text that includes metadata patterns (filenames, ISO dates) that
/// must NEVER appear in a content-derived Detective case.
pub(crate) fn looks_like_metadata_leak(text: &str) -> bool {
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
pub(crate) fn looks_like_generic_label(text: &str) -> bool {
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
pub(crate) fn looks_like_platitude(text: &str) -> bool {
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
pub(crate) fn looks_like_admin_trivia(text: &str) -> bool {
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

pub(crate) fn contains_iso_date(text: &str) -> bool {
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

pub(crate) fn clean_ai_text(value: Option<String>, max_chars: usize) -> Option<String> {
    let text = value?;
    let text = text.trim().trim_matches('"').trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(truncate_chars(&text, max_chars))
    }
}

pub(crate) fn is_allowed_case_type(value: &str) -> bool {
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
pub(crate) fn extract_json_object(text: &str) -> Option<String> {
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
