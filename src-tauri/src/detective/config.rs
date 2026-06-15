//! Tuning constants, generation targets, and per-chapter planning.
use super::*;

pub(crate) const DETECTIVE_DOUBTS_KEY: &str = "detective_doubts_v1";
pub(crate) const DETECTIVE_RESULTS_KEY: &str = "detective_case_results_v1";
pub(crate) const DETECTIVE_INCLUDED_KEY: &str = "detective_included_courses_v1";
pub(crate) const DETECTIVE_MEMORY_KEY: &str = "detective_memory_v1";
/// Per-course campaign bible key prefix (+ course_key).
pub(crate) const DETECTIVE_CAMPAIGN_PREFIX: &str = "detective_campaign_v1:";
/// Per-chapter cached case key prefix (+ "{course_key}:{live_id}"). Each Live
/// note is one chapter; its generated case is cached so replays are instant and
/// new lectures only add NEW chapters (existing ones are never regenerated).
pub(crate) const DETECTIVE_CHAPTER_PREFIX: &str = "detective_chapter_v1:";
/// Per-course alignment cache (+ course_key): maps a Live note id to the 授業計画
/// 第N回 it was matched to BY CONTENT. Survives world rebuilds (alignment is
/// about what the lecture covered, not the campaign dressing).
pub(crate) const DETECTIVE_ALIGN_PREFIX: &str = "detective_align_v1:";
/// Per-chapter extracted knowledge-point checklist (+ "{course_key}:{live_id}").
/// Drives the chapter generator + the coverage check. Built once per Live note
/// (cheap AI call) and cached; survives world rebuilds.
pub(crate) const DETECTIVE_KNOWLEDGE_PREFIX: &str = "detective_knowledge_v1:";

/// A chapter MUST cover at least this many knowledge points across its
/// evidence cards + testimony statements, on top of covering every
/// `must_cover: true` point.
pub(crate) const COVERAGE_MIN_POINTS: usize = 10;
pub(crate) const LIVE_EXCERPT_MAX_CHARS: usize = 8000;
pub(crate) const LIVE_EXCERPT_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Minimum number of lies (one per testimony act) across a whole chapter.
/// Bumped from 2 → 3 so a chapter forces enough testimony beats to cover the
/// lecture's surface area properly.
pub(crate) const SESSION_LIES_MIN: usize = 3;

/// A chapter (1 case) is split into mixed investigation / testimony acts.
/// 6–8 acts so one lecture's content actually gets the room to breathe; an
/// investigation act bears 2–4 evidence cards and a testimony act 3–5
/// statements (see the prompt).
pub(crate) const ACTS_MIN: usize = 6;
pub(crate) const ACTS_MAX: usize = 8;
/// Each investigation act must reveal at least this many evidence cards.
pub(crate) const INVESTIGATION_EV_MIN: usize = 2;
/// Each testimony act must contain at least this many statements (still ONE lie).
pub(crate) const TESTIMONY_STMT_MIN: usize = 3;
/// Absolute floor of usable knowledge points a Live note must yield to be
/// playable at all. Below the full `COVERAGE_MIN_POINTS` target we scale the
/// chapter DOWN (see `gen_targets`) instead of hard-failing, so a thin lecture
/// still produces a tight real chapter rather than an error.
pub(crate) const KNOWLEDGE_FLOOR: usize = 5;

/// Structure targets for one chapter, scaled to how much testable content the
/// Live note actually yielded. A rich lecture gets the full 6–8 act / 10-point
/// chapter; a thin one gets a smaller-but-still-real chapter rather than
/// repeated validation failures or filler padding.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GenTargets {
    pub(crate) acts_min: usize,
    pub(crate) acts_max: usize,
    pub(crate) coverage_min: usize,
    pub(crate) lies_min: usize,
}

pub(crate) fn gen_targets(knowledge_len: usize) -> GenTargets {
    if knowledge_len >= COVERAGE_MIN_POINTS + 2 {
        GenTargets {
            acts_min: ACTS_MIN,
            acts_max: ACTS_MAX,
            coverage_min: COVERAGE_MIN_POINTS,
            lies_min: SESSION_LIES_MIN,
        }
    } else if knowledge_len >= 8 {
        GenTargets {
            acts_min: 4,
            acts_max: 6,
            coverage_min: knowledge_len.saturating_sub(1).min(8),
            lies_min: 2,
        }
    } else {
        GenTargets {
            acts_min: 4,
            acts_max: 5,
            coverage_min: knowledge_len.clamp(3, 6),
            lies_min: 2,
        }
    }
}

/// Rotating dramatic templates so a season's chapters don't all feel like the
/// same "spot the lie" beat. Picked by chapter ordinal — pure authoring hint.
pub(crate) const CASE_ARCHETYPES: &[&str] = &[
    "食い違う二つの証言（どちらかが嘘）",
    "崩れる不在証明（アリバイ崩し）",
    "濡れ衣を着せられた者（誤った告発を覆す）",
    "二重の嘘（最初の自白がミスリード）",
    "見落とされた一枚（決定的な証拠の欠落）",
    "因果の取り違え（順序・原因の逆転）",
];

/// Per-chapter authoring plan derived from the chapter's position in the season
/// (independent of play order). Drives which 暗线 stage the chapter advances and
/// which dramatic archetype it uses, so the serialized story stays coherent
/// even when chapters are generated / replayed out of order.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ChapterPlan {
    /// 1-based meta-arc stage this chapter should plant toward (None = no arc).
    pub(crate) arc_focus: Option<u8>,
    /// Total meta-arc stages (for "how present is the antagonist" escalation).
    pub(crate) arc_total: usize,
    /// Rotating dramatic archetype hint.
    pub(crate) archetype: &'static str,
}

/// Build the chapter plan from the chapter's ordinal among the course's Live
/// notes (download order) mapped onto the campaign's meta-arc + archetype list.
pub(crate) fn chapter_plan(
    course: &DetectiveCourse,
    live_id: &str,
    campaign: Option<&DetectiveCampaign>,
) -> ChapterPlan {
    let mut recs: Vec<&DetectiveLiveRecord> = course.live_records.iter().collect();
    recs.sort_by(|a, b| a.downloaded_at.cmp(&b.downloaded_at));
    let idx = recs.iter().position(|r| r.id == live_id).unwrap_or(0);
    let total = recs.len().max(1);
    let archetype = CASE_ARCHETYPES[idx % CASE_ARCHETYPES.len()];
    let (arc_focus, arc_total) = match campaign {
        Some(c) if !c.meta_arc.is_empty() => {
            let arc_len = c.meta_arc.len();
            // Map this chapter's position in the season onto the arc stages.
            let frac = (idx as f32 + 1.0) / total as f32; // (0, 1]
            let stage = ((frac * arc_len as f32).ceil() as usize).clamp(1, arc_len);
            (Some(stage as u8), arc_len)
        }
        _ => (None, 0),
    };
    ChapterPlan {
        arc_focus,
        arc_total,
        archetype,
    }
}

pub(crate) const EXAM_KEYWORDS: &[&str] = &[
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

