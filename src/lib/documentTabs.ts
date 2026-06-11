// Shared helpers for the Copilot document-tabs surfaces (dock, tab strip,
// new-tab page). Keeps the favicon logic and tab shape in one place instead of
// re-deriving them per component.

export interface DocumentTabSummary {
  id: string;
  title: string;
  url: string;
  type: string;
  active: boolean;
  loading?: boolean;
}

/** Google S2 favicon URL for a page URL, or "" when the host can't be parsed. */
export function tabFaviconUrl(url: string): string {
  try {
    const host = new URL(url).hostname;
    return host ? `https://www.google.com/s2/favicons?sz=64&domain=${encodeURIComponent(host)}` : "";
  } catch {
    return "";
  }
}
