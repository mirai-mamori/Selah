<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import {
    isDemoActive,
    getStoredSessionStates,
    lunaCheckSession,
    kwicCheckSession,
    resetUniversityLogin,
    serviceRegistry,
    syncSession,
    validateSession,
  } from "../../api";

  type SvcState = { state: "loading" | "saved" | "ok" | "ng" | "error"; label: string };
  type ServiceKey = "kgc" | "luna" | "kwic";

  let kg = $state<SvcState>({ state: "loading", label: "確認中..." });
  let luna = $state<SvcState>({ state: "loading", label: "確認中..." });
  let kwic = $state<SvcState>({ state: "loading", label: "確認中..." });

  let checkBusy = $state(false);
  let repairBusy = $state(false);
  let kgcRefreshBusy = $state(false);
  let resetBusy = $state(false);
  let resetArmed = $state(false);
  let statusMsg = $state("");
  let statusColor = $state("");
  let resetTimer: ReturnType<typeof setTimeout> | null = null;

  let disconnectedCore = $derived([
    ...(luna.state === "ng" ? ["luna" as const] : []),
    ...(kwic.state === "ng" ? ["kwic" as const] : []),
  ]);
  let coreReady = $derived(luna.state === "ok" && kwic.state === "ok");
  let coreStored = $derived(
    (luna.state === "ok" || luna.state === "saved")
      && (kwic.state === "ok" || kwic.state === "saved"),
  );
  let checking = $derived(checkBusy || repairBusy || kgcRefreshBusy || resetBusy);

  function clearStatusLater() {
    setTimeout(() => { statusMsg = ""; }, 5000);
  }

  async function checkKg(): Promise<boolean | null> {
    kg = { state: "loading", label: "確認中..." };
    try {
      const s = await validateSession();
      const label = s.valid ? "有効" + (s.student_id ? ` (${s.student_id})` : "") : "無効・期限切れ";
      kg = { state: s.valid ? "ok" : "ng", label };
      return s.valid;
    } catch {
      kg = { state: "error", label: "確認できません" };
      return null;
    }
  }

  async function checkLuna(): Promise<boolean | null> {
    luna = { state: "loading", label: "確認中..." };
    try {
      const ok = await lunaCheckSession();
      luna = { state: ok ? "ok" : "ng", label: ok ? "有効" : "無効・未接続" };
      return ok;
    } catch {
      luna = { state: "error", label: "確認できません" };
      return null;
    }
  }

  async function checkKwic(): Promise<boolean | null> {
    kwic = { state: "loading", label: "確認中..." };
    try {
      const ok = await kwicCheckSession();
      kwic = { state: ok ? "ok" : "ng", label: ok ? "有効" : "無効・未接続" };
      return ok;
    } catch {
      kwic = { state: "error", label: "確認できません" };
      return null;
    }
  }

  async function checkAll(showResult = true) {
    checkBusy = true;
    try {
      const results = await Promise.all([checkKg(), checkLuna(), checkKwic()]);
      if (!showResult) return;
      if (results.some(result => result === null)) {
        statusColor = "var(--orange, #ff9500)";
        statusMsg = "一部の状態を確認できませんでした。接続は変更していません。";
      } else if (results[1] && results[2]) {
        statusColor = "var(--green)";
        statusMsg = results[0]
          ? "主要サービスと KG Course は利用可能です"
          : "主要サービスは利用可能です。KG Course のみ未接続です";
      } else {
        statusColor = "var(--red)";
        statusMsg = "主要サービスの再認証が必要です";
      }
      clearStatusLater();
    } finally {
      checkBusy = false;
    }
  }

  async function loadStoredStates() {
    try {
      const states = await getStoredSessionStates();
      kg = states.kgc
        ? { state: "saved", label: "保存済み・未確認" }
        : { state: "ng", label: "未接続" };
      luna = states.luna
        ? { state: "saved", label: "保存済み・未確認" }
        : { state: "ng", label: "未接続" };
      kwic = states.kwic
        ? { state: "saved", label: "保存済み・未確認" }
        : { state: "ng", label: "未接続" };
    } catch {
      kg = { state: "error", label: "状態を読み込めません" };
      luna = { state: "error", label: "状態を読み込めません" };
      kwic = { state: "error", label: "状態を読み込めません" };
    }
  }

  function setRepairing(service: ServiceKey) {
    if (service === "kgc") kg = { state: "loading", label: "復旧中..." };
    if (service === "luna") luna = { state: "loading", label: "復旧中..." };
    if (service === "kwic") kwic = { state: "loading", label: "復旧中..." };
  }

  function setRepairResult(service: ServiceKey, ok: boolean) {
    const next: SvcState = {
      state: ok ? "ok" : "ng",
      label: ok ? "有効・復旧済み" : "復旧できませんでした",
    };
    if (ok) serviceRegistry[service].onRecovered();
    else serviceRegistry[service].onReset();
    if (service === "kgc") kg = next;
    if (service === "luna") luna = next;
    if (service === "kwic") kwic = next;
  }

  async function repairDisconnected() {
    if (disconnectedCore.length === 0) {
      await checkAll();
      return;
    }

    repairBusy = true;
    statusColor = "var(--text-secondary)";
    statusMsg = "Luna・KWIC の接続を復旧しています...";
    const targets = [...disconnectedCore];
    try {
      for (const service of targets) {
        setRepairing(service);
        const ok = await syncSession(service).catch(() => false);
        setRepairResult(service, ok);
      }
      if (luna.state === "ok" && kwic.state === "ok") {
        statusColor = "var(--green)";
        statusMsg = kg.state === "ok"
          ? "すべての接続を復旧しました"
          : "主要サービスを復旧しました。KG Course は引き続き未接続です";
      } else {
        statusColor = "var(--orange, #ff9500)";
        statusMsg = "主要サービスを復旧できませんでした。完全再ログインを試してください";
      }
    } finally {
      repairBusy = false;
      clearStatusLater();
    }
  }

  async function refreshKgcAndCore() {
    kgcRefreshBusy = true;
    statusColor = "var(--text-secondary)";
    statusMsg = "KG Course を更新しています...";
    setRepairing("kgc");
    try {
      const kgcOk = await syncSession("kgc").catch(() => false);
      setRepairResult("kgc", kgcOk);
      if (!kgcOk) {
        statusColor = "var(--orange, #ff9500)";
        statusMsg = "KG Course を更新できませんでした。Luna・KWIC は変更していません";
        return;
      }

      statusMsg = "KG Course を更新しました。Luna・KWIC の Cookie を更新しています...";
      setRepairing("luna");
      setRepairing("kwic");
      const lunaOk = await syncSession("luna").catch(() => false);
      setRepairResult("luna", lunaOk);
      const kwicOk = await syncSession("kwic").catch(() => false);
      setRepairResult("kwic", kwicOk);

      if (lunaOk && kwicOk) {
        statusColor = "var(--green)";
        statusMsg = "KG Course・Luna・KWIC の Cookie を更新しました";
      } else {
        statusColor = "var(--orange, #ff9500)";
        statusMsg = "KG Course は更新しましたが、一部の主要サービスを更新できませんでした";
      }
    } finally {
      kgcRefreshBusy = false;
      clearStatusLater();
    }
  }

  function armReset() {
    resetArmed = true;
    statusColor = "var(--red)";
    statusMsg = "Luna・KWIC・KG Course の認証 Cookie を削除します。設定や保存データは削除しません。";
    if (resetTimer) clearTimeout(resetTimer);
    resetTimer = setTimeout(() => {
      resetArmed = false;
      statusMsg = "";
    }, 7000);
  }

  async function clearCookiesAndRelogin() {
    if (!resetArmed) {
      armReset();
      return;
    }

    resetArmed = false;
    if (resetTimer) clearTimeout(resetTimer);
    resetBusy = true;
    statusColor = "var(--text-secondary)";
    statusMsg = "認証 Cookie を削除してログイン画面を開いています...";
    try {
      const result = await resetUniversityLogin();
      if (isDemoActive()) {
        statusColor = "var(--green)";
        statusMsg = "デモモードの認証状態を初期化しました";
      } else {
        kg = result.core
          ? { state: "ok", label: "有効・再ログイン済み" }
          : { state: "ng", label: "再ログインできませんでした" };
        luna = result.core?.luna_authenticated
          ? { state: "ok", label: "有効・再ログイン済み" }
          : { state: "ng", label: "再ログインできませんでした" };
        kwic = result.core?.kwic_authenticated
          ? { state: "ok", label: "有効・再ログイン済み" }
          : { state: "ng", label: "再ログインできませんでした" };
        statusColor = luna.state === "ok" && kwic.state === "ok"
          ? "var(--green)"
          : "var(--orange, #ff9500)";
        statusMsg = luna.state === "ok" && kwic.state === "ok"
          ? `${result.deleted} 件の Cookie を削除し、完全再ログインしました`
          : `${result.deleted} 件の Cookie を削除しましたが、一部の主要サービスに再ログインできませんでした`;
      }
    } catch (e) {
      statusColor = "var(--red)";
      statusMsg = "失敗: " + String(e);
    } finally {
      resetBusy = false;
      clearStatusLater();
    }
  }

  onMount(() => {
    void (async () => {
      await loadStoredStates();
      await checkAll(false);
    })();
  });

  onDestroy(() => {
    if (resetTimer) clearTimeout(resetTimer);
  });
</script>

<div class="hero-card">
  <div class="hero-icon" style="background:linear-gradient(135deg,rgba(52,199,89,0.15),rgba(0,122,255,0.15));">
    <svg viewBox="0 0 20 20" fill="none" stroke="#2d8a4e" stroke-width="1.3">
      <rect x="3" y="5" width="14" height="10" rx="2"/>
      <path d="M7 5V3.5a3 3 0 016 0V5" stroke-linecap="round"/>
      <circle cx="10" cy="10.5" r="1.5"/>
      <path d="M10 12v1.5" stroke-linecap="round"/>
    </svg>
  </div>
  <div class="hero-text">
    <h2 class="panel-title">セッション</h2>
    <p class="panel-desc">Luna と KWIC を主要サービスとして監視します。KG Course は自動復旧せず、状態行の更新操作で KG Course に続けて Luna・KWIC の Cookie を更新します。</p>
  </div>
</div>

<div class="card-label">セッション状態</div>
<div class="card">
  <div class="row">
    <span class="row-label">KG Course <span class="service-role auxiliary">補助・短期</span></span>
    <div class="row-input">
      <div class="session-indicator">
        {#if kg.state === "loading"}<span class="spinner-sm"></span>
        {:else if kg.state === "ok"}<span class="session-dot ok"></span>
        {:else if kg.state === "saved"}<span class="session-dot saved"></span>
        {:else if kg.state === "error"}<span class="session-dot unknown"></span>
        {:else}<span class="session-dot ng"></span>{/if}
        {kg.label}
      </div>
      <button class="btn-test session-update" disabled={checking} onclick={refreshKgcAndCore}>
        {kgcRefreshBusy ? "更新中..." : "KGCを更新"}
      </button>
    </div>
  </div>
  <div class="row">
    <span class="row-label">Luna LMS <span class="service-role core">主要</span></span>
    <div class="row-input">
      <div class="session-indicator">
        {#if luna.state === "loading"}<span class="spinner-sm"></span>
        {:else if luna.state === "ok"}<span class="session-dot ok"></span>
        {:else if luna.state === "saved"}<span class="session-dot saved"></span>
        {:else if luna.state === "error"}<span class="session-dot unknown"></span>
        {:else}<span class="session-dot ng"></span>{/if}
        {luna.label}
      </div>
    </div>
  </div>
  <div class="row">
    <span class="row-label">KWIC Portal <span class="service-role core">主要</span></span>
    <div class="row-input">
      <div class="session-indicator">
        {#if kwic.state === "loading"}<span class="spinner-sm"></span>
        {:else if kwic.state === "ok"}<span class="session-dot ok"></span>
        {:else if kwic.state === "saved"}<span class="session-dot saved"></span>
        {:else if kwic.state === "error"}<span class="session-dot unknown"></span>
        {:else}<span class="session-dot ng"></span>{/if}
        {kwic.label}
      </div>
    </div>
  </div>
</div>

<div class:ready={coreReady} class:stored={!coreReady && coreStored} class="core-summary">
  <strong>{coreReady ? "主要サービスは利用可能です" : coreStored ? "主要サービスのセッションは保存されています" : "主要サービスの状態を確認してください"}</strong>
  <span>{coreReady ? "KG Course が切れていても通常利用を継続できます。" : coreStored ? "保存済みの状態を確認しています。" : "Luna または KWIC が未接続の場合は、主要サービスの復旧を試してください。"}</span>
</div>

<div class="action-bar">
  <button class="btn-test" disabled={checking} onclick={() => checkAll()}>状態を確認</button>
  <button class="btn-test primary" disabled={checking || disconnectedCore.length === 0} onclick={repairDisconnected}>
    {repairBusy ? "Luna・KWICを復旧中..." : `Luna・KWICを復旧${disconnectedCore.length ? ` (${disconnectedCore.length})` : ""}`}
  </button>
  <button class:armed={resetArmed} class="btn-test danger" disabled={checking} onclick={clearCookiesAndRelogin}>
    {resetBusy ? "初期化中..." : resetArmed ? "もう一度押して完全再ログイン" : "Cookieを削除して完全再ログイン"}
  </button>
  {#if statusMsg}
    <span class="hint action-status" style="color:{statusColor};">{statusMsg}</span>
  {/if}
</div>

<style>
  .service-role {
    display: inline-flex;
    align-items: center;
    margin-left: 6px;
    padding: 2px 6px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 650;
    vertical-align: 2px;
  }
  .service-role.core {
    color: var(--green, #34c759);
    background: color-mix(in srgb, var(--green, #34c759) 12%, transparent);
  }
  .service-role.auxiliary {
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--text-secondary) 10%, transparent);
  }
  .core-summary {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: 10px;
    padding: 11px 14px;
    border: 1px solid color-mix(in srgb, var(--orange, #ff9500) 25%, transparent);
    border-radius: 12px;
    background: color-mix(in srgb, var(--orange, #ff9500) 7%, transparent);
    color: var(--text-secondary);
    font-size: 11px;
  }
  .core-summary strong {
    color: var(--orange, #ff9500);
    font-size: 12px;
  }
  .core-summary.ready {
    border-color: color-mix(in srgb, var(--green, #34c759) 24%, transparent);
    background: color-mix(in srgb, var(--green, #34c759) 7%, transparent);
  }
  .core-summary.ready strong {
    color: var(--green, #34c759);
  }
  .core-summary.stored {
    border-color: color-mix(in srgb, var(--accent) 24%, transparent);
    background: color-mix(in srgb, var(--accent) 7%, transparent);
  }
  .core-summary.stored strong {
    color: var(--accent);
  }
  .action-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 7px;
    align-items: center;
    margin-top: 10px;
  }
  .action-status {
    flex-basis: 100%;
    margin-top: 2px;
  }
  .row-input {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .session-indicator {
    flex: 1;
    min-width: 0;
  }
  .session-update {
    flex: 0 0 auto;
    min-width: 88px;
  }
  :global(.settings-main .btn-test.primary) {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 28%, transparent);
  }
  :global(.settings-main .btn-test.primary:hover:not(:disabled)) {
    background: var(--accent);
    color: #fff;
    border-color: var(--accent);
  }
  :global(.settings-main .btn-test.danger) {
    background: color-mix(in srgb, var(--red) 8%, transparent);
    color: var(--red);
    border-color: color-mix(in srgb, var(--red) 24%, transparent);
  }
  :global(.settings-main .btn-test.danger:hover:not(:disabled)),
  :global(.settings-main .btn-test.danger.armed) {
    background: var(--red);
    color: #fff;
    border-color: var(--red);
  }
  :global(.settings-main .session-dot.unknown) {
    background: var(--orange, #ff9500);
  }
  :global(.settings-main .session-dot.saved) {
    background: var(--accent);
    opacity: 0.65;
  }
</style>
