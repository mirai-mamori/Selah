<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import Icon from "./Icon.svelte";

  interface DocumentTab {
    id: string;
    title: string;
    url: string;
    type: string;
    active: boolean;
    loading?: boolean;
  }

  const OWNER = "document-tabs";

  let tabs = $state<DocumentTab[]>([]);
  let busy = $state(false);

  // Cap the dock list; the full set stays reachable inside the Copilot window.
  const MAX_VISIBLE = 4;
  const visibleTabs = $derived(tabs.slice(0, MAX_VISIBLE));
  let unlisten: (() => void) | null = null;
  let faviconFailed = $state<Set<string>>(new Set());

  function faviconFor(url: string): string {
    try {
      const host = new URL(url).hostname;
      return host ? `https://www.google.com/s2/favicons?sz=64&domain=${encodeURIComponent(host)}` : "";
    } catch {
      return "";
    }
  }

  function onFaviconError(src: string): void {
    if (src && !faviconFailed.has(src)) faviconFailed = new Set(faviconFailed).add(src);
  }

  function shortTitle(tab: DocumentTab): string {
    const s = (tab.title || tab.url || tab.type || "タブ").trim();
    return s.length > 26 ? `${s.slice(0, 25)}…` : s;
  }

  async function refresh(): Promise<void> {
    try {
      tabs = await invoke<DocumentTab[]>("document_tabs_list", { owner: OWNER });
    } catch {
      /* window not ready yet */
    }
  }

  async function run(action: () => Promise<unknown>): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await action();
    } finally {
      busy = false;
    }
  }

  function newTab(): void {
    // A fresh tab also shows/focuses the Copilot window (ensure_window).
    void run(() => invoke("document_tabs_new_tab"));
  }

  function revealWindow(): void {
    void run(() => invoke("document_tabs_reveal", { owner: OWNER }));
  }

  function revealTab(id: string): void {
    void run(() => invoke("document_tabs_reveal", { owner: OWNER, id }));
  }

  onMount(async () => {
    await refresh();
    unlisten = await listen<{ owner: string; tabs: DocumentTab[] }>("document-tabs-changed", (event) => {
      if (event.payload?.owner === OWNER) tabs = event.payload.tabs || [];
    });
  });

  onDestroy(() => unlisten?.());
</script>

<div class="copilot-dock">
  <div class="cd-header">
    <span class="cd-logo" aria-hidden="true"><Icon name="copilot" size={19} /></span>
    <span class="cd-title">Copilot</span>
    <button class="cd-new" type="button" onclick={newTab} title="新しいタブ" aria-label="新しいタブ" disabled={busy}>
      <Icon name="plus" size={14} />
    </button>
  </div>

  {#if tabs.length}
    <div class="cd-tabs">
      {#each visibleTabs as tab (tab.id)}
        <button
          class="cd-tab"
          class:active={tab.active}
          type="button"
          title={tab.title || tab.url}
          disabled={busy}
          onclick={() => revealTab(tab.id)}
        >
          {#if tab.loading}
            <span class="cd-ico cd-spinner" aria-hidden="true"></span>
          {:else if tab.type === "detective"}
            <img class="cd-ico cd-img" src="/naruhodo.png" alt="" aria-hidden="true" draggable="false" />
          {:else if tab.type === "browser" && faviconFor(tab.url) && !faviconFailed.has(faviconFor(tab.url))}
            <img class="cd-ico cd-img" src={faviconFor(tab.url)} alt="" aria-hidden="true" draggable="false" onerror={() => onFaviconError(faviconFor(tab.url))} />
          {:else}
            <span class="cd-ico cd-dot" data-kind={tab.type} aria-hidden="true"></span>
          {/if}
          <span class="cd-tab-title">{shortTitle(tab)}</span>
        </button>
      {/each}
    </div>
  {/if}

  {#if tabs.length > MAX_VISIBLE}
    <button class="cd-more" type="button" onclick={revealWindow} disabled={busy} title="Copilot ですべてのタブを表示">
      他に {tabs.length - MAX_VISIBLE} 件のタブ
    </button>
  {/if}
</div>

<style>
  /* Animated angle drives the flowing conic-gradient border. */
  @property --cd-angle {
    syntax: "<angle>";
    inherits: false;
    initial-value: 0deg;
  }

  /* Inset glass module docked at the sidebar foot. */
  .copilot-dock {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 5px 8px 8px;
    padding: 6px;
    border-radius: 12px;
    border: 0.5px solid var(--glass-border);
    background: var(--bg-card);
    box-shadow:
      0 4px 16px rgba(0, 0, 0, 0.08),
      inset 0 0.5px 0 rgba(255, 255, 255, 0.45);
  }

  @keyframes cd-rotate {
    to { --cd-angle: 360deg; }
  }

  .cd-header {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 28px;
    padding: 0 4px 0 6px;
  }

  .cd-logo {
    flex: 0 0 auto;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    color: var(--text-primary);
  }

  .cd-title {
    flex: 1 1 auto;
    min-width: 0;
    text-align: left;
    font-size: 13px;
    font-weight: 700;
    letter-spacing: 0.2px;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cd-more {
    align-self: stretch;
    margin-top: 2px;
    padding: 5px 8px;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--text-tertiary);
    font-size: 11.5px;
    font-weight: 600;
    text-align: center;
    cursor: pointer;
    transition: background 0.14s ease, color 0.14s ease;
  }

  .cd-more:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    color: var(--accent);
  }

  .cd-more:disabled {
    cursor: default;
  }

  .cd-new {
    position: relative;
    flex: 0 0 auto;
    width: 26px;
    height: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--text-primary);
    cursor: pointer;
    transition: background 0.14s ease, color 0.14s ease, transform 0.1s ease;
  }

  /* Flowing AI-gradient border lives on the add button. */
  .cd-new::before {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    padding: 1.5px;
    background: conic-gradient(
      from var(--cd-angle),
      #af52de,
      #007aff,
      color-mix(in srgb, #007aff 55%, #ffffff),
      #007aff,
      #af52de
    );
    -webkit-mask:
      linear-gradient(#000 0 0) content-box,
      linear-gradient(#000 0 0);
    mask:
      linear-gradient(#000 0 0) content-box,
      linear-gradient(#000 0 0);
    -webkit-mask-composite: xor;
    mask-composite: exclude;
    pointer-events: none;
    animation: cd-rotate 6s linear infinite;
  }

  .cd-new :global(.icon) {
    display: block;
    position: relative;
    z-index: 1;
  }

  .cd-new:hover:not(:disabled) {
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
    color: var(--text-primary);
  }

  .cd-new:active:not(:disabled) {
    transform: scale(0.9);
  }

  .cd-new:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .cd-tabs {
    display: flex;
    flex-direction: column;
    gap: 1px;
    max-height: 168px;
    overflow-y: auto;
    scrollbar-width: thin;
  }

  .cd-tab {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 27px;
    padding: 0 8px 0 10px;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    text-align: left;
    transition: background 0.15s ease, color 0.15s ease, transform 0.14s cubic-bezier(0.2, 0.8, 0.2, 1);
  }

  .cd-tab:hover:not(:disabled) {
    background: color-mix(in srgb, var(--text-primary) 6%, transparent);
    color: var(--text-primary);
    transform: translateX(2px);
  }

  .cd-tab.active {
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
    color: var(--text-primary);
    box-shadow: inset 0 0 0 0.5px color-mix(in srgb, var(--text-primary) 18%, transparent);
  }

  .cd-tab.active .cd-tab-title {
    font-weight: 650;
  }

  .cd-tab:disabled {
    cursor: default;
  }

  .cd-ico {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
  }

  .cd-img {
    border-radius: 4px;
    object-fit: cover;
    box-shadow: 0 0 0 0.5px rgba(0, 0, 0, 0.06);
  }

  .cd-dot {
    width: 8px;
    height: 8px;
    margin: 0 3.5px;
    border-radius: 50%;
    background: #64748b;
    box-shadow: 0 0 0 3px color-mix(in srgb, #64748b 16%, transparent);
  }

  .cd-dot[data-kind="reader"] { background: #0f766e; box-shadow: 0 0 0 3px rgba(15,118,110,0.16); }
  .cd-dot[data-kind="browser"] { background: #2563eb; box-shadow: 0 0 0 3px rgba(37,99,235,0.16); }
  .cd-dot[data-kind="kwic"] { background: #b45309; box-shadow: 0 0 0 3px rgba(180,83,9,0.16); }
  .cd-dot[data-kind="kgc"] { background: #7c3aed; box-shadow: 0 0 0 3px rgba(124,58,237,0.16); }
  .cd-dot[data-kind="files"] { background: #0891b2; box-shadow: 0 0 0 3px rgba(8,145,178,0.16); }
  .cd-dot[data-kind="home"] { background: #475569; box-shadow: 0 0 0 3px rgba(71,85,105,0.16); }
  .cd-dot[data-kind="detective"] { background: #b91c1c; box-shadow: 0 0 0 3px rgba(185,28,28,0.16); }

  .cd-tab-title {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
  }

  .cd-spinner {
    width: 12px;
    height: 12px;
    margin: 1.5px;
    border-radius: 50%;
    border: 1.6px solid var(--border-strong, rgba(0,0,0,0.18));
    border-top-color: var(--accent);
    animation: cd-spin 0.7s linear infinite;
  }

  @keyframes cd-spin {
    to { transform: rotate(360deg); }
  }

  @media (prefers-reduced-motion: reduce) {
    .cd-new::before { animation: none; }
    .cd-tab:hover:not(:disabled) { transform: none; }
  }
</style>
