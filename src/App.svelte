<script lang="ts">
  import "./styles.css";
  import Login from "./lib/Login.svelte";
  import Dashboard from "./lib/Dashboard.svelte";
  import type { Component } from "svelte";
  import { demoMode } from "./lib/demoStore";
  import { authState, sessionExpired, invalidateCache } from "./lib/stores";
  import { restoreAllSessions, startBackgroundPolling, stopBackgroundPolling, serviceRegistry } from "./lib/api";
  import { startTrayStatus, stopTrayStatus } from "./lib/trayStatus";
  import { startSilentUpdateCheck } from "./lib/updater";
  import { listen } from "@tauri-apps/api/event";
  import { get } from "svelte/store";
  import { onMount, onDestroy } from "svelte";
  // Persistent latch: once the user has EVER logged in, always show Dashboard
  // (with cached data + re-auth badge). Only cleared by explicit logout.
  // Demo sessions do not participate in this latch.
  function readDemoBootFlag(): boolean {
    try {
      return localStorage.getItem("selah-demo-mode") === "1";
    } catch {
      return false;
    }
  }
  function readEverLoggedIn(): boolean {
    try {
      if (localStorage.getItem("selah-ever-auth") !== "1") return false;
      const source = localStorage.getItem("selah-ever-auth-source");
      if (source === "real") return true;
      // Backward compatibility: older builds only stored the boolean flag.
      // Keep treating it as a real login latch unless demo mode itself is active.
      if (!source && localStorage.getItem("selah-demo-mode") !== "1") return true;
      return false;
    } catch {
      return false;
    }
  }

  function debugLog(...args: unknown[]): void {
    try {
      if (localStorage.getItem("selah-debug-logs") === "1") console.log(...args);
    } catch {
      // Ignore storage access failures during early app startup.
    }
  }

  function readSurfaceParam(name: string): string {
    const search = new URLSearchParams(window.location.search);
    const fromSearch = search.get(name);
    if (fromSearch) return fromSearch;
    const rawHash = window.location.hash.startsWith("#") ? window.location.hash.slice(1) : window.location.hash;
    return new URLSearchParams(rawHash).get(name) || "";
  }

  const surface = readSurfaceParam("surface");
  const isDocumentTabsSurface = surface === "document-tabs";
  const isMarkdownReaderSurface = surface === "markdown-reader";
  const isUniversityDetailSurface = surface === "university-detail" || surface === "luna-detail";
  const isAgentPanelSurface = surface === "agent-panel";
  const isFilesSurface = surface === "files";
  const isHomeSurface = surface === "home";
  const isDetectiveSurface = surface === "detective";
  const isSplitDividerSurface = surface === "split-divider";
  const isBrowserMouseSelftestSurface = surface === "browser-mouse-selftest";
  const isAuxiliarySurface = isDocumentTabsSurface || isMarkdownReaderSurface || isUniversityDetailSurface || isAgentPanelSurface || isFilesSurface || isHomeSurface || isDetectiveSurface || isSplitDividerSurface || isBrowserMouseSelftestSurface;

  let demoBootFlag = $state(readDemoBootFlag());
  let everLoggedIn = $state(readEverLoggedIn());
  let currentView = $derived(($demoMode || demoBootFlag || $authState.authenticated || $sessionExpired || everLoggedIn) ? "dashboard" : "login");
  let restoring = $state(true);
  let AuxiliarySurface = $state<Component | null>(null);
  let auxiliaryError = $state("");
  let unlistenLogout: (() => void) | null = null;

  async function restoreDemoState(): Promise<boolean> {
    const { restoreDemo } = await import("./lib/demo");
    return restoreDemo();
  }

  onMount(async () => {
    if (isAuxiliarySurface) {
      try {
        if (isDocumentTabsSurface) {
          AuxiliarySurface = (await import("./lib/DocumentTabs.svelte")).default;
        } else if (isMarkdownReaderSurface) {
          AuxiliarySurface = (await import("./lib/MarkdownReaderSurface.svelte")).default;
        } else if (isAgentPanelSurface) {
          AuxiliarySurface = (await import("./lib/AgentPanel.svelte")).default;
        } else if (isFilesSurface) {
          AuxiliarySurface = (await import("./lib/FilesSurface.svelte")).default;
        } else if (isHomeSurface) {
          AuxiliarySurface = (await import("./lib/NewTabSurface.svelte")).default;
        } else if (isDetectiveSurface) {
          AuxiliarySurface = (await import("./lib/views/Detective.svelte")).default;
        } else if (isSplitDividerSurface) {
          AuxiliarySurface = (await import("./lib/SplitDividerSurface.svelte")).default;
        } else if (isBrowserMouseSelftestSurface) {
          AuxiliarySurface = (await import("./lib/BrowserMouseSelftestSurface.svelte")).default;
        } else {
          AuxiliarySurface = (await import("./lib/UniversityDetailSurface.svelte")).default;
        }
      } catch (error) {
        auxiliaryError = error instanceof Error ? error.message : String(error);
      }
      restoring = false;
      return;
    }
    // Handle logout triggered from settings window (or other windows)
    unlistenLogout = await listen("logout", async () => {
      const { deactivateDemo, isDemoMode: checkDemo } = await import("./lib/demo");
      if (checkDemo()) deactivateDemo();
      stopBackgroundPolling();
      stopTrayStatus();
      sessionExpired.set(false);
      for (const svc of Object.values(serviceRegistry)) svc.onReset();
      invalidateCache();
      try {
        localStorage.removeItem("selah-ever-auth");
        localStorage.removeItem("selah-ever-auth-source");
      } catch {}
      demoBootFlag = false;
      everLoggedIn = false;
    });

    // Demo mode: restore from previous session, skip real network calls
    if (await restoreDemoState()) {
      demoBootFlag = true;
      startTrayStatus();
      void startSilentUpdateCheck();
      restoring = false;
      return;
    }

    try {
      // Restore all service sessions (KGC + Luna + future)
      const session = await restoreAllSessions();
      debugLog("[Selah] App.onMount: restoreAllSessions returned", session ? "non-null" : "null",
        "authState.authenticated =", get(authState).authenticated,
        "sessionExpired =", get(sessionExpired),
        "everLoggedIn =", everLoggedIn);
      if (session) {
        startBackgroundPolling();
      } else if (everLoggedIn) {
        // Had a previous session (from a past app run) but recovery failed.
        // Set sessionExpired so the re-auth badge shows, and start polling
        // so cached/SWR data is served.
        sessionExpired.set(true);
        startBackgroundPolling();
      }
      startTrayStatus();
    } catch (e) {
      console.warn("Session restore failed:", e);
      if (everLoggedIn) {
        sessionExpired.set(true);
        startBackgroundPolling();
      }
    } finally {
      restoring = false;
      void startSilentUpdateCheck();
    }
  });

  onDestroy(() => {
    unlistenLogout?.();
    stopTrayStatus();
    stopBackgroundPolling();
  });
</script>

{#if isAuxiliarySurface}
  {#if AuxiliarySurface}
    <AuxiliarySurface />
  {:else}
    <main class="auxiliary-state" class:errored={!!auxiliaryError}>
      {#if auxiliaryError}
        <strong>ページを読み込めませんでした</strong>
        <span>{auxiliaryError}</span>
      {:else}
        <span class="auxiliary-spinner" aria-hidden="true"></span>
      {/if}
    </main>
  {/if}
{:else if currentView === "login" && !restoring}
  <main class="app-main">
    <div class="page-transition">
      <Login />
    </div>
  </main>
{:else}
  <Dashboard />
{/if}

<style>
  .auxiliary-state {
    min-height: 100vh;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 10px;
    box-sizing: border-box;
    padding: 32px;
    color: #60646c;
    background: #f7f8fa;
    font: 13px/1.5 -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", "Noto Sans JP", sans-serif;
    text-align: center;
    /* Hold the surface blank briefly: fast module imports never flash a spinner,
       and the surface's own skeleton takes over almost immediately. */
    opacity: 0;
    animation: auxiliary-appear 0.3s ease 0.16s forwards;
  }

  .auxiliary-state.errored {
    opacity: 1;
    animation: none;
  }

  @keyframes auxiliary-appear {
    to { opacity: 1; }
  }

  .auxiliary-state strong {
    color: #24262b;
    font-size: 15px;
  }

  .auxiliary-spinner {
    width: 16px;
    height: 16px;
    border: 2px solid rgba(96, 100, 108, 0.2);
    border-top-color: #60646c;
    border-radius: 50%;
    animation: auxiliary-spin 0.8s linear infinite;
  }

  :global([data-theme="dark"]) .auxiliary-state {
    color: #a9abb2;
    background: #1c1c1e;
  }

  :global([data-theme="dark"]) .auxiliary-state strong {
    color: #f5f5f7;
  }

  @keyframes auxiliary-spin {
    to { transform: rotate(360deg); }
  }

  .app-main {
    flex: 1;
    overflow: hidden;
  }
  .page-transition {
    height: 100%;
    animation: fade-in-scale 0.4s cubic-bezier(0.2, 0.8, 0.2, 1) both;
  }
</style>
