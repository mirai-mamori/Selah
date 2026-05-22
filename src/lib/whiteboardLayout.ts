// Thin typed wrapper around the global WhiteboardLayout module
// (static/whiteboard-layout.js). The module is loaded by a <script> tag in
// index.html before the Svelte bundle, so window.WhiteboardLayout is
// guaranteed to exist by the time anything imports this file.

import type { LiveWhiteboard } from "./api";

export type WhiteboardLayoutChip = {
  label: string;
  detail: string;
  sourceType: string;
  sourceLabel: string;
};

export type WhiteboardLayoutNode = {
  id: string;
  label: string;
  detail: string;
  nodeType: string;
  kind: string;
  role: string;
  parentId: string;
  sourceType: string;
  sourceLabel: string;
  x: number;
  y: number;
  // Term annotations folded into this node, rendered as in-card chips.
  chips?: WhiteboardLayoutChip[];
};

export type WhiteboardLayoutEdge = {
  id: string;
  from: string;
  to: string;
  label: string;
  colorKind: string;
  colorSourceType: string;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  cx: number;
  cy: number;
  lx: number;
  ly: number;
  trunk: boolean;
  redundant: boolean;
};

export type WhiteboardLayoutResult = {
  title: string;
  nodes: WhiteboardLayoutNode[];
  edges: WhiteboardLayoutEdge[];
  // Pixel canvas the layout was computed against; the renderer sizes the
  // stage to match so the 0..100 coordinates scale without distortion.
  stage?: { width: number; height: number } | null;
};

export type WhiteboardLayoutTopic = {
  id: string;
  label: string;
};

export type WhiteboardLayoutOptions = {
  fallbackBoardTitle?: string;
  externalNodeLabel?: string;
  // When set, only nodes belonging to these main topics are laid out.
  topicIds?: string[];
};

declare global {
  interface Window {
    WhiteboardLayout?: {
      compute(
        board: unknown,
        options?: WhiteboardLayoutOptions,
      ): WhiteboardLayoutResult | null;
      topics(board: unknown): WhiteboardLayoutTopic[];
    };
  }
}

// Cache layouts by whiteboard object identity. The upstream `snapshot` object
// is replaced on every transcript chunk in Live mode, but the per-summary
// `whiteboard` reference is stable as long as the summary itself hasn't
// changed — so we can skip the (expensive) relaxation entirely when nothing
// material is different. WeakMap means cached entries are GC'd as soon as
// the underlying summary is dropped from the snapshot, no manual bookkeeping.
const layoutCache = new WeakMap<object, { optsKey: string; result: WhiteboardLayoutResult | null }>();

function makeOptionsKey(options?: WhiteboardLayoutOptions): string {
  if (!options) return "";
  // Manual concat — faster than JSON.stringify on a hot path that runs per
  // reactive read.
  return (
    (options.fallbackBoardTitle ?? "") + "|" +
    (options.externalNodeLabel ?? "") + "|" +
    ((options.topicIds ?? []).join(","))
  );
}

export function whiteboardTopics(
  board: LiveWhiteboard | null,
): WhiteboardLayoutTopic[] {
  if (!board) return [];
  const impl = typeof window !== "undefined" ? window.WhiteboardLayout : undefined;
  if (!impl || typeof impl.topics !== "function") return [];
  return impl.topics(board);
}

export function computeWhiteboardLayout(
  board: LiveWhiteboard | null,
  options?: WhiteboardLayoutOptions,
): WhiteboardLayoutResult | null {
  if (!board) return null;
  const impl = typeof window !== "undefined" ? window.WhiteboardLayout : undefined;
  if (!impl) return null;
  const optsKey = makeOptionsKey(options);
  const boardKey = board as unknown as object;
  const cached = layoutCache.get(boardKey);
  if (cached && cached.optsKey === optsKey) return cached.result;
  const result = impl.compute(board, options);
  layoutCache.set(boardKey, { optsKey, result });
  return result;
}
