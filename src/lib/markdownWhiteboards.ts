import type { LiveWhiteboard, LiveWhiteboardEdge, LiveWhiteboardNode } from "./api";

export interface MarkdownWhiteboardBlock {
  source?: string;
  board?: LiveWhiteboard;
}

interface BoardEvent {
  start: number;
  end: number;
  board: LiveWhiteboard;
}

function parseStructuredBoard(raw: string): LiveWhiteboard | null {
  try {
    const board = JSON.parse(raw) as LiveWhiteboard;
    return board && Array.isArray(board.nodes) && board.nodes.length ? board : null;
  } catch {
    return null;
  }
}

function parseLegacyNode(line: string, index: number): LiveWhiteboardNode | null {
  const match = line.match(/^\s*-\s+\*\*(.+?)\*\*(?::\s*|\s*)(.*)$/);
  if (!match) return null;
  const label = match[1].trim();
  if (!label) return null;

  let detail = match[2].trim();
  let sourceType = "lecture";
  let sourceExcerpt = "";
  let externalSource = "";
  const external = detail.match(/（外部補足(?:[:：]\s*([^）]+))?）/);
  if (external) {
    sourceType = "external";
    externalSource = external[1]?.trim() || "";
    detail = detail.replace(external[0], "").trim();
  } else {
    const lecture = detail.match(/（(?:講義内根拠|録音内根拠)[:：]\s*([^）]+)）/);
    if (lecture) {
      sourceExcerpt = lecture[1].trim();
      detail = detail.replace(lecture[0], "").trim();
    }
  }

  return {
    id: `legacy-${index + 1}`,
    label,
    detail,
    node_type: "structure",
    kind: "support",
    role: index === 0 ? "main" : "branch",
    source_type: sourceType,
    source_excerpt: sourceExcerpt,
    external_source: externalSource,
  };
}

function parseLegacyEdge(line: string, nodes: LiveWhiteboardNode[]): LiveWhiteboardEdge | null {
  const text = line.replace(/^\s*-\s+/, "").trim();
  let match = text.match(/^(.+?)\s+--(.+?)-->\s+(.+)$/);
  if (!match) match = text.match(/^(.+?)\s+(?:→|-->)\s+(.+)$/);
  if (!match) return null;

  const fromLabel = match[1].trim();
  const hasLabel = match.length === 4;
  const edgeLabel = hasLabel ? match[2].trim() : "";
  const toLabel = match[hasLabel ? 3 : 2].trim();
  const from = nodes.find((node) => node.label === fromLabel);
  const to = nodes.find((node) => node.label === toLabel);
  return from && to ? { from: from.id, to: to.id, label: edgeLabel } : null;
}

function parseLegacyBoard(title: string, section: string): LiveWhiteboard | null {
  const nodes: LiveWhiteboardNode[] = [];
  const relationLines: string[] = [];
  let relations = false;

  for (const line of section.split(/\r?\n/)) {
    if (/^\s*関係[:：]\s*$/.test(line)) {
      relations = true;
      continue;
    }
    if (relations) {
      if (/^\s*-\s+/.test(line)) relationLines.push(line);
      continue;
    }
    const node = parseLegacyNode(line, nodes.length);
    if (node) nodes.push(node);
  }

  if (nodes.length < 2) return null;
  const edges = relationLines
    .map((line) => parseLegacyEdge(line, nodes))
    .filter((edge): edge is LiveWhiteboardEdge => edge !== null);
  return {
    title: title.trim() || "知識整理ボード",
    layout: "grid",
    nodes,
    edges,
  };
}

export function splitMarkdownWhiteboards(source: string): MarkdownWhiteboardBlock[] {
  const events: BoardEvent[] = [];
  const structuredRanges: Array<{ start: number; end: number }> = [];
  const fence = /```live-whiteboard[^\S\r\n]*(?:\r?\n)([\s\S]*?)```/g;
  let match: RegExpExecArray | null;

  while ((match = fence.exec(source))) {
    const board = parseStructuredBoard(match[1]);
    if (!board) continue;
    events.push({ start: match.index, end: match.index + match[0].length, board });
    structuredRanges.push({ start: match.index, end: match.index + match[0].length });
  }

  const heading = /^###\s+知[识識]整理ボード(?:[:：]\s*(.*))?\s*$/gm;
  while ((match = heading.exec(source))) {
    const headingEnd = match.index + match[0].length;
    const nextHeading = source.slice(headingEnd).search(/^#{1,6}\s+/m);
    const sectionEnd = nextHeading < 0 ? source.length : headingEnd + nextHeading;
    const hasStructuredBoard = structuredRanges.some((range) => range.start >= headingEnd && range.start < sectionEnd);
    if (hasStructuredBoard) continue;
    const board = parseLegacyBoard(match[1] || "", source.slice(headingEnd, sectionEnd));
    if (board) events.push({ start: headingEnd, end: headingEnd, board });
  }

  events.sort((a, b) => a.start - b.start || b.end - a.end);
  const blocks: MarkdownWhiteboardBlock[] = [];
  let cursor = 0;
  for (const event of events) {
    if (event.start < cursor) continue;
    if (event.start > cursor) blocks.push({ source: source.slice(cursor, event.start) });
    blocks.push({ board: event.board });
    cursor = event.end;
  }
  if (cursor < source.length || !blocks.length) blocks.push({ source: source.slice(cursor) });
  return blocks;
}
