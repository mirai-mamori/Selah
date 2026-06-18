<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount, onDestroy } from "svelte";
  import Icon from "../Icon.svelte";
  import { cachedBackendFetch, getCached, onCacheUpdate } from "../stores";
  import type { LunaNotification } from "../types";

  interface Props {
    courseName: string;
    idnumberParam: string;
  }

  let { courseName, idnumberParam }: Props = $props();
  let updates = $state<LunaNotification[]>(getCached<LunaNotification[]>("luna_updates") ?? []);

  // Communication notifications: Luna surfaces inquiry/message activity under a
  // 「メッセージ」/「お問い合わせ」module, and the message thread lives under /message.
  function isMessageNotification(n: LunaNotification): boolean {
    const module = n.module || "";
    return (
      module.includes("メッセージ") ||
      module.includes("問い合わせ") ||
      (n.url || "").includes("/message")
    );
  }

  function normalize(value: string): string {
    return String(value || "").replace(/[\s_・\-–（）()［］\[\]]/g, "").toLowerCase();
  }

  function matchesCourse(n: LunaNotification): boolean {
    if (idnumberParam && n.idnumber) {
      return n.idnumber === idnumberParam || n.idnumber.includes(idnumberParam) || idnumberParam.includes(n.idnumber);
    }
    const target = normalize(courseName);
    const candidate = normalize(n.course_info);
    return !!candidate && !!target && (target.includes(candidate) || candidate.includes(target));
  }

  interface MessageThread extends LunaNotification {
    count: number;
  }

  function dateValue(value: string): number {
    return Date.parse(String(value || "").replace(/\//g, "-")) || 0;
  }

  // Collapse notifications that belong to the same conversation (Luna reuses one
  // url per message thread) into a single entry, keeping the latest update and a
  // running count of how many notifications it absorbed.
  const messages = $derived.by<MessageThread[]>(() => {
    const threads = new Map<string, MessageThread>();
    for (const n of updates) {
      if (!matchesCourse(n) || !isMessageNotification(n)) continue;
      const key = n.url || n.content;
      const existing = threads.get(key);
      if (!existing) {
        threads.set(key, { ...n, count: 1 });
      } else {
        existing.count += 1;
        if (dateValue(n.date) >= dateValue(existing.date)) {
          existing.date = n.date;
          existing.content = n.content;
          existing.module = n.module;
        }
      }
    }
    return [...threads.values()].sort((a, b) => dateValue(b.date) - dateValue(a.date));
  });

  function shortDate(value: string): string {
    const match = String(value || "").match(/(\d{4})\/(\d{1,2})\/(\d{1,2})/);
    return match ? `${Number(match[2])}/${Number(match[3])}` : value;
  }

  // Luna message notifications read like
  //   「・メッセージ(【金】個人発表スライドに対するコメント)で発言がありました。」
  //   「・メッセージ(【水】ミニレポート課題⑤返却)が追加されました。(2026/05/27 13:09)」
  // Pull out the thread title from the parentheses and turn the trailing verb
  // into a compact status so the card reads cleanly.
  function parseThread(content: string): { title: string; status: string } {
    const text = String(content || "").replace(/^[・·•\s]+/, "").trim();
    const match = text.match(/^メッセージ\s*[(（](.+?)[)）]\s*(.*)$/s);
    if (!match) return { title: text, status: "" };
    const rest = match[2];
    let status = "";
    if (/発言|返信|コメント/.test(rest)) status = "新しい発言";
    else if (/追加|登録|作成/.test(rest)) status = "新規";
    return { title: match[1].trim(), status };
  }

  function openMessage(n: LunaNotification): void {
    if (!n.url) return;
    void invoke("university_open_detail_window", {
      path: n.url,
      title: n.content || "メッセージ",
      courseName: courseName || n.course_info || null,
    }).catch((error) => console.error("Failed to open Luna message:", error));
  }

  const unsubscribe = onCacheUpdate<LunaNotification[]>("luna_updates", (fresh) => {
    updates = fresh ?? [];
  });

  onMount(() => {
    cachedBackendFetch<LunaNotification[]>("luna_updates")
      .then((fresh) => { updates = fresh ?? updates; })
      .catch(() => {});
  });

  onDestroy(unsubscribe);
</script>

{#if messages.length}
  <section class="section">
    <div class="section-head"><h2>メッセージ</h2><span>{messages.length}</span></div>
    <div class="grid">
      {#each messages as message (message.url || message.content)}
        {@const thread = parseThread(message.content)}
        <button class="item-card msg-card" type="button" onclick={() => openMessage(message)}>
          <span class="item-row">
            <span class="item-title">{thread.title}</span>
            {#if message.count > 1}<span class="msg-count">{message.count}</span>{/if}
          </span>
          <span class="msg-meta">
            {#if thread.status}<span class="msg-status">{thread.status}</span>{/if}
            <span class="item-sub">{shortDate(message.date)}</span>
          </span>
        </button>
      {/each}
    </div>
  </section>
{/if}

<style>
  .msg-card {
    gap: 8px;
  }

  .msg-card .item-title {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .msg-meta {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .msg-status {
    flex: none;
    padding: 1px 8px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 650;
    color: var(--detail-accent);
    background: color-mix(in srgb, var(--detail-accent) 14%, transparent);
  }

  .msg-count {
    flex: none;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    color: var(--detail-muted);
    background: color-mix(in srgb, var(--detail-text) 9%, transparent);
  }
</style>
