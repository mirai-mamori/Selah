<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import DOMPurify from "dompurify";
  import { marked } from "marked";
  import { onDestroy, onMount, tick } from "svelte";
  import selahLogoUrl from "../assets/logo.png";
  import { applyAuxiliaryTheme, syncAuxiliaryTheme } from "./auxiliarySurfaceTheme";
  import Icon from "./Icon.svelte";
  import type { AgentMessage, AgentStreamEvent } from "./api";
  import "./agent-panel.css";

  interface DocumentTab {
    id: string;
    target: string;
    title: string;
    type: string;
    active: boolean;
  }

  interface DocumentTabsChanged {
    owner: string;
    tabs: DocumentTab[];
  }

  interface BrowserAgentStatus {
    target?: string;
    active: boolean;
    action?: string;
  }

  type ToolChip = { id: number; name: string; state: "running" | "ok" | "err" };
  type ActionMode = "send" | "mic" | "stop";

  function readParam(name: string): string {
    const search = new URLSearchParams(window.location.search);
    const fromSearch = search.get(name);
    if (fromSearch) return fromSearch;
    const rawHash = window.location.hash.startsWith("#") ? window.location.hash.slice(1) : window.location.hash;
    return new URLSearchParams(rawHash).get(name) || "";
  }

  const owner = readParam("owner") || "document-tabs";
  const initialTarget = readParam("target");
  const initialTitle = readParam("title") || "エージェント";
  const initialKind = readParam("kind") || "agent";
  const standalone = owner === "agent-popup";

  let pageTarget = $state(initialTarget);
  let pageTitle = $state(initialTitle);
  let pageKind = $state(initialKind);
  let convId = $state("");
  let messages = $state<AgentMessage[]>([]);
  let draft = $state("");
  let sending = $state(false);
  let phase = $state("");
  let error = $state("");
  let streamText = $state("");
  let toolChips = $state<ToolChip[]>([]);
  let sttListening = $state(false);
  let sttBaseText = $state("");
  let sttCommittedText = $state("");
  let sttPartialText = $state("");
  let sttStopRequested = $state(false);
  let preemptedCaller = $state<string | null>(null);
  let messagesEl = $state<HTMLElement | null>(null);
  let composerEl = $state<HTMLTextAreaElement | null>(null);
  let composing = false;
  let suppressEnterUntil = 0;
  let contextSequence = 0;
  let chipCounter = 0;
  let contextTimer: number | null = null;
  let unlistenStream: UnlistenFn | null = null;
  let unlistenTabs: UnlistenFn | null = null;
  let unlistenBrowserStatus: UnlistenFn | null = null;
  let unlistenTheme: UnlistenFn | null = null;
  let unlistenAppTheme: UnlistenFn | null = null;
  let unlistenSttPartial: UnlistenFn | null = null;
  let unlistenSttFinal: UnlistenFn | null = null;
  let unlistenSttState: UnlistenFn | null = null;
  let unlistenSttError: UnlistenFn | null = null;

  const actionMode = $derived<ActionMode>(sending ? "stop" : sttListening || !draft.trim() ? "mic" : "send");
  const hasPageContext = $derived(pageKind !== "agent" && !!pageTarget && pageTarget !== owner);
  const kindLabel = $derived(
    pageKind === "reader" ? "リーダー"
      : pageKind === "browser" ? "ブラウザ"
      : pageKind === "kwic" ? "KWIC"
      : pageKind === "kgc" ? "KGC"
      : pageKind === "agent" ? "エージェント"
      : "詳細"
  );
  marked.setOptions({ breaks: true, gfm: true });

  function renderMessage(content: string): string {
    return DOMPurify.sanitize(marked.parse(content || "") as string);
  }

  function conversationKey(target: string, contextual: boolean): string {
    return contextual ? `selah-agent-popup-conv-id:${target}` : "selah-agent-popup-conv-id";
  }

  function conversationStorage(contextual: boolean): Storage {
    return contextual ? sessionStorage : localStorage;
  }

  function toolLabel(name: string): string {
    const labels: Record<string, string> = {
      read_browser_page: "ページ読取",
      browser_click: "クリック",
      browser_fill: "入力",
      browser_scroll: "スクロール",
      browser_wait_for: "待機",
      list_today_classes: "今日の授業",
      list_recent_notifications: "お知らせ",
      list_recent_mail: "メール",
      search_courses: "科目検索",
      list_downloaded_files: "ファイル検索",
      read_downloaded_file: "ファイル読込",
    };
    return labels[name] || name;
  }

  function scrollBottom(): void {
    void tick().then(() => requestAnimationFrame(() => {
      if (messagesEl) messagesEl.scrollTop = messagesEl.scrollHeight;
    }));
  }

  function resizeComposer(): void {
    if (!composerEl) return;
    composerEl.style.height = "auto";
    composerEl.style.height = `${Math.min(composerEl.scrollHeight, 132)}px`;
  }

  function mergeSttText(base: string, committed: string, partial: string): string {
    const spoken = [committed.trim(), partial.trim()].filter(Boolean).join(" ").trim();
    if (!spoken) return base;
    if (!base) return spoken;
    return /\s$/.test(base) ? `${base}${spoken}` : `${base}\n${spoken}`;
  }

  async function bindStream(id: string): Promise<void> {
    unlistenStream?.();
    unlistenStream = await listen<AgentStreamEvent>(`agent_stream:${id}`, (event) => {
      handleStream(event.payload);
    });
  }

  async function loadConversation(target: string): Promise<void> {
    const sequence = ++contextSequence;
    error = "";
    sending = false;
    phase = "";
    streamText = "";
    toolChips = [];
    const contextual = pageKind !== "agent" && target !== owner;
    const storage = conversationStorage(contextual);
    const key = conversationKey(target, contextual);
    let id = storage.getItem(key) || "";
    if (id) {
      try {
        await invoke<AgentMessage[]>("agent_load_messages", { convId: id });
      } catch {
        id = "";
      }
    }
    if (!id) {
      id = await invoke<string>("agent_create_conversation", {
        title: contextual ? "ページエージェント" : "ミニエージェント",
      });
      storage.setItem(key, id);
    }
    const rows = await invoke<AgentMessage[]>("agent_load_messages", { convId: id });
    if (sequence !== contextSequence) return;
    convId = id;
    messages = rows.filter((row) => row.role === "user" || row.role === "assistant");
    await bindStream(id);
    scrollBottom();
  }

  async function applyContext(target: string, title: string, kind: string): Promise<void> {
    const normalizedTarget = target.trim();
    if (!normalizedTarget) return;
    const changed = normalizedTarget !== pageTarget;
    pageTarget = normalizedTarget;
    pageTitle = title.trim() || pageTitle || "エージェント";
    pageKind = kind.trim() || pageKind || "detail";
    document.title = pageTitle;
    if (changed || !convId) {
      try {
        await loadConversation(normalizedTarget);
      } catch (cause) {
        error = `エージェントを起動できませんでした: ${String(cause)}`;
      }
    }
  }

  async function refreshActiveContext(): Promise<void> {
    if (owner !== "document-tabs") return;
    try {
      const tabs = await invoke<DocumentTab[]>("document_tabs_list", { owner });
      const active = tabs.find((tab) => tab.active);
      if (active) await applyContext(active.target, active.title, active.type);
      else await applyContext(owner, "エージェント", "agent");
    } catch {}
  }

  function handleStream(event: AgentStreamEvent): void {
    if (!sending) return;
    if (event.type === "phase") {
      phase = event.stage === "planning" ? "考え中…" : "返答中…";
    } else if (event.type === "tool_call") {
      toolChips = [...toolChips, { id: ++chipCounter, name: event.name, state: "running" }];
      phase = "操作中…";
    } else if (event.type === "tool_result") {
      const match = [...toolChips].reverse().find((chip) => chip.name === event.name && chip.state === "running");
      if (match) toolChips = toolChips.map((chip) => chip.id === match.id ? { ...chip, state: event.ok ? "ok" : "err" } : chip);
    } else if (event.type === "token") {
      streamText += event.text;
    } else if (event.type === "error") {
      error = event.message;
      void finishTurn(false);
    } else if (event.type === "done") {
      void finishTurn(true);
    }
    scrollBottom();
  }

  async function finishTurn(reload: boolean): Promise<void> {
    sending = false;
    phase = "";
    toolChips = [];
    streamText = "";
    if (reload && convId) {
      try {
        messages = (await invoke<AgentMessage[]>("agent_load_messages", { convId }))
          .filter((row) => row.role === "user" || row.role === "assistant");
      } catch {}
    }
    scrollBottom();
  }

  async function send(): Promise<void> {
    const content = draft.trim();
    if (!content || sending || !pageTarget) return;
    if (!convId) await loadConversation(pageTarget);
    const currentConv = convId;
    error = "";
    draft = "";
    resizeComposer();
    messages = [...messages, {
      id: -Date.now(),
      conv_id: currentConv,
      role: "user",
      content,
      created_at: Math.floor(Date.now() / 1000),
    }];
    sending = true;
    phase = "考え中…";
    streamText = "";
    toolChips = [];
    scrollBottom();
    try {
      if (hasPageContext) {
        await invoke("agent_send_with_context", {
          convId: currentConv,
          content,
          images: [],
          browserTarget: pageTarget,
          pageTitle,
          pageKind,
        });
      } else {
        await invoke("agent_send", {
          convId: currentConv,
          content,
          images: [],
        });
      }
      if (sending) await finishTurn(true);
    } catch (cause) {
      error = `送信に失敗しました: ${String(cause)}`;
      await finishTurn(false);
    }
  }

  async function stop(): Promise<void> {
    if (!sending || !convId) return;
    sending = false;
    phase = "";
    await invoke("agent_cancel", { convId }).catch(() => {});
  }

  async function toggleStt(): Promise<void> {
    if (sttListening) {
      sttStopRequested = true;
      await invoke("stt_stop_stream").catch((cause) => error = String(cause));
      return;
    }
    try {
      sttBaseText = draft;
      sttCommittedText = "";
      sttPartialText = "";
      sttStopRequested = false;
      preemptedCaller = await invoke<string | null>("stt_start_stream", { caller: "agent", preempt: true });
    } catch (cause) {
      error = `音声入力を開始できませんでした: ${String(cause)}`;
    }
  }

  async function runAction(): Promise<void> {
    if (actionMode === "stop") await stop();
    else if (actionMode === "mic") await toggleStt();
    else await send();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter" || event.shiftKey) return;
    if (composing || event.isComposing || performance.now() < suppressEnterUntil) return;
    event.preventDefault();
    void send();
  }

  async function closePanel(): Promise<void> {
    if (standalone) {
      await getCurrentWindow().close().catch(() => {});
    } else {
      await invoke("document_tabs_close_agent").catch(() => {});
    }
  }

  async function refreshSttState(): Promise<void> {
    try {
      const [running, caller] = await Promise.all([
        invoke<boolean>("stt_is_running"),
        invoke<string | null>("stt_get_active_caller"),
      ]);
      sttListening = running && caller === "agent";
    } catch {
      sttListening = false;
    }
  }

  onMount(async () => {
    await syncAuxiliaryTheme();
    unlistenTheme = await listen<string>("theme-changed", (event) => applyAuxiliaryTheme(event.payload)).catch(() => null);
    unlistenAppTheme = await listen("app-theme-changed", () => void syncAuxiliaryTheme()).catch(() => null);
    unlistenTabs = await listen<DocumentTabsChanged>("document-tabs-changed", (event) => {
      if (!event.payload || event.payload.owner !== owner) return;
      const active = event.payload.tabs.find((tab) => tab.active);
      if (active) void applyContext(active.target, active.title, active.type);
      else void applyContext(owner, "エージェント", "agent");
    }).catch(() => null);
    unlistenBrowserStatus = await listen<BrowserAgentStatus>("browser-agent-status", (event) => {
      if (event.payload.target && event.payload.target !== pageTarget) return;
      if (event.payload.active) phase = event.payload.action?.trim() || "操作中";
      else if (!sending) phase = "";
    }).catch(() => null);
    unlistenSttPartial = await listen<{ text: string; caller: string }>("stt-partial", (event) => {
      if (event.payload.caller !== "agent") return;
      sttPartialText = event.payload.text || "";
      draft = mergeSttText(sttBaseText, sttCommittedText, sttPartialText);
      resizeComposer();
    }).catch(() => null);
    unlistenSttFinal = await listen<{ text: string; caller: string }>("stt-final", (event) => {
      if (event.payload.caller !== "agent") return;
      sttCommittedText = event.payload.text || sttCommittedText;
      sttPartialText = "";
      draft = mergeSttText(sttBaseText, sttCommittedText, "");
      resizeComposer();
    }).catch(() => null);
    unlistenSttState = await listen<{ state: string; caller: string }>("stt-state", (event) => {
      if (event.payload.caller !== "agent") return;
      const wasListening = sttListening;
      sttListening = event.payload.state === "initializing" || event.payload.state === "listening";
      if (!sttListening) {
        draft = mergeSttText(sttBaseText, sttCommittedText, "");
        const shouldSend = wasListening && sttStopRequested && !!sttCommittedText.trim();
        sttStopRequested = false;
        if (preemptedCaller) {
          const caller = preemptedCaller;
          preemptedCaller = null;
          void invoke("stt_start_stream", { caller }).catch(() => {});
        }
        if (shouldSend) void send();
      }
    }).catch(() => null);
    unlistenSttError = await listen<{ message: string; caller: string }>("stt-error", (event) => {
      if (event.payload.caller !== "agent") return;
      sttListening = false;
      error = event.payload.message || "音声入力エラー";
    }).catch(() => null);
    await refreshSttState();
    await applyContext(initialTarget || owner, initialTitle, initialKind);
    await refreshActiveContext();
    contextTimer = window.setInterval(() => void refreshActiveContext(), 900);
    composerEl?.focus();
  });

  onDestroy(() => {
    contextSequence++;
    unlistenStream?.();
    unlistenTabs?.();
    unlistenBrowserStatus?.();
    unlistenTheme?.();
    unlistenAppTheme?.();
    unlistenSttPartial?.();
    unlistenSttFinal?.();
    unlistenSttState?.();
    unlistenSttError?.();
    if (contextTimer !== null) window.clearInterval(contextTimer);
    if (sttListening) void invoke("stt_stop_stream").catch(() => {});
    if (sending && convId) void invoke("agent_cancel", { convId }).catch(() => {});
  });

  $effect(() => {
    draft;
    void tick().then(resizeComposer);
  });
</script>

<aside class="agent-panel" class:standalone class:embedded={!standalone}>
  <header class="agent-topbar" data-tauri-drag-region={standalone ? "" : undefined}>
    <img class="agent-brand" src={selahLogoUrl} alt="" aria-hidden="true" />
    <div class="agent-head-text">
      <div class="agent-title-row">
        <div class="agent-title" title={pageTitle}>{pageTitle}</div>
        <div class="agent-live-pill" class:active={!!phase}>
          <span class="agent-live-dot"></span>
          <span>{phase || "操作中"}</span>
        </div>
      </div>
      <div class="agent-meta-row">
        <span class="agent-kind-tag" class:reader={pageKind === "reader"}>{kindLabel}</span>
      </div>
    </div>
    {#if !standalone}
      <button class="agent-icon-button agent-close" type="button" title="閉じる" aria-label="閉じる" onclick={closePanel}>
        <Icon name="xmark" size={16} />
      </button>
    {/if}
  </header>

  <section class="agent-messages" bind:this={messagesEl} aria-live="polite">
    {#if messages.length === 0 && !sending}
      <div class="agent-empty">
        <img src={selahLogoUrl} alt="Selah" />
        <strong>ページを見ながら頼めます</strong>
        <span>開いているブラウザや詳細ページの内容を読んで、クリックや要約までこの場で続けます。</span>
      </div>
    {/if}

    {#each messages as message (message.id)}
      <article class:user={message.role === "user"} class:assistant={message.role === "assistant"} class="agent-row">
        {#if message.role === "assistant"}
          <img class="agent-avatar" src={selahLogoUrl} alt="" aria-hidden="true" />
          <div class="agent-bubble assistant-copy">{@html renderMessage(message.content)}</div>
        {:else}
          <div class="agent-bubble user-copy">{message.content}</div>
        {/if}
      </article>
    {/each}

    {#if toolChips.length}
      <div class="agent-tool-stack">
        {#each toolChips as tool (tool.id)}
          <span class="agent-tool-chip" class:ok={tool.state === "ok"} class:err={tool.state === "err"}>
            <span class="agent-tool-icon"></span>
            <span class="agent-tool-name">{toolLabel(tool.name)}</span>
          </span>
        {/each}
      </div>
    {/if}

    {#if sending}
      <article class="agent-row assistant">
        <img class="agent-avatar" class:pulse={!streamText} src={selahLogoUrl} alt="" aria-hidden="true" />
        <div class="agent-bubble assistant-copy streaming">
          {#if streamText}{@html renderMessage(streamText)}{/if}
        </div>
      </article>
    {/if}

    {#if error}
      <article class="agent-row assistant">
        <img class="agent-avatar" src={selahLogoUrl} alt="" aria-hidden="true" />
        <div class="agent-bubble agent-error">……エラーが出たみたい。<br /><br />{error}</div>
      </article>
    {/if}
  </section>

  <footer class="agent-composer-wrap">
    <div class="agent-send-row">
      <div class="agent-composer-island">
        <textarea
          bind:this={composerEl}
          bind:value={draft}
          rows="1"
          placeholder={hasPageContext ? "このページについて聞く" : "エージェントに相談する"}
          aria-label="エージェントへのメッセージ"
          onkeydown={handleKeydown}
          oncompositionstart={() => composing = true}
          oncompositionend={() => {
            composing = false;
            suppressEnterUntil = performance.now() + 160;
          }}
        ></textarea>
      </div>
      <div class="agent-action-slot">
        <button
          class:mic={actionMode === "mic"}
          class:recording={sttListening}
          class:stop={actionMode === "stop"}
          class="agent-action-capsule"
          type="button"
          title={actionMode === "stop" ? "停止" : actionMode === "mic" ? (sttListening ? "音声入力を停止" : "音声入力") : "送信"}
          aria-label={actionMode === "stop" ? "停止" : actionMode === "mic" ? (sttListening ? "音声入力を停止" : "音声入力") : "送信"}
          onclick={runAction}
        >
          <span class="agent-action-capsule-stack" aria-hidden="true">
            <span class="agent-action-face" class:visible={actionMode === "send"}>
              <Icon name="paperplane" size={14} />
              <span>送る</span>
            </span>
            <span class="agent-action-face" class:visible={actionMode === "mic"}>
              <Icon name="microphone" size={14} />
              <span>{sttListening ? "停止" : "音声"}</span>
            </span>
            <span class="agent-action-face" class:visible={actionMode === "stop"}>
              <Icon name="stop" size={14} />
              <span>停止</span>
            </span>
          </span>
        </button>
      </div>
    </div>
    <div class="agent-composer-hint">Enter で送信、Shift+Enter で改行</div>
  </footer>
</aside>
