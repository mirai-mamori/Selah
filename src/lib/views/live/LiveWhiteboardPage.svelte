<script lang="ts">
  import type { WhiteboardLayoutResult, WhiteboardLayoutTopic } from "../../whiteboardLayout";
  import type { BoardHighlight, TermFloatLabels, WhiteboardStagePreset } from "./liveTypes";

  type WhiteboardDragStart = { x: number; y: number; panX: number; panY: number } | null;

  interface Props {
    activeWhiteboardLayout: WhiteboardLayoutResult;
    activeWhiteboardStage: WhiteboardStagePreset;
    termFloatLabels: TermFloatLabels;
    whiteboardZoom: number;
    whiteboardPanX: number;
    whiteboardPanY: number;
    whiteboardDragStart: WhiteboardDragStart;
    selectedBoardNodeId: string | null;
    boardHighlight: BoardHighlight;
    topics: WhiteboardLayoutTopic[];
    selectedTopicIds: string[];
    onToggleTopic: (id: string) => void;
    onToggleAllTopics: () => void;
    boardCanvasWidth: number;
    boardCanvasHeight: number;
    bindWhiteboardOverlayDismiss: (node: HTMLElement) => { destroy?: () => void } | void;
    onClose: () => void;
    onZoomOut: () => void;
    onResetZoom: () => void;
    onZoomIn: () => void;
    onWheel: (event: WheelEvent) => void;
    onPointerDown: (event: PointerEvent) => void;
    onPointerMove: (event: PointerEvent) => void;
    onPointerUp: (event: PointerEvent) => void;
    onClearSelection: () => void;
    onToggleNodeSelection: (id: string, event: MouseEvent | KeyboardEvent) => void;
  }

  let {
    activeWhiteboardLayout,
    activeWhiteboardStage,
    termFloatLabels,
    whiteboardZoom,
    whiteboardPanX,
    whiteboardPanY,
    whiteboardDragStart,
    selectedBoardNodeId,
    boardHighlight,
    topics,
    selectedTopicIds,
    onToggleTopic,
    onToggleAllTopics,
    boardCanvasWidth = $bindable(),
    boardCanvasHeight = $bindable(),
    bindWhiteboardOverlayDismiss,
    onClose,
    onZoomOut,
    onResetZoom,
    onZoomIn,
    onWheel,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onClearSelection,
    onToggleNodeSelection,
  }: Props = $props();

  // Left topic sidebar: collapsible, with a one-click select-all toggle.
  let topicSidebarCollapsed = $state(false);
  const allTopicsSelected = $derived(
    topics.length > 0 && selectedTopicIds.length >= topics.length,
  );

  // Edges are drawn in stage-pixel space so the SVG viewBox matches the stage
  // aspect — a 0..100 viewBox stretched to a non-square stage distorts curves.
  const boardViewBox = $derived(
    activeWhiteboardLayout.stage
      ? `0 0 ${activeWhiteboardLayout.stage.width} ${activeWhiteboardLayout.stage.height}`
      : "0 0 100 100",
  );

  function handleNodeKeydown(id: string, event: KeyboardEvent) {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    onToggleNodeSelection(id, event);
  }
</script>

<section
  class="board-page"
  use:bindWhiteboardOverlayDismiss
  aria-label={termFloatLabels.boardTitle}
>
  <button
    type="button"
    class="board-page-back"
    onclick={onClose}
    aria-label={termFloatLabels.collapse}
    title={termFloatLabels.collapse}
  >
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
  </button>
  <div class="board-zoom-controls" aria-label={termFloatLabels.boardTitle}>
    <button type="button" onclick={onZoomOut} title="Zoom out" aria-label="Zoom out">−</button>
    <button type="button" onclick={onResetZoom} title="Reset zoom" aria-label="Reset zoom">{Math.round(whiteboardZoom * 100)}%</button>
    <button type="button" onclick={onZoomIn} title="Zoom in" aria-label="Zoom in">＋</button>
  </div>
  {#if topics.length > 1}
    {#if topicSidebarCollapsed}
      <button
        type="button"
        class="board-topic-reopen"
        onclick={() => (topicSidebarCollapsed = false)}
        aria-label={termFloatLabels.expand}
        title={termFloatLabels.expand}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><line x1="4" y1="7" x2="20" y2="7"/><line x1="4" y1="12" x2="20" y2="12"/><line x1="4" y1="17" x2="20" y2="17"/></svg>
      </button>
    {:else}
      <aside class="board-topic-sidebar" aria-label={termFloatLabels.boardTitle}>
        <div class="board-topic-head">
          <button
            type="button"
            class="board-topic-fold"
            onclick={() => (topicSidebarCollapsed = true)}
            title={termFloatLabels.collapse}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="11 7 6 12 11 17"/><polyline points="18 7 13 12 18 17"/></svg>
            <span>{termFloatLabels.collapse}</span>
          </button>
          <button
            type="button"
            class="board-topic-all"
            onclick={onToggleAllTopics}
            title={allTopicsSelected ? termFloatLabels.deselectAll : termFloatLabels.selectAll}
          >
            {#if allTopicsSelected}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="3.5" width="17" height="17" rx="4"/><path d="M8.2 12h7.6"/></svg>
            {:else}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="3.5" width="17" height="17" rx="4"/><path d="M8 12.4l2.9 2.9L16.5 9"/></svg>
            {/if}
            <span>{allTopicsSelected ? termFloatLabels.deselectAll : termFloatLabels.selectAll}</span>
          </button>
        </div>
        <div class="board-topic-list">
          {#each topics as topic (topic.id)}
            <button
              type="button"
              class="board-topic-chip"
              class:is-active={selectedTopicIds.includes(topic.id)}
              aria-pressed={selectedTopicIds.includes(topic.id)}
              onclick={() => onToggleTopic(topic.id)}
            >{topic.label}</button>
          {/each}
        </div>
      </aside>
    {/if}
  {/if}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="visual-board-canvas"
    class:dragging={!!whiteboardDragStart}
    class:has-selection={selectedBoardNodeId !== null}
    role="application"
    aria-label={termFloatLabels.boardTitle}
    bind:clientWidth={boardCanvasWidth}
    bind:clientHeight={boardCanvasHeight}
    onwheel={onWheel}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    onclick={onClearSelection}
  >
    <div
      class="visual-board-stage"
      style="width: {activeWhiteboardStage.width}px; height: {activeWhiteboardStage.height}px; transform: translate(-50%, -50%) translate({whiteboardPanX}px, {whiteboardPanY}px) scale({whiteboardZoom});"
    >
      <svg class="visual-board-links" viewBox={boardViewBox} preserveAspectRatio="none" aria-hidden="true">
        {#each activeWhiteboardLayout.edges as edge (edge.id)}
          <path
            class="visual-board-edge edge-kind-{edge.colorKind} edge-source-{edge.colorSourceType}"
            class:trunk={edge.trunk}
            class:redundant={edge.redundant}
            class:is-highlighted={boardHighlight?.edges.has(edge.id)}
            d="M {edge.x1} {edge.y1} Q {edge.cx} {edge.cy} {edge.x2} {edge.y2}"
          />
        {/each}
      </svg>
      {#each activeWhiteboardLayout.edges as edge (edge.id + "-label")}
        {#if edge.label}
          <span
            class="visual-board-edge-label edge-kind-{edge.colorKind} edge-source-{edge.colorSourceType}"
            class:is-highlighted={boardHighlight?.edges.has(edge.id)}
            style="left: {edge.lx}%; top: {edge.ly}%;"
          >{edge.label}</span>
        {/if}
      {/each}
      {#each activeWhiteboardLayout.nodes as node (node.id)}
        <div
          class="visual-board-node kind-{node.kind} source-{node.sourceType}"
          class:role-main={node.role === "main"}
          class:role-branch={node.role !== "main"}
          class:is-highlighted={boardHighlight?.nodes.has(node.id)}
          class:is-selected={selectedBoardNodeId === node.id}
          style="left: {node.x}%; top: {node.y}%;"
          title={node.sourceType === "external" ? `${termFloatLabels.externalSource}: ${node.sourceLabel}` : ""}
          onclick={(e) => onToggleNodeSelection(node.id, e)}
          onkeydown={(e) => handleNodeKeydown(node.id, e)}
          role="button"
          tabindex="0"
        >
          {#if node.sourceType === "external"}
            <span class="visual-board-source-badge">{termFloatLabels.externalNode}</span>
          {/if}
          <span class="visual-board-node-label">{node.label}</span>
          {#if node.detail}
            <span class="visual-board-node-detail">{node.detail}</span>
          {/if}
          {#if node.chips?.length}
            <span class="visual-board-node-chips">
              {#each node.chips as chip, ci (ci)}
                <span class="visual-board-chip" title={chip.detail}>{chip.label}</span>
              {/each}
            </span>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</section>

<style>
  .board-page {
    position: absolute;
    inset: -24px;
    z-index: 60;
    padding: 0;
    background: var(--bg-primary);
    display: flex;
    flex-direction: column;
    animation: board-page-in 0.26s cubic-bezier(0.22, 1, 0.36, 1) both;
  }
  .board-page .visual-board-canvas {
    flex: 1 1 auto;
    height: auto;
    min-height: 0;
    border-radius: 0;
  }
  .board-page-back {
    position: absolute;
    top: 20px;
    left: 20px;
    z-index: 5;
    width: 36px;
    height: 36px;
    padding: 0;
    border: 0.5px solid var(--glass-border);
    border-radius: 12px;
    background: var(--glass-bg, rgba(255, 255, 255, 0.5));
    backdrop-filter: var(--glass-blur) var(--glass-saturate);
    -webkit-backdrop-filter: var(--glass-blur) var(--glass-saturate);
    color: var(--text-secondary);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-shadow: var(--shadow-glass), 0 4px 16px rgba(0, 0, 0, 0.06);
    transition: background 0.15s, color 0.15s, transform 0.15s;
  }
  .board-page-back:hover {
    background: color-mix(in srgb, var(--text-primary) 8%, var(--glass-bg, rgba(255, 255, 255, 0.5)));
    color: var(--text-primary);
  }
  .board-page-back:active {
    transform: scale(0.94);
  }
  .board-zoom-controls {
    position: absolute;
    top: 20px;
    right: 20px;
    z-index: 6;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    border-radius: 14px;
    border: 0.5px solid var(--glass-border);
    background: var(--glass-bg, rgba(255, 255, 255, 0.5));
    backdrop-filter: var(--glass-blur) var(--glass-saturate);
    -webkit-backdrop-filter: var(--glass-blur) var(--glass-saturate);
    box-shadow: var(--shadow-glass), 0 4px 16px rgba(0, 0, 0, 0.06);
  }
  .board-zoom-controls button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 34px;
    height: 30px;
    padding: 0 10px;
    border: none;
    border-radius: 10px;
    background: transparent;
    color: var(--text-tertiary);
    font: inherit;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .board-zoom-controls button:hover {
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
    color: var(--text-primary);
  }

  /* Topic switcher — a compact glass panel floating over the canvas, the
     same family as the back button and zoom pill. Sized to its content. */
  .board-topic-sidebar,
  .board-topic-reopen {
    position: absolute;
    left: 20px;
    top: 66px;
    z-index: 5;
    box-sizing: border-box;
    border: 0.5px solid var(--glass-border);
    background: var(--glass-bg, rgba(255, 255, 255, 0.5));
    backdrop-filter: var(--glass-blur) var(--glass-saturate);
    -webkit-backdrop-filter: var(--glass-blur) var(--glass-saturate);
    box-shadow: var(--shadow-glass), 0 4px 16px rgba(0, 0, 0, 0.06);
  }
  .board-topic-sidebar {
    width: 220px;
    max-height: calc(100% - 88px);
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    overflow-y: auto;
    border-radius: 14px;
    scrollbar-width: thin;
  }
  .board-topic-reopen {
    width: 36px;
    height: 36px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border-radius: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .board-topic-reopen:hover {
    background: color-mix(in srgb, var(--text-primary) 8%, var(--glass-bg, rgba(255, 255, 255, 0.5)));
    color: var(--text-primary);
  }
  .board-topic-head {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  /* Header: two equal-size buttons — fold (left) and select-all (right),
     each an icon + label, each taking half the width. */
  .board-topic-all,
  .board-topic-fold {
    flex: 1 1 0;
    min-width: 0;
    height: 30px;
    box-sizing: border-box;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    padding: 0 8px;
    border: 0.5px solid transparent;
    border-radius: 8px;
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 10.5px;
    font-weight: 700;
    white-space: nowrap;
    cursor: pointer;
    transition: background 0.14s, color 0.14s;
  }
  .board-topic-all:hover,
  .board-topic-fold:hover {
    background: color-mix(in srgb, var(--text-primary) 9%, transparent);
    color: var(--text-primary);
  }
  .board-topic-all svg,
  .board-topic-fold svg {
    flex: 0 0 auto;
  }
  .board-topic-all span,
  .board-topic-fold span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .board-topic-list {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  /* Topic chips — each carries a subtle surface; the selected one uses accent. */
  .board-topic-chip {
    box-sizing: border-box;
    width: 100%;
    padding: 7px 10px;
    border: 0.5px solid transparent;
    border-radius: 8px;
    background: color-mix(in srgb, var(--text-primary) 5%, transparent);
    color: var(--text-secondary);
    font: inherit;
    font-size: 11.5px;
    font-weight: 700;
    line-height: 1.3;
    text-align: left;
    cursor: pointer;
    transition: background 0.14s, color 0.14s, border-color 0.14s;
  }
  .board-topic-chip:hover {
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
    color: var(--text-primary);
  }
  .board-topic-chip.is-active {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
    background: color-mix(in srgb, var(--accent) 13%, transparent);
    color: var(--text-primary);
    font-weight: 800;
  }
  .board-topic-chip.is-active:hover {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--text-primary);
  }

  @keyframes board-page-in {
    from { transform: translateY(10px); }
    to { transform: translateY(0); }
  }

  .visual-board-canvas {
    position: relative;
    height: 380px;
    border-radius: 8px;
    overflow: hidden;
    background:
      linear-gradient(color-mix(in srgb, var(--text-tertiary) 9%, transparent) 1px, transparent 1px),
      linear-gradient(90deg, color-mix(in srgb, var(--text-tertiary) 9%, transparent) 1px, transparent 1px),
      color-mix(in srgb, var(--bg-secondary) 72%, transparent);
    background-size: 22px 22px;
    cursor: grab;
    touch-action: none;
  }
  .visual-board-canvas.dragging {
    cursor: grabbing;
  }
  .visual-board-stage {
    position: absolute;
    left: 50%;
    top: 50%;
    transform-origin: 50% 50%;
  }
  .visual-board-links {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    color: color-mix(in srgb, var(--blue) 52%, var(--text-tertiary));
    overflow: visible;
  }
  /* Relations are de-emphasised by default (structure first, relations on
     demand); hierarchy/trunk edges stay visible; selection makes the
     relevant edges pop. Stroke widths are pixels (the viewBox is the stage). */
  .visual-board-links path {
    fill: none;
    stroke: var(--edge-color, currentColor);
    stroke-width: 1.5;
    stroke-linecap: round;
    opacity: 0.32;
  }
  .visual-board-links path.trunk {
    stroke-width: 1.8;
    opacity: 0.7;
  }
  .visual-board-links path.edge-kind-core {
    --edge-color: color-mix(in srgb, var(--text-tertiary) 76%, var(--text-secondary));
  }
  .visual-board-links path.edge-kind-result {
    --edge-color: color-mix(in srgb, #34c759 62%, var(--text-tertiary));
  }
  .visual-board-links path.edge-kind-question {
    --edge-color: color-mix(in srgb, var(--orange, #e67700) 64%, var(--text-tertiary));
  }
  .visual-board-links path.edge-kind-support {
    --edge-color: color-mix(in srgb, var(--text-tertiary) 76%, var(--text-secondary));
  }
  .visual-board-links path.edge-source-external {
    stroke-dasharray: 5 4;
  }
  .visual-board-links path.redundant {
    stroke-dasharray: 4 4;
    opacity: 0.2;
  }
  .visual-board-canvas.has-selection .visual-board-links path,
  .visual-board-canvas.has-selection .visual-board-edge-label,
  .visual-board-canvas.has-selection .visual-board-node {
    transition: opacity 0.16s ease, box-shadow 0.16s ease, border-color 0.16s ease;
  }
  .visual-board-canvas.has-selection .visual-board-links path {
    opacity: 0.08;
  }
  .visual-board-canvas.has-selection .visual-board-links path.is-highlighted {
    opacity: 0.96;
    stroke-width: 2.6;
  }
  .visual-board-canvas.has-selection .visual-board-edge-label {
    opacity: 0.14;
  }
  .visual-board-canvas.has-selection .visual-board-edge-label.is-highlighted {
    opacity: 1;
  }
  .visual-board-canvas.has-selection .visual-board-node {
    opacity: 0.24;
  }
  .visual-board-canvas.has-selection .visual-board-node.is-highlighted {
    opacity: 1;
  }
  .visual-board-canvas.has-selection .visual-board-node.is-selected {
    opacity: 1;
    box-shadow:
      0 0 0 2px color-mix(in srgb, var(--blue) 65%, transparent),
      0 6px 16px rgba(33, 116, 223, 0.22);
  }
  .visual-board-node {
    cursor: pointer;
  }
  .visual-board-edge-label {
    position: absolute;
    transform: translate(-50%, -50%);
    transform-origin: 50% 50%;
    z-index: 2;
    max-width: 132px;
    padding: 2px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg-primary) 96%, transparent);
    border: 0.5px solid var(--edge-label-border, color-mix(in srgb, var(--blue) 24%, transparent));
    color: var(--edge-label-color, color-mix(in srgb, var(--blue) 78%, var(--text-secondary)));
    font-size: 10px;
    font-weight: 800;
    line-height: 1.15;
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .visual-board-edge-label.edge-kind-core {
    --edge-label-border: color-mix(in srgb, var(--text-tertiary) 30%, transparent);
    --edge-label-color: color-mix(in srgb, var(--text-tertiary) 82%, var(--text-secondary));
  }
  .visual-board-edge-label.edge-kind-result {
    --edge-label-border: color-mix(in srgb, #34c759 30%, transparent);
    --edge-label-color: color-mix(in srgb, #34c759 76%, var(--text-secondary));
  }
  .visual-board-edge-label.edge-kind-question {
    --edge-label-border: color-mix(in srgb, var(--orange, #e67700) 32%, transparent);
    --edge-label-color: color-mix(in srgb, var(--orange, #e67700) 76%, var(--text-secondary));
  }
  .visual-board-edge-label.edge-kind-support {
    --edge-label-border: color-mix(in srgb, var(--text-tertiary) 28%, transparent);
    --edge-label-color: color-mix(in srgb, var(--text-tertiary) 82%, var(--text-secondary));
  }
  .visual-board-edge-label.edge-source-external {
    border-style: dashed;
  }
  .visual-board-node {
    position: absolute;
    transform: translate(-50%, -50%);
    z-index: 3;
    width: 122px;
    min-height: 66px;
    padding: 8px 9px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    border-radius: 8px;
    border: 0.5px solid color-mix(in srgb, var(--blue) 22%, var(--glass-border));
    background: color-mix(in srgb, var(--bg-primary) 96%, transparent);
    box-shadow: 0 3px 8px rgba(0, 0, 0, 0.08);
    text-align: center;
  }
  .visual-board-node.role-main {
    width: 142px;
    min-height: 74px;
    border-width: 1px;
    box-shadow: 0 5px 14px rgba(33, 116, 223, 0.14);
  }
  .visual-board-node.role-branch {
    width: 114px;
    min-height: 62px;
    opacity: 0.94;
  }
  .visual-board-node.kind-core {
    background: color-mix(in srgb, var(--blue) 14%, var(--bg-primary));
    border-color: color-mix(in srgb, var(--blue) 38%, var(--glass-border));
  }
  .visual-board-node.kind-result {
    background: color-mix(in srgb, #34c759 13%, var(--bg-primary));
    border-color: color-mix(in srgb, #34c759 34%, var(--glass-border));
  }
  .visual-board-node.kind-question {
    background: color-mix(in srgb, var(--orange, #e67700) 13%, var(--bg-primary));
    border-color: color-mix(in srgb, var(--orange, #e67700) 32%, var(--glass-border));
  }
  .visual-board-node.source-external {
    border-style: dashed;
    border-color: color-mix(in srgb, var(--accent) 38%, var(--glass-border));
    background:
      linear-gradient(135deg, color-mix(in srgb, var(--accent) 9%, transparent), transparent 52%),
      color-mix(in srgb, var(--bg-primary) 96%, transparent);
  }
  .visual-board-source-badge {
    position: absolute;
    top: -7px;
    right: -7px;
    max-width: 44px;
    padding: 1px 5px;
    border-radius: 999px;
    border: 0.5px solid color-mix(in srgb, var(--accent) 34%, var(--glass-border));
    background: color-mix(in srgb, var(--bg-primary) 96%, transparent);
    color: color-mix(in srgb, var(--accent) 82%, var(--text-secondary));
    font-size: 8.5px;
    font-weight: 800;
    line-height: 1.35;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    pointer-events: none;
  }
  .visual-board-node-label {
    max-width: 100%;
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 800;
    line-height: 1.25;
    overflow-wrap: anywhere;
  }
  .visual-board-node-chips {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 3px;
    margin-top: 4px;
    max-width: 100%;
  }
  .visual-board-chip {
    max-width: 100%;
    padding: 1px 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text-tertiary) 15%, transparent);
    color: var(--text-secondary);
    font-size: 8.5px;
    font-weight: 700;
    line-height: 1.55;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .visual-board-node-detail {
    max-width: 100%;
    color: var(--text-secondary);
    font-size: 10px;
    font-weight: 600;
    line-height: 1.25;
    display: -webkit-box;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }

  @media (max-width: 700px) {
    .board-page {
      padding: 52px 12px 12px;
    }
    .board-page-back {
      top: 10px;
      left: 10px;
    }
    .board-zoom-controls {
      top: 10px;
      right: 10px;
    }
    .visual-board-node {
      width: 108px;
      min-height: 58px;
    }
  }
</style>
