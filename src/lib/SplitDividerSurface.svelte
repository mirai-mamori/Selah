<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onDestroy, onMount } from "svelte";
  import { applyAuxiliaryTheme, syncAuxiliaryTheme } from "./auxiliarySurfaceTheme";

  interface DocumentTab {
    id: string;
    label: string;
    target: string;
    active: boolean;
    splitRatios?: number[];
  }

  interface DocumentTabsChangedPayload {
    owner?: string;
    tabs?: DocumentTab[];
  }

  function readParam(name: string): string {
    const search = new URLSearchParams(window.location.search);
    const fromSearch = search.get(name);
    if (fromSearch) return fromSearch;
    const rawHash = window.location.hash.startsWith("#") ? window.location.hash.slice(1) : window.location.hash;
    return new URLSearchParams(rawHash).get(name) || "";
  }

  const owner = readParam("ownerLabel") || readParam("owner") || "document-tabs";
  const parentTarget = readParam("parentTarget") || readParam("parent") || "";
  const handleIndex = Number.parseInt(readParam("handleIndex") || "0", 10) || 0;
  const DIVIDER_WIDTH = 12;

  let dragging = $state(false);
  let dragSession = $state<{ pointerId: number; startScreenX: number; baseRatios: number[] } | null>(null);
  let themeUnlisten: (() => void) | null = null;
  let appThemeUnlisten: (() => void) | null = null;
  let tabsUnlisten: (() => void) | null = null;
  let queuedDelta: number | null = null;
  let dragInFlight = false;
  let dragFlushPromise: Promise<void> | null = null;
  let currentRatios = $state<number[]>([]);
  let surfaceEl = $state<HTMLButtonElement | null>(null);
  let surfaceWidth = $state(0);
  let resizeObserver: ResizeObserver | null = null;

  function normalizeRatios(values: number[]): number[] {
    const cleaned = values.filter((value) => Number.isFinite(value) && value > 0);
    if (!cleaned.length || cleaned.length !== values.length) return [];
    const sum = cleaned.reduce((total, value) => total + value, 0);
    if (sum <= Number.EPSILON) return [];
    return cleaned.map((value) => value / sum);
  }

  async function fetchBaseRatios(): Promise<number[]> {
    if (!parentTarget) return [];
    const tabs = await invoke<DocumentTab[]>("document_tabs_list", { owner });
    return applyTabState(tabs);
  }

  function applyTabState(tabs: DocumentTab[]): number[] {
    const tab = tabs.find((item) => item.target === parentTarget || item.id === parentTarget || item.label === parentTarget);
    const next = normalizeRatios(tab?.splitRatios || []);
    currentRatios = next;
    return next;
  }

  function syncSurfaceWidth(): void {
    surfaceWidth = surfaceEl?.clientWidth || 0;
  }

  function dividerOffsetPx(): number {
    if (!dragging || surfaceWidth <= 0 || currentRatios.length <= handleIndex) return 0;
    const dividerCount = Math.max(0, currentRatios.length - 1);
    const usableWidth = Math.max(0, surfaceWidth - dividerCount * DIVIDER_WIDTH);
    const beforeRatio = currentRatios
      .slice(0, handleIndex + 1)
      .reduce((total, value) => total + value, 0);
    return beforeRatio * usableWidth + handleIndex * DIVIDER_WIDTH;
  }

  // Layout of each pane region across the full content width, used to place the
  // ratio placeholders shown while dragging (so the resizing panes underneath —
  // which briefly repaint white — are covered by a frosted overlay instead).
  function regionLayout(): { index: number; x: number; width: number; ratio: number }[] {
    const ratios = currentRatios;
    if (!ratios.length || surfaceWidth <= 0) return [];
    const usableWidth = Math.max(0, surfaceWidth - (ratios.length - 1) * DIVIDER_WIDTH);
    const out: { index: number; x: number; width: number; ratio: number }[] = [];
    let x = 0;
    for (let i = 0; i < ratios.length; i++) {
      const width = ratios[i] * usableWidth;
      out.push({ index: i, x, width, ratio: ratios[i] });
      x += width + DIVIDER_WIDTH;
    }
    return out;
  }

  function queueDrag(deltaPx: number): void {
    if (!dragSession) return;
    queuedDelta = deltaPx;
    if (dragInFlight) return;
    dragInFlight = true;
    const flush = async () => {
      while (queuedDelta !== null && dragSession) {
        const nextDelta = queuedDelta;
        queuedDelta = null;
        try {
          await invoke<number[]>("document_tabs_drag_split", {
            owner,
            parent: parentTarget,
            handleIndex,
            baseRatios: dragSession.baseRatios,
            deltaPx: nextDelta,
          });
        } catch {
          queuedDelta = null;
          break;
        }
      }
      dragInFlight = false;
    };
    dragFlushPromise = flush();
  }

  async function beginDrag(event: PointerEvent): Promise<void> {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const baseRatios = await fetchBaseRatios();
    if (baseRatios.length <= handleIndex + 1) return;
    dragSession = {
      pointerId: event.pointerId,
      startScreenX: event.screenX,
      baseRatios,
    };
    currentRatios = baseRatios;
    dragging = true;
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {}
    await invoke("document_tabs_begin_split_drag", {
      owner,
      parent: parentTarget,
      handleIndex,
    }).catch(() => {
      dragging = false;
      dragSession = null;
    });
  }

  function updateDrag(event: PointerEvent): void {
    if (!dragSession || event.pointerId !== dragSession.pointerId) return;
    event.preventDefault();
    queueDrag(event.screenX - dragSession.startScreenX);
  }

  function endDrag(event?: PointerEvent): void {
    if (!dragSession) return;
    if (event && event.pointerId !== dragSession.pointerId) return;
    try {
      (event?.currentTarget as HTMLElement | undefined)?.releasePointerCapture(dragSession.pointerId);
    } catch {}
    const finish = async () => {
      try {
        await dragFlushPromise;
      } catch {}
      await invoke("document_tabs_end_split_drag", {
        owner,
        parent: parentTarget,
      }).catch(() => {});
      dragging = false;
      dragSession = null;
      queuedDelta = null;
      dragFlushPromise = null;
    };
    void finish();
  }

  onMount(async () => {
    document.documentElement.setAttribute("data-aux-surface", "split-divider");
    document.body.setAttribute("data-aux-surface", "split-divider");
    if (surfaceEl) {
      resizeObserver = new ResizeObserver(() => syncSurfaceWidth());
      resizeObserver.observe(surfaceEl);
      syncSurfaceWidth();
    }
    currentRatios = await fetchBaseRatios();
    await syncAuxiliaryTheme();
    themeUnlisten = await listen<string>("theme-changed", (event) => {
      applyAuxiliaryTheme(event.payload);
    });
    appThemeUnlisten = await listen("app-theme-changed", () => {
      void syncAuxiliaryTheme();
    });
    tabsUnlisten = await listen<DocumentTabsChangedPayload>("document-tabs-changed", (event) => {
      if (!event.payload || event.payload.owner !== owner) return;
      applyTabState(event.payload.tabs || []);
    });
  });

  onDestroy(() => {
    endDrag();
    themeUnlisten?.();
    appThemeUnlisten?.();
    tabsUnlisten?.();
    resizeObserver?.disconnect();
    document.documentElement.removeAttribute("data-aux-surface");
    document.body.removeAttribute("data-aux-surface");
  });
</script>

<button
  bind:this={surfaceEl}
  class="divider"
  class:dragging
  class:expanded={dragging}
  type="button"
  aria-label="Resize split panes"
  title="Drag to resize panes"
  onpointerdown={beginDrag}
  onpointermove={updateDrag}
  onpointerup={endDrag}
  onpointercancel={endDrag}
>
  {#if dragging}
    <div class="drag-overlay" aria-hidden="true">
      {#each regionLayout() as region (region.index)}
        <div class="drag-pane" style:left={`${region.x}px`} style:width={`${region.width}px`}>
          <span class="drag-ratio">{Math.round(region.ratio * 100)}%</span>
        </div>
      {/each}
    </div>
  {/if}
  <span class="divider-visual" style:left={`${dividerOffsetPx()}px`}>
    <span class="divider-rail"></span>
    <span class="divider-grip"></span>
  </span>
</button>

<style>
  :global(html[data-aux-surface="split-divider"]),
  :global(body[data-aux-surface="split-divider"]),
  :global(body[data-aux-surface="split-divider"] #app) {
    margin: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
  }

  .divider {
    position: relative;
    width: 100%;
    height: 100%;
    padding: 0;
    border: none;
    background: transparent;
    cursor: col-resize;
    user-select: none;
    -webkit-user-select: none;
    touch-action: none;
    overflow: hidden;
  }

  /* Frosted cover shown while dragging, so the live-resizing panes underneath
     (which briefly repaint white) are masked behind a placeholder with the
     pane ratios instead of flashing white. */
  .drag-overlay {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: rgba(244, 245, 247, 0.95);
    backdrop-filter: blur(10px) saturate(1.15);
    -webkit-backdrop-filter: blur(10px) saturate(1.15);
    animation: split-overlay-in 0.12s ease both;
  }

  @keyframes split-overlay-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .drag-pane {
    position: absolute;
    top: 0;
    bottom: 0;
    display: grid;
    place-items: center;
  }

  .drag-ratio {
    font-size: clamp(18px, 4vw, 26px);
    font-weight: 800;
    letter-spacing: 0.02em;
    font-variant-numeric: tabular-nums;
    color: rgba(23, 59, 104, 0.5);
  }

  .divider-visual {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 12px;
    display: grid;
    place-items: center;
    z-index: 2;
  }

  .divider-rail {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 50%;
    width: 1px;
    transform: translateX(-50%);
    background: rgba(0, 0, 0, 0.07);
  }

  .divider-grip {
    position: relative;
    width: 4px;
    min-height: 72px;
    height: min(16vh, 96px);
    border-radius: 999px;
    background: rgba(23,59,104,0.28);
    transition: width 0.14s ease, background 0.14s ease, opacity 0.14s ease;
  }

  /* Match the detail page background so the gap blends in — only the hairline
     and grip mark the divider. */
  .divider:not(.expanded) {
    background: #f4f5f7;
  }

  .divider.expanded {
    background: transparent;
  }

  .divider:hover .divider-grip,
  .divider:focus-visible .divider-grip,
  .divider.dragging .divider-grip {
    width: 6px;
    background: rgba(23,59,104,0.5);
  }

  .divider:focus-visible {
    outline: none;
  }

  :global(html[data-theme="dark"][data-aux-surface="split-divider"]),
  :global(body[data-theme="dark"][data-aux-surface="split-divider"]),
  :global(body[data-theme="dark"][data-aux-surface="split-divider"] #app) {
    background: transparent;
  }

  :global([data-theme="dark"]) .divider {
    background: transparent;
  }

  :global([data-theme="dark"]) .divider:not(.expanded) {
    background: #15161a;
  }

  :global([data-theme="dark"]) .drag-overlay {
    background: rgba(21, 22, 26, 0.95);
  }

  :global([data-theme="dark"]) .drag-ratio {
    color: rgba(191, 219, 254, 0.55);
  }

  :global([data-theme="dark"]) .divider-rail {
    background: rgba(255, 255, 255, 0.08);
  }

  :global([data-theme="dark"]) .divider-grip {
    background: rgba(191, 219, 254, 0.34);
  }

  :global([data-theme="dark"]) .divider:hover .divider-grip,
  :global([data-theme="dark"]) .divider:focus-visible .divider-grip,
  :global([data-theme="dark"]) .divider.dragging .divider-grip {
    background: rgba(191, 219, 254, 0.62);
  }
</style>
