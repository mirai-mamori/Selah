<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onDestroy, onMount } from "svelte";
  import Icon from "../Icon.svelte";

  interface CourseAutomationConfig {
    enabled: boolean;
    intervalMinutes: number;
  }

  interface CourseAutomationStatus {
    lunaId: string;
    running: boolean;
    stage: string;
    lastRun?: number;
    lastOk?: boolean;
    lastError: string;
    downloadedFiles: string[];
    externalLinks: string[];
    totalDocuments: number;
    processedDocuments: number;
    currentDocument: string;
    pendingSummaryIds: string[];
    documentAnalyses: Array<{
      id: string;
      kind: string;
      title: string;
      filename: string;
      status: string;
      summary: string;
      triggerDecision: string;
      observationContext: string;
      error: string;
    }>;
    analysis: {
      summary: string;
      findings: string[];
      standingContext: string[];
      seat: { assignment: string; evidence: string[]; confidence: number };
      printCandidates: Array<{ filename: string; reason: string; confidence: number }>;
    };
    printResults: Array<{ filename: string; status: string; detail: string }>;
  }

  interface CourseAutomationView {
    config: CourseAutomationConfig;
    status: CourseAutomationStatus;
  }

  interface Props {
    lunaId: string;
    courseName: string;
  }

  let { lunaId, courseName }: Props = $props();
  let view = $state<CourseAutomationView | null>(null);
  let busy = $state(false);
  let error = $state("");
  // Which bento cell is drilled into. Tapping a cell opens its full content as
  // a focused second page; null is the home (bento) view.
  type DetailKey = "findings" | "seat" | "print" | "standing";
  let detail = $state<DetailKey | null>(null);
  const detailTitle: Record<DetailKey, string> = {
    findings: "確認事項",
    seat: "座席",
    print: "印刷",
    standing: "メモ",
  };
  let unlisten: (() => void) | null = null;

  const enabled = $derived(view?.config.enabled ?? false);
  const running = $derived(view?.status.running ?? false);

  const stageLabel = $derived.by(() => {
    const stage = view?.status.stage || "";
    return {
      checking: "更新を確認中",
      downloading: "資料を取得中",
      analyzing: "資料を一件ずつ分析中",
      summarizing: "個別摘要を最終確認中",
      pending_summary: "新しい摘要を蓄積中",
      printing: "印刷を検証中",
      unchanged: "変更なし",
      done: "確認済み",
      error: "確認エラー",
    }[stage] || (enabled ? "次の確認を待機中" : "無効");
  });

  // Documents the agent flagged as needing prompt attention. Drives the
  // headline "要確認" emphasis and the header status tone.
  const immediateCount = $derived(
    view?.status.documentAnalyses?.filter(
      (doc) => doc.status === "done" && doc.triggerDecision === "immediate",
    ).length ?? 0,
  );

  const progressPct = $derived(
    view?.status.totalDocuments
      ? Math.min(100, Math.round((view.status.processedDocuments / view.status.totalDocuments) * 100))
      : 0,
  );

  // Only the analyzing stage has a countable total; other stages are
  // indeterminate and show an animated marker instead of a number.
  const hasProgress = $derived((view?.status.totalDocuments ?? 0) > 0);

  const failureNote = $derived(error || view?.status.lastError || "");

  // Control capsule: dot state + a compact status line. It also carries the
  // error message when a run fails, so there is no separate error pill.
  const barState = $derived(running ? "busy" : failureNote ? "error" : "idle");
  const controlText = $derived.by(() => {
    if (running) return hasProgress ? `${progressPct}% ・ ${stageLabel}` : stageLabel;
    return failureNote || stageLabel;
  });

  // ── Bento content ────────────────────────────────────────────────────────
  const summary = $derived(view?.status.analysis?.summary?.trim() ?? "");
  const findings = $derived(view?.status.analysis?.findings ?? []);
  const standingContext = $derived(view?.status.analysis?.standingContext ?? []);
  const seat = $derived(view?.status.analysis?.seat);
  const seatConfidence = $derived(Math.round(Math.max(0, Math.min(1, seat?.confidence ?? 0)) * 100));
  const printResults = $derived(view?.status.printResults ?? []);

  const lastChecked = $derived.by(() => {
    const secs = view?.status.lastRun;
    if (!secs) return "未確認";
    const delta = Date.now() / 1000 - secs;
    if (delta < 60) return "たった今";
    if (delta < 3600) return `${Math.floor(delta / 60)}分前`;
    if (delta < 86400) return `${Math.floor(delta / 3600)}時間前`;
    return `${Math.floor(delta / 86400)}日前`;
  });

  function printLabel(status: string): { text: string; tone: "ok" | "warn" | "bad" } {
    switch (status) {
      case "printed": return { text: "印刷済み", tone: "ok" };
      case "already_printed": return { text: "印刷済み", tone: "ok" };
      case "error": return { text: "印刷失敗", tone: "bad" };
      case "not_found": return { text: "対象不明", tone: "bad" };
      case "skipped_low_confidence": return { text: "確度不足で保留", tone: "warn" };
      default: return { text: status, tone: "warn" };
    }
  }

  async function load(): Promise<void> {
    view = await invoke<CourseAutomationView>("course_automation_get", {
      lunaId,
      courseName,
    });
  }

  async function runNow(): Promise<void> {
    if (busy || view?.status.running) return;
    busy = true;
    error = "";
    try {
      view = await invoke<CourseAutomationView>("course_automation_run_now", {
        lunaId,
        courseName,
      });
    } catch (cause) {
      error = String(cause);
      await load().catch(() => {});
    } finally {
      busy = false;
    }
  }

  onMount(async () => {
    await load().catch((cause) => error = String(cause));
    unlisten = await listen<CourseAutomationStatus>("course-automation-updated", (event) => {
      if (event.payload.lunaId !== lunaId || !view) return;
      view = { ...view, status: event.payload };
    });
  });

  onDestroy(() => unlisten?.());
</script>
<section class="sa" aria-label="自動検知">
  {#if enabled}
    {#if detail}
      <!-- SECOND PAGE: the tapped facet, in full, as its own card. -->
      <div class="sa-cell sa-page-card">
        <button class="sa-back" type="button" onclick={() => (detail = null)}>
          <Icon name="chevron.left" size={15} />
          <strong>{detailTitle[detail]}</strong>
        </button>
        <div class="sa-page">
          {#if detail === "findings"}
            <ul class="sa-list">
              {#each findings as finding}<li>{finding}</li>{/each}
            </ul>
          {:else if detail === "standing"}
            <ul class="sa-list dim">
              {#each standingContext as item}<li>{item}</li>{/each}
            </ul>
          {:else if detail === "seat" && seat}
            <strong class="sa-seat-val sa-seat-val-lg">{seat.assignment}</strong>
            <span class="sa-seat-conf">確度 {seatConfidence}%</span>
            {#if seat.evidence?.length}
              <ul class="sa-list dim sa-seat-ev">
                {#each seat.evidence as ev}<li>{ev}</li>{/each}
              </ul>
            {/if}
          {:else if detail === "print"}
            <div class="sa-prints">
              {#each printResults as item}
                {@const meta = printLabel(item.status)}
                <div class="sa-print" data-tone={meta.tone}>
                  <span class="sa-print-name">{item.filename}</span>
                  <span class="sa-print-tag">{meta.text}</span>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      </div>
    {:else}
      <!-- Capsule row above the cards: control (carries status + errors) and
           compact entries. Each pill hugs its content; no borders, one style. -->
      <div class="sa-pills">
        <div class="sa-pill sa-pill-ctl" data-state={barState} title={controlText}>
          <span class="sa-pill-dot"></span>
          <span class="sa-pill-text">{controlText}</span>
          <button class="sa-pill-btn" type="button" disabled={busy || running} onclick={runNow} aria-label="今すぐ確認" title="今すぐ確認">
            <Icon name="arrow.clockwise" size={13} />
          </button>
        </div>
        {#if standingContext.length}
          <button class="sa-pill sa-pill-go" data-cat="standing" type="button" onclick={() => (detail = "standing")}>
            <Icon name="book" size={13} />
            <span>メモ</span>
            <b>{standingContext.length}</b>
            <Icon name="chevron.right" size={12} />
          </button>
        {/if}
        {#if printResults.length}
          <button class="sa-pill sa-pill-go" data-cat="print" type="button" onclick={() => (detail = "print")}>
            <Icon name="doc" size={13} />
            <span>印刷</span>
            <b>{printResults.length}</b>
            <Icon name="chevron.right" size={12} />
          </button>
        {/if}
      </div>

      {#if summary}
        <div class="sa-bento">
          <article class="sa-hero sa-wide" data-attn={immediateCount > 0}>
            <div class="sa-hero-head">
              <span class="sa-attn">
                <Icon name={immediateCount > 0 ? "bell" : "sparkles"} size={12} />
                {immediateCount > 0 ? "要確認" : "今のまとめ"}
              </span>
              <span class="sa-hero-time">{lastChecked}</span>
            </div>
            <p class="sa-hero-text">{summary}</p>
          </article>

          {#if findings.length}
            <button class="sa-cell sa-wide" data-cat="findings" type="button" onclick={() => (detail = "findings")}>
              <header class="sa-cell-head">
                <Icon name="list.clipboard" size={13} />
                <span>確認事項</span>
                <b>{findings.length}</b>
                <Icon name="chevron.right" size={13} />
              </header>
              <ul class="sa-list">
                {#each findings.slice(0, 2) as finding}<li>{finding}</li>{/each}
              </ul>
              {#if findings.length > 2}
                <span class="sa-more">ほか {findings.length - 2} 件</span>
              {/if}
            </button>
          {/if}

          {#if seat?.assignment}
            <button class="sa-cell sa-seat" data-cat="seat" type="button" onclick={() => (detail = "seat")}>
              <header class="sa-cell-head">
                <Icon name="pin" size={13} />
                <span>座席</span>
                <Icon name="chevron.right" size={13} />
              </header>
              <strong class="sa-seat-val">{seat.assignment}</strong>
              <span class="sa-seat-conf">確度 {seatConfidence}%</span>
            </button>
          {/if}

        </div>
      {:else if !running}
        <p class="sa-empty">確認後、ここに今知るべき要点が表示されます。</p>
      {/if}
    {/if}
  {:else}
    <p class="sa-empty">資料・お知らせの変更を見張り、必要な時だけ知らせます。</p>
  {/if}
</section>

<style>
  /* SenseA dock — no outer card; just a layout for dispersed bento cards that
     sit directly on the surface. Aligned with the Luna surface tokens. */
  .sa {
    display: grid;
    gap: 9px;
    background: transparent;
    /* One inset for every card, so content sits the same distance from each
       edge regardless of the card's size. */
    --sa-pad: 13px 14px;
  }

  /* Back affordance shown on a facet's second page. */
  .sa-back {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    margin: 0;
    padding: 4px 8px 4px 4px;
    border: 0;
    border-radius: 9px;
    background: transparent;
    color: var(--detail-text);
    font: inherit;
    cursor: pointer;
    transition: background 0.14s ease, transform 0.1s ease;
  }
  .sa-back:hover { background: color-mix(in srgb, var(--detail-text) 6%, transparent); }
  .sa-back:active { transform: scale(0.97); }
  .sa-back strong { font-size: 14px; font-weight: 800; }
  .sa-back :global(.icon) { color: var(--detail-muted); }

  /* A facet's full content, shown after tapping its bento cell. */
  .sa-page {
    display: grid;
    gap: 10px;
    padding: 2px 2px 4px;
  }
  .sa-seat-val-lg { font-size: 22px; }
  .sa-seat-ev { margin-top: 4px; }

  /* The detail page is itself a card; the back row sits at its top. */
  .sa-page-card { gap: 8px; cursor: default; }
  .sa-page-card:hover { background: var(--detail-card); box-shadow: none; }
  .sa-page-card:active { transform: none; }

  /* ── Capsules above the cards ─────────────────────────────────────────
     One consistent pill style: borderless, neutral fill, hugging its own
     content. Status, control, and the 継続メモ / 印刷 entries all share it. */
  .sa-pills {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 7px;
  }
  .sa-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 28px;
    max-width: 100%;
    margin: 0;
    padding: 0 12px;
    border: 0;
    border-radius: 999px;
    background: color-mix(in srgb, var(--detail-text) 5%, transparent);
    color: var(--detail-text);
    font: inherit;
  }
  .sa-pill > span {
    font-size: 11.5px;
    font-weight: 700;
  }
  .sa-pill b {
    color: var(--detail-faint);
    font-size: 11px;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
  }
  .sa-pill-text {
    min-width: 0;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Control: a state dot, status line, and the refresh action hugging the end. */
  .sa-pill-ctl { padding-right: 3px; }
  .sa-pill-ctl .sa-pill-text { color: var(--detail-muted); }
  .sa-pill-dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--detail-faint);
  }
  .sa-pill-ctl[data-state="busy"] .sa-pill-dot {
    background: var(--detail-accent);
    animation: sa-pulse 1.3s ease-in-out infinite;
  }
  .sa-pill-ctl[data-state="error"] .sa-pill-dot { background: var(--detail-danger); }
  .sa-pill-ctl[data-state="error"] .sa-pill-text { color: var(--detail-danger); }
  @keyframes sa-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }
  .sa-pill-btn {
    flex: none;
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--detail-muted);
    cursor: pointer;
    transition: background 0.14s ease, color 0.14s ease, transform 0.1s ease;
  }
  .sa-pill-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--detail-text) 9%, transparent);
    color: var(--detail-text);
  }
  .sa-pill-btn:active:not(:disabled) { transform: scale(0.9); }
  .sa-pill-btn:disabled { opacity: 0.4; cursor: default; }

  /* Tappable entries (継続メモ / 印刷): muted leading icon, faint chevron. */
  .sa-pill-go { cursor: pointer; transition: background 0.14s ease, transform 0.1s ease; }
  .sa-pill-go:hover { background: color-mix(in srgb, var(--detail-text) 9%, transparent); }
  .sa-pill-go:active { transform: scale(0.97); }
  .sa-pill-go :global(.icon:first-child) { color: var(--detail-muted); }
  .sa-pill-go :global(.icon:last-child) { color: var(--detail-faint); }
  .sa-pill-go[data-cat="print"] :global(.icon:first-child) { color: #8b5cf6; }

  /* ── Hero (primary) ─────────────────────────────────────────────────── */
  .sa-hero {
    display: grid;
    gap: 8px;
    padding: var(--sa-pad);
    border-radius: 13px;
    border: 0.5px solid var(--detail-border-soft);
    background: var(--detail-card);
  }
  .sa-hero-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .sa-attn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 9px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--detail-accent) 14%, transparent);
    color: var(--detail-accent);
    font-size: 10.5px;
    font-weight: 820;
    letter-spacing: 0.02em;
  }
  .sa-hero[data-attn="true"] .sa-attn {
    background: color-mix(in srgb, var(--detail-warn) 18%, transparent);
    color: var(--detail-warn);
  }
  .sa-hero-time {
    margin-left: auto;
    color: var(--detail-faint);
    font-size: 11px;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }
  .sa-hero-text {
    margin: 0;
    color: var(--detail-text);
    font-size: 14px;
    font-weight: 680;
    line-height: 1.55;
  }

  /* ── Bento grid ─────────────────────────────────────────────────────── */
  .sa-bento {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(186px, 1fr));
    grid-auto-flow: dense;
    gap: 9px;
  }
  .sa-cell {
    display: grid;
    align-content: start;
    gap: 7px;
    width: 100%;
    margin: 0;
    padding: var(--sa-pad);
    border-radius: 12px;
    border: 0.5px solid var(--detail-border-soft);
    background: var(--detail-card);
    font: inherit;
    color: inherit;
    text-align: left;
    cursor: pointer;
    transition: background 0.15s ease, transform 0.1s ease;
  }
  .sa-cell:hover { background: var(--detail-card-hover); }
  .sa-cell:active { transform: scale(0.99); }
  .sa-wide { grid-column: span 2; }
  @media (max-width: 560px) { .sa-wide { grid-column: span 1; } }

  .sa-cell-head {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--detail-muted);
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.02em;
  }
  .sa-cell-head b {
    color: var(--detail-faint);
    font-size: 11px;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
  }
  /* Trailing chevron: the "tap for full content" affordance. */
  .sa-cell-head :global(.icon:last-child) {
    margin-left: auto;
    color: var(--detail-faint);
  }
  /* Category identity: each facet's leading icon takes a hue from the brand
     gradient (violet→blue→cyan), so the bento reads as classified at a glance.
     継続メモ stays muted to keep it the quietest, secondary entry. */
  .sa-cell[data-cat="findings"] .sa-cell-head :global(.icon:first-child) { color: #4f7cff; }
  .sa-cell[data-cat="seat"] .sa-cell-head :global(.icon:first-child) { color: #16b8c4; }

  .sa-list {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .sa-list li {
    position: relative;
    padding-left: 13px;
    color: var(--detail-text);
    font-size: 12px;
    font-weight: 600;
    line-height: 1.45;
  }
  .sa-list li::before {
    content: "";
    position: absolute;
    left: 2px;
    top: 7px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--detail-faint);
  }
  .sa-list.dim li { color: var(--detail-muted); font-weight: 550; font-size: 11.5px; }
  .sa-more { color: var(--detail-faint); font-size: 11px; font-weight: 700; }

  /* Seat cell: the assignment with a plain confidence figure (no bar). */
  .sa-seat-val {
    color: var(--detail-text);
    font-size: 15.5px;
    font-weight: 820;
    line-height: 1.2;
  }
  .sa-seat-conf {
    color: var(--detail-muted);
    font-size: 11px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  /* Print cell. */
  .sa-prints { display: grid; gap: 6px; }
  .sa-print {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 9px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--detail-text) 4%, transparent);
  }
  .sa-print-name {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    color: var(--detail-text);
    font-size: 11.5px;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sa-print-tag {
    flex: none;
    padding: 1px 7px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 800;
  }
  .sa-print[data-tone="ok"] .sa-print-tag {
    background: color-mix(in srgb, #34c759 18%, transparent);
    color: #1a7f37;
  }
  .sa-print[data-tone="warn"] .sa-print-tag {
    background: color-mix(in srgb, var(--detail-warn) 18%, transparent);
    color: var(--detail-warn);
  }
  .sa-print[data-tone="bad"] .sa-print-tag {
    background: color-mix(in srgb, var(--detail-danger) 16%, transparent);
    color: var(--detail-danger);
  }

  /* ── Empty state ────────────────────────────────────────────────────── */
  .sa-empty {
    margin: 0;
    padding: var(--sa-pad);
    border-radius: 12px;
    border: 0.5px solid var(--detail-border-soft);
    background: var(--detail-card);
    color: var(--detail-faint);
    font-size: 12px;
    font-weight: 600;
    line-height: 1.5;
  }
</style>
