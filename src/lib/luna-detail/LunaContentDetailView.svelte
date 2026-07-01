<script lang="ts">
  import Icon from "../Icon.svelte";
  import AttachmentDownloadRow from "./AttachmentDownloadRow.svelte";
  import ReportSubmissionView from "./ReportSubmissionView.svelte";
  import type {
    DownloadMark,
    LunaAttachment,
    LunaDetailPage,
    LunaMaterialFile,
  } from "./types";

  interface Props {
    detail: LunaDetailPage;
    materialFiles: LunaMaterialFile[];
    mode: string;
    titleParam: string;
    idnumberParam: string;
    reportIdParam: string;
    pathParam: string;
    periodParam: string;
    richText: (value: string | undefined | null) => string;
    openOrDownloadMaterial: (file: LunaMaterialFile, title: string) => void | Promise<void>;
    getDownloadMark: (name: string) => DownloadMark;
    openExternal: (url: string, title?: string) => void | Promise<void>;
    attachmentName: (attachment: LunaAttachment) => string;
    downloadAttachment: (attachment: LunaAttachment) => void | Promise<void>;
    forceDownloadAttachment: (attachment: LunaAttachment) => void | Promise<void>;
  }

  let {
    detail,
    materialFiles,
    mode,
    titleParam,
    idnumberParam,
    reportIdParam,
    pathParam,
    periodParam,
    richText,
    openOrDownloadMaterial,
    getDownloadMark,
    openExternal,
    attachmentName,
    downloadAttachment,
    forceDownloadAttachment,
  }: Props = $props();

  const fileAttachments = $derived((detail.attachments || []).filter((attachment) => !attachment.link_type || attachment.link_type === "file"));
  const linkAttachments = $derived((detail.attachments || []).filter((attachment) => !!attachment.link_type && attachment.link_type !== "file"));
  const materialDownloadFiles = $derived(materialFiles.filter((file) => (file.link_type || (file.file_type === "0" ? "file" : "web")) === "file"));
  const materialLinkFiles = $derived(materialFiles.filter((file) => (file.link_type || (file.file_type === "0" ? "file" : "web")) !== "file"));

  function linkBadge(linkType: string | undefined): string {
    if (linkType === "zoom") return "Zoom";
    if (linkType === "panopto") return "Panopto";
    if (linkType === "video") return "動画";
    if (linkType === "cloud") return "Cloud";
    if (linkType === "google") return "Google";
    if (linkType === "teams") return "Teams";
    return "";
  }
</script>

<article class="detail-wrap">
  {#if detail.course_name}<div class="course-label">{detail.course_name}</div>{/if}
  <h1>{detail.title}</h1>
  {#each detail.sections || [] as section}
    <section class="body-section">
      {#if section.heading}<h2>{section.heading}</h2>{/if}
      <div class="rich">{@html richText(section.body)}</div>
    </section>
  {/each}
  {#if detail.meta?.length}
    <div class="meta-table">
      {#each detail.meta as row}
        <div><span>{row[0]}</span><strong class="rich compact">{@html richText(row[1] || "-")}</strong></div>
      {/each}
    </div>
  {/if}
  {#if materialDownloadFiles.length}
    <div class="attachments">
      <h2>添付ファイル ({materialDownloadFiles.length})</h2>
      {#each materialDownloadFiles as file}
        <button type="button" onclick={() => openOrDownloadMaterial(file, detail.title || titleParam)}>
          <Icon name="paperclip" size={15} />
          <span>{file.display_name || file.file_name}</span>
          {#if getDownloadMark(file.display_name || file.file_name).path}<Icon name="checkmark.circle" size={15} />{/if}
          {#if !getDownloadMark(file.display_name || file.file_name).path}<Icon name="arrow.down.circle" size={15} />{/if}
        </button>
      {/each}
    </div>
  {/if}
  {#if materialLinkFiles.length}
    <div class="attachments">
      <h2>リンク ({materialLinkFiles.length})</h2>
      {#each materialLinkFiles as file}
        {@const fileLinkType = file.link_type || "web"}
        <button type="button" onclick={() => openOrDownloadMaterial(file, detail.title || titleParam)}>
          <Icon name="arrow.up.right.square" size={15} />
          <span>
            {#if linkBadge(fileLinkType)}<strong class="link-badge">{linkBadge(fileLinkType)}</strong>{/if}
            {file.display_name || file.file_name}
          </span>
          <Icon name="arrow.up.right.square" size={15} />
        </button>
      {/each}
    </div>
  {/if}
  {#if fileAttachments.length}
    <div class="attachments">
      <h2>添付ファイル</h2>
      {#each fileAttachments as attachment}
        <AttachmentDownloadRow
          name={attachmentName(attachment)}
          mark={getDownloadMark(attachmentName(attachment))}
          onopen={() => downloadAttachment(attachment)}
          onredownload={() => forceDownloadAttachment(attachment)}
        />
      {/each}
    </div>
  {/if}
  {#if linkAttachments.length}
    <div class="attachments">
      <h2>リンク</h2>
      {#each linkAttachments as attachment}
        <button type="button" onclick={() => openExternal(attachment.url || "", attachmentName(attachment))}>
          <Icon name="arrow.up.right.square" size={15} />
          <span>
            {#if linkBadge(attachment.link_type)}<strong class="link-badge">{linkBadge(attachment.link_type)}</strong>{/if}
            {attachmentName(attachment)}
          </span>
          <Icon name="arrow.up.right.square" size={15} />
        </button>
      {/each}
    </div>
  {/if}
  {#if !(detail.sections?.length || detail.meta?.length || detail.attachments?.length)}
    <div class="empty-state">詳細情報を取得できませんでした</div>
  {/if}
  {#if mode === "report"}
    <ReportSubmissionView
      {idnumberParam}
      {reportIdParam}
      {pathParam}
      {periodParam}
      {openExternal}
    />
  {/if}
</article>
