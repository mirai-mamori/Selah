import { emit } from "@tauri-apps/api/event";

// A bookmark stores the backend reopen spec verbatim so any surface (the new-tab
// page especially) can ask the backend to recreate the tab. All document-tab
// surfaces share the same webview origin, so localStorage is shared between them.
export interface Bookmark {
  id: string;
  title: string;
  spec: Record<string, unknown>;
  addedAt: number;
}

const KEY = "selah-bookmarks";

export function listBookmarks(): Bookmark[] {
  try {
    const raw = localStorage.getItem(KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function persist(list: Bookmark[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    // ignore storage failures
  }
  // Notify other open surfaces (new-tab pages, the tab strip) to re-read.
  void emit("bookmarks-changed").catch(() => {});
}

export function isBookmarked(id: string): boolean {
  return listBookmarks().some((b) => b.id === id);
}

export function addBookmark(bookmark: Bookmark): void {
  const list = listBookmarks();
  if (list.some((b) => b.id === bookmark.id)) return;
  list.unshift(bookmark);
  persist(list);
}

export function removeBookmark(id: string): void {
  persist(listBookmarks().filter((b) => b.id !== id));
}

export function toggleBookmark(bookmark: Bookmark): boolean {
  if (isBookmarked(bookmark.id)) {
    removeBookmark(bookmark.id);
    return false;
  }
  addBookmark(bookmark);
  return true;
}
