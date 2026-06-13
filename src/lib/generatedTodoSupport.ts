export const LIVE_GENERATED_TODO_KEY = "live_generated_todo";
export const DETAIL_GENERATED_TODO_KEY = "detail_generated_todo";

type MessageWithId = {
  id?: unknown;
};

function commonAsciiSuffixLength(left: string, right: string): number {
  let count = 0;
  let i = left.length - 1;
  let j = right.length - 1;
  while (i >= 0 && j >= 0 && left.charCodeAt(i) === right.charCodeAt(j)) {
    count += 1;
    i -= 1;
    j -= 1;
  }
  return count;
}

function hasMatchingIdPrefix(partial: string, full: string): boolean {
  const prefixLength = Math.min(partial.length, 24);
  return prefixLength >= 8 && full.startsWith(partial.slice(0, prefixLength));
}

export function repairMailSourceUrl(sourceUrl: unknown, messages: readonly MessageWithId[]): string {
  const trimmed = String(sourceUrl || "").trim();
  if (!trimmed.startsWith("mail://")) return trimmed;
  const id = trimmed.slice("mail://".length).trim();
  if (!id || messages.some((message) => String(message.id || "") === id)) return trimmed;

  let bestId = "";
  let bestScore = 0;
  let tied = false;
  for (const message of messages) {
    const candidate = String(message.id || "");
    if (!hasMatchingIdPrefix(id, candidate)) continue;
    const score = commonAsciiSuffixLength(id, candidate);
    if (score < 12) continue;
    if (score === bestScore) {
      tied = true;
    } else if (score > bestScore) {
      bestId = candidate;
      bestScore = score;
      tied = false;
    }
  }
  return bestId && !tied ? `mail://${bestId}` : trimmed;
}
