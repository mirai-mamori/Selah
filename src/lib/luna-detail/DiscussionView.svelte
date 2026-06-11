<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "../Icon.svelte";
  import AttachmentDownloadRow from "./AttachmentDownloadRow.svelte";
  import MessageComposer from "./MessageComposer.svelte";
  import { lunaDraftKey, writeDraft } from "./drafts";
  import { filesToPayload } from "./filePayload";
  import type { DownloadMark, LunaAttachment, LunaDiscussionPost, LunaDiscussionThread } from "./types";

  interface Props {
    discussion: LunaDiscussionThread;
    mode: string;
    pathParam: string;
    titleParam: string;
    idnumberParam: string;
    courseName: string;
    split?: boolean;
    richText: (value: string | undefined | null) => string;
    attachmentName: (attachment: LunaAttachment) => string;
    downloadAttachment: (attachment: LunaAttachment) => void | Promise<void>;
    forceDownloadAttachment: (attachment: LunaAttachment) => void | Promise<void>;
    getDownloadMark: (name: string) => DownloadMark;
    reportStatus: (message: string, isError?: boolean) => void;
    onupdate: (discussion: LunaDiscussionThread) => void;
  }

  let {
    discussion,
    mode,
    pathParam,
    titleParam,
    idnumberParam,
    courseName,
    split = false,
    richText,
    attachmentName,
    downloadAttachment,
    forceDownloadAttachment,
    getDownloadMark,
    reportStatus,
    onupdate,
  }: Props = $props();

  let newThreadTitle = $state("");
  let newThreadContent = $state("");
  let newThreadFiles = $state<File[]>([]);
  let replyContent = $state("");
  let replyFiles = $state<File[]>([]);
  let inlineReplyContent = $state("");
  let inlineReplyFiles = $state<File[]>([]);
  let posting = $state(false);
  let selectedThread = $state<LunaDiscussionPost | null>(null);
  let replyParentId = $state<string | null>(null);

  // The board page title is a generic "掲示板 テーマトップ"; prefer the actual
  // theme title carried in the meta rows.
  const themeTitleRow = $derived(
    (discussion.meta || []).find(([label]) => String(label).replace(/\s+/g, "").includes("テーマタイトル")),
  );
  const headerTitle = $derived((themeTitleRow?.[1] || "").trim() || discussion.title || titleParam);
  const visibleMeta = $derived((discussion.meta || []).filter((row) => row !== themeTitleRow));

  const newThreadTitleDraft = $derived(lunaDraftKey(["discussion-new-thread", pathParam || window.location.search, "title"]));
  const newThreadContentDraft = $derived(lunaDraftKey(["discussion-new-thread", pathParam || window.location.search, "content"]));
  const rootReplyDraft = $derived(lunaDraftKey(["discussion-reply", pathParam, "root"]));

  function replyDraftKey(parentPostId: string): string {
    return lunaDraftKey(["discussion-reply", pathParam, parentPostId || "root"]);
  }

  function postFlags(post: LunaDiscussionPost): string[] {
    return String(post.status || "").split(",").map((flag) => flag.trim()).filter(Boolean);
  }

  function threadPreviewHtml(value: string | undefined | null): string {
    return richText(value)
      .replace(/<a\b[^>]*>/gi, "")
      .replace(/<\/a>/gi, "");
  }

  function toggleInlineReply(parentPostId: string): void {
    const closing = replyParentId === parentPostId;
    replyParentId = closing ? null : parentPostId;
    inlineReplyContent = "";
    inlineReplyFiles = [];
  }

  // A subtopic (スレッド) detail lives at /lms/course/forums/thread, not at the
  // themetop list path. Mirror the legacy renderer: swap the endpoint and carry
  // over idnumber/forumId, then pin threadId/screen. Appending threadId onto the
  // themetop path instead re-fetches the theme list, so the opened thread shows
  // the wrong (theme-level) content.
  function threadPathFor(threadId: string): string {
    const [base, query = ""] = (pathParam || "").split("?");
    const threadBase = base.replace("/forums/themetop", "/forums/thread");
    const params = new URLSearchParams(query);
    params.set("threadId", threadId);
    params.set("screen", "2");
    return `${threadBase}?${params.toString()}`;
  }

  async function openPost(post: LunaDiscussionPost): Promise<void> {
    if (!post.thread_id) {
      selectedThread = post;
      return;
    }
    await invoke("university_open_detail_window", {
      path: threadPathFor(post.thread_id),
      title: post.title || discussion.title || titleParam,
      mode: "thread",
      period: null,
      status: null,
      idnumber: idnumberParam || null,
      infoId: post.thread_id,
      kgcPath: null,
      courseName: courseName || null,
      split,
    }).catch((error) => reportStatus(String(error), true));
  }

  async function postNewThread(): Promise<void> {
    if (!pathParam || !newThreadTitle.trim() || posting || (!newThreadContent.trim() && !newThreadFiles.length)) return;
    posting = true;
    try {
      const result = await invoke<string>("luna_post_discussion", {
        url: pathParam,
        title: newThreadTitle.trim(),
        content: newThreadContent.trim(),
        attachments: newThreadFiles.length ? await filesToPayload(newThreadFiles) : null,
      });
      reportStatus(result || "投稿しました");
      newThreadTitle = "";
      newThreadContent = "";
      newThreadFiles = [];
      writeDraft(newThreadTitleDraft, "");
      writeDraft(newThreadContentDraft, "");
      onupdate(await invoke<LunaDiscussionThread>("luna_fetch_discussion_detail", { url: pathParam }));
    } catch (error) {
      reportStatus(`投稿エラー: ${String(error)}`, true);
    } finally {
      posting = false;
    }
  }

  async function postReply(
    parentPostId: string | null = null,
    content = replyContent,
    files = replyFiles,
  ): Promise<void> {
    if (!pathParam || posting || (!content.trim() && !files.length)) return;
    posting = true;
    try {
      const result = await invoke<string>("luna_reply_discussion", {
        url: pathParam,
        content: content.trim(),
        parentPostId,
        attachments: files.length ? await filesToPayload(files) : null,
      });
      reportStatus(result || "返信しました");
      if (parentPostId) {
        inlineReplyContent = "";
        inlineReplyFiles = [];
      } else {
        replyContent = "";
        replyFiles = [];
      }
      writeDraft(parentPostId ? replyDraftKey(parentPostId) : rootReplyDraft, "");
      replyParentId = null;
      onupdate(await invoke<LunaDiscussionThread>("luna_fetch_thread_posts", { url: pathParam }));
    } catch (error) {
      reportStatus(`返信エラー: ${String(error)}`, true);
    } finally {
      posting = false;
    }
  }
</script>

<article class="detail-wrap">
  {#if discussion.course_name}<div class="course-label">{discussion.course_name}</div>{/if}
  <h1>{headerTitle}</h1>
  {#if discussion.description}<div class="rich lead">{@html richText(discussion.description)}</div>{/if}
  {#if visibleMeta.length}
    <div class="meta-table">
      {#each visibleMeta as row}<div><span>{row[0]}</span><strong class="rich compact">{@html richText(row[1] || "—")}</strong></div>{/each}
    </div>
  {/if}

  {#if mode === "discussion"}
    {#if selectedThread}
      <button class="back-btn" type="button" onclick={() => selectedThread = null}>
        <Icon name="chevron.left" size={14} />
        <span>スレッド一覧</span>
      </button>
      <section class="thread-card selected">
        <h2>{selectedThread.title}</h2>
        <div class="post-meta">{selectedThread.author} {selectedThread.date}</div>
        <div class="rich">{@html richText(selectedThread.content)}</div>
      </section>
    {:else}
      <div class="thread-list">
        {#each discussion.posts || [] as post}
          <button class="thread-card" type="button" onclick={() => openPost(post)}>
            <strong>{post.title || "スレッド"}</strong>
            {#if post.content}<div class="thread-preview rich">{@html threadPreviewHtml(post.content)}</div>{/if}
            <small>{post.author} {post.date} {post.status}</small>
          </button>
        {/each}
      </div>
      <MessageComposer
        heading="新しいスレッド"
        subjectPlaceholder="タイトル..."
        placeholder="内容を入力..."
        submitLabel="投稿"
        busyLabel="投稿中..."
        busy={posting}
        submitDisabled={!newThreadTitle.trim() || (!newThreadContent.trim() && !newThreadFiles.length)}
        multiple
        subjectDraftKey={newThreadTitleDraft}
        draftKey={newThreadContentDraft}
        bind:subject={newThreadTitle}
        bind:content={newThreadContent}
        bind:files={newThreadFiles}
        onsubmit={postNewThread}
      />
    {/if}
  {:else}
    <div class="post-list">
      {#if discussion.posts?.length}<div class="post-list-header">投稿 ({discussion.posts.length})</div>{/if}
      {#each discussion.posts || [] as post}
        <section class="post">
          <div class="post-meta">
            {#if postFlags(post).includes("teacher")}<em class="teacher">教員</em>{/if}
            {#if postFlags(post).includes("self")}<em class="self">自分</em>{/if}
            <strong>{post.author}</strong><span>{post.date}</span>
          </div>
          <div class="rich">{@html richText(post.content)}</div>
          {#if post.attachments?.length}
            <div class="attachments inline">
              {#each post.attachments as attachment}
                <AttachmentDownloadRow
                  name={attachmentName(attachment)}
                  mark={getDownloadMark(attachmentName(attachment))}
                  iconSize={14}
                  onopen={() => downloadAttachment(attachment)}
                  onredownload={() => forceDownloadAttachment(attachment)}
                />
              {/each}
            </div>
          {/if}
          {#if post.thread_id}
            <div class="post-actions">
              <button type="button" onclick={() => toggleInlineReply(post.thread_id)}>
                {replyParentId === post.thread_id ? "返信を閉じる" : "返信"}
              </button>
            </div>
            {#if replyParentId === post.thread_id}
              <MessageComposer
                heading={`${post.author || "投稿"}へ返信`}
                placeholder="返信内容を入力..."
                busy={posting}
                submitDisabled={!inlineReplyContent.trim() && !inlineReplyFiles.length}
                multiple
                draftKey={replyDraftKey(post.thread_id)}
                bind:content={inlineReplyContent}
                bind:files={inlineReplyFiles}
                onsubmit={() => postReply(post.thread_id, inlineReplyContent, inlineReplyFiles)}
              />
            {/if}
          {/if}
        </section>
      {/each}
      {#if !discussion.posts?.length}<div class="empty-state">まだ投稿がありません</div>{/if}
    </div>
    <MessageComposer
      heading="返信"
      placeholder="返信内容を入力..."
      busy={posting}
      submitDisabled={!replyContent.trim() && !replyFiles.length}
      multiple
      draftKey={rootReplyDraft}
      bind:content={replyContent}
      bind:files={replyFiles}
      onsubmit={postReply}
    />
  {/if}
</article>

<style>
  .thread-preview {
    color: #64748b;
    font-size: 13px;
    line-height: 1.55;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    word-break: break-word;
  }

  .thread-preview :global(p),
  .thread-preview :global(ul),
  .thread-preview :global(ol),
  .thread-preview :global(blockquote),
  .thread-preview :global(pre) {
    display: inline;
    margin: 0;
    padding: 0;
  }

  .thread-preview :global(li) {
    display: inline;
  }

  .post-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 8px;
  }

  .post-actions button {
    padding: 5px 9px;
    border: 0;
    border-radius: 6px;
    color: var(--detail-muted);
    background: transparent;
    font: inherit;
    font-size: 12px;
    font-weight: 750;
    cursor: pointer;
  }

  .post-actions button:hover {
    color: var(--detail-text);
    background: var(--detail-hover);
  }
</style>
