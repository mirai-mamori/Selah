const PREFIX = "selah-luna-draft:v1:";

export function lunaDraftKey(parts: Array<string | number | null | undefined>): string {
  return `${PREFIX}${parts
    .filter((part) => part !== null && part !== undefined && String(part) !== "")
    .map((part) => encodeURIComponent(String(part)))
    .join("|")}`;
}

export function readDraft(key: string): string {
  if (!key) return "";
  try {
    return localStorage.getItem(key) || "";
  } catch {
    return "";
  }
}

export function writeDraft(key: string, value: string): void {
  if (!key) return;
  try {
    if (value) localStorage.setItem(key, value);
    else localStorage.removeItem(key);
  } catch {}
}
