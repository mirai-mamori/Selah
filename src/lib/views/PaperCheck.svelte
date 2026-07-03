<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke, Channel } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { applyAuxiliaryTheme, syncAuxiliaryTheme } from "../auxiliarySurfaceTheme";

  interface FeatureStats {
    burstiness: number;
    sentence_len_cv: number;
    lexical_diversity: number;
    ngram_repetition: number;
    transition_density: number;
    compressibility: number;
    feature_ai_score: number;
  }
  interface SentenceJudgement {
    text: string;
    prob: number;
    reason: string;
    judged: boolean;
    reviewed: boolean;
  }
  interface ChannelScore {
    key: string;
    label: string;
    score: number;
    raw: number;
    available: boolean;
  }
  interface AiRateResult {
    probability: number;
    percent: number;
    confidence: string;
    calibrated: boolean;
    method_accuracy: number | null;
    accuracy_low: number | null;
    accuracy_high: number | null;
    validation_n: number | null;
    features: FeatureStats;
    channels: ChannelScore[];
    sentences: SentenceJudgement[];
    notes: string[];
  }
  interface SimilarityMatch {
    sentence: string;
    source_url: string;
    source_title: string;
    snippet: string;
    overlap: number;
  }
  interface SimilarityResult {
    overall_pct: number;
    matches: SimilarityMatch[];
    available: boolean;
    notes: string[];
  }
  interface TextStats {
    char_count: number;
    sentence_count: number;
  }
  interface Calibration {
    calibrated: boolean;
    method_accuracy: number;
    accuracy_low: number;
    accuracy_high: number;
    false_positive_rate: number;
    false_negative_rate: number;
    validation_n: number;
    /// Raw decision threshold (internal fused-probability space).
    threshold: number;
    /// The same threshold in the display-rescaled space the percent uses —
    /// what the verdict bands must be centred on.
    display_threshold: number;
  }
  interface SearchConfig {
    provider: string;
    brave_api_key: string;
    google_api_key: string;
    google_cx: string;
  }

  /// Sentence flag threshold — shared by the list filter, the colour bands and
  /// the exported markdown so they can never disagree. Aligned with the
  /// judge's "two real signatures" ladder step (0.55) minus margin, and with
  /// the backend REVIEW_THRESHOLD: everything flagged has been re-audited.
  const FLAG_THRESHOLD = 0.5;
  /// Query budget for the web similarity check (cost/time).
  const MAX_SIMILARITY_QUERIES = 12;

  let text = $state("");
  let filename = $state("");
  let extracting = $state(false);
  let calibrating = $state(false);
  let calibProgress = $state<{ done: number; total: number } | null>(null);
  let error = $state("");
  // The two pipelines run as separate backend commands so the faster one (AI
  // rate) renders while the similarity crawl is still working.
  let started = $state(false);
  let aiPending = $state(false);
  let simPending = $state(false);
  let aiResult = $state<AiRateResult | null>(null);
  let simResult = $state<SimilarityResult | null>(null);
  let aiError = $state("");
  let simError = $state("");
  let stats = $state<TextStats | null>(null);
  // Monotonic run id: results arriving after クリア/新規 are dropped.
  let runId = 0;
  let calibration = $state<Calibration | null>(null);
  let calibSamples = $state<{ name: string; text: string }[]>([]);
  let calibAiSamples = $state<{ name: string; text: string }[]>([]);
  let calibFileInput = $state<HTMLInputElement | null>(null);
  let calibAiFileInput = $state<HTMLInputElement | null>(null);
  let runAiRate = $state(true);
  let runSimilarity = $state(true);
  let dragOver = $state(false);
  let fileInput = $state<HTMLInputElement | null>(null);

  const busy = $derived(aiPending || simPending);

  let showSettings = $state(false);
  let previewExpanded = $state(false);
  let showFeatures = $state(false);
  let searchConfig = $state<SearchConfig>({ provider: "free", brave_api_key: "", google_api_key: "", google_cx: "" });

  const providerOptions = [
    { v: "free", label: "無料" },
    { v: "brave", label: "Brave" },
    { v: "google", label: "Google" },
  ];
  let savingSettings = $state(false);
  let settingsSaved = $state(false);
  let copied = $state(false);
  let savedPath = $state("");

  const charCount = $derived(text.replace(/\s/g, "").length);
  const previewText = $derived(text.replace(/\s+/g, " ").trim().slice(0, 180));

  // Collapse runs of spaces/tabs/full-width spaces (docx extraction leaves ugly
  // gaps) while keeping paragraph breaks. Applied to both display and analysis.
  function tidy(raw: string): string {
    return raw
      .split("\n")
      .map((line) => line.replace(/[ \t　]+/g, " ").trimEnd())
      .join("\n")
      .replace(/\n{3,}/g, "\n\n");
  }

  function fileToBase64(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => {
        const result = typeof reader.result === "string" ? reader.result : "";
        const comma = result.indexOf(",");
        resolve(comma >= 0 ? result.slice(comma + 1) : "");
      };
      reader.onerror = () => reject(new Error("読み込みに失敗しました"));
      reader.readAsDataURL(file);
    });
  }

  async function extractFileText(file: File): Promise<string> {
    // Plain-text files can be read directly; anything else goes through the
    // Rust extractor (PDF/docx/pptx/xlsx) via base64.
    if (/\.(txt|md|csv|json|html?)$/i.test(file.name)) {
      return tidy(await file.text());
    }
    const fileBase64 = await fileToBase64(file);
    return tidy(await invoke<string>("paper_check_extract_text", { fileBase64, filename: file.name }));
  }

  function resetResults(): void {
    runId += 1;
    started = false;
    aiPending = false;
    simPending = false;
    aiResult = null;
    simResult = null;
    aiError = "";
    simError = "";
    stats = null;
  }

  async function ingestFile(file: File): Promise<void> {
    error = "";
    extracting = true;
    filename = file.name;
    try {
      text = await extractFileText(file);
      resetResults();
      previewExpanded = false;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      extracting = false;
    }
  }

  function onPick(event: Event): void {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (file) void ingestFile(file);
    input.value = "";
  }

  function onDrop(event: DragEvent): void {
    event.preventDefault();
    dragOver = false;
    const file = event.dataTransfer?.files?.[0];
    if (file) void ingestFile(file);
  }

  function analyze(): void {
    if (busy || charCount < 40 || (!runAiRate && !runSimilarity)) return;
    resetResults();
    const id = runId;
    error = "";
    started = true;

    void invoke<TextStats>("paper_check_text_stats", { text })
      .then((s) => { if (id === runId) stats = s; })
      .catch(() => {});

    if (runAiRate) {
      aiPending = true;
      void invoke<AiRateResult>("paper_check_analyze_ai", { text })
        .then((r) => { if (id === runId) aiResult = r; })
        .catch((e) => { if (id === runId) aiError = e instanceof Error ? e.message : String(e); })
        .finally(() => { if (id === runId) aiPending = false; });
    }
    if (runSimilarity) {
      simPending = true;
      void invoke<SimilarityResult>("paper_check_analyze_similarity", { text, maxQueries: MAX_SIMILARITY_QUERIES })
        .then((r) => { if (id === runId) simResult = r; })
        .catch((e) => { if (id === runId) simError = e instanceof Error ? e.message : String(e); })
        .finally(() => { if (id === runId) simPending = false; });
    }
  }

  async function calibrate(): Promise<void> {
    if (calibrating) return;
    calibrating = true;
    calibProgress = null;
    error = "";
    try {
      const onProgress = new Channel<{ done: number; total: number }>();
      onProgress.onmessage = (p) => { calibProgress = p; };
      calibration = await invoke<Calibration>("paper_check_calibrate", {
        humanSamples: calibSamples.map((s) => s.text),
        aiSamples: calibAiSamples.map((s) => s.text),
        onProgress,
      });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      calibrating = false;
      calibProgress = null;
    }
  }

  /// Add calibration sample files: the user's own writing (label "human") or
  /// documents they know were AI-written (label "ai") — the latter ground the
  /// fit in real threat-model data instead of only generated counterparts.
  async function addCalibFiles(event: Event, kind: "human" | "ai"): Promise<void> {
    const input = event.currentTarget as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = "";
    error = "";
    for (const file of files) {
      const list = kind === "human" ? calibSamples : calibAiSamples;
      if (list.length >= 8) {
        error = "校正サンプルは各8件までです。";
        break;
      }
      try {
        const extracted = await extractFileText(file);
        if (extracted.replace(/\s/g, "").length < 200) {
          error = `「${file.name}」は短すぎます(200文字以上必要)。`;
          continue;
        }
        const entry = { name: file.name, text: extracted };
        if (kind === "human") calibSamples = [...calibSamples, entry];
        else calibAiSamples = [...calibAiSamples, entry];
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
      }
    }
  }

  function removeCalibSample(kind: "human" | "ai", index: number): void {
    if (kind === "human") calibSamples = calibSamples.filter((_, i) => i !== index);
    else calibAiSamples = calibAiSamples.filter((_, i) => i !== index);
  }

  function openSource(url: string): void {
    void invoke("open_external_url", { url }).catch(() => {});
  }

  async function saveSearchConfig(): Promise<void> {
    if (savingSettings) return;
    savingSettings = true;
    settingsSaved = false;
    error = "";
    try {
      await invoke("paper_check_save_search_config", { config: searchConfig });
      settingsSaved = true;
      setTimeout(() => (settingsSaved = false), 2000);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      savingSettings = false;
    }
  }

  function buildMarkdown(): string {
    const lines: string[] = ["# 論文チェック レポート", ""];
    if (stats) {
      lines.push(`- 文字数: ${stats.char_count}`, `- 文数: ${stats.sentence_count}`, "");
    }
    const ai = aiResult;
    if (ai) {
      lines.push("## AI生成の可能性", "");
      lines.push(`**${ai.percent}%**(痕跡: ${level(ai.percent).label} ・ 信頼度: ${confidenceLabel(ai.confidence)})`);
      lines.push(
        ai.calibrated && ai.method_accuracy != null
          ? `検証${ai.validation_n}件で精度 ${accuracyText(ai.method_accuracy, ai.accuracy_low, ai.accuracy_high)}${ai.accuracy_high != null && ai.accuracy_high > 0 ? "(95%区間)" : ""}`
          : "未校正 — 精度未測定の暫定値",
        "",
      );
      {
        const judged = ai.sentences.filter((s) => s.judged).length;
        const flagged = ai.sentences.filter((s) => s.judged && s.prob >= FLAG_THRESHOLD).length;
        if (judged > 0) {
          lines.push(`高リスク文: ${flagged}/${judged}文(${Math.round((flagged / judged) * 100)}%)`, "");
        }
      }
      if (ai.channels.length) {
        lines.push(
          "判定内訳: " +
            ai.channels
              .map((c) => `${c.label} ${c.available ? `${Math.round(c.score * 100)}%` : "—"}`)
              .join(" / "),
          "",
        );
      }
      lines.push(
        `統計特徴: バースト性 ${ai.features.burstiness.toFixed(2)} / 語彙多様性 ${ai.features.lexical_diversity.toFixed(2)} / n-gram反復 ${pct(ai.features.ngram_repetition)} / 接続語密度 ${ai.features.transition_density.toFixed(2)} / 圧縮率 ${ai.features.compressibility.toFixed(2)}`,
        "",
      );
      const flagged = ai.sentences.filter((s) => s.judged && s.prob >= FLAG_THRESHOLD);
      if (flagged.length) {
        lines.push("### 注意が必要な文", "");
        for (const s of flagged) lines.push(`- (${Math.round(s.prob * 100)}%) ${s.text} — ${s.reason}`);
        lines.push("");
      }
    }
    if (simResult) {
      lines.push("## 重複(ウェブ照合)", "");
      lines.push(`重複率 **${simResult.overall_pct}%** ・ ${simResult.matches.length} 箇所一致`, "");
      for (const m of simResult.matches) {
        lines.push(`- (${Math.round(m.overlap * 100)}%一致) ${m.sentence}`);
        lines.push(`  - 出典: ${m.source_title || m.source_url} — ${m.source_url}`);
      }
      lines.push("");
    }
    lines.push(
      "---",
      "※ AI検出は科学的に不確実で誤判定があり得ます。学術不正の認定根拠にはなりません。",
      "※ 表示値は20%(痕跡なし=検出下限)〜95%の範囲。20%は人間作の証明ではありません。",
    );
    return lines.join("\n");
  }

  async function exportReport(): Promise<void> {
    if (!aiResult && !simResult) return;
    try {
      await navigator.clipboard.writeText(buildMarkdown());
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function saveReportFile(): Promise<void> {
    if (!aiResult && !simResult) return;
    const stamp = new Date().toISOString().slice(0, 16).replace(/[:T]/g, "-");
    const name = `${filename ? filename.replace(/\.[^.]+$/, "") : "paper_check"}_${stamp}`;
    try {
      savedPath = await invoke<string>("paper_check_save_report", { markdown: buildMarkdown(), filename: name });
      setTimeout(() => (savedPath = ""), 4000);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function clearAll(): void {
    text = "";
    filename = "";
    error = "";
    previewExpanded = false;
    resetResults();
  }

  // ── Presentation helpers ──
  function level(percent: number): { label: string; cls: string } {
    // When calibrated, centre the bands on the fitted decision threshold so the
    // "高/中/低" verdict matches the operating point behind the accuracy number;
    // otherwise fall back to neutral thirds. The percent shown is display-
    // rescaled, so the bands use the threshold mapped into that same space.
    let mid = 33;
    let high = 66;
    const t = calibration?.display_threshold || calibration?.threshold || 0;
    if (calibration?.calibrated && t > 0) {
      mid = Math.round(t * 100);
      high = Math.round(mid + (100 - mid) / 2);
    }
    if (percent >= high) return { label: "高", cls: "high" };
    if (percent >= mid) return { label: "中", cls: "mid" };
    return { label: "低", cls: "low" };
  }
  function probClass(s: SentenceJudgement): string {
    if (!s.judged) return "s-na";
    if (s.prob >= 0.66) return "s-high";
    if (s.prob >= FLAG_THRESHOLD) return "s-mid";
    return "s-low";
  }
  function confidenceLabel(c: string): string {
    return c === "high" ? "高" : c === "medium" ? "中" : "低";
  }
  const pct = (v: number) => `${Math.round(v * 100)}%`;

  // Honest accuracy display: prefer the Wilson 95% range (a point estimate on
  // n≈15–30 validation samples overstates certainty), fall back to the point
  // value for calibrations saved before intervals existed.
  function accuracyText(acc: number | null, low: number | null, high: number | null): string {
    if (low != null && high != null && high > 0) {
      return `${Math.round(low * 100)}–${Math.round(high * 100)}%`;
    }
    return acc != null ? pct(acc) : "";
  }

  // Circular gauge geometry (custom SVG ring — the result hero's focal point).
  const GAUGE_R = 52;
  const GAUGE_C = 2 * Math.PI * GAUGE_R;
  const gaugeOffset = (percent: number) => GAUGE_C * (1 - Math.max(0, Math.min(100, percent)) / 100);

  let showAllSentences = $state(false);
  // Unjudged rows (failed judge batches) are never flagged — they carry no
  // verdict, only a "could not evaluate" marker in the full list.
  const flaggedSentences = $derived(
    aiResult?.sentences.filter((s) => s.judged && s.prob >= FLAG_THRESHOLD) ?? [],
  );
  const shownSentences = $derived(
    showAllSentences ? (aiResult?.sentences ?? []) : flaggedSentences,
  );
  // Fraction of judged sentences that were flagged — the more meaningful
  // headline for mixed-authorship documents (a human draft with AI-polished
  // sections dilutes the document-level mean, but not this ratio).
  const judgedCount = $derived(aiResult?.sentences.filter((s) => s.judged).length ?? 0);
  const flaggedPct = $derived(
    judgedCount > 0 ? Math.round((flaggedSentences.length / judgedCount) * 100) : null,
  );

  let themeUnlisten: (() => void) | null = null;
  let appThemeUnlisten: (() => void) | null = null;

  onMount(async () => {
    document.documentElement.setAttribute("data-aux-surface", "paper-check");
    document.body.setAttribute("data-aux-surface", "paper-check");
    await syncAuxiliaryTheme();
    themeUnlisten = await listen<string>("theme-changed", (e) => applyAuxiliaryTheme(e.payload)).catch(() => null);
    appThemeUnlisten = await listen("app-theme-changed", () => void syncAuxiliaryTheme()).catch(() => null);
    try {
      calibration = await invoke<Calibration>("paper_check_get_calibration");
    } catch {}
    try {
      searchConfig = await invoke<SearchConfig>("paper_check_get_search_config");
    } catch {}
  });

  onDestroy(() => {
    themeUnlisten?.();
    appThemeUnlisten?.();
    document.documentElement.removeAttribute("data-aux-surface");
    document.body.removeAttribute("data-aux-surface");
  });
</script>

<main class="pc-root">
  <div class="pc-inner">
    <header class="pc-head">
      <div class="pc-head-row">
        <div class="pc-head-text">
          <h1 class="pc-title">論文<span class="pc-title-accent">チェック</span></h1>
          <p class="pc-sub">AI生成の可能性と重複を、科学的に推定します。</p>
        </div>
        <button class="pc-icon-btn" class:on={showSettings} type="button" aria-label="検索設定・校正" onclick={() => (showSettings = !showSettings)}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
        </button>
      </div>
    </header>

    {#if showSettings}
      <section class="pc-panel">
        <div class="pc-set-group">
          <div class="pc-set-head">
            <span class="pc-set-ico" aria-hidden="true">
              <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
            </span>
            <div>
              <div class="pc-set-title">重複チェックの検索</div>
              <div class="pc-set-sub">既定は無料・キー不要ですが、レート制限で不安定になることがあります。無料枠のAPIキーを登録すると安定します。キーはこの端末内のみに平文で保存されます。</div>
            </div>
          </div>
          <div class="pc-segmented" role="radiogroup" aria-label="検索プロバイダ">
            {#each providerOptions as opt}
              <button
                class="pc-seg"
                class:on={searchConfig.provider === opt.v}
                type="button"
                role="radio"
                aria-checked={searchConfig.provider === opt.v}
                onclick={() => (searchConfig.provider = opt.v)}
              >{opt.label}</button>
            {/each}
          </div>
          {#if searchConfig.provider === "brave"}
            <label class="pc-field">
              <span>Brave API キー</span>
              <input type="password" bind:value={searchConfig.brave_api_key} placeholder="X-Subscription-Token" spellcheck="false" />
            </label>
          {:else if searchConfig.provider === "google"}
            <label class="pc-field">
              <span>Google API キー</span>
              <input type="password" bind:value={searchConfig.google_api_key} placeholder="API key" spellcheck="false" />
            </label>
            <label class="pc-field">
              <span>検索エンジン ID (cx)</span>
              <input type="text" bind:value={searchConfig.google_cx} placeholder="Programmable Search Engine ID" spellcheck="false" />
            </label>
          {/if}
          <div class="pc-set-foot">
            {#if settingsSaved}<span class="pc-saved">保存しました</span>{/if}
            <button class="pc-btn primary small" type="button" onclick={saveSearchConfig} disabled={savingSettings}>
              {#if savingSettings}<span class="pc-spinner small"></span>保存中…{:else}保存{/if}
            </button>
          </div>
        </div>

        <div class="pc-set-group">
          <div class="pc-set-head">
            <span class="pc-set-ico" aria-hidden="true">
              <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v3M12 18v3M3 12h3M18 12h3"/><circle cx="12" cy="12" r="3.4"/></svg>
            </span>
            <div>
              <div class="pc-set-title">AI率の校正</div>
              <div class="pc-set-sub">検証セット(人間の文 + AIの文)で精度を実測します。校正すると暫定値ではなくなります。自分で書いたレポートと、AIが書いたと分かっているレポートを追加すると、実際の文体・脅威モデルに合った校正になります。</div>
            </div>
          </div>
          <div class="pc-calib-samples">
            <span class="pc-calib-kind">自分が書いた:</span>
            {#each calibSamples as s, i}
              <span class="pc-chip" title={s.name}>
                <span class="pc-chip-name">{s.name}</span>
                <button class="pc-chip-x" type="button" aria-label="削除" onclick={() => removeCalibSample("human", i)}>×</button>
              </span>
            {/each}
            <button class="pc-pill" type="button" onclick={() => calibFileInput?.click()} disabled={calibrating}>
              + 追加
            </button>
          </div>
          <div class="pc-calib-samples">
            <span class="pc-calib-kind">AIが書いた:</span>
            {#each calibAiSamples as s, i}
              <span class="pc-chip ai" title={s.name}>
                <span class="pc-chip-name">{s.name}</span>
                <button class="pc-chip-x" type="button" aria-label="削除" onclick={() => removeCalibSample("ai", i)}>×</button>
              </span>
            {/each}
            <button class="pc-pill" type="button" onclick={() => calibAiFileInput?.click()} disabled={calibrating}>
              + 追加
            </button>
          </div>
          <input bind:this={calibFileInput} type="file" multiple accept=".pdf,.docx,.pptx,.txt,.md" hidden onchange={(e) => addCalibFiles(e, "human")} />
          <input bind:this={calibAiFileInput} type="file" multiple accept=".pdf,.docx,.pptx,.txt,.md" hidden onchange={(e) => addCalibFiles(e, "ai")} />
          <div class="pc-set-foot between">
            {#if calibration?.calibrated}
              <span class="pc-calib-pill ok">精度 {accuracyText(calibration.method_accuracy, calibration.accuracy_low, calibration.accuracy_high)} ・ 誤検出 {pct(calibration.false_positive_rate)} ・ 見逃し {pct(calibration.false_negative_rate)} ・ 検証{calibration.validation_n}件</span>
            {:else}
              <span class="pc-calib-pill">未校正 — 暫定値</span>
            {/if}
            <button class="pc-btn ghost small" type="button" onclick={calibrate} disabled={calibrating}>
              {#if calibrating}
                <span class="pc-spinner small"></span>
                校正中… {calibProgress ? `${calibProgress.done}/${calibProgress.total}` : ""}
              {:else}検証セットで校正{/if}
            </button>
          </div>
        </div>
      </section>
    {/if}

    {#if error}
      <div class="pc-error" role="alert">{error}</div>
    {/if}

    {#if !started}
      <!-- ── INPUT PHASE ── -->
      <div
        class="pc-uploader"
        class:over={dragOver}
        class:loaded={!!text && !extracting}
        role="button"
        tabindex="0"
        aria-label="ファイルをドロップまたは選択"
        ondragover={(e) => { e.preventDefault(); dragOver = true; }}
        ondragleave={() => (dragOver = false)}
        ondrop={onDrop}
        onclick={() => fileInput?.click()}
        onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") fileInput?.click(); }}
      >
        <div class="pc-uploader-glow" aria-hidden="true"></div>
        {#if extracting}
          <span class="pc-upload-badge scanning" aria-hidden="true">
            <svg class="pc-scan-icon" width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><path d="M8 13h8M8 17h5"/></svg>
            <span class="pc-scan"></span>
          </span>
          <span class="pc-upload-title">テキストを抽出中…</span>
          <span class="pc-upload-hint">しばらくお待ちください</span>
        {:else if text}
          <span class="pc-upload-badge done" aria-hidden="true">
            <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
          </span>
          <span class="pc-upload-title file" title={filename}>{filename || "アップロード済み"}</span>
          <span class="pc-upload-hint">{charCount} 文字 ・ クリックで別のファイルに変更</span>
        {:else}
          <span class="pc-upload-badge" aria-hidden="true">
            <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 16V4"/><path d="m7 9 5-5 5 5"/><path d="M4 20h16"/></svg>
          </span>
          <span class="pc-upload-title">ファイルをドロップ / クリックして選択</span>
          <span class="pc-upload-hint">レポートを解析します</span>
          <div class="pc-format-chips" aria-hidden="true">
            <span>PDF</span><span>Word</span><span>PowerPoint</span><span>テキスト</span>
          </div>
        {/if}
      </div>
      <input bind:this={fileInput} type="file" accept=".pdf,.docx,.pptx,.xlsx,.txt,.md,.csv,.json,.html,.htm" hidden onchange={onPick} />

      {#if text}
        <div class="pc-preview" class:expanded={previewExpanded}>
          <button class="pc-pill" type="button" onclick={() => (previewExpanded = !previewExpanded)}>
            {previewExpanded ? "折りたたむ" : "全文"}
          </button>
          <p class="pc-preview-text">{previewExpanded ? text : previewText + (previewText.length >= 180 ? "…" : "")}</p>
        </div>
      {/if}

      <!-- Segmented capability toggles + run button, one slim row -->
      <div class="pc-run">
        <div class="pc-toggles" role="group" aria-label="検査項目">
          <button class="pc-toggle" class:on={runAiRate} type="button" aria-pressed={runAiRate} onclick={() => (runAiRate = !runAiRate)}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M9.94 15.5A2 2 0 0 0 8.5 14.06l-6.14-1.58a.5.5 0 0 1 0-.96L8.5 9.94A2 2 0 0 0 9.94 8.5l1.58-6.14a.5.5 0 0 1 .96 0L14.06 8.5A2 2 0 0 0 15.5 9.94l6.14 1.58a.5.5 0 0 1 0 .96L15.5 14.06a2 2 0 0 0-1.44 1.44l-1.58 6.14a.5.5 0 0 1-.96 0z"/><path d="M20 3v4M22 5h-4"/></svg>
            AI生成率
          </button>
          <button class="pc-toggle" class:on={runSimilarity} type="button" aria-pressed={runSimilarity} onclick={() => (runSimilarity = !runSimilarity)}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
            重複チェック
          </button>
        </div>
        {#if text && !busy}
          <button class="pc-textlink" type="button" onclick={clearAll}>クリア</button>
        {/if}
        <button class="pc-cta" type="button" onclick={analyze} disabled={busy || charCount < 40 || (!runAiRate && !runSimilarity)}>
          {#if busy}<span class="pc-spinner small"></span>解析中…{:else}検査開始{/if}
        </button>
      </div>
    {:else}
      <!-- ── RESULT PHASE: the AI-rate result is the hero. The two pipelines
           resolve independently — whichever finishes first renders first. ── -->
      {#if runAiRate}
        {#if aiPending}
          <section class="pc-hero">
            <div class="pc-gauge pending" aria-hidden="true">
              <span class="pc-spinner"></span>
            </div>
            <div class="pc-hero-info">
              <div class="pc-hero-eyebrow">AI生成の可能性</div>
              <div class="pc-hero-level muted">解析中…</div>
              <div class="pc-hero-meta">文ごとの判定・継続再現・書換類似を実行しています</div>
            </div>
          </section>
        {:else if aiResult}
          {@const ai = aiResult}
          {@const lv = level(ai.percent)}
          <section class="pc-hero {lv.cls}">
            <div class="pc-gauge">
              <svg viewBox="0 0 120 120" aria-hidden="true">
                <circle class="pc-gauge-track" cx="60" cy="60" r={GAUGE_R} />
                <circle
                  class="pc-gauge-fill"
                  cx="60" cy="60" r={GAUGE_R}
                  stroke-dasharray={GAUGE_C}
                  stroke-dashoffset={gaugeOffset(ai.percent)}
                />
              </svg>
              <div class="pc-gauge-center">
                <span class="pc-gauge-val"><span class="pc-gauge-num">{ai.percent}</span><span class="pc-gauge-pct">%</span></span>
              </div>
            </div>
            <div class="pc-hero-info">
              <div class="pc-hero-eyebrow">AI生成の可能性</div>
              <div class="pc-hero-level">痕跡 {lv.label}</div>
              <div class="pc-hero-meta">
                信頼度 {confidenceLabel(ai.confidence)}
                {#if ai.calibrated && ai.method_accuracy != null}
                  <span class="pc-dot-sep">·</span> 精度 {accuracyText(ai.method_accuracy, ai.accuracy_low, ai.accuracy_high)}{#if ai.validation_n != null}(検証{ai.validation_n}件){/if}
                {:else}
                  <span class="pc-dot-sep">·</span> <span class="pc-warn">未校正</span>
                {/if}
              </div>
            </div>
          </section>
        {:else if aiError}
          <div class="pc-error" role="alert">AI率の解析に失敗しました: {aiError}</div>
        {/if}
      {/if}

      <div class="pc-substats">
        {#if runSimilarity}
          <div class="pc-stat">
            {#if simPending}
              <span class="pc-stat-num pending"><span class="pc-spinner small"></span></span>
              <span class="pc-stat-label">重複率 · 照合中…</span>
            {:else if simResult}
              <span class="pc-stat-num">{simResult.overall_pct}<small>%</small></span>
              <span class="pc-stat-label">重複率 · {simResult.available ? `${simResult.matches.length}件` : "照合不可"}</span>
            {:else}
              <span class="pc-stat-num">—</span>
              <span class="pc-stat-label">重複率 · 失敗</span>
            {/if}
          </div>
        {/if}
        {#if aiResult}
          <div class="pc-stat">
            {#if flaggedPct != null}
              <span class="pc-stat-num">{flaggedPct}<small>%</small></span>
              <span class="pc-stat-label">高リスク文 · {flaggedSentences.length}/{judgedCount}文</span>
            {:else}
              <span class="pc-stat-num">—</span>
              <span class="pc-stat-label">高リスク文 · 判定なし</span>
            {/if}
          </div>
        {/if}
        <div class="pc-substats-actions">
          <button class="pc-btn ghost small" type="button" onclick={exportReport} disabled={busy || (!aiResult && !simResult)}>{copied ? "コピー済" : "コピー"}</button>
          <button class="pc-btn ghost small" type="button" onclick={saveReportFile} disabled={busy || (!aiResult && !simResult)}>{savedPath ? "保存済" : "保存"}</button>
          <button class="pc-btn ghost small" type="button" onclick={clearAll}>新規</button>
        </div>
      </div>

      <p class="pc-disclaimer">
        ※ AI検出は科学的に不確実で誤判定があり得ます。この結果は学術不正の認定根拠にはなりません。自己点検・推敲の補助としてご利用ください。表示値は20%(痕跡なし=検出下限)〜95%の範囲です。20%は人間作の証明ではなく、95%も断定ではありません。
      </p>

      <!-- Evidence: flagged sentences (secondary) -->
      {#if aiResult}
        {#if aiResult.notes.length}
          <div class="pc-notes">{#each aiResult.notes as n}<span>{n}</span>{/each}</div>
        {/if}

        {#if aiResult.channels.length}
          <div class="pc-channels">
            {#each aiResult.channels as ch}
              <div
                class="pc-channel"
                class:off={!ch.available}
                title={!ch.available
                  ? "この手法は実行できませんでした"
                  : ch.raw !== ch.score
                    ? `生の測定値 ${Math.round(ch.raw * 100)}% を共通スケールに正規化した値です`
                    : ""}
              >
                <div class="pc-channel-head">
                  <span class="pc-channel-label">{ch.label}</span>
                  <span class="pc-channel-val">{ch.available ? `${Math.round(ch.score * 100)}%` : "—"}</span>
                </div>
                <div class="pc-channel-track"><div class="pc-channel-fill" style="width:{ch.available ? Math.round(ch.score * 100) : 0}%"></div></div>
              </div>
            {/each}
          </div>
        {/if}

        {#if aiResult.sentences.length}
          <section class="pc-block">
            <div class="pc-block-head">
              <h2>{showAllSentences ? "文ごとの判定" : "要注意の文"}</h2>
              <button class="pc-pill" type="button" onclick={() => (showAllSentences = !showAllSentences)}>
                {showAllSentences ? "要注意のみ" : `すべて表示 (${aiResult.sentences.length})`}
              </button>
            </div>
            {#if shownSentences.length}
              {#each shownSentences as s}
                <div class="pc-sentence {probClass(s)}">
                  <div class="pc-sentence-top">
                    <span class="pc-sentence-prob">{s.judged ? `${Math.round(s.prob * 100)}%` : "—"}</span>
                    <span class="pc-sentence-reason">{s.reason}</span>
                    {#if s.reviewed}<span class="pc-reviewed" title="文脈付きでAIが再監査した判定です">複判済</span>{/if}
                  </div>
                  <p class="pc-sentence-text">{s.text}</p>
                </div>
              {/each}
            {:else}
              <p class="pc-empty">AIの痕跡が強い文は見つかりませんでした。</p>
            {/if}
          </section>
        {/if}

        <div class="pc-disclosure-wrap">
          <button class="pc-disclosure" class:open={showFeatures} type="button" onclick={() => (showFeatures = !showFeatures)}>
            <span class="pc-chevron" aria-hidden="true"></span>
            統計特徴(再現可能・モデル非依存)
          </button>
          {#if showFeatures}
            <div class="pc-features">
              <span>バースト性 <b>{aiResult.features.burstiness.toFixed(2)}</b></span>
              <span>語彙多様性 <b>{aiResult.features.lexical_diversity.toFixed(2)}</b></span>
              <span>n-gram反復 <b>{pct(aiResult.features.ngram_repetition)}</b></span>
              <span>接続語密度 <b>{aiResult.features.transition_density.toFixed(2)}</b></span>
              <span>圧縮率 <b>{aiResult.features.compressibility.toFixed(2)}</b></span>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Evidence: similarity matches (secondary) -->
      {#if simError}
        <div class="pc-error" role="alert">重複チェックに失敗しました: {simError}</div>
      {/if}
      {#if simResult}
        {#if simResult.notes.length}
          <div class="pc-notes">{#each simResult.notes as n}<span>{n}</span>{/each}</div>
        {/if}
        {#if simResult.matches.length}
          <section class="pc-block">
            <div class="pc-block-head"><h2>一致した箇所</h2></div>
            {#each simResult.matches as m}
              <div class="pc-match">
                <div class="pc-match-top">
                  <span class="pc-overlap">{Math.round(m.overlap * 100)}% 一致</span>
                  <button class="pc-source" type="button" onclick={() => openSource(m.source_url)} title={m.source_url}>
                    {m.source_title || m.source_url}
                  </button>
                </div>
                <p class="pc-match-sentence">{m.sentence}</p>
                {#if m.snippet}<p class="pc-match-snippet">{m.snippet}</p>{/if}
              </div>
            {/each}
          </section>
        {/if}
      {/if}
    {/if}

  </div>
</main>

<style>
  .pc-root {
    width: 100vw;
    height: 100vh;
    overflow-y: auto;
    box-sizing: border-box;
    padding: clamp(28px, 6vh, 60px) 24px 64px;
    display: grid;
    place-items: start center;
    color: #1d1d1f;
    background: #f7f8fa;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", "Noto Sans JP", sans-serif;
    /* Single theme accent (navy). Light/dark adaptive; keep colour use minimal. */
    --pc-accent: #173b68;
    --pc-accent-weak: rgba(23,59,104,0.07);
    --pc-accent-line: rgba(23,59,104,0.3);
    --pc-accent-glow: rgba(23,59,104,0.22);
    --pc-surface: rgba(255,255,255,0.9);
    --pc-border: rgba(0,0,0,0.08);
    --pc-muted: #86868b;
  }
  .pc-inner { width: 100%; max-width: 680px; display: flex; flex-direction: column; gap: 18px; }

  /* ── Header ── */
  .pc-head-row { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .pc-title { margin: 0; font-size: 32px; font-weight: 850; letter-spacing: -0.01em; line-height: 1.05; }
  .pc-title-accent { color: var(--pc-accent); }
  .pc-sub { margin: 8px 0 0; font-size: 13.5px; color: var(--pc-muted); }
  .pc-icon-btn {
    flex: 0 0 auto; width: 36px; height: 36px; padding: 0; box-sizing: border-box;
    display: grid; place-items: center; cursor: pointer;
    border-radius: 10px; border: none; background: transparent; color: var(--pc-muted); box-shadow: none;
    transition: background 0.15s ease, color 0.15s ease;
  }
  .pc-icon-btn:hover, .pc-icon-btn.on { background: var(--pc-accent-weak); color: var(--pc-accent); }
  .pc-icon-btn:focus, .pc-icon-btn:focus-visible { outline: none; box-shadow: none; }
  /* Inline SVGs carry a baseline gap that pushes the glyph off-centre inside a
     grid/flex box — force block so the icon sits dead-centre. */
  .pc-icon-btn svg, .pc-cap-icon svg, .pc-upload-badge svg, .pc-set-ico svg { display: block; }

  /* Focus: no outer ring (it renders offset/misaligned around rounded cards in
     this webview). Use an inset ring that hugs the radius exactly. */
  button:focus, .pc-uploader:focus { outline: none; }
  button:focus-visible, .pc-uploader:focus-visible {
    outline: none; box-shadow: inset 0 0 0 2px var(--pc-accent-line);
  }

  /* ── Buttons ── */
  .pc-btn {
    display: inline-flex; align-items: center; justify-content: center; gap: 7px;
    padding: 8px 16px; border-radius: 9px; border: 0.5px solid transparent;
    font-size: 13px; font-weight: 650; cursor: pointer; white-space: nowrap;
    transition: background 0.15s ease, border-color 0.15s ease, opacity 0.15s ease;
  }
  .pc-btn.primary { background: var(--pc-accent); color: #fff; }
  .pc-btn.primary:hover:not(:disabled) { filter: brightness(1.08); }
  .pc-btn.ghost { background: transparent; border-color: rgba(0,0,0,0.14); color: inherit; }
  .pc-btn.ghost:hover:not(:disabled) { background: rgba(0,0,0,0.05); }
  .pc-btn.small { padding: 6px 12px; font-size: 12px; }
  .pc-btn:disabled { opacity: 0.4; cursor: default; }
  .pc-textlink { align-self: center; padding: 4px; border: none; background: none; cursor: pointer; color: #86868b; font-size: 12px; }
  .pc-textlink:hover { color: #60646c; }

  .pc-error {
    padding: 11px 14px; border-radius: 11px; font-size: 13px;
    background: rgba(180,35,24,0.08); color: #b42318; border: 0.5px solid rgba(180,35,24,0.2);
  }

  /* ── Settings panel ── */
  .pc-panel {
    display: flex; flex-direction: column; gap: 20px; padding: 20px; border-radius: 18px;
    border: 0.5px solid var(--pc-border); background: var(--pc-surface);
    box-shadow: 0 1px 2px rgba(0,0,0,0.04), 0 12px 34px rgba(0,0,0,0.05);
  }
  .pc-set-group { display: flex; flex-direction: column; gap: 12px; }
  .pc-set-group + .pc-set-group { padding-top: 20px; border-top: 0.5px solid var(--pc-border); }
  .pc-set-head { display: flex; align-items: flex-start; gap: 12px; }
  .pc-set-ico {
    flex: 0 0 auto; width: 34px; height: 34px; border-radius: 10px; display: grid; place-items: center;
    color: var(--pc-accent); background: var(--pc-accent-weak);
  }
  .pc-set-title { font-size: 14px; font-weight: 750; }
  .pc-set-sub { margin-top: 2px; font-size: 12px; line-height: 1.55; color: var(--pc-muted); }
  .pc-field { align-self: stretch; max-width: 420px; display: flex; flex-direction: column; gap: 6px; font-size: 12px; font-weight: 650; color: var(--pc-muted); }
  .pc-field input {
    padding: 9px 11px; border-radius: 9px; border: 0.5px solid var(--pc-border);
    background: rgba(0,0,0,0.02); font: inherit; font-size: 13px; font-weight: 500; color: inherit; outline: none;
  }
  .pc-field input:focus { border-color: var(--pc-accent-line); box-shadow: 0 0 0 3px var(--pc-accent-weak); }
  .pc-set-foot { display: flex; align-items: center; justify-content: flex-end; gap: 12px; }
  .pc-set-foot.between { justify-content: space-between; }
  .pc-saved { font-size: 12px; color: var(--pc-accent); font-weight: 650; }
  .pc-calib-pill {
    display: inline-flex; align-items: center; gap: 6px; padding: 5px 12px; border-radius: 999px;
    font-size: 11.5px; font-weight: 600; color: var(--pc-muted); background: rgba(0,0,0,0.04);
  }
  .pc-calib-pill.ok { color: var(--pc-accent); background: var(--pc-accent-weak); }

  /* User-supplied calibration samples (chips + add pill). */
  .pc-calib-samples { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .pc-calib-kind { flex: 0 0 auto; font-size: 11.5px; font-weight: 650; color: var(--pc-muted); }
  .pc-chip.ai { color: #8a5a1e; background: rgba(183,121,31,0.12); }
  .pc-chip.ai .pc-chip-x { color: #8a5a1e; }
  .pc-calib-samples .pc-pill { float: none; margin: 0; }
  .pc-pill:disabled { opacity: 0.4; cursor: default; }
  .pc-chip {
    display: inline-flex; align-items: center; gap: 4px; max-width: 220px;
    padding: 4px 6px 4px 11px; border-radius: 999px;
    font-size: 11.5px; font-weight: 600; color: var(--pc-accent); background: var(--pc-accent-weak);
  }
  .pc-chip-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pc-chip-x {
    flex: 0 0 auto; width: 16px; height: 16px; padding: 0; border: none; border-radius: 50%;
    display: grid; place-items: center; cursor: pointer; font-size: 12px; line-height: 1;
    color: var(--pc-accent); background: transparent;
  }
  .pc-chip-x:hover { background: rgba(0,0,0,0.08); }

  /* ── Uploader: the dramatic focal surface of the input phase ── */
  .pc-uploader {
    position: relative; overflow: hidden;
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px;
    min-height: 216px; padding: 36px 28px; border-radius: 22px; cursor: pointer; text-align: center;
    border: 1.5px dashed var(--pc-accent-line);
    background: var(--pc-surface);
    transition: transform 0.18s cubic-bezier(0.2,0.8,0.2,1), border-color 0.18s ease, box-shadow 0.18s ease;
  }
  .pc-uploader:hover { transform: translateY(-2px); border-color: var(--pc-accent); }
  .pc-uploader.over { border-style: solid; border-color: var(--pc-accent); box-shadow: 0 0 0 4px var(--pc-accent-weak); }
  .pc-uploader.loaded { min-height: 150px; border-style: solid; border-color: var(--pc-accent-line); }
  .pc-uploader.busy { cursor: default; }
  /* Soft accent glow that sits behind the badge. */
  .pc-uploader-glow {
    position: absolute; top: -40%; left: 50%; width: 340px; height: 340px; transform: translateX(-50%);
    background: radial-gradient(circle, var(--pc-accent-weak), transparent 70%);
    pointer-events: none;
  }
  .pc-upload-badge {
    position: relative; z-index: 1;
    width: 60px; height: 60px; border-radius: 18px; display: grid; place-items: center; color: #fff;
    background: var(--pc-accent);
    box-shadow: 0 8px 20px var(--pc-accent-glow);
    margin-bottom: 4px;
  }
  .pc-upload-badge.scanning { overflow: hidden; }
  .pc-scan-icon { position: relative; z-index: 1; color: #fff; }
  .pc-scan { position: absolute; inset: 0; z-index: 2; }
  .pc-scan::after {
    content: ""; position: absolute; left: 0; right: 0; height: 40%;
    background: linear-gradient(180deg, transparent, rgba(255,255,255,0.75), transparent);
    animation: pc-scanline 1.1s ease-in-out infinite;
  }
  @keyframes pc-scanline { 0% { top: -40%; } 100% { top: 100%; } }
  .pc-upload-title { position: relative; z-index: 1; font-size: 16px; font-weight: 750; }
  .pc-upload-title.file { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--pc-accent); }
  .pc-upload-hint { position: relative; z-index: 1; font-size: 12px; color: var(--pc-muted); }
  .pc-format-chips { position: relative; z-index: 1; display: flex; gap: 6px; margin-top: 8px; }
  .pc-format-chips span {
    padding: 3px 10px; border-radius: 999px; font-size: 11px; font-weight: 650; color: #60646c;
    background: rgba(0,0,0,0.05);
  }

  /* ── Capability cards (selection shown by the card itself — no checkbox) ── */
  /* Slim run row: segmented toggles on the left, solid run button on the right. */
  .pc-run { display: flex; align-items: center; gap: 12px; }
  .pc-toggles {
    flex: 0 0 auto; margin-right: auto; height: 38px; box-sizing: border-box;
    display: inline-flex; align-items: stretch; gap: 3px; padding: 3px; border-radius: 11px;
    background: rgba(0,0,0,0.05);
  }
  .pc-toggle {
    flex: 0 0 auto;
    display: inline-flex; align-items: center; justify-content: center; gap: 7px;
    padding: 0 13px; border: none; border-radius: 9px; cursor: pointer; background: transparent;
    font-size: 12.5px; font-weight: 700; color: var(--pc-muted); white-space: nowrap;
    transition: background 0.15s ease, color 0.15s ease, box-shadow 0.15s ease;
  }
  .pc-toggle svg { display: block; flex: 0 0 auto; }
  .pc-toggle:hover:not(.on) { color: #60646c; }
  .pc-toggle.on { background: var(--pc-surface); color: var(--pc-accent); box-shadow: 0 1px 3px rgba(0,0,0,0.12); }

  /* ── Primary CTA (solid theme accent) ── */
  .pc-cta {
    flex: 0 0 auto; height: 38px; box-sizing: border-box;
    display: inline-flex; align-items: center; justify-content: center; gap: 8px;
    min-width: 108px; padding: 0 22px; border: none; border-radius: 11px; cursor: pointer;
    font-size: 13px; font-weight: 750; color: #fff; background: var(--pc-accent);
    transition: filter 0.15s ease, opacity 0.15s ease;
  }
  .pc-cta:hover:not(:disabled) { filter: brightness(1.08); }
  .pc-cta:disabled { opacity: 0.4; cursor: default; }

  /* Read-only extracted-text preview (no native textarea). */
  .pc-preview {
    display: block; padding: 12px 14px; border-radius: 12px;
    border: 0.5px solid rgba(0,0,0,0.08); background: rgba(255,255,255,0.6);
  }
  .pc-preview-text {
    margin: 0; font-size: 12.5px; line-height: 1.6; color: #60646c;
    max-height: 3.2em; overflow: hidden;
  }
  .pc-preview.expanded { max-height: 260px; overflow-y: auto; scrollbar-width: thin; }
  .pc-preview.expanded .pc-preview-text {
    max-height: none; overflow: visible; white-space: pre-wrap; word-break: break-word;
  }
  /* Floats into the top-right corner so the text flows around it at full width. */
  .pc-pill {
    float: right; margin: 0 0 6px 12px; padding: 5px 12px; border-radius: 999px; cursor: pointer;
    border: 0.5px solid rgba(0,0,0,0.14); background: rgba(255,255,255,0.7); color: #60646c;
    font-size: 11.5px; font-weight: 650; transition: background 0.15s ease;
  }
  .pc-pill:hover { background: rgba(0,0,0,0.05); }

  /* Segmented control (replaces native select). */
  .pc-segmented { align-self: flex-start; display: inline-flex; padding: 3px; border-radius: 10px; background: rgba(0,0,0,0.05); gap: 2px; }
  .pc-seg {
    padding: 7px 16px; border: none; border-radius: 8px; cursor: pointer; background: transparent;
    font-size: 12.5px; font-weight: 650; color: #60646c; transition: background 0.15s ease, color 0.15s ease;
  }
  .pc-seg.on { background: #fff; color: var(--pc-accent); box-shadow: 0 1px 3px rgba(0,0,0,0.1); }

  /* ── Result hero: AI-rate gauge is the single focal point ── */
  .pc-hero {
    display: flex; align-items: center; gap: 26px;
    padding: 24px 28px; border-radius: 20px;
    border: 0.5px solid rgba(0,0,0,0.06); background: rgba(255,255,255,0.9);
    box-shadow: 0 1px 2px rgba(0,0,0,0.04), 0 14px 40px rgba(0,0,0,0.08);
  }
  .pc-gauge { position: relative; width: 128px; height: 128px; flex: 0 0 auto; }
  .pc-gauge.pending { display: grid; place-items: center; }
  .pc-gauge.pending .pc-spinner { width: 30px; height: 30px; border-width: 3px; }
  .pc-hero-level.muted { color: #86868b; }
  .pc-gauge svg { width: 100%; height: 100%; transform: rotate(-90deg); }
  .pc-gauge-track { fill: none; stroke: rgba(0,0,0,0.07); stroke-width: 11; }
  .pc-gauge-fill { fill: none; stroke-width: 11; stroke-linecap: round; transition: stroke-dashoffset 0.7s cubic-bezier(0.2,0.8,0.2,1); }
  .pc-gauge-center { position: absolute; inset: 0; display: grid; place-items: center; }
  .pc-gauge-val { display: inline-flex; align-items: baseline; }
  .pc-gauge-num { font-size: 40px; font-weight: 840; line-height: 1; letter-spacing: -0.02em; }
  .pc-gauge-pct { font-size: 17px; font-weight: 700; margin-left: 1px; }
  .pc-hero-info { min-width: 0; display: flex; flex-direction: column; gap: 4px; }
  .pc-hero-eyebrow { font-size: 12px; font-weight: 700; color: #86868b; letter-spacing: 0.02em; }
  .pc-hero-level { font-size: 21px; font-weight: 800; }
  .pc-hero-meta { font-size: 12.5px; color: #86868b; font-weight: 600; }
  .pc-dot-sep { opacity: 0.5; margin: 0 2px; }
  .pc-warn { color: #b7791f; }
  .pc-hero.high .pc-gauge-fill { stroke: #d0432f; }
  .pc-hero.mid .pc-gauge-fill { stroke: #c98a1f; }
  .pc-hero.low .pc-gauge-fill { stroke: #1e9e57; }
  .pc-hero.high .pc-gauge-num, .pc-hero.high .pc-hero-level { color: #c0392b; }
  .pc-hero.mid .pc-gauge-num, .pc-hero.mid .pc-hero-level { color: #b7791f; }
  .pc-hero.low .pc-gauge-num, .pc-hero.low .pc-hero-level { color: #1e874b; }

  /* ── Sub-stats row (secondary) ── */
  .pc-substats { display: flex; align-items: center; gap: 26px; flex-wrap: wrap; padding: 0 6px; }
  .pc-stat { display: flex; flex-direction: column; gap: 1px; }
  .pc-stat-num { font-size: 22px; font-weight: 800; line-height: 1; }
  .pc-stat-num.pending { display: flex; align-items: center; height: 22px; }
  .pc-stat-num small { font-size: 12px; font-weight: 700; margin-left: 1px; }
  .pc-stat-label { font-size: 11.5px; color: #86868b; }
  .pc-substats-actions { margin-left: auto; display: flex; gap: 8px; }

  .pc-disclaimer { margin: 0; font-size: 11.5px; line-height: 1.6; color: #86868b; }

  .pc-notes { display: flex; flex-direction: column; gap: 4px; }
  .pc-notes span { font-size: 11.5px; color: #86868b; }

  /* ── Per-channel breakdown (feature / judge / DNA-GPT / Raidar) ── */
  .pc-channels {
    display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 10px 18px;
    padding: 14px 16px; border-radius: 12px;
    border: 0.5px solid var(--pc-border); background: var(--pc-surface);
  }
  .pc-channel { display: flex; flex-direction: column; gap: 5px; }
  .pc-channel.off { opacity: 0.4; }
  .pc-channel-head { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; }
  .pc-channel-label { font-size: 12px; font-weight: 650; color: #60646c; }
  .pc-channel-val { font-size: 12.5px; font-weight: 750; font-variant-numeric: tabular-nums; }
  .pc-channel-track { height: 5px; border-radius: 999px; background: rgba(0,0,0,0.07); overflow: hidden; }
  .pc-channel-fill { height: 100%; border-radius: 999px; background: var(--pc-accent); transition: width 0.5s cubic-bezier(0.2,0.8,0.2,1); }

  /* ── Evidence blocks (secondary) ── */
  .pc-block-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin: 4px 0 10px; }
  .pc-block-head h2 { margin: 0; font-size: 14.5px; font-weight: 750; }
  .pc-empty { margin: 0; padding: 6px 2px; font-size: 12.5px; color: #86868b; }

  .pc-sentence {
    padding: 10px 12px; border-radius: 10px; margin-bottom: 6px;
    border: 0.5px solid rgba(0,0,0,0.06); background: rgba(255,255,255,0.7);
  }
  .pc-sentence.s-high { background: rgba(192,57,43,0.06); border-color: rgba(192,57,43,0.2); }
  .pc-sentence.s-mid { background: rgba(183,121,31,0.06); border-color: rgba(183,121,31,0.18); }
  .pc-sentence-top { display: flex; align-items: baseline; gap: 8px; margin-bottom: 3px; }
  .pc-sentence-prob { flex: 0 0 auto; font-size: 12px; font-weight: 800; }
  .pc-sentence.s-high .pc-sentence-prob { color: #c0392b; }
  .pc-sentence.s-mid .pc-sentence-prob { color: #b7791f; }
  .pc-sentence.s-low .pc-sentence-prob { color: #1e874b; }
  .pc-sentence.s-na { opacity: 0.65; }
  .pc-sentence.s-na .pc-sentence-prob { color: #86868b; }
  .pc-sentence-reason { font-size: 11.5px; color: #86868b; }
  .pc-reviewed {
    flex: 0 0 auto; margin-left: auto; padding: 1px 8px; border-radius: 999px;
    font-size: 10px; font-weight: 650; color: var(--pc-accent); background: var(--pc-accent-weak);
  }
  .pc-sentence-text { margin: 0; font-size: 13px; line-height: 1.6; }

  /* Statistical features — tucked into a custom disclosure. */
  .pc-disclosure-wrap { border-radius: 11px; border: 0.5px solid rgba(0,0,0,0.08); background: rgba(255,255,255,0.6); }
  .pc-disclosure {
    display: flex; align-items: center; gap: 8px; width: 100%; padding: 10px 14px;
    border: none; background: transparent; cursor: pointer; font-size: 12.5px; font-weight: 650; color: #60646c; text-align: left;
  }
  .pc-chevron {
    width: 6px; height: 6px; flex: 0 0 auto;
    border-right: 1.6px solid currentColor; border-bottom: 1.6px solid currentColor;
    transform: rotate(-45deg); transition: transform 0.15s ease;
  }
  .pc-disclosure.open .pc-chevron { transform: rotate(45deg); }
  .pc-features { display: flex; flex-wrap: wrap; gap: 14px; padding: 0 14px 12px; font-size: 12px; color: #60646c; }
  .pc-features b { color: #1d1d1f; }

  .pc-match {
    padding: 12px 14px; border-radius: 12px; margin-bottom: 8px;
    border: 0.5px solid rgba(0,0,0,0.08); background: rgba(255,255,255,0.7);
  }
  .pc-match-top { display: flex; align-items: center; gap: 10px; margin-bottom: 6px; }
  .pc-overlap { font-size: 12px; font-weight: 800; color: #c0392b; }
  .pc-source {
    min-width: 0; flex: 1 1 auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    padding: 0; border: none; background: none; cursor: pointer; text-align: left;
    font-size: 12px; font-weight: 600; color: #2563eb; text-decoration: underline;
  }
  .pc-match-sentence { margin: 0 0 5px; font-size: 13px; line-height: 1.55; }
  .pc-match-snippet { margin: 0; font-size: 11.5px; line-height: 1.5; color: #86868b; }

  .pc-spinner {
    width: 15px; height: 15px; border-radius: 50%;
    border: 2px solid rgba(0,0,0,0.18); border-top-color: #173b68;
    animation: pc-spin 0.7s linear infinite;
  }
  .pc-spinner.small { width: 13px; height: 13px; border-width: 1.6px; border-top-color: currentColor; }
  @keyframes pc-spin { to { transform: rotate(360deg); } }

  /* ── Dark ── */
  :global([data-theme="dark"]) .pc-root {
    color: #f5f5f7; background: #1c1c1e;
    --pc-accent: #7ea7e0;
    --pc-accent-weak: rgba(126,167,224,0.14);
    --pc-accent-line: rgba(126,167,224,0.5);
    --pc-accent-glow: rgba(0,0,0,0.45);
    --pc-surface: rgba(255,255,255,0.06);
    --pc-border: rgba(255,255,255,0.1);
    --pc-muted: #98989d;
  }
  :global([data-theme="dark"]) .pc-sub,
  :global([data-theme="dark"]) .pc-upload-hint,
  :global([data-theme="dark"]) .pc-hero-eyebrow,
  :global([data-theme="dark"]) .pc-hero-meta,
  :global([data-theme="dark"]) .pc-stat-label,
  :global([data-theme="dark"]) .pc-disclaimer,
  :global([data-theme="dark"]) .pc-notes span,
  :global([data-theme="dark"]) .pc-sentence-reason,
  :global([data-theme="dark"]) .pc-match-snippet,
  :global([data-theme="dark"]) .pc-preview-text,
  :global([data-theme="dark"]) .pc-empty,
  :global([data-theme="dark"]) .pc-set-sub { color: #98989d; }
  :global([data-theme="dark"]) .pc-channel-label { color: #a9abb2; }
  :global([data-theme="dark"]) .pc-channel-track { background: rgba(255,255,255,0.12); }
  :global([data-theme="dark"]) .pc-hero { background: rgba(255,255,255,0.06); border-color: rgba(255,255,255,0.1); box-shadow: 0 14px 40px rgba(0,0,0,0.4); }
  :global([data-theme="dark"]) .pc-panel { background: rgba(255,255,255,0.05); border-color: rgba(255,255,255,0.1); }
  :global([data-theme="dark"]) .pc-gauge-track { stroke: rgba(255,255,255,0.1); }
  :global([data-theme="dark"]) .pc-icon-btn { background: rgba(255,255,255,0.06); border-color: rgba(255,255,255,0.12); color: #98989d; }
  :global([data-theme="dark"]) .pc-icon-btn:hover, :global([data-theme="dark"]) .pc-icon-btn.on { background: rgba(120,160,220,0.16); color: #cddcf5; border-color: rgba(120,160,220,0.4); }
  :global([data-theme="dark"]) .pc-field input { background: rgba(255,255,255,0.04); border-color: rgba(255,255,255,0.12); color: #f5f5f7; }
  :global([data-theme="dark"]) .pc-format-chips span { background: rgba(255,255,255,0.1); color: #cddcf5; }
  :global([data-theme="dark"]) .pc-toggles { background: rgba(255,255,255,0.07); }
  :global([data-theme="dark"]) .pc-toggle { color: #98989d; }
  :global([data-theme="dark"]) .pc-toggle:hover:not(.on) { color: #cddcf5; }
  :global([data-theme="dark"]) .pc-toggle.on { background: rgba(255,255,255,0.14); color: #cddcf5; box-shadow: none; }
  :global([data-theme="dark"]) .pc-features b { color: #f5f5f7; }
  :global([data-theme="dark"]) .pc-disclosure-wrap,
  :global([data-theme="dark"]) .pc-preview,
  :global([data-theme="dark"]) .pc-sentence,
  :global([data-theme="dark"]) .pc-match { background: rgba(255,255,255,0.05); border-color: rgba(255,255,255,0.1); }
  :global([data-theme="dark"]) .pc-segmented { background: rgba(255,255,255,0.08); }
  :global([data-theme="dark"]) .pc-seg { color: #98989d; }
  :global([data-theme="dark"]) .pc-seg.on { background: rgba(255,255,255,0.14); color: #cddcf5; box-shadow: none; }
  :global([data-theme="dark"]) .pc-pill { border-color: rgba(255,255,255,0.16); color: #98989d; }
  :global([data-theme="dark"]) .pc-pill:hover { background: rgba(255,255,255,0.08); }
  :global([data-theme="dark"]) .pc-chip-x:hover { background: rgba(255,255,255,0.12); }
  :global([data-theme="dark"]) .pc-chip.ai { color: #e0b46a; background: rgba(201,138,31,0.16); }
  :global([data-theme="dark"]) .pc-chip.ai .pc-chip-x { color: #e0b46a; }
  :global([data-theme="dark"]) .pc-hero-level.muted { color: #98989d; }
  :global([data-theme="dark"]) .pc-sentence.s-na .pc-sentence-prob { color: #98989d; }
  :global([data-theme="dark"]) .pc-btn.ghost { border-color: rgba(255,255,255,0.16); }
  :global([data-theme="dark"]) .pc-btn.ghost:hover:not(:disabled) { background: rgba(255,255,255,0.08); }
  :global([data-theme="dark"]) .pc-textlink:hover { color: #cddcf5; }
  :global([data-theme="dark"]) .pc-source { color: #6ea8fe; }
</style>
