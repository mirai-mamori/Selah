//! All Detective domain types + AI draft (deserialize) shapes + builders.
use crate::db::AiScheduleItem;
use serde::{Deserialize, Serialize};

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
    /// The 推理 spine of this chapter (明线): the actual truth, who's
    /// responsible, their motive, planted red herrings, and the deduction chain
    /// the busted contradictions reconstruct. Authored in the outline pass and
    /// kept coherent through drafting + editing.
    #[serde(default)]
    pub case_logic: CaseLogic,
    /// What this chapter contributes to the campaign's 暗线 — one concrete beat
    /// for its position in the arc. Recorded back into the campaign canon so
    /// later (independently generated) chapters stay mutually consistent.
    #[serde(default)]
    pub meta_beat: String,
}

/// The 推理 spine of a chapter — what really happened and why, separate from the
/// surface testimony. Authored in the outline pass, held consistent through
/// drafting + editing, and surfaced on the chapter-clear review.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseLogic {
    /// 1–3 sentences: the actual truth of the case (what really happened).
    #[serde(default)]
    pub truth: String,
    /// Who / what is responsible. Prefer a bible cast member by name.
    #[serde(default)]
    pub culprit: String,
    /// Why they did it / why they lie — means + opportunity folded in.
    #[serde(default)]
    pub motive: String,
    /// Plausible-but-wrong leads planted for fair-play misdirection.
    #[serde(default)]
    pub red_herrings: Vec<String>,
    /// Ordered steps: how the busted contradictions reconstruct the truth.
    /// The final step answers `final_question`.
    #[serde(default)]
    pub deduction_chain: Vec<String>,
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
    /// Web of relationships among the cast + the meta-antagonist — gives the
    /// world social tension that chapters can draw on.
    #[serde(default)]
    pub relationships: Vec<CampaignRelationship>,
    /// Living canon: facts every chapter must stay consistent with, plus the
    /// 暗线 hooks already dropped. Fed into every chapter's outline pass so
    /// independently generated chapters share one coherent world.
    #[serde(default)]
    pub canon: CampaignCanon,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignRelationship {
    /// Two parties (cast names, or the antagonist force) and how they relate.
    pub from: String,
    pub to: String,
    /// e.g. 「兄弟」「師弟」「対立」「秘密の協力者」.
    pub relation: String,
    /// One phrase of the underlying tension / unresolved friction.
    #[serde(default)]
    pub tension: String,
}

/// The shared, append-only story canon for a campaign. Fed into every chapter's
/// outline pass so independently generated chapters stay mutually consistent
/// (weak-continuity model: any play order, but one coherent world).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignCanon {
    /// Hard facts established about the world / cast (timeline, places,
    /// who-did-what) that no chapter may contradict.
    #[serde(default)]
    pub facts: Vec<String>,
    /// 暗线 hooks already dropped, so chapters vary their hints instead of
    /// repeating one detail.
    #[serde(default)]
    pub dropped_hooks: Vec<CanonHook>,
    /// Recurring-cast appearance log — "{name}: 「{chapter}」に登場" — so reused
    /// characters carry forward a felt history across chapters.
    #[serde(default)]
    pub cast_log: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonHook {
    /// Which meta-arc stage this hook served (0 = unknown).
    #[serde(default)]
    pub stage: u8,
    /// The hint that was dropped (kept so it isn't repeated verbatim).
    pub hook: String,
    /// Chapter (case id) that dropped it.
    #[serde(default)]
    pub chapter_id: String,
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
    /// Voice card — speech register / tics / first-person pronoun — so a reused
    /// character sounds the same from chapter to chapter.
    #[serde(default)]
    pub voice: String,
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
    /// Player-facing reveal text shown when the stage unlocks.
    pub reveal: String,
    #[serde(default)]
    pub unlocked: bool,
    /// Authoring guidance (not shown to the player): the hook chapters at this
    /// stage should plant to seed the 暗线.
    #[serde(default)]
    pub setup: String,
    /// Authoring guidance: the false lead that misdirects from this stage's
    /// truth, so the reveal lands as a fair-play surprise.
    #[serde(default)]
    pub misdirection: String,
    /// Soft guidance for which chapters carry this stage, as a 第N回 band, e.g.
    /// "1-3". Empty ⇒ derive from `threshold`.
    #[serde(default)]
    pub session_band: String,
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
pub(crate) struct CampaignBibleDraft {
    pub(crate) world_label: Option<String>,
    pub(crate) setting: Option<String>,
    pub(crate) tagline: Option<String>,
    pub(crate) meta_mystery: Option<String>,
    pub(crate) cast: Option<Vec<CampaignCharacterDraft>>,
    pub(crate) meta_arc: Option<Vec<CampaignArcDraft>>,
    pub(crate) relationships: Option<Vec<CampaignRelationshipDraft>>,
    pub(crate) finale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CampaignArcDraft {
    pub(crate) title: Option<String>,
    pub(crate) reveal: Option<String>,
    pub(crate) setup: Option<String>,
    pub(crate) misdirection: Option<String>,
    pub(crate) session_band: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CampaignRelationshipDraft {
    pub(crate) from: Option<String>,
    pub(crate) to: Option<String>,
    pub(crate) relation: Option<String>,
    pub(crate) tension: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CampaignCharacterDraft {
    pub(crate) name: Option<String>,
    pub(crate) role: Option<String>,
    pub(crate) bond: Option<String>,
    pub(crate) background: Option<String>,
    pub(crate) motivation: Option<String>,
    pub(crate) stake: Option<String>,
    pub(crate) voice: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DetectiveAiCaseDraft {
    pub(crate) title: Option<String>,
    pub(crate) case_type: Option<String>,
    pub(crate) difficulty: Option<u8>,
    pub(crate) briefing: Option<String>,
    pub(crate) scenario: Option<String>,
    pub(crate) witness_name: Option<String>,
    pub(crate) witness_role: Option<String>,
    pub(crate) final_question: Option<String>,
    /// Which 授業計画 第N回 this Live note matches (content alignment); 0/absent
    /// when the model can't tell.
    pub(crate) session_num: Option<i32>,
    pub(crate) acts: Option<Vec<DetectiveAiActDraft>>,
    /// Where each knowledge point landed. Hard-validated against the must-cover
    /// list + COVERAGE_MIN_POINTS in `apply_ai_case_draft`.
    pub(crate) coverage: Option<Vec<CoverageDraft>>,
    /// The 推理 spine the draft realized (carried from the outline pass, may be
    /// refined by the draft / editor pass).
    pub(crate) case_logic: Option<CaseLogicDraft>,
    /// What this chapter contributes to the campaign's 暗线.
    pub(crate) meta_beat: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CaseLogicDraft {
    pub(crate) truth: Option<String>,
    pub(crate) culprit: Option<String>,
    pub(crate) motive: Option<String>,
    pub(crate) red_herrings: Option<Vec<String>>,
    pub(crate) deduction_chain: Option<Vec<String>>,
}

/// Pass A output (the parts Rust reads back). The FULL outline — act plan,
/// coverage plan, per-act lie targets — is carried to the draft pass verbatim
/// as an embedded JSON string, so those fields don't need to round-trip through
/// Rust; only the logic spine / 暗线 beat / session number are reused here.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CaseOutlineDraft {
    pub(crate) session_num: Option<i32>,
    pub(crate) case_logic: Option<CaseLogicDraft>,
    pub(crate) meta_beat: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoverageDraft {
    pub(crate) point_id: Option<String>,
    pub(crate) placement: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DetectiveAiActDraft {
    pub(crate) id: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) location: Option<String>,
    pub(crate) narrative: Option<String>,
    pub(crate) seeds_meta: Option<bool>,
    /// Investigation acts: the evidence cards discovered in this beat.
    pub(crate) evidence: Option<Vec<DetectiveAiEvidenceDraft>>,
    /// Testimony acts: who testifies + their statements.
    pub(crate) witness_name: Option<String>,
    pub(crate) witness_role: Option<String>,
    pub(crate) testimony: Option<Vec<DetectiveAiTestimonyDraft>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DetectiveAiTestimonyDraft {
    pub(crate) id: Option<String>,
    pub(crate) text: Option<String>,
    pub(crate) is_false: Option<bool>,
    pub(crate) key_evidence_id: Option<String>,
    #[serde(default)]
    pub(crate) highlights: Option<Vec<String>>,
    pub(crate) press_response: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DetectiveAiEvidenceDraft {
    pub(crate) id: String,
    pub(crate) title: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) source_ref: Option<String>,
}

#[derive(Default)]
pub(crate) struct CourseBuilder {
    pub(crate) name: String,
    pub(crate) key: String,
    pub(crate) live_records: Vec<DetectiveLiveRecord>,
    pub(crate) exam_signals: Vec<DetectiveSignal>,
    pub(crate) schedule_items: Vec<AiScheduleItem>,
    pub(crate) doubts: Vec<DetectiveDoubt>,
    pub(crate) recent_results: Vec<DetectiveCaseResult>,
    pub(crate) latest_at: i64,
}
