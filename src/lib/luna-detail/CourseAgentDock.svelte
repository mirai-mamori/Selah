<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onDestroy, onMount } from "svelte";
  import { fly } from "svelte/transition";
  import Icon from "../Icon.svelte";
  import type { IconName } from "../Icon.svelte";

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
      findings: Array<{
        id: string;
        facet?: string;
        text: string;
        category?: string;
        expiresAt?: string;
        flags?: { action?: boolean; urgent?: boolean };
      }>;
      standingContext: Array<{
        id: string;
        text: string;
        expiresAt?: string;
        flags?: { action?: boolean; urgent?: boolean };
      }>;
      archivedContext?: Array<{ label: string; items: string[] }>;
      seat: { assignment: string; evidence: string[]; confidence: number };
      printCandidates: Array<{ filename: string; reason: string; confidence: number }>;
    };
    printResults: Array<{ filename: string; status: string; detail: string; category?: string }>;
    itemStates?: Record<string, string>;
    runLog?: Array<{ at: number; level: string; message: string }>;
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
  type DetailKey = "findings" | "seat" | "print" | "standing" | "documents" | "log";
  let detail = $state<DetailKey | null>(null);
  const detailTitle: Record<DetailKey, string> = {
    findings: "確認事項",
    seat: "座席",
    print: "印刷",
    standing: "記憶",
    documents: "分析した資料",
    log: "ログ",
  };

  // Relative "time ago" for log entries (epoch seconds).
  function timeAgo(secs: number): string {
    if (!secs) return "";
    const delta = Date.now() / 1000 - secs;
    if (delta < 60) return "たった今";
    if (delta < 3600) return `${Math.floor(delta / 60)}分前`;
    if (delta < 86400) return `${Math.floor(delta / 3600)}時間前`;
    return `${Math.floor(delta / 86400)}日前`;
  }

  function kindLabel(kind: string): string {
    switch (kind) {
      case "material": return "教材";
      case "announcement": return "お知らせ";
      case "report": return "課題";
      default: return kind || "資料";
    }
  }
  let unlisten: (() => void) | null = null;
  // The home view's live height, reused as the detail page's height so the two
  // levels occupy exactly the same footprint (measured while home is shown).
  let homeHeight = $state(0);

  const enabled = $derived(view?.config.enabled ?? false);
  const running = $derived(view?.status.running ?? false);

  function stageText(stage: string): string {
    return (
      {
        checking: "更新を確認中",
        downloading: "資料を取得中",
        analyzing: "資料を一件ずつ分析中",
        summarizing: "個別摘要を最終確認中",
        pending_summary: "新しい摘要を蓄積中",
        printing: "印刷を検証中",
        unchanged: "変更なし",
        done: "確認済み",
        error: "確認エラー",
      }[stage] || (enabled ? "次の確認を待機中" : "無効")
    );
  }
  const stageLabel = $derived(stageText(view?.status.stage || ""));

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

  // Control capsule: dot state + a compact status line. The error MESSAGE is no
  // longer shown on the face — it lives on the control capsule's ログ detail
  // page. The dot still reflects an error state so it stays glanceable.
  const barState = $derived(running ? "busy" : failureNote ? "error" : "idle");
  const controlText = $derived.by(() => {
    if (running) return hasProgress ? `${progressPct}% ・ ${stageLabel}` : stageLabel;
    return stageLabel;
  });
  const runLog = $derived((view?.status.runLog ?? []).slice().reverse());

  // ── Bento content ────────────────────────────────────────────────────────
  const summary = $derived(view?.status.analysis?.summary?.trim() ?? "");
  // Memories carry an expiry date. Active ones drive the メモ pill; expired ones
  // (the backend's archive, plus any active item whose date just rolled past)
  // are not dropped — they stay as a dim 過去 reference, since old context can
  // still relate to new material. Split client-side so display is correct even
  // between backend summary runs.
  const todayStr = $derived.by(() => {
    const now = new Date();
    const m = `${now.getMonth() + 1}`.padStart(2, "0");
    const d = `${now.getDate()}`.padStart(2, "0");
    return `${now.getFullYear()}-${m}-${d}`;
  });
  const isExpired = (m: { expiresAt?: string }) =>
    !!m.expiresAt && m.expiresAt.slice(0, 10) < todayStr;
  const standingContext = $derived(
    (view?.status.analysis?.standingContext ?? [])
      .filter((m) => !isExpired(m))
      // Time-critical memories (urgent) lead so the student sees them first.
      .slice()
      .sort((a, b) => Number(b.flags?.urgent ?? false) - Number(a.flags?.urgent ?? false)),
  );
  // Past memory shown grouped under headings (完了した課題, etc.). Items whose
  // date just rolled past but the backend hasn't consolidated yet appear under
  // a 最近終了 group so they never vanish between summary runs.
  const standingArchive = $derived.by(() => {
    const groups: Array<{ label: string; items: string[] }> = [];
    const stragglers = (view?.status.analysis?.standingContext ?? [])
      .filter(isExpired)
      .map((m) => m.text);
    if (stragglers.length) groups.push({ label: "最近終了", items: stragglers });
    for (const group of view?.status.analysis?.archivedContext ?? []) {
      if (group.items?.length) groups.push(group);
    }
    return groups;
  });
  // Unified per-item user-state overlay (id → "done"/"known"). 確認事項 carry an
  // expiry (auto-drop once past) and an action flag. Items the user marked
  // done/known are hidden; action items lead.
  const itemStates = $derived(view?.status.itemStates ?? {});
  const isHandled = (id: string) => itemStates[id] === "done" || itemStates[id] === "known";
  const findings = $derived(
    (view?.status.analysis?.findings ?? [])
      .filter((f) => !isExpired(f) && !isHandled(f.id))
      .slice()
      .sort((a, b) => Number(b.flags?.action ?? false) - Number(a.flags?.action ?? false)),
  );

  const seat = $derived(view?.status.analysis?.seat);
  const seatConfidence = $derived(Math.round(Math.max(0, Math.min(1, seat?.confidence ?? 0)) * 100));
  const printResults = $derived(view?.status.printResults ?? []);
  // Print problems are surfaced on the 印刷 capsule itself, not on the control
  // capsule. `unknown` is intentionally not auto-retried, but still needs attention.
  const printFailures = $derived(
    printResults.filter((r) => r.status === "error" || r.status === "not_found" || r.status === "unknown").length,
  );
  // Files waiting for the user to approve their print type. The first file of a
  // type asks once; approving it auto-prints that category from then on.
  const printPending = $derived(printResults.filter((r) => r.status === "needs_confirmation").length);

  // 確認事項 plays one item at a time, banner / widget style: an index advances
  // on a timer and the line cross-slides in. Pauses on hover and under reduced
  // motion (then it simply rests on the first item; the full list is one tap
  // away via the caption).
  let findingIndex = $state(0);
  let findingsPaused = $state(false);
  let reduceMotion = $state(false);
  const safeFindingIndex = $derived(findings.length ? findingIndex % findings.length : 0);
  const currentFinding = $derived(findings[safeFindingIndex]?.text ?? "");

  // Materials / announcements the agent has individually analysed & summarised.
  const analyzedDocs = $derived(
    view?.status.documentAnalyses?.filter((doc) => doc.status === "done" && doc.summary?.trim()) ?? [],
  );

  // Count shown next to a detail page's title (seat has no list, so null).
  const detailCount = $derived.by(() => {
    const counts: Partial<Record<DetailKey, number>> = {
      findings: findings.length,
      standing: standingContext.length,
      print: printResults.length,
      documents: analyzedDocs.length,
      log: runLog.length,
    };
    return detail ? counts[detail] ?? null : null;
  });

  // ── Facet registry ───────────────────────────────────────────────────────
  // The capsule row is data-driven: each "go" pill is one entry here, so adding
  // a facet pill is a single array push rather than bespoke markup. The control
  // pill and the highlight/確認事項 cards stay special-cased above.
  type PillEntry = {
    key: DetailKey;
    icon: IconName;
    label: string;
    badge: string | number;
    badgeClass?: string;
    pillClass?: string;
    title?: string;
  };
  const pills = $derived.by<PillEntry[]>(() => {
    const out: PillEntry[] = [];
    if (seat?.assignment) {
      out.push({
        key: "seat",
        icon: "seat",
        label: "座席",
        badge: seat.assignment,
        badgeClass: "sa-pill-val",
        title: seat.assignment,
      });
    }
    if (standingContext.length) {
      out.push({ key: "standing", icon: "note", label: "記憶", badge: standingContext.length });
    }
    if (printResults.length) {
      if (printPending > 0) {
        out.push({
          key: "print",
          icon: "printer",
          label: "印刷",
          badge: `${printPending} 件確認`,
          badgeClass: "sa-pill-accent",
          pillClass: "sa-pill-info",
          title: `${printPending} 件が印刷の許可待ちです`,
        });
      } else if (printFailures > 0) {
        out.push({
          key: "print",
          icon: "printer",
          label: "印刷",
          badge: `${printFailures} 件失敗`,
          badgeClass: "sa-pill-warn",
          pillClass: "sa-pill-attn",
          title: `${printFailures} 件失敗・次回再試行します`,
        });
      } else {
        out.push({ key: "print", icon: "printer", label: "印刷", badge: printResults.length });
      }
    }
    if (analyzedDocs.length) {
      out.push({ key: "documents", icon: "doc.search", label: "分析", badge: analyzedDocs.length });
    }
    return out;
  });

  $effect(() => {
    const n = findings.length;
    if (n <= 1 || findingsPaused || reduceMotion) return;
    const id = setInterval(() => {
      findingIndex = (findingIndex + 1) % n;
    }, 3600);
    return () => clearInterval(id);
  });

  const lastChecked = $derived.by(() => {
    const secs = view?.status.lastRun;
    if (!secs) return "未確認";
    const delta = Date.now() / 1000 - secs;
    if (delta < 60) return "たった今";
    if (delta < 3600) return `${Math.floor(delta / 60)}分前`;
    if (delta < 86400) return `${Math.floor(delta / 3600)}時間前`;
    return `${Math.floor(delta / 86400)}日前`;
  });

  function printLabel(status: string): { text: string; tone: "ok" | "warn" | "bad" | "info" } {
    switch (status) {
      case "printed": return { text: "印刷済み", tone: "ok" };
      case "already_printed": return { text: "印刷済み", tone: "ok" };
      case "error": return { text: "印刷失敗", tone: "bad" };
      case "not_found": return { text: "対象不明", tone: "bad" };
      case "needs_confirmation": return { text: "確認待ち", tone: "info" };
      case "dispatching": return { text: "送信中", tone: "info" };
      case "unknown": return { text: "結果要確認", tone: "warn" };
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
    if (busy || reanalyzingIds.length || view?.status.running) return;
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

  // Single-document re-analyses run independently and concurrently (the backend
  // bounds the real parallelism); each tracks its own in-flight state so only
  // its button spins and the others stay usable.
  let reanalyzingIds = $state<string[]>([]);
  let reanalyzingAll = $state(false);
  const docBusy = (id: string) => reanalyzingIds.includes(id);

  // "Re-analyze all" is the heavy full cycle — exclusive, like 今すぐ確認.
  async function reanalyzeAll(): Promise<void> {
    if (busy || reanalyzingAll || reanalyzingIds.length || view?.status.running) return;
    busy = true;
    reanalyzingAll = true;
    error = "";
    try {
      view = await invoke<CourseAutomationView>("course_automation_reanalyze_all", { lunaId, courseName });
    } catch (cause) {
      error = String(cause);
      await load().catch(() => {});
    } finally {
      busy = false;
      reanalyzingAll = false;
    }
  }

  // Rebuild 記憶 — re-derives the whole working memory from the existing
  // per-document analyses (no re-download / re-analyze), clearing accumulated
  // drift. Heavy and exclusive like a full cycle.
  let rebuildingMemory = $state(false);
  async function rebuildMemory(): Promise<void> {
    if (busy || rebuildingMemory || reanalyzingAll || reanalyzingIds.length || view?.status.running) return;
    busy = true;
    rebuildingMemory = true;
    error = "";
    try {
      view = await invoke<CourseAutomationView>("course_automation_rebuild_memory", { lunaId, courseName });
    } catch (cause) {
      error = String(cause);
      await load().catch(() => {});
    } finally {
      busy = false;
      rebuildingMemory = false;
    }
  }

  // Approve a print category: prints the files waiting under it now and
  // remembers the type so future same-type files print automatically.
  let confirmingCategory = $state<string | null>(null);
  async function confirmPrint(category: string): Promise<void> {
    if (busy || confirmingCategory) return;
    confirmingCategory = category;
    error = "";
    try {
      view = await invoke<CourseAutomationView>("course_automation_confirm_print", {
        lunaId,
        courseName,
        category,
      });
    } catch (cause) {
      error = String(cause);
      await load().catch(() => {});
    } finally {
      confirmingCategory = null;
    }
  }

  // Mark a 確認事項 as done/known by its stable id: the backend hides it via the
  // unified item-state overlay and, for action items, completes the matching task
  // on the main TODO page (the backend TODO sink keeps the two in sync).
  let acknowledging = $state<string | null>(null);
  async function acknowledgeFinding(finding: { id: string }): Promise<void> {
    if (acknowledging) return;
    acknowledging = finding.id;
    error = "";
    try {
      view = await invoke<CourseAutomationView>("course_automation_set_item_state", {
        lunaId,
        courseName,
        id: finding.id,
        state: "done",
      });
    } catch (cause) {
      error = String(cause);
      await load().catch(() => {});
    } finally {
      acknowledging = null;
    }
  }

  // One document — lightweight; several may be in flight at once. Not gated on
  // status.running: if a full cycle is active the backend queue makes this wait
  // its turn, so the click always gives feedback instead of a dead button.
  async function reanalyzeDocument(documentId: string): Promise<void> {
    if (busy || docBusy(documentId)) return;
    reanalyzingIds = [...reanalyzingIds, documentId];
    error = "";
    try {
      view = await invoke<CourseAutomationView>("course_automation_reanalyze_document", {
        lunaId,
        courseName,
        documentId,
      });
    } catch (cause) {
      error = String(cause);
      await load().catch(() => {});
    } finally {
      reanalyzingIds = reanalyzingIds.filter((id) => id !== documentId);
    }
  }

  let motionMedia: MediaQueryList | null = null;
  const syncMotion = () => (reduceMotion = motionMedia?.matches ?? false);

  onMount(async () => {
    motionMedia = window.matchMedia("(prefers-reduced-motion: reduce)");
    syncMotion();
    motionMedia.addEventListener("change", syncMotion);
    await load().catch((cause) => error = String(cause));
    unlisten = await listen<CourseAutomationStatus>("course-automation-updated", (event) => {
      if (event.payload.lunaId !== lunaId || !view) return;
      view = { ...view, status: event.payload };
    });
  });

  onDestroy(() => {
    unlisten?.();
    motionMedia?.removeEventListener("change", syncMotion);
  });
</script>
<section class="sa" aria-label="自動検知">
  {#if enabled}
    {#if detail}
      <!-- SECOND PAGE: the tapped facet in full. A standalone header row (no
           outer card) then the items as individual neutral rows, in the same
           borderless capsule language as the home view. -->
      <div class="sa-detail" style={homeHeight ? `height:${homeHeight}px` : ""}>
        <header class="sa-detail-head">
          <button class="sa-back" type="button" onclick={() => (detail = null)} aria-label="戻る">
            <Icon name="chevron.left" size={17} />
          </button>
          <strong class="sa-detail-title">{detailTitle[detail]}</strong>
          {#if detailCount !== null}<span class="sa-detail-count">{detailCount}</span>{/if}
          {#if detail === "documents" && analyzedDocs.length}
            <button class="sa-detail-action" class:spin={reanalyzingAll} type="button" disabled={busy || running || reanalyzingIds.length > 0} onclick={reanalyzeAll}>
              <Icon name="arrow.clockwise" size={13} />
              <span>全て再分析</span>
            </button>
          {:else if detail === "standing"}
            <button class="sa-detail-action" class:spin={rebuildingMemory} type="button" disabled={busy || running || reanalyzingIds.length > 0} onclick={rebuildMemory}>
              <Icon name="arrow.clockwise" size={13} />
              <span>記憶を再構築</span>
            </button>
          {/if}
        </header>

        {#if error}
          <p class="sa-detail-err">{error}</p>
        {/if}

        <!-- Fixed-height body; content scrolls within when it overflows. -->
        <div class="sa-detail-body">
          {#if detail === "findings"}
            <ol class="sa-rows">
              {#each findings as finding, i}
                <li class="sa-row sa-row-find">
                  <span class="sa-row-ix">{i + 1}</span>
                  <p class="sa-row-text">
                    {#if finding.flags?.action}<span class="sa-row-flag sa-row-flag-do">要対応</span>{/if}
                    {finding.text}
                    {#if finding.expiresAt}<span class="sa-row-until">〜{finding.expiresAt}</span>{/if}
                  </p>
                  <button
                    class="sa-find-done"
                    type="button"
                    disabled={acknowledging === finding.id}
                    onclick={() => acknowledgeFinding(finding)}
                    title="完了 / 既知にする"
                    aria-label="完了 / 既知にする"
                  >
                    <Icon name="check" size={14} />
                  </button>
                </li>
              {/each}
              {#if findings.length === 0}
                <li class="sa-row sa-row-dim"><p class="sa-row-text">確認事項はありません。</p></li>
              {/if}
            </ol>
          {:else if detail === "standing"}
            <ul class="sa-rows">
              {#each standingContext as item}
                <li class="sa-row sa-row-dim" class:sa-row-urgent={item.flags?.urgent}>
                  <span class="sa-row-dot"></span>
                  <p class="sa-row-text">
                    {#if item.flags?.urgent}<span class="sa-row-flag">要対応</span>{/if}
                    {item.text}
                    {#if item.expiresAt}<span class="sa-row-until">〜{item.expiresAt}</span>{/if}
                  </p>
                </li>
              {/each}
            </ul>
            {#if standingArchive.length}
              <!-- Past memory, consolidated into labeled groups (完了した課題,
                   etc.). Kept as reference — the agent still sees it when
                   relating new material — and shown dimmer, set apart. -->
              <p class="sa-rows-sub">過去(参考)</p>
              {#each standingArchive as group}
                <p class="sa-arch-label">{group.label}</p>
                <ul class="sa-rows">
                  {#each group.items as item}
                    <li class="sa-row sa-row-dim sa-row-past">
                      <span class="sa-row-dot"></span>
                      <p class="sa-row-text">{item}</p>
                    </li>
                  {/each}
                </ul>
              {/each}
            {/if}
          {:else if detail === "seat" && seat}
            <div class="sa-seat-grid">
              <div class="sa-seat-hero">
                <strong class="sa-seat-big">{seat.assignment}</strong>
                <span class="sa-seat-conf">確度 {seatConfidence}%</span>
              </div>
              {#if seat.evidence?.length}
                <div class="sa-seat-evi">
                  <span class="sa-seat-evi-label">根拠</span>
                  <ul class="sa-seat-evi-list">
                    {#each seat.evidence as ev}
                      <li class="sa-seat-evi-item">{ev}</li>
                    {/each}
                  </ul>
                </div>
              {/if}
            </div>
          {:else if detail === "print"}
            <ul class="sa-rows">
              {#each printResults as item}
                {@const meta = printLabel(item.status)}
                <li class="sa-row sa-row-print">
                  <span class="sa-row-text sa-row-name">{item.filename}</span>
                  {#if item.status === "needs_confirmation"}
                    <button
                      class="sa-print-approve"
                      class:spin={confirmingCategory === (item.category ?? "")}
                      type="button"
                      disabled={busy || confirmingCategory !== null}
                      onclick={() => confirmPrint(item.category ?? "")}
                      title={item.category ? `「${item.category}」を許可して印刷(以降は自動)` : "許可して印刷"}
                    >
                      <Icon name="printer" size={12} />
                      <span>許可{item.category ? `(${item.category})` : ""}</span>
                    </button>
                  {:else}
                    <span class="sa-print-tag" data-tone={meta.tone}>{meta.text}</span>
                  {/if}
                </li>
              {/each}
            </ul>
          {:else if detail === "documents"}
            <ul class="sa-rows">
              {#each analyzedDocs as doc}
                <li class="sa-doc">
                  <header class="sa-doc-head">
                    <span class="sa-doc-kind">{kindLabel(doc.kind)}</span>
                    <span class="sa-doc-title">{doc.title || doc.filename}</span>
                    <button class="sa-doc-redo" class:spin={docBusy(doc.id)} type="button" disabled={busy || docBusy(doc.id)} onclick={() => reanalyzeDocument(doc.id)} aria-label="再分析" title="再分析">
                      <Icon name="arrow.clockwise" size={12} />
                    </button>
                  </header>
                  <p class="sa-doc-summary">{doc.summary}</p>
                </li>
              {/each}
            </ul>
          {:else if detail === "log"}
            <!-- Control capsule's log: recent run history. The current error (if
                 any) is briefly surfaced at the top; the rest is run-by-run. -->
            {#if failureNote}
              <p class="sa-detail-err">{failureNote}</p>
            {/if}
            <ul class="sa-rows">
              {#each runLog as entry}
                <li class="sa-row sa-row-log">
                  <span class="sa-log-dot" data-level={entry.level}></span>
                  <p class="sa-row-text">{entry.message}</p>
                  <span class="sa-row-until">{timeAgo(entry.at)}</span>
                </li>
              {/each}
              {#if runLog.length === 0}
                <li class="sa-row sa-row-dim"><p class="sa-row-text">まだ実行ログはありません。</p></li>
              {/if}
            </ul>
          {/if}
        </div>
      </div>
    {:else}
      <div class="sa-home" bind:clientHeight={homeHeight}>
      <!-- Capsule row above the cards: control (carries status + errors) and
           compact entries. Each pill hugs its content; no borders, one style. -->
      <div class="sa-pills">
        <div class="sa-pill sa-pill-ctl" data-state={barState}>
          <button class="sa-pill-ctl-open" type="button" onclick={() => (detail = "log")} title="ログを見る">
            <span class="sa-pill-dot"></span>
            <span class="sa-pill-text">{controlText}</span>
          </button>
          <button class="sa-pill-btn" type="button" disabled={busy || running || reanalyzingIds.length > 0} onclick={runNow} aria-label="今すぐ確認" title="今すぐ確認">
            <Icon name="arrow.clockwise" size={13} />
          </button>
        </div>
        {#each pills as pill (pill.key)}
          <button
            class="sa-pill sa-pill-go {pill.pillClass ?? ''}"
            data-cat={pill.key}
            type="button"
            onclick={() => (detail = pill.key)}
            title={pill.title}
          >
            <Icon name={pill.icon} size={13} />
            <span>{pill.label}</span>
            <b class={pill.badgeClass}>{pill.badge}</b>
            <Icon name="chevron.right" size={12} />
          </button>
        {/each}
      </div>

      {#if summary}
        <div class="sa-bento">
          <!-- Highlight — the single most important thing right now. Content
               leads at a large size; a quiet italic caption names it at the
               foot, so the eye reads the message before the label. -->
          <article class="sa-card sa-hl" data-attn={immediateCount > 0}>
            <p class="sa-hl-text">{summary}</p>
            <footer class="sa-cap" data-cat="highlight">
              <Icon name="star" size={13} />
              <em>highlight</em>
              <span class="sa-cap-end">{lastChecked}</span>
            </footer>
          </article>

          {#if findings.length}
            <!-- Confirmations — a banner that plays the items one at a time:
                 each line cross-slides in, advancing on a timer. Hover pauses.
                 The caption shows position and opens the full list. -->
            <section
              class="sa-card sa-find"
              data-cat="findings"
              onmouseenter={() => (findingsPaused = true)}
              onmouseleave={() => (findingsPaused = false)}
              role="group"
              aria-label="確認事項"
            >
              <div class="sa-find-view">
                {#key safeFindingIndex}
                  <p
                    class="sa-find-line"
                    in:fly={{ y: 14, duration: reduceMotion ? 0 : 340 }}
                    out:fly={{ y: -14, duration: reduceMotion ? 0 : 340 }}
                  >
                    {currentFinding}
                  </p>
                {/key}
              </div>
              <button class="sa-cap sa-cap-btn" type="button" onclick={() => (detail = "findings")}>
                <Icon name="list.checks" size={13} />
                <em>確認事項</em>
                <b>{findings.length > 1 ? `${safeFindingIndex + 1}/${findings.length}` : findings.length}</b>
                <Icon name="chevron.right" size={13} />
              </button>
            </section>
          {/if}

        </div>
      {:else if !running}
        <p class="sa-empty">確認後、ここに今知るべき要点が表示されます。</p>
      {/if}
      </div>
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
    /* Width follows the host surface — no fixed cap — so the dock fills
       whatever the page gives it. */
    /* One inset for every card, so content sits the same distance from each
       edge regardless of the card's size. */
    --sa-pad: 13px 14px;
  }

  /* Home view wrapper — measured (bind:clientHeight) to drive the detail
     page's height. Keeps the pills→bento spacing the section used to provide. */
  .sa-home {
    display: grid;
    gap: 9px;
  }

  /* ── Second page (facet detail) ──────────────────────────────────────
     A standalone header row (circular back + title + count) over the items,
     each shown as a neutral row in the same borderless language as home. */
  .sa-detail {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .sa-detail-head {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-bottom: 2px;
  }
  /* The detail page is locked to the home view's measured height (inline
     style), so the body just fills whatever space is left under the header and
     the two levels share an identical footprint. Items lay out in responsive
     columns to fill the width rather than running down one tall list. */
  .sa-detail { min-height: 132px; }
  .sa-detail-body {
    flex: 1;
    min-height: 0;
    display: grid;
    gap: 8px;
    align-content: start;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding-right: 5px;
    scrollbar-width: thin;
    scrollbar-color: color-mix(in srgb, var(--detail-text) 18%, transparent) transparent;
  }
  .sa-detail-body::-webkit-scrollbar { width: 7px; }
  .sa-detail-body::-webkit-scrollbar-thumb {
    background: color-mix(in srgb, var(--detail-text) 16%, transparent);
    border-radius: 999px;
  }
  .sa-back {
    flex: none;
    width: 30px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    margin: 0;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: color-mix(in srgb, var(--detail-text) 5%, transparent);
    color: var(--detail-text);
    cursor: pointer;
    transition: background 0.14s ease, transform 0.1s ease;
  }
  .sa-back:hover { background: color-mix(in srgb, var(--detail-text) 10%, transparent); }
  .sa-back:active { transform: scale(0.92); }
  .sa-detail-title {
    color: var(--detail-text);
    font-size: 16px;
    font-weight: 820;
  }
  .sa-detail-count {
    color: var(--detail-faint);
    font-size: 13px;
    font-weight: 750;
    font-variant-numeric: tabular-nums;
  }
  /* Errors surface here too — the detail page has no control capsule. */
  .sa-detail-err {
    margin: 0;
    padding: 9px 12px;
    border-radius: 11px;
    background: color-mix(in srgb, var(--detail-danger) 12%, transparent);
    color: var(--detail-danger);
    font-size: 12px;
    font-weight: 650;
    line-height: 1.45;
  }
  /* "Re-analyze all" — pushed to the end of the detail header. */
  .sa-detail-action {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 11px;
    border: 0;
    border-radius: 999px;
    background: color-mix(in srgb, var(--detail-text) 6%, transparent);
    color: var(--detail-muted);
    font: inherit;
    font-size: 11.5px;
    font-weight: 750;
    cursor: pointer;
    transition: background 0.14s ease, color 0.14s ease, transform 0.1s ease;
  }
  .sa-detail-action:hover:not(:disabled) {
    background: color-mix(in srgb, var(--detail-text) 11%, transparent);
    color: var(--detail-text);
  }
  .sa-detail-action:active:not(:disabled) { transform: scale(0.97); }
  .sa-detail-action:disabled { opacity: 0.45; cursor: default; }
  /* Spinning refresh icon while a manual re-analysis is in flight. */
  .spin :global(.icon) { animation: sa-spin 0.8s linear infinite; }
  @keyframes sa-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    .spin :global(.icon) { animation: none; }
  }

  /* Item rows: borderless neutral fill, content-first. Two columns (matching
     the home bento) so they fill the width instead of one tall list; collapse
     to a single column when the surface is narrow. */
  .sa-rows {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 7px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  @media (max-width: 560px) {
    .sa-rows { grid-template-columns: 1fr; }
  }
  /* Log rows (two columns, like the other detail lists): status dot + the
     file/operation line + a relative time hugging the end. */
  .sa-row.sa-row-log { align-items: center; }
  .sa-row-log .sa-row-text { flex: 1; min-width: 0; }
  .sa-log-dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #1a7f37;
  }
  .sa-log-dot[data-level="warn"] { background: var(--detail-warn); }
  .sa-log-dot[data-level="error"] { background: var(--detail-danger); }
  .sa-row {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 11px 13px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--detail-text) 5%, transparent);
  }
  .sa-row-ix {
    flex: none;
    min-width: 16px;
    color: var(--detail-accent);
    font-size: 13px;
    font-weight: 820;
    line-height: 1.5;
    font-variant-numeric: tabular-nums;
  }
  .sa-row-dot {
    flex: none;
    width: 5px;
    height: 5px;
    margin-top: 8px;
    border-radius: 50%;
    background: var(--detail-faint);
  }
  .sa-row-text {
    margin: 0;
    color: var(--detail-text);
    font-size: 14px;
    font-weight: 600;
    line-height: 1.5;
  }
  .sa-row-dim .sa-row-text { color: var(--detail-muted); font-weight: 550; }
  /* Urgent (time-critical) memories: brighter text and a small leading flag. */
  .sa-row-urgent .sa-row-text { color: var(--detail-text); font-weight: 650; }
  .sa-row-urgent .sa-row-dot { background: var(--detail-warn); }
  .sa-row-flag {
    margin-right: 6px;
    padding: 1px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--detail-warn) 18%, transparent);
    color: var(--detail-warn);
    font-size: 10px;
    font-weight: 800;
    letter-spacing: 0.02em;
    white-space: nowrap;
    vertical-align: 1px;
  }
  /* Action 確認事項 (pushed to TODO) flag — accent rather than warn. */
  .sa-row-flag-do {
    background: color-mix(in srgb, var(--detail-accent) 18%, transparent);
    color: var(--detail-accent);
  }
  /* 確認事項 rows carry a trailing done/known toggle. */
  .sa-row-find { align-items: center; }
  .sa-row-find .sa-row-text { flex: 1; min-width: 0; }
  .sa-find-done {
    flex: none;
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 50%;
    background: color-mix(in srgb, var(--detail-text) 7%, transparent);
    color: var(--detail-muted);
    cursor: pointer;
    transition: background 0.14s ease, color 0.14s ease, transform 0.1s ease;
  }
  .sa-find-done:hover:not(:disabled) {
    background: color-mix(in srgb, #34c759 22%, transparent);
    color: #1a7f37;
  }
  .sa-find-done:active:not(:disabled) { transform: scale(0.9); }
  .sa-find-done:disabled { opacity: 0.5; cursor: default; }
  .sa-row-until {
    margin-left: 6px;
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--detail-muted);
    opacity: 0.8;
    white-space: nowrap;
  }
  /* Label introducing the expired/past-reference memories. */
  .sa-rows-sub {
    margin: 6px 0 0;
    color: var(--detail-faint);
    font-size: 11px;
    font-weight: 800;
    font-style: italic;
    letter-spacing: 0.02em;
  }
  /* Expired memories sit quieter than active ones — still legible, clearly past. */
  .sa-row-past { opacity: 0.62; }
  /* Heading for each consolidated past group (完了した課題, etc.). */
  .sa-arch-label {
    margin: 4px 0 0;
    color: var(--detail-muted);
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.01em;
  }

  /* Seat detail: two columns — the prominent value block beside its 根拠
     list — collapsing to one column on a narrow surface. */
  .sa-seat-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    align-items: stretch;
    gap: 8px;
  }
  @media (max-width: 560px) {
    .sa-seat-grid { grid-template-columns: 1fr; }
  }
  /* Both sides are a single card: the value block and the 根拠 card. */
  .sa-seat-hero,
  .sa-seat-evi {
    display: grid;
    gap: 7px;
    align-content: start;
    padding: 16px 15px;
    border-radius: 14px;
    background: color-mix(in srgb, var(--detail-text) 5%, transparent);
  }
  .sa-seat-hero { gap: 4px; }
  .sa-seat-evi-label {
    color: var(--detail-muted);
    font-size: 11px;
    font-weight: 800;
    font-style: italic;
    letter-spacing: 0.02em;
  }
  .sa-seat-evi-list {
    display: grid;
    gap: 7px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .sa-seat-evi-item {
    position: relative;
    padding-left: 14px;
    color: var(--detail-muted);
    font-size: 13px;
    font-weight: 550;
    line-height: 1.5;
  }
  .sa-seat-evi-item::before {
    content: "";
    position: absolute;
    left: 2px;
    top: 8px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--detail-faint);
  }
  .sa-seat-big {
    color: var(--detail-text);
    font-size: 24px;
    font-weight: 820;
    line-height: 1.15;
    letter-spacing: -0.01em;
  }

  /* Print rows: filename then a status tag pill. */
  .sa-row-print { align-items: center; }
  .sa-row-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sa-print-tag {
    flex: none;
    padding: 2px 9px;
    border-radius: 999px;
    font-size: 10.5px;
    font-weight: 800;
  }
  .sa-print-tag[data-tone="ok"] {
    background: color-mix(in srgb, #34c759 18%, transparent);
    color: #1a7f37;
  }
  .sa-print-tag[data-tone="warn"] {
    background: color-mix(in srgb, var(--detail-warn) 18%, transparent);
    color: var(--detail-warn);
  }
  .sa-print-tag[data-tone="bad"] {
    background: color-mix(in srgb, var(--detail-danger) 16%, transparent);
    color: var(--detail-danger);
  }
  .sa-print-tag[data-tone="info"] {
    background: color-mix(in srgb, var(--detail-accent) 16%, transparent);
    color: var(--detail-accent);
  }
  /* Approve-and-print button on a 確認待ち row. */
  .sa-print-approve {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 11px;
    border: 0;
    border-radius: 999px;
    background: color-mix(in srgb, var(--detail-accent) 16%, transparent);
    color: var(--detail-accent);
    font: inherit;
    font-size: 11px;
    font-weight: 800;
    cursor: pointer;
    transition: background 0.14s ease, transform 0.1s ease;
  }
  .sa-print-approve :global(.icon) { color: var(--detail-accent); }
  .sa-print-approve:hover:not(:disabled) {
    background: color-mix(in srgb, var(--detail-accent) 24%, transparent);
  }
  .sa-print-approve:active:not(:disabled) { transform: scale(0.96); }
  .sa-print-approve:disabled { opacity: 0.5; cursor: default; }

  /* Analyzed material / announcement cards: a kind badge + title, then the
     agent's per-document summary. */
  .sa-doc {
    display: grid;
    gap: 6px;
    align-content: start;
    padding: 12px 13px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--detail-text) 5%, transparent);
  }
  .sa-doc-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .sa-doc-redo {
    flex: none;
    width: 22px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--detail-faint);
    cursor: pointer;
    transition: background 0.14s ease, color 0.14s ease, transform 0.1s ease;
  }
  .sa-doc-redo:hover:not(:disabled) {
    background: color-mix(in srgb, var(--detail-text) 9%, transparent);
    color: var(--detail-text);
  }
  .sa-doc-redo:active:not(:disabled) { transform: scale(0.9); }
  .sa-doc-redo:disabled { opacity: 0.4; cursor: default; }
  .sa-doc-kind {
    flex: none;
    padding: 1px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, #4f7cff 16%, transparent);
    color: #4f7cff;
    font-size: 10px;
    font-weight: 820;
    letter-spacing: 0.02em;
  }
  .sa-doc-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--detail-text);
    font-size: 13px;
    font-weight: 750;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sa-doc-summary {
    margin: 0;
    color: var(--detail-muted);
    font-size: 12.5px;
    font-weight: 550;
    line-height: 1.5;
  }

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
    /* Cap every pill so a long value can't stretch the row; the variable part
       inside (status text / seat value) ellipsis-truncates within this width. */
    max-width: 480px;
    margin: 0;
    padding: 0 12px;
    border: 0;
    border-radius: 999px;
    background: color-mix(in srgb, var(--detail-text) 5%, transparent);
    color: var(--detail-text);
    font: inherit;
  }
  .sa-pill > span {
    flex: none;
    font-size: 11.5px;
    font-weight: 700;
  }
  .sa-pill b {
    color: var(--detail-faint);
    font-size: 11px;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
  }
  /* A pill's variable value (e.g. the seat assignment) shrinks and truncates
     rather than widening the pill past its cap. */
  .sa-pill-val {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sa-pill-text {
    min-width: 0;
    max-width: 100%;
    /* Match the other pill labels' sizing — the text now lives inside a button,
       so it no longer inherits it from `.sa-pill > span`. */
    font-size: 11.5px;
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Control: a state dot, status line, and the refresh action hugging the end. */
  .sa-pill-ctl { padding-right: 3px; }
  .sa-pill-ctl .sa-pill-text { color: var(--detail-muted); }
  /* The dot + status line are one button opening the ログ detail page. */
  .sa-pill-ctl-open {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    max-width: 100%;
    margin: 0;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
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
  .sa-pill :global(.icon) { flex: none; }
  .sa-pill-go :global(.icon:first-child) { color: var(--detail-muted); }
  .sa-pill-go :global(.icon:last-child) { color: var(--detail-faint); }
  /* A pill carrying its own failure feedback (印刷): warn-tinted fill + icon. */
  .sa-pill-attn {
    background: color-mix(in srgb, var(--detail-warn) 16%, transparent);
  }
  .sa-pill-attn :global(.icon:first-child) { color: var(--detail-warn); }
  .sa-pill-warn { color: var(--detail-warn) !important; }
  /* A pill awaiting user confirmation (印刷の許可待ち): accent-tinted, not an error. */
  .sa-pill-info {
    background: color-mix(in srgb, var(--detail-accent) 16%, transparent);
  }
  .sa-pill-info :global(.icon:first-child) { color: var(--detail-accent); }
  .sa-pill-accent { color: var(--detail-accent) !important; }

  /* ── Bento cards ─────────────────────────────────────────────────────
     Same visual language as the capsules above: borderless, the same neutral
     translucent fill, no hard card chrome — just larger, rounded panels.
     Content-first: substance leads at a readable size, a quiet italic caption
     names the facet at the foot. */
  /* Responsive two-up: highlight and 確認事項 sit side by side and fill the
     width when there's room, then stack below ~620px. The row stretches both
     to equal height, so they stay a unified pair at any width. A min-height
     floor keeps short content from collapsing; content grows past it instead
     of being clipped. */
  .sa-bento {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 9px;
  }
  .sa-card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
    min-height: 132px;
    margin: 0;
    padding: var(--sa-pad);
    border: 0;
    border-radius: 16px;
    background: color-mix(in srgb, var(--detail-text) 5%, transparent);
    font: inherit;
    color: inherit;
    text-align: left;
  }

  /* The 確認事項 footer entry is the only tappable surface on the cards. */
  .sa-cap-btn { transition: background 0.14s ease; }

  /* ── Highlight ───────────────────────────────────────────────────────
     The single most important thing now: large lead text, caption beneath. */
  .sa-hl-text {
    flex: 1;
    min-height: 0;
    margin: 0;
    color: var(--detail-text);
    font-size: clamp(16px, 2.6vw, 19px);
    font-weight: 700;
    line-height: 1.45;
  }

  /* ── Confirmations banner ────────────────────────────────────────────
     One item shown at a time, like a widget/banner. The line is absolutely
     positioned so the outgoing and incoming lines overlap during the
     cross-slide instead of pushing the layout. */
  .sa-find { gap: 8px; }
  .sa-find-view {
    flex: 1;
    min-height: 0;
    position: relative;
    overflow: hidden;
  }
  .sa-find-line {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    margin: 0;
    color: var(--detail-text);
    font-size: clamp(15px, 2.4vw, 18px);
    font-weight: 680;
    line-height: 1.4;
  }
  .sa-cap-btn {
    margin-top: 8px;
    padding: 9px 0 0;
    border: 0;
    border-top: 0.5px solid color-mix(in srgb, var(--detail-text) 10%, transparent);
    background: transparent;
    cursor: pointer;
  }
  .sa-cap-btn:hover em { color: var(--detail-text); }

  /* Confidence figure under the seat value (no bar). */
  .sa-seat-conf {
    color: var(--detail-muted);
    font-size: 12px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  /* ── Foot caption (shared) ───────────────────────────────────────────
     Small italic label, a category dot for brand identity, and trailing
     count / figure / chevron. Anchored to the card foot via margin-top. */
  .sa-cap {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    font: inherit;
    text-align: left;
  }
  .sa-cap em {
    font-style: italic;
    font-size: 11px;
    font-weight: 640;
    color: var(--detail-muted);
    letter-spacing: 0.01em;
  }
  .sa-cap b {
    color: var(--detail-faint);
    font-size: 11px;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
  }
  .sa-cap-end {
    margin-left: auto;
    color: var(--detail-faint);
    font-size: 11px;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }
  /* Leading icon names the facet; trailing chevron (findings) hugs the end. */
  .sa-cap > :global(.icon:first-child) { flex: none; color: var(--detail-muted); }
  .sa-cap-btn :global(.icon:last-child) { margin-left: auto; color: var(--detail-faint); }
  /* Attention tints the highlight caption (icon + label) without any extra word. */
  .sa-hl[data-attn="true"] .sa-cap > :global(.icon:first-child),
  .sa-hl[data-attn="true"] .sa-cap em { color: var(--detail-warn); }

  /* ── Empty state ────────────────────────────────────────────────────── */
  .sa-empty {
    margin: 0;
    padding: var(--sa-pad);
    border: 0;
    border-radius: 16px;
    background: color-mix(in srgb, var(--detail-text) 5%, transparent);
    color: var(--detail-faint);
    font-size: 12px;
    font-weight: 600;
    line-height: 1.5;
  }
</style>
