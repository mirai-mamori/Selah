<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { applyAuxiliaryTheme, syncAuxiliaryTheme } from "./auxiliarySurfaceTheme";
  // Copilot window brand — copilot.png lives in static/ (served from web root).
  const copilotLogoUrl = "/copilot.png";
  import { listBookmarks, removeBookmark as removeStoredBookmark, type Bookmark } from "./bookmarks";
  import { tabFaviconUrl } from "./documentTabs";

  interface QuickNav {
    label: string;
    hint: string;
    url?: string;
    action?: "files";
  }

  interface FileRecord {
    id: string;
    filename: string;
    path: string;
    course_name: string;
    file_exists: boolean;
  }

  function readParam(name: string): string {
    try {
      const search = new URLSearchParams(window.location.search);
      const fromSearch = search.get(name);
      if (fromSearch) return fromSearch;
      const h = (location.hash || "").replace(/^#/, "");
      return new URLSearchParams(h).get(name) || "";
    } catch {
      return "";
    }
  }

  // Identifies this tab so it can close itself once it hands off to a real page.
  const tabTarget = readParam("tabLabel");
  const tabOwner = readParam("ownerLabel") || "document-tabs";

  const quickNav: QuickNav[] = [
    { label: "LUNA", url: "https://luna.kwansei.ac.jp", hint: "学習支援システム" },
    { label: "KWIC", url: "https://kwic.kwansei.ac.jp", hint: "学内ポータル" },
    { label: "学生メニュー", url: "https://kg-course.kwansei.ac.jp", hint: "履修・成績" },
    { label: "ファイル管理", action: "files", hint: "保存したファイル" },
  ];

  let query = $state("");
  let busy = $state(false);

  async function openDetective(): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await invoke("open_detective_tab");
      if (tabTarget) await invoke("document_tabs_close", { owner: tabOwner, id: tabTarget }).catch(() => {});
    } catch (e) {
      console.error("[Selah] open detective failed:", e);
      busy = false;
    }
  }

  async function openPaperCheck(): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await invoke("open_paper_check_tab");
      if (tabTarget) await invoke("document_tabs_close", { owner: tabOwner, id: tabTarget }).catch(() => {});
    } catch (e) {
      console.error("[Selah] open paper check failed:", e);
      busy = false;
    }
  }
  let inputEl = $state<HTMLInputElement | null>(null);
  let allFiles = $state<FileRecord[]>([]);

  const fileResults = $derived.by<FileRecord[]>(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return allFiles
      .filter(
        (f) =>
          f.file_exists !== false &&
          ((f.filename || "").toLowerCase().includes(q) || (f.course_name || "").toLowerCase().includes(q)),
      )
      .slice(0, 6);
  });

  async function loadFiles(): Promise<void> {
    try {
      allFiles = await invoke<FileRecord[]>("list_downloads");
    } catch {
      allFiles = [];
    }
  }

  async function openFiles(): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await invoke("open_files_tab", { focusCourse: null });
      if (tabTarget) {
        await invoke("document_tabs_close", { owner: tabOwner, id: tabTarget }).catch(() => {});
      }
    } catch (e) {
      console.error("[Selah] open files failed:", e);
      busy = false;
    }
  }

  function onNavClick(item: QuickNav): void {
    if (item.action === "files") void openFiles();
    else if (item.url) void navigateTo(item.url);
  }

  async function openFileResult(file: FileRecord): Promise<void> {
    await invoke("open_downloaded_file", { path: file.path }).catch((e) =>
      console.error("[Selah] open file failed:", e),
    );
  }

  function normalizeQuery(raw: string): string {
    const q = raw.trim();
    if (!q) return "";
    if (/^https?:\/\//i.test(q)) return q;
    // A bare domain/path with no spaces is treated as a URL; anything else
    // becomes a Google search.
    if (!/\s/.test(q) && /^[^\s.]+\.[^\s.]{2,}.*$/.test(q)) return `https://${q}`;
    return `https://www.google.com/search?q=${encodeURIComponent(q)}`;
  }

  async function navigateTo(url: string): Promise<void> {
    if (busy || !url) return;
    busy = true;
    try {
      // Open the destination as a browser tab, then retire this new-tab page so
      // it behaves like an address-bar navigation rather than stacking tabs.
      await invoke("open_external_url", { url });
      if (tabTarget) {
        await invoke("document_tabs_close", { owner: tabOwner, id: tabTarget }).catch(() => {});
      }
    } catch (e) {
      console.error("[Selah] new-tab navigation failed:", e);
      busy = false;
    }
  }

  function submit(event: Event): void {
    event.preventDefault();
    const url = normalizeQuery(query);
    if (url) void navigateTo(url);
  }

  // ── Bookmarks ──
  let bookmarks = $state<Bookmark[]>([]);

  function loadBookmarks(): void {
    bookmarks = listBookmarks();
  }

  // Favicons come from Google's service (allowed in CSP); failures fall back to
  // no icon. Keyed by URL so a broken icon isn't retried every render.
  let faviconFailed = $state<Set<string>>(new Set());

  function bookmarkFavicon(bm: Bookmark): string {
    const spec = (bm.spec || {}) as Record<string, unknown>;
    if (String(spec.type || "") === "browser") return tabFaviconUrl(String(spec.url || ""));
    return "";
  }

  function onFaviconError(src: string): void {
    if (faviconFailed.has(src)) return;
    faviconFailed = new Set(faviconFailed).add(src);
  }

  function bookmarkHint(bm: Bookmark): string {
    const spec = (bm.spec || {}) as Record<string, unknown>;
    const type = String(spec.type || "");
    if (type === "browser") {
      try {
        return new URL(String(spec.url || "")).hostname.replace(/^www\./, "");
      } catch {
        return "Web";
      }
    }
    if (type === "reader") return "Markdown";
    if (type === "detail") return "詳細";
    return "ページ";
  }

  async function openBookmark(bm: Bookmark): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await invoke("document_tabs_open_bookmark", { spec: bm.spec });
      if (tabTarget) {
        await invoke("document_tabs_close", { owner: tabOwner, id: tabTarget }).catch(() => {});
      }
    } catch (e) {
      console.error("[Selah] open bookmark failed:", e);
      busy = false;
    }
  }

  function removeBookmark(id: string): void {
    removeStoredBookmark(id);
    loadBookmarks();
  }

  let themeUnlisten: (() => void) | null = null;
  let appThemeUnlisten: (() => void) | null = null;
  let bookmarksUnlisten: (() => void) | null = null;

  onMount(async () => {
    document.documentElement.setAttribute("data-aux-surface", "home");
    document.body.setAttribute("data-aux-surface", "home");
    await syncAuxiliaryTheme();
    themeUnlisten = await listen<string>("theme-changed", (event) => applyAuxiliaryTheme(event.payload)).catch(() => null);
    appThemeUnlisten = await listen("app-theme-changed", () => void syncAuxiliaryTheme()).catch(() => null);
    loadBookmarks();
    void loadFiles();
    bookmarksUnlisten = await listen("bookmarks-changed", () => loadBookmarks()).catch(() => null);
    inputEl?.focus();
  });

  onDestroy(() => {
    themeUnlisten?.();
    appThemeUnlisten?.();
    bookmarksUnlisten?.();
    document.documentElement.removeAttribute("data-aux-surface");
    document.body.removeAttribute("data-aux-surface");
  });
</script>

<main class="home-root">
  <div class="home-inner">
    <div class="brand">
      <img class="brand-logo" src={copilotLogoUrl} alt="" aria-hidden="true" />
      <span class="brand-name">Copilot</span>
    </div>

    <form class="search" onsubmit={submit}>
      <span class="search-icon" aria-hidden="true">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" /></svg>
      </span>
      <input
        bind:this={inputEl}
        bind:value={query}
        type="text"
        inputmode="url"
        autocapitalize="off"
        autocomplete="off"
        autocorrect="off"
        spellcheck="false"
        placeholder="検索・URL・ファイル名を入力"
        aria-label="検索・URL・ファイル名を入力"
        disabled={busy}
      />
      <span class="search-hint" aria-hidden="true">Enter で移動</span>
    </form>

    {#if fileResults.length}
      <div class="file-results" aria-label="ファイル検索結果">
        {#each fileResults as file (file.id)}
          <button class="file-result" type="button" title={file.path} disabled={busy} onclick={() => openFileResult(file)}>
            <span class="fr-icon" aria-hidden="true">
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><polyline points="14 2 14 8 20 8" /></svg>
            </span>
            <span class="fr-text">
              <span class="fr-name">{file.filename}</span>
              <span class="fr-meta">{file.course_name || "ファイル"}</span>
            </span>
          </button>
        {/each}
      </div>
    {/if}

    <section class="links" aria-label="クイックアクセス">
      <div class="section-label">クイックアクセス</div>
      {#each quickNav as item (item.label)}
        <button class="link-tile" type="button" title={item.url || item.hint} disabled={busy} onclick={() => onNavClick(item)}>
          <span class="link-label">{item.label}</span>
          <span class="link-hint">{item.hint}</span>
        </button>
      {/each}
    </section>

    <section class="links" aria-label="アプリ">
      <div class="section-label">アプリ</div>
      <button class="link-tile det-tile" type="button" title="なるほど —— 講義の矛盾を暴く" disabled={busy} onclick={openDetective}>
        <img class="det-tile-logo" src="/naruhodo.png" alt="" aria-hidden="true" draggable="false" />
        <span class="det-tile-text">
          <span class="link-label">なるほど</span>
          <span class="link-hint">講義の矛盾を暴く</span>
        </span>
      </button>
      <button class="link-tile" type="button" title="論文チェック —— AI率・重複を検査" disabled={busy} onclick={openPaperCheck}>
        <span class="link-label">論文チェック</span>
        <span class="link-hint">AI率・重複を検査</span>
      </button>
    </section>

    {#if bookmarks.length}
      <section class="bookmarks" aria-label="ブックマーク">
        <div class="section-label">ブックマーク</div>
        <div class="bm-chips">
          {#each bookmarks as bm (bm.id)}
            {@const fav = bookmarkFavicon(bm)}
            <div class="bm-chip">
              <button class="bm-open" type="button" title={`${bm.title} · ${bookmarkHint(bm)}`} disabled={busy} onclick={() => openBookmark(bm)}>
                {#if fav && !faviconFailed.has(fav)}
                  <img class="bm-fav" src={fav} alt="" aria-hidden="true" onerror={() => onFaviconError(fav)} />
                {:else}
                  <span class="bm-fav placeholder" aria-hidden="true"></span>
                {/if}
                <span class="bm-title">{bm.title}</span>
                <span class="bm-host">{bookmarkHint(bm)}</span>
              </button>
              <button class="bm-remove" type="button" title="削除" aria-label="ブックマークを削除" onclick={() => removeBookmark(bm.id)}>
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
              </button>
            </div>
          {/each}
        </div>
      </section>
    {/if}
  </div>
</main>

<style>
  .home-root {
    width: 100vw;
    min-height: 100vh;
    box-sizing: border-box;
    display: grid;
    place-items: start center;
    padding: clamp(56px, 14vh, 140px) 24px 48px;
    color: #1d1d1f;
    background: radial-gradient(120% 90% at 50% -10%, rgba(23,59,104,0.06), transparent 60%), #f7f8fa;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", "Noto Sans JP", sans-serif;
  }

  .home-inner {
    width: 100%;
    max-width: 620px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 28px;
  }

  /* Gentle staggered entrance for the new-tab content. */
  .home-inner > :global(*) {
    animation: home-enter 0.42s cubic-bezier(0.2, 0.8, 0.2, 1) both;
  }
  .home-inner > :global(:nth-child(1)) { animation-delay: 0.02s; }
  .home-inner > :global(:nth-child(2)) { animation-delay: 0.08s; }
  .home-inner > :global(:nth-child(3)) { animation-delay: 0.14s; }
  .home-inner > :global(:nth-child(4)) { animation-delay: 0.2s; }
  .home-inner > :global(:nth-child(5)) { animation-delay: 0.26s; }

  @keyframes home-enter {
    from { opacity: 0; transform: translateY(10px); }
    to { opacity: 1; transform: translateY(0); }
  }

  @media (prefers-reduced-motion: reduce) {
    .home-inner > :global(*) { animation: none; }
  }

  .brand {
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }

  .brand-logo {
    width: 56px;
    height: 56px;
    object-fit: contain;
  }

  .brand-name {
    font-size: 24px;
    font-weight: 800;
    letter-spacing: 0.01em;
    color: #1d1d1f;
  }

  .search {
    width: 100%;
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    height: 52px;
    padding: 0 16px 0 18px;
    border-radius: 999px;
    border: 0.5px solid rgba(0,0,0,0.1);
    background: rgba(255,255,255,0.92);
    box-shadow: 0 1px 2px rgba(0,0,0,0.04), 0 8px 28px rgba(0,0,0,0.07);
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }

  .search:focus-within {
    border-color: rgba(23,59,104,0.32);
    box-shadow: 0 1px 2px rgba(0,0,0,0.04), 0 0 0 4px rgba(23,59,104,0.12);
  }

  .search-icon {
    display: inline-grid;
    place-items: center;
    color: #86868b;
  }

  .search input {
    min-width: 0;
    height: 100%;
    margin: 0;
    padding: 0;
    border: none;
    border-radius: 0;
    background: none;
    box-shadow: none;
    outline: none;
    font: inherit;
    font-size: 16px;
    color: inherit;
  }

  .search input::placeholder {
    color: #86868b;
  }

  .search-hint {
    flex: 0 0 auto;
    padding: 3px 9px;
    border-radius: 7px;
    background: rgba(0,0,0,0.05);
    color: #86868b;
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
  }

  .section-label {
    width: 100%;
    grid-column: 1 / -1;
    margin-bottom: 2px;
    color: #86868b;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.02em;
  }

  .links {
    width: 100%;
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
  }

  /* ── File search results ── */
  .file-results {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: -14px;
  }

  .file-result {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 11px;
    border-radius: 10px;
    border: 0.5px solid rgba(0,0,0,0.08);
    background: rgba(255,255,255,0.86);
    color: inherit;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s ease, border-color 0.15s ease;
  }

  .file-result:hover:not(:disabled) {
    background: #fff;
    border-color: rgba(0,0,0,0.12);
  }

  .fr-icon {
    flex: 0 0 auto;
    display: inline-grid;
    place-items: center;
    color: #2563eb;
  }

  .fr-text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .fr-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 600;
  }

  .fr-meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: #86868b;
  }

  /* Bookmarks are compact chips that hug their content and wrap, so a single
     bookmark stays small instead of stretching into a full tile. */
  .bookmarks {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .bm-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 7px;
  }

  .bm-chip {
    display: inline-flex;
    align-items: stretch;
    max-width: 100%;
    border-radius: 9px;
    border: 0.5px solid rgba(0,0,0,0.08);
    background: rgba(255,255,255,0.86);
    overflow: hidden;
    transition: background 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease;
  }

  .bm-chip:hover {
    background: #fff;
    border-color: rgba(0,0,0,0.12);
    box-shadow: 0 2px 8px rgba(0,0,0,0.06);
  }

  .bm-open {
    min-width: 0;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 6px 6px 9px;
    border: none;
    background: transparent;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }

  .bm-open:disabled {
    cursor: default;
  }

  .bm-fav {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    border-radius: 3px;
    object-fit: contain;
  }

  .bm-fav.placeholder {
    border-radius: 999px;
    background: #c7c7cc;
  }

  .bm-title {
    min-width: 0;
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 650;
    line-height: 1.2;
  }

  .bm-host {
    flex: 0 0 auto;
    color: #86868b;
    font-size: 10.5px;
    white-space: nowrap;
  }

  .bm-remove {
    flex: 0 0 auto;
    width: 24px;
    margin: 0;
    padding: 0;
    border: none;
    border-radius: 0;
    background: transparent;
    color: #86868b;
    cursor: pointer;
    display: grid;
    place-items: center;
    opacity: 0;
    transition: opacity 0.12s ease, color 0.12s ease, background 0.12s ease;
  }

  .bm-chip:hover .bm-remove {
    opacity: 1;
  }

  .bm-remove:hover {
    color: #b42318;
    background: rgba(0,0,0,0.06);
  }

  .link-tile {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 12px;
    border-radius: 11px;
    border: 0.5px solid rgba(0,0,0,0.08);
    background: rgba(255,255,255,0.86);
    color: inherit;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s ease, border-color 0.15s ease, transform 0.1s ease, box-shadow 0.15s ease;
  }

  .link-tile:hover:not(:disabled) {
    background: #fff;
    border-color: rgba(0,0,0,0.12);
    box-shadow: 0 4px 14px rgba(0,0,0,0.07);
  }

  .link-tile:active:not(:disabled) {
    transform: scale(0.98);
  }

  .link-tile:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .link-label {
    font-size: 13.5px;
    font-weight: 700;
    line-height: 1.1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .link-hint {
    font-size: 11.5px;
    color: #86868b;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .det-tile {
    flex-direction: row;
    align-items: center;
    gap: 10px;
    border-color: rgba(150,113,71,0.22);
    background: linear-gradient(180deg, rgba(150,113,71,0.07), rgba(150,113,71,0.025));
  }
  .det-tile:hover:not(:disabled) {
    border-color: rgba(150,113,71,0.42);
    background: linear-gradient(180deg, rgba(150,113,71,0.13), rgba(150,113,71,0.05));
    box-shadow: 0 4px 14px rgba(150,113,71,0.16);
  }
  .det-tile .link-label {
    color: #7a5a32;
  }
  .det-tile-logo {
    width: 32px;
    height: 32px;
    flex: 0 0 auto;
    object-fit: contain;
    object-position: center;
    transition: transform 0.22s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .det-tile:hover:not(:disabled) .det-tile-logo {
    transform: scale(1.1) rotate(-4deg);
  }
  .det-tile-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  :global([data-theme="dark"]) .home-root {
    color: #f5f5f7;
    background: radial-gradient(120% 90% at 50% -10%, rgba(120,160,220,0.08), transparent 60%), #1c1c1e;
  }

  :global([data-theme="dark"]) .det-tile {
    border-color: rgba(201,166,120,0.28);
    background: linear-gradient(180deg, rgba(201,166,120,0.1), rgba(201,166,120,0.04));
  }
  :global([data-theme="dark"]) .det-tile:hover:not(:disabled) {
    border-color: rgba(201,166,120,0.45);
    background: linear-gradient(180deg, rgba(201,166,120,0.16), rgba(201,166,120,0.07));
    box-shadow: 0 4px 14px rgba(0,0,0,0.35);
  }
  :global([data-theme="dark"]) .det-tile .link-label {
    color: #d8bb92;
  }

  :global([data-theme="dark"]) .brand-name {
    color: #f5f5f7;
  }

  :global([data-theme="dark"]) .search {
    border-color: rgba(255,255,255,0.12);
    background: rgba(255,255,255,0.07);
    box-shadow: 0 1px 2px rgba(0,0,0,0.3), 0 8px 28px rgba(0,0,0,0.35);
  }

  :global([data-theme="dark"]) .search:focus-within {
    border-color: rgba(120,160,220,0.4);
    box-shadow: 0 1px 2px rgba(0,0,0,0.3), 0 0 0 4px rgba(120,160,220,0.16);
  }

  :global([data-theme="dark"]) .search-icon,
  :global([data-theme="dark"]) .search input::placeholder,
  :global([data-theme="dark"]) .link-hint,
  :global([data-theme="dark"]) .section-label,
  :global([data-theme="dark"]) .bm-host,
  :global([data-theme="dark"]) .bm-remove {
    color: #98989d;
  }

  :global([data-theme="dark"]) .search-hint {
    background: rgba(255,255,255,0.09);
    color: #98989d;
  }

  :global([data-theme="dark"]) .bm-remove:hover {
    color: #ff9f92;
    background: rgba(255,255,255,0.1);
  }

  :global([data-theme="dark"]) .bm-chip {
    border-color: rgba(255,255,255,0.1);
    background: rgba(255,255,255,0.06);
  }

  :global([data-theme="dark"]) .bm-chip:hover {
    background: rgba(255,255,255,0.1);
    border-color: rgba(255,255,255,0.16);
    box-shadow: 0 2px 8px rgba(0,0,0,0.3);
  }

  :global([data-theme="dark"]) .link-tile {
    border-color: rgba(255,255,255,0.1);
    background: rgba(255,255,255,0.06);
  }

  :global([data-theme="dark"]) .link-tile:hover:not(:disabled) {
    background: rgba(255,255,255,0.1);
    border-color: rgba(255,255,255,0.16);
    box-shadow: 0 4px 14px rgba(0,0,0,0.3);
  }

  :global([data-theme="dark"]) .file-result {
    border-color: rgba(255,255,255,0.1);
    background: rgba(255,255,255,0.06);
  }

  :global([data-theme="dark"]) .file-result:hover:not(:disabled) {
    background: rgba(255,255,255,0.1);
    border-color: rgba(255,255,255,0.16);
  }

  :global([data-theme="dark"]) .fr-meta {
    color: #98989d;
  }

  :global([data-theme="dark"]) .fr-icon {
    color: #6ea8fe;
  }

  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme="light"])) .home-root {
      color: #f5f5f7;
      background: radial-gradient(120% 90% at 50% -10%, rgba(120,160,220,0.08), transparent 60%), #1c1c1e;
    }

    :global(:root:not([data-theme="light"])) .brand-name {
      color: #f5f5f7;
    }

    :global(:root:not([data-theme="light"])) .search {
      border-color: rgba(255,255,255,0.12);
      background: rgba(255,255,255,0.07);
      box-shadow: 0 1px 2px rgba(0,0,0,0.3), 0 8px 28px rgba(0,0,0,0.35);
    }

    :global(:root:not([data-theme="light"])) .search:focus-within {
      border-color: rgba(120,160,220,0.4);
      box-shadow: 0 1px 2px rgba(0,0,0,0.3), 0 0 0 4px rgba(120,160,220,0.16);
    }

    :global(:root:not([data-theme="light"])) .search-icon,
    :global(:root:not([data-theme="light"])) .search input::placeholder,
    :global(:root:not([data-theme="light"])) .link-hint,
    :global(:root:not([data-theme="light"])) .section-label,
    :global(:root:not([data-theme="light"])) .bm-host,
    :global(:root:not([data-theme="light"])) .bm-remove {
      color: #98989d;
    }

    :global(:root:not([data-theme="light"])) .search-hint {
      background: rgba(255,255,255,0.09);
      color: #98989d;
    }

    :global(:root:not([data-theme="light"])) .bm-remove:hover {
      color: #ff9f92;
      background: rgba(255,255,255,0.1);
    }

    :global(:root:not([data-theme="light"])) .bm-chip {
      border-color: rgba(255,255,255,0.1);
      background: rgba(255,255,255,0.06);
    }

    :global(:root:not([data-theme="light"])) .bm-chip:hover {
      background: rgba(255,255,255,0.1);
      border-color: rgba(255,255,255,0.16);
      box-shadow: 0 2px 8px rgba(0,0,0,0.3);
    }

    :global(:root:not([data-theme="light"])) .link-tile {
      border-color: rgba(255,255,255,0.1);
      background: rgba(255,255,255,0.06);
    }

    :global(:root:not([data-theme="light"])) .link-tile:hover:not(:disabled) {
      background: rgba(255,255,255,0.1);
      border-color: rgba(255,255,255,0.16);
      box-shadow: 0 4px 14px rgba(0,0,0,0.3);
    }

    :global(:root:not([data-theme="light"])) .file-result {
      border-color: rgba(255,255,255,0.1);
      background: rgba(255,255,255,0.06);
    }

    :global(:root:not([data-theme="light"])) .file-result:hover:not(:disabled) {
      background: rgba(255,255,255,0.1);
      border-color: rgba(255,255,255,0.16);
    }

    :global(:root:not([data-theme="light"])) .fr-meta {
      color: #98989d;
    }

    :global(:root:not([data-theme="light"])) .fr-icon {
      color: #6ea8fe;
    }

    :global(:root:not([data-theme="light"])) .det-tile {
      border-color: rgba(201,166,120,0.28);
      background: linear-gradient(180deg, rgba(201,166,120,0.1), rgba(201,166,120,0.04));
    }
    :global(:root:not([data-theme="light"])) .det-tile:hover:not(:disabled) {
      border-color: rgba(201,166,120,0.45);
      background: linear-gradient(180deg, rgba(201,166,120,0.16), rgba(201,166,120,0.07));
      box-shadow: 0 4px 14px rgba(0,0,0,0.35);
    }
    :global(:root:not([data-theme="light"])) .det-tile .link-label {
      color: #d8bb92;
    }
  }
</style>
