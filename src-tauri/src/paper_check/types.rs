//! Serialisable data shapes exchanged with the PaperCheck frontend surface.

use serde::Serialize;

/// Document-level counts, shown while the heavier channels are still running.
#[derive(Debug, Clone, Serialize)]
pub struct TextStats {
    pub char_count: usize,
    pub sentence_count: usize,
}

/// Streamed progress for the (slow, many-request) calibration run.
#[derive(Debug, Clone, Serialize)]
pub struct CalibrationProgress {
    pub done: usize,
    pub total: usize,
}

/// Deterministic stylometric statistics — computed in Rust, no model, no
/// randomness. Every field is reproducible from the same input text.
#[derive(Debug, Clone, Serialize)]
pub struct FeatureStats {
    /// Burstiness of sentence lengths, (σ−μ)/(σ+μ). Human writing trends higher
    /// (more variable); AI text trends lower (more uniform).
    pub burstiness: f32,
    /// Coefficient of variation of sentence character-lengths (σ/μ).
    pub sentence_len_cv: f32,
    /// Moving-average type-token ratio — lexical diversity, length-robust.
    pub lexical_diversity: f32,
    /// Fraction of repeated word 3-grams (higher = more templated).
    pub ngram_repetition: f32,
    /// Transition-word density per sentence (AI over-uses connectives).
    pub transition_density: f32,
    /// Deflate compression ratio of the text (compressed/raw bytes). AI text is
    /// lower-entropy and compresses more (smaller ratio); a cheap, language-
    /// agnostic proxy for perplexity (cf. Jiang et al., ACL Findings 2023).
    pub compressibility: f32,
    /// Combined 0..1 feature-only AI score (higher = more AI-like).
    pub feature_ai_score: f32,
}

/// One fused decision channel, surfaced for transparency in the report.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelScore {
    /// Stable key ("feature" | "judge" | "dna" | "raidar").
    pub key: String,
    /// Localised label for display.
    pub label: String,
    /// Normalised 0..1 AI-likelihood used for fusion/display. Channels measure
    /// different raw quantities (probability, n-gram overlap, Jaccard) — each is
    /// mapped onto a common likelihood scale before averaging (see `score`).
    pub score: f32,
    /// The channel's raw measurement before normalisation (equals `score` for
    /// channels that already emit a likelihood).
    pub raw: f32,
    /// False when the channel could not run (e.g. AI disabled) and was skipped.
    pub available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentenceJudgement {
    pub text: String,
    /// 0..1 AI-authored likelihood from the LLM judge. Meaningless when
    /// `judged` is false.
    pub prob: f32,
    pub reason: String,
    /// False when the judge request failed for this sentence — such rows are
    /// excluded from the channel mean and must not be flagged in the UI.
    pub judged: bool,
    /// True when the sentence went through the second, precision-oriented
    /// review pass (flagged sentences are re-audited with context).
    pub reviewed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiRateResult {
    /// Final fused probability, 0..1.
    pub probability: f32,
    /// Display percentage, 0..100.
    pub percent: u8,
    /// "low" | "medium" | "high" confidence in the estimate.
    pub confidence: String,
    /// Whether a calibration set has been fitted; if false the number is a
    /// prior/uncalibrated estimate and must be labelled as such in the UI.
    pub calibrated: bool,
    /// Measured accuracy of the method on the local validation set, if calibrated.
    pub method_accuracy: Option<f32>,
    /// Wilson 95% interval for `method_accuracy` — the honest range to show
    /// given the small validation n. `None` for pre-interval calibrations.
    pub accuracy_low: Option<f32>,
    pub accuracy_high: Option<f32>,
    /// Size of the validation set behind `method_accuracy`.
    pub validation_n: Option<usize>,
    pub features: FeatureStats,
    /// Per-channel breakdown behind `probability` (feature/judge/DNA-GPT/Raidar).
    pub channels: Vec<ChannelScore>,
    pub sentences: Vec<SentenceJudgement>,
    /// Human-readable caveats/notes (e.g. "judge unavailable, features only").
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimilarityMatch {
    pub sentence: String,
    pub source_url: String,
    pub source_title: String,
    pub snippet: String,
    /// 0..1 overlap between the sentence and the matched source span.
    pub overlap: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimilarityResult {
    /// Overall estimated duplicated fraction, 0..100.
    pub overall_pct: u8,
    pub matches: Vec<SimilarityMatch>,
    /// Whether a search backend actually ran (false → degraded/manual mode).
    pub available: bool,
    pub notes: Vec<String>,
}
