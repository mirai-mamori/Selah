<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onDestroy, onMount } from "svelte";
  import Icon from "../Icon.svelte";
  import {
    COURSE_AGENT_DETAIL_TITLES,
    COURSE_AGENT_MODULE_KEYS,
    buildCourseAgentModules,
    kindLabel,
    printLabel,
    timeAgo,
    type CourseAgentDetailKey,
    type CourseAgentModules,
    type CourseAutomationStatus,
    type CourseAutomationView,
  } from "./courseAgentModules";

  interface Props {
    lunaId: string;
    courseName: string;
    ondetailchange?: (open: boolean) => void;
  }

  let { lunaId, courseName, ondetailchange }: Props = $props();
  let view = $state<CourseAutomationView | null>(null);
  let busy = $state(false);
  let error = $state("");
  // Which bento cell is drilled into. Tapping a cell opens its full content as
  // a focused second page; null is the home (bento) view.
  let detail = $state<CourseAgentDetailKey | null>(null);
  let unlisten: (() => void) | null = null;
  // The home view's live height, reused as the detail page's height so the two
  // levels occupy exactly the same footprint (measured while home is shown).
  let homeHeight = $state(0);

  const enabled = $derived(view?.config.enabled ?? false);
  const initialModules = buildCourseAgentModules(null, { enabled: false });
  let controlModule = $state(initialModules.control);
  let highlightModule = $state(initialModules.highlight);
  let pendingModule = $state(initialModules.pending);
  let standingModule = $state(initialModules.standing);
  let seatModule = $state(initialModules.seat);
  let printModule = $state(initialModules.print);
  let organizeModule = $state(initialModules.organize);
  let documentsModule = $state(initialModules.documents);
  let logModule = $state(initialModules.log);
  let navigationModule = $state(initialModules.navigation);
  let moduleSignatures = initialModules.signatures;

  function applyCourseAgentModules(next: CourseAgentModules): void {
    for (const key of COURSE_AGENT_MODULE_KEYS) {
      if (moduleSignatures[key] === next.signatures[key]) continue;
      switch (key) {
        case "control":
          controlModule = next.control;
          break;
        case "highlight":
          highlightModule = next.highlight;
          break;
        case "pending":
          pendingModule = next.pending;
          break;
        case "standing":
          standingModule = next.standing;
          break;
        case "seat":
          seatModule = next.seat;
          break;
        case "print":
          printModule = next.print;
          break;
        case "organize":
          organizeModule = next.organize;
          break;
        case "documents":
          documentsModule = next.documents;
          break;
        case "log":
          logModule = next.log;
          break;
        case "navigation":
          navigationModule = next.navigation;
          break;
      }
    }
    moduleSignatures = next.signatures;
  }

  $effect(() => {
    applyCourseAgentModules(buildCourseAgentModules(view?.status, { enabled, localError: error }));
  });

  $effect(() => {
    ondetailchange?.(detail !== null);
  });

  const running = $derived(controlModule.running);
  const barState = $derived(controlModule.barState);
  const controlText = $derived(controlModule.text);
  const failureNote = $derived(logModule.failureNote);
  const runLog = $derived(logModule.entries);
  const summary = $derived(highlightModule.summary);
  const standingContext = $derived(standingModule.active);
  const standingArchive = $derived(standingModule.archive);
  const pendingItems = $derived(pendingModule.items);
  const seat = $derived(seatModule.seat);
  const seatConfidence = $derived(seatModule.confidencePct);
  const printResults = $derived(printModule.results);
  const printCandidates = $derived(printModule.unresolvedCandidates);
  const analyzedDocs = $derived(documentsModule.items);
  const pendingDocumentCount = $derived(documentsModule.pendingCount);
  const organizeGroups = $derived(organizeModule.groups);
  const organizeCanUndo = $derived(organizeModule.canUndo);
  const detailCount = $derived.by(() => (detail ? navigationModule.detailCounts[detail] ?? null : null));
  const pills = $derived(navigationModule.pills);
  const lastChecked = $derived(highlightModule.lastChecked);

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

  // Mark a 自動検知 notice as known by its stable id. Action items live in TODO,
  // so this only acknowledges non-TODO notices shown in 保留中.
  let acknowledging = $state<string | null>(null);
  async function acknowledgePending(item: { sourceId?: string }): Promise<void> {
    if (acknowledging || !item.sourceId) return;
    acknowledging = item.sourceId;
    error = "";
    try {
      view = await invoke<CourseAutomationView>("course_automation_set_item_state", {
        lunaId,
        courseName,
        id: item.sourceId,
        state: "known",
      });
    } catch (cause) {
      error = String(cause);
      await load().catch(() => {});
    } finally {
      acknowledging = null;
    }
  }

  // Re-run the theme filing on demand instead of waiting for the next cycle.
  let organizingNow = $state(false);
  async function organizeNow(): Promise<void> {
    if (busy || organizingNow) return;
    busy = true;
    organizingNow = true;
    error = "";
    try {
      view = await invoke<CourseAutomationView>("course_automation_organize_now", {
        lunaId,
        courseName,
      });
    } catch (cause) {
      error = String(cause);
      await load().catch(() => {});
    } finally {
      busy = false;
      organizingNow = false;
    }
  }

  // Revert the last auto-organize batch: moves the filed documents back to where
  // they were and removes the now-empty theme folders. Exclusive while running.
  let undoingOrganize = $state(false);
  async function undoOrganize(): Promise<void> {
    if (busy || undoingOrganize || !organizeCanUndo) return;
    busy = true;
    undoingOrganize = true;
    error = "";
    try {
      view = await invoke<CourseAutomationView>("course_automation_undo_organize", {
        lunaId,
        courseName,
      });
    } catch (cause) {
      error = String(cause);
      await load().catch(() => {});
    } finally {
      busy = false;
      undoingOrganize = false;
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

  onMount(async () => {
    await load().catch((cause) => error = String(cause));
    unlisten = await listen<CourseAutomationStatus>("course-automation-updated", (event) => {
      if (event.payload.lunaId !== lunaId || !view) return;
      view = { ...view, status: event.payload };
    });
  });

  onDestroy(() => {
    unlisten?.();
    ondetailchange?.(false);
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
          <strong class="sa-detail-title">{COURSE_AGENT_DETAIL_TITLES[detail]}</strong>
          {#if detailCount !== null}<span class="sa-detail-count">{detailCount}</span>{/if}
          {#if detail === "documents" && analyzedDocs.length}
            <button class="sa-detail-action" class:spin={reanalyzingAll} type="button" disabled={busy || running || reanalyzingIds.length > 0} onclick={reanalyzeAll}>
              <Icon name="arrow.clockwise" size={13} />
              <span>全て再分析</span>
            </button>
          {:else if detail === "seat" && seat}
            <button class="sa-detail-action" class:spin={rebuildingMemory} type="button" disabled={busy || rebuildingMemory || running || reanalyzingIds.length > 0} onclick={rebuildMemory}>
              <Icon name="arrow.clockwise" size={13} />
              <span>座席を再生成</span>
            </button>
          {:else if detail === "standing"}
            <button class="sa-detail-action" class:spin={rebuildingMemory} type="button" disabled={busy || running || reanalyzingIds.length > 0} onclick={rebuildMemory}>
              <Icon name="arrow.clockwise" size={13} />
              <span>記憶を再構築</span>
            </button>
          {:else if detail === "organize"}
            <div class="sa-detail-actions">
              {#if organizeCanUndo}
                <button class="sa-detail-action" class:spin={undoingOrganize} type="button" disabled={busy} onclick={undoOrganize}>
                  <Icon name="arrow.clockwise" size={13} />
                  <span>元に戻す</span>
                </button>
              {/if}
              <button class="sa-detail-action" class:spin={organizingNow} type="button" disabled={busy} onclick={organizeNow}>
                <Icon name="folder.open" size={13} />
                <span>今すぐ整理</span>
              </button>
            </div>
          {/if}
        </header>

        {#if error}
          <p class="sa-detail-err">{error}</p>
        {/if}

        <!-- Fixed-height body; content scrolls within when it overflows. -->
        <div class="sa-detail-body">
          {#if detail === "pending"}
            <ol class="sa-rows">
              {#each pendingItems as item, i}
                <li class="sa-row sa-row-pending" data-tone={item.tone ?? "info"}>
                  <span class="sa-row-ix">{i + 1}</span>
                  <p class="sa-row-text">
                    <span class="sa-row-flag">{item.kind === "print" ? "印刷" : item.kind === "seat" ? "座席" : "通知"}</span>
                    {item.text}
                    {#if item.detail}<span class="sa-row-note">{item.detail}</span>{/if}
                    {#if item.expiresAt}<span class="sa-row-until">〜{item.expiresAt}</span>{/if}
                  </p>
                  {#if item.target}
                    <button class="sa-row-open" type="button" onclick={() => (detail = item.target ?? null)}>
                      <span>開く</span>
                      <Icon name="chevron.right" size={13} />
                    </button>
                  {:else}
                    <button
                      class="sa-pending-done"
                      type="button"
                      disabled={acknowledging === item.sourceId}
                      onclick={() => acknowledgePending(item)}
                      title="確認済みにする"
                      aria-label="確認済みにする"
                    >
                      <Icon name="check" size={14} />
                    </button>
                  {/if}
                </li>
              {/each}
              {#if pendingItems.length === 0}
                <li class="sa-row sa-row-dim"><p class="sa-row-text">保留中の項目はありません。</p></li>
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
              {#each printCandidates as item}
                <li class="sa-row sa-row-print sa-row-print-candidate">
                  <span class="sa-row-text sa-row-name">{item.filename}</span>
                  <span class="sa-print-detail">{item.reason}</span>
                  <span class="sa-print-tag" data-tone="info">候補 {Math.round(Math.max(0, Math.min(1, item.confidence)) * 100)}%</span>
                </li>
              {/each}
              {#if printResults.length === 0 && printCandidates.length === 0}
                <li class="sa-row sa-row-dim"><p class="sa-row-text">印刷対象はありません。</p></li>
              {/if}
            </ul>
          {:else if detail === "organize"}
            <!-- Files the agent auto-filed into theme folders. Each group is one
                 theme/session; the whole batch can be reverted in one tap. -->
            {#each organizeGroups as group (group.id)}
              <p class="sa-arch-label">
                <Icon name="folder.open" size={12} />
                {group.label}
                <span class="sa-organize-count">{group.files.length}</span>
              </p>
              <ul class="sa-rows">
                {#each group.files as file}
                  <li class="sa-row sa-row-organize">
                    <span class="sa-row-dot"></span>
                    <p class="sa-row-text sa-row-name">{file.title || file.filename}</p>
                    {#if group.folder}<span class="sa-organize-folder">{group.folder}/</span>{/if}
                  </li>
                {/each}
              </ul>
            {/each}
            {#if organizeGroups.length === 0}
              <ul class="sa-rows">
                <li class="sa-row sa-row-dim">
                  <p class="sa-row-text">資料がたまると、テーマ(同じ回・課題など)ごとに自動で整理します。</p>
                </li>
              </ul>
            {/if}
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
              {#if analyzedDocs.length === 0 && pendingDocumentCount > 0}
                <li class="sa-row sa-row-dim">
                  <p class="sa-row-text">{pendingDocumentCount}件の新しい資料を最終摘要に反映中です。</p>
                </li>
              {:else if analyzedDocs.length === 0}
                <li class="sa-row sa-row-dim"><p class="sa-row-text">分析済みの資料はまだありません。</p></li>
              {/if}
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
      <!-- Capsule row: control (carries status + errors) and compact entries.
           Each pill hugs its content; no borders, one style. -->
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
        {#if summary}
          <div class="sa-pill sa-pill-highlight" aria-label="highlight" title={summary}>
            <Icon name="star" size={13} />
            <span class="sa-hl-text">{summary}</span>
            <b class="sa-highlight-when">{lastChecked}</b>
          </div>
        {/if}
        {#each pills as pill (pill.key)}
          <button
            class="sa-pill sa-pill-go"
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

      {#if !summary && !running}
        <p class="sa-empty">確認後、ここに今知るべき要点が表示されます。</p>
      {/if}
      </div>
    {/if}
  {:else}
    <p class="sa-empty">自動検知は必要な変化だけを知らせます。</p>
  {/if}
</section>

<style>
  /* 自動検知 dock — no outer card; just a layout for dispersed bento cards that
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
     page's height. Keeps the capsule stack spacing stable. */
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
  /* The detail page matches the home view's measured height so the two levels
     keep the same footprint inside the default hero rhythm. */
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
    background: color-mix(in srgb, var(--detail-accent) 18%, transparent);
    color: var(--detail-accent);
    font-size: 10px;
    font-weight: 800;
    letter-spacing: 0.02em;
    white-space: nowrap;
    vertical-align: 1px;
  }
  .sa-row-pending { align-items: center; }
  .sa-row-pending[data-tone="warn"] .sa-row-flag {
    background: color-mix(in srgb, var(--detail-warn) 18%, transparent);
    color: var(--detail-warn);
  }
  .sa-row-pending .sa-row-text { flex: 1; min-width: 0; }
  .sa-row-note {
    display: block;
    margin-top: 2px;
    color: var(--detail-muted);
    font-size: 12px;
    font-weight: 550;
    line-height: 1.45;
  }
  .sa-pending-done {
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
  .sa-pending-done:hover:not(:disabled) {
    background: color-mix(in srgb, #34c759 22%, transparent);
    color: #1a7f37;
  }
  .sa-pending-done:active:not(:disabled) { transform: scale(0.9); }
  .sa-pending-done:disabled { opacity: 0.5; cursor: default; }
  .sa-row-open {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 5px 10px;
    border: 0;
    border-radius: 999px;
    background: color-mix(in srgb, var(--detail-text) 7%, transparent);
    color: var(--detail-muted);
    font: inherit;
    font-size: 11px;
    font-weight: 800;
    cursor: pointer;
    transition: background 0.14s ease, color 0.14s ease, transform 0.1s ease;
  }
  .sa-row-open:hover {
    background: color-mix(in srgb, var(--detail-text) 11%, transparent);
    color: var(--detail-text);
  }
  .sa-row-open:active { transform: scale(0.96); }
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

  /* ── Organize (整理) detail ───────────────────────────────────────────
     A theme-folder header per group + the filed documents under it, and an
     undo control for the whole last batch. */
  /* Group the 整理 header actions together on the right (a single margin-left
     auto on the wrapper, so the two buttons sit side by side instead of each
     claiming half the free space and drifting to opposite ends). */
  .sa-detail-actions {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }
  .sa-detail-actions .sa-detail-action { margin-left: 0; }

  /* The group header carries a folder glyph + the theme name + a count chip. */
  .sa-arch-label:has(.sa-organize-count) {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 10px;
  }
  .sa-arch-label > :global(.icon) { flex: none; color: var(--detail-faint); }
  .sa-organize-count {
    margin-left: auto;
    color: var(--detail-faint);
    font-size: 11px;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
  }
  .sa-organize-folder {
    flex: none;
    color: var(--detail-faint);
    font-size: 11px;
    font-weight: 650;
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
  .sa-row-print-candidate {
    gap: 8px;
  }
  .sa-row-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sa-print-detail {
    flex: 1;
    min-width: 80px;
    overflow: hidden;
    color: var(--detail-muted);
    font-size: 11.5px;
    font-weight: 600;
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
    max-width: 400px;
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
    color: var(--detail-text) !important;
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
  .sa-pill-go[data-cat="seat"] > span { color: var(--detail-muted); }
  /* Semantic pills keep the shared neutral capsule background and icons. */
  .sa-pill-warn { color: var(--detail-warn) !important; }
  .sa-pill-accent { color: var(--detail-accent) !important; }

  .sa-pill-highlight {
    min-width: 0;
  }
  .sa-pill-highlight > :global(.icon:first-child) { color: var(--detail-muted); }
  .sa-pill-highlight > .sa-hl-text {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    color: var(--detail-text);
    font-size: 11.5px;
    font-weight: 700;
    line-height: 1.2;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sa-pill b.sa-highlight-when {
    color: var(--detail-faint);
  }

  /* Confidence figure under the seat value (no bar). */
  .sa-seat-conf {
    color: var(--detail-muted);
    font-size: 12px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

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
