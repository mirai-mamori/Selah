<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import AttachmentDownloadRow from "./AttachmentDownloadRow.svelte";
  import MessageComposer from "./MessageComposer.svelte";
  import { lunaDraftKey, writeDraft } from "./drafts";
  import { fileToPayload } from "./filePayload";
  import type { DownloadMark, LunaAttachment, LunaInquiryDetail } from "./types";

  interface Props {
    inquiry: LunaInquiryDetail;
    pathParam: string;
    titleParam: string;
    richText: (value: string | undefined | null) => string;
    attachmentName: (attachment: LunaAttachment) => string;
    downloadAttachment: (attachment: LunaAttachment) => void | Promise<void>;
    forceDownloadAttachment: (attachment: LunaAttachment) => void | Promise<void>;
    getDownloadMark: (name: string) => DownloadMark;
    reportStatus: (message: string, isError?: boolean) => void;
    onupdate: (inquiry: LunaInquiryDetail) => void;
  }

  let {
    inquiry,
    pathParam,
    titleParam,
    richText,
    attachmentName,
    downloadAttachment,
    forceDownloadAttachment,
    getDownloadMark,
    reportStatus,
    onupdate,
  }: Props = $props();
  let content = $state("");
  let files = $state<File[]>([]);
  let posting = $state(false);
  const replyDraftKey = $derived(lunaDraftKey(["inquiry-reply", inquiry.idnumber, inquiry.inquiry_id]));

  async function postReply(): Promise<void> {
    if (!pathParam || (!content.trim() && !files.length) || posting) return;
    posting = true;
    try {
      const result = await invoke<string>("luna_reply_inquiry", {
        url: pathParam,
        content: content.trim(),
        attachment: files[0] ? await fileToPayload(files[0]) : null,
      });
      reportStatus(result || "送信しました");
      content = "";
      files = [];
      writeDraft(replyDraftKey, "");
      onupdate(await invoke<LunaInquiryDetail>("luna_fetch_inquiry_detail", { path: pathParam }));
    } catch (error) {
      reportStatus(`送信エラー: ${String(error)}`, true);
    } finally {
      posting = false;
    }
  }
</script>

<article class="detail-wrap">
  {#if inquiry.course_name}<div class="course-label">{inquiry.course_name}</div>{/if}
  <h1>{inquiry.title || titleParam}</h1>
  <div class="post-list">
    {#each inquiry.posts || [] as post}
      <section class:post-self={post.is_self} class:post-teacher={post.is_teacher} class="post">
        <div class="post-meta">
          {#if post.is_teacher}<em class="teacher">教員</em>{/if}
          {#if post.is_self}<em class="self">自分</em>{/if}
          <strong>{post.author}</strong><span>{post.date}</span>
        </div>
        <div class="rich">{@html richText(post.content_html || post.content_text)}</div>
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
      </section>
    {/each}
    {#if !inquiry.posts?.length}<div class="empty-state">まだメッセージがありません</div>{/if}
  </div>
  {#if inquiry.post_action && (inquiry.idnumber || inquiry.inquiry_id)}
    <MessageComposer
      heading="返信"
      placeholder="メッセージを入力..."
      busy={posting}
      submitDisabled={!content.trim() && !files.length}
      showFileNameInSizeError={false}
      draftKey={replyDraftKey}
      bind:content
      bind:files
      onsubmit={postReply}
    />
  {/if}
</article>
