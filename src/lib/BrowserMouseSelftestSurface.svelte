<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let count = $state(0);

  function readRequestId(): string {
    const search = new URLSearchParams(window.location.search);
    const fromSearch = search.get("request");
    if (fromSearch) return fromSearch;
    const rawHash = window.location.hash.startsWith("#") ? window.location.hash.slice(1) : window.location.hash;
    return new URLSearchParams(rawHash).get("request") || "";
  }

  const requestId = readRequestId();

  async function report(): Promise<void> {
    if (!requestId) return;
    try {
      await invoke("debug_browser_mouse_selftest_report", {
        report: {
          requestId,
          count,
          href: String(window.location.href || ""),
        },
      });
    } catch (error) {
      console.warn("debug_browser_mouse_selftest_report failed", error);
    }
  }

  function handleClick(): void {
    count += 1;
    document.body.dataset.clickCount = String(count);
    window.location.hash = `clicked-${count}`;
    void report();
  }

  onMount(() => {
    const timers = [
      window.setTimeout(report, 0),
      window.setTimeout(report, 100),
      window.setTimeout(report, 500),
    ];
    return () => {
      for (const timer of timers) window.clearTimeout(timer);
      delete document.body.dataset.clickCount;
    };
  });
</script>

<main>
  <h1>Browser Mouse Selftest</h1>
  <p>This surface proves that Selah can click a real browser WebView with OS mouse events.</p>
  <button type="button" onclick={handleClick}>Click target</button>
  <div class="status" aria-live="polite">Clicked count: {count}</div>
</main>

<style>
  main {
    position: relative;
    width: 100vw;
    min-height: 100vh;
    padding: 32px;
    background: #ffffff;
    color: #172033;
    box-sizing: border-box;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }

  h1 {
    margin: 0 0 12px;
    font-size: 22px;
    font-weight: 700;
    letter-spacing: 0;
  }

  p {
    margin: 0 0 24px;
    line-height: 1.5;
    color: #42506a;
  }

  button {
    position: absolute;
    left: 120px;
    top: 112px;
    width: 220px;
    height: 48px;
    border: 0;
    border-radius: 8px;
    background: #1f6feb;
    color: #ffffff;
    font-size: 16px;
    font-weight: 650;
    cursor: pointer;
  }

  button:active {
    transform: translateY(1px);
  }

  .status {
    position: absolute;
    left: 120px;
    top: 188px;
    margin-top: 20px;
    color: #0f7b45;
    font-size: 15px;
    font-weight: 650;
  }
</style>
