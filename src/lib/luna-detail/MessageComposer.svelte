<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "../Icon.svelte";
  import { readDraft, writeDraft } from "./drafts";

  interface Props {
    heading: string;
    content?: string;
    files?: File[];
    subject?: string;
    subjectPlaceholder?: string;
    placeholder?: string;
    submitLabel?: string;
    busyLabel?: string;
    busy?: boolean;
    disabled?: boolean;
    submitDisabled?: boolean;
    multiple?: boolean;
    accept?: string;
    rows?: number;
    maxFiles?: number;
    maxFileSize?: number;
    maxFileNameLength?: number;
    showFileNameInSizeError?: boolean;
    draftKey?: string;
    subjectDraftKey?: string;
    onsubmit: () => void | Promise<void>;
  }

  let {
    heading,
    content = $bindable(""),
    files = $bindable<File[]>([]),
    subject = $bindable(""),
    subjectPlaceholder = "",
    placeholder = "メッセージ",
    submitLabel = "送信",
    busyLabel = "送信中...",
    busy = false,
    disabled = false,
    submitDisabled = false,
    multiple = false,
    accept = "",
    rows = 4,
    maxFiles = 10,
    maxFileSize = 100 * 1024 * 1024,
    maxFileNameLength = 60,
    showFileNameInSizeError = true,
    draftKey = "",
    subjectDraftKey = "",
    onsubmit,
  }: Props = $props();
  let draftReady = $state(false);
  let fileError = $state("");

  $effect(() => {
    if (!draftReady) return;
    writeDraft(draftKey, content);
    writeDraft(subjectDraftKey, subject);
  });

  function selectFiles(event: Event): void {
    const input = event.currentTarget as HTMLInputElement;
    const selected = Array.from(input.files || []);
    fileError = "";
    if (!multiple) {
      const file = selected[0];
      if (file) {
        const error = validateFile(file);
        if (error) fileError = error;
        else files = [file];
      }
      input.value = "";
      return;
    }

    const next = [...files];
    for (const file of selected) {
      if (next.length >= maxFiles) {
        fileError = `添付ファイルは${maxFiles}個以下にしてください。`;
        break;
      }
      const error = validateFile(file);
      if (error) {
        fileError = error;
        continue;
      }
      next.push(file);
    }
    files = next;
    input.value = "";
  }

  function removeFile(index: number): void {
    files = files.filter((_, fileIndex) => fileIndex !== index);
    fileError = "";
  }

  function validateFile(file: File): string {
    if (file.size <= 0) return "ファイルサイズが0バイトです。";
    if (file.size > maxFileSize) {
      return showFileNameInSizeError
        ? `「${file.name}」は最大サイズ（${formatFileSize(maxFileSize)}）を超えています。`
        : `${formatFileSize(maxFileSize)}を超えています。`;
    }
    if (file.name.length > maxFileNameLength) return `ファイル名は${maxFileNameLength}文字以下にしてください。`;
    if (/[*|~:;"%?</>\\]/.test(file.name)) return "ファイル名に使用できない文字が含まれています。";
    return "";
  }

  function formatFileSize(bytes: number): string {
    if (bytes < 1024) return `${bytes}B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
    return `${Math.round(bytes / (1024 * 1024))}MB`;
  }

  function handleKeydown(event: KeyboardEvent): void {
    if ((event.metaKey || event.ctrlKey) && event.key === "Enter" && !busy && !disabled && !submitDisabled) {
      event.preventDefault();
      void onsubmit();
    }
  }

  onMount(() => {
    if (draftKey && !content) content = readDraft(draftKey);
    if (subjectDraftKey && !subject) subject = readDraft(subjectDraftKey);
    draftReady = true;
  });
</script>

<section class="composer">
  <h2>{heading}</h2>

  {#if subjectPlaceholder}
    <input
      class="subject"
      type="text"
      placeholder={subjectPlaceholder}
      bind:value={subject}
      disabled={busy || disabled}
    />
  {/if}

  <textarea
    {rows}
    {placeholder}
    bind:value={content}
    disabled={busy || disabled}
    onkeydown={handleKeydown}
  ></textarea>

  <div class="actions">
    <label class:disabled={busy || disabled} class="attach" title="添付ファイルを選択">
      <Icon name="paperclip" size={15} />
      <span>添付</span>
      <input type="file" {multiple} {accept} disabled={busy || disabled} onchange={selectFiles} />
    </label>

    {#each files as file, index (`${file.name}-${file.size}-${file.lastModified}`)}
      <span class="file-chip" title={file.name}>
        <Icon name="doc" size={13} />
        <span>{file.name}</span>
        <small>{formatFileSize(file.size)}</small>
        <button type="button" title="添付を削除" aria-label={`${file.name}を削除`} disabled={busy || disabled} onclick={() => removeFile(index)}>
          <Icon name="xmark" size={11} />
        </button>
      </span>
    {/each}

    {#if multiple}<span class="file-hint">最大{maxFiles}件 / {formatFileSize(maxFileSize)}</span>{/if}
    <span class="fill"></span>

    <button
      class="submit"
      type="button"
      disabled={busy || disabled || submitDisabled}
      onclick={() => void onsubmit()}
    >
      <Icon name="square.and.arrow.up" size={15} />
      <span>{busy ? busyLabel : submitLabel}</span>
    </button>
  </div>
  {#if fileError}<div class="file-error" role="alert">{fileError}</div>{/if}
</section>

<style>
  .composer {
    display: grid;
    gap: 10px;
    padding: 14px;
    border: 0.5px solid var(--detail-border, rgba(0,0,0,0.08));
    border-radius: 8px;
    background: var(--detail-card, rgba(255,255,255,0.92));
    box-shadow: var(--detail-shadow, 0 1px 2px rgba(0,0,0,0.035));
  }

  h2 {
    margin: 0;
    color: var(--detail-text, #1d1d1f);
    font-size: 17px;
    letter-spacing: 0;
  }

  .subject,
  textarea {
    width: 100%;
    box-sizing: border-box;
    border: 0.5px solid var(--detail-border, rgba(0,0,0,0.12));
    border-radius: 8px;
    background: var(--detail-surface, #fff);
    color: var(--detail-text, #1d1d1f);
    padding: 10px 11px;
    font: inherit;
    font-size: 13px;
    outline: none;
    transition: border-color 0.15s ease, box-shadow 0.15s ease, background 0.15s ease;
  }

  textarea {
    min-height: 104px;
    max-height: 240px;
    resize: vertical;
    line-height: 1.55;
  }

  .subject:focus,
  textarea:focus {
    border-color: color-mix(in srgb, var(--detail-accent, #173b68) 42%, transparent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--detail-accent, #173b68) 12%, transparent);
  }

  .subject::placeholder,
  textarea::placeholder {
    color: var(--detail-faint, #a3a7ae);
  }

  .actions {
    min-width: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .attach,
  .submit,
  .file-chip {
    min-height: 30px;
    box-sizing: border-box;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 750;
  }

  .attach {
    position: relative;
    padding: 0 9px;
    background: transparent;
    color: var(--detail-muted, #686e78);
    cursor: pointer;
  }

  .attach:hover {
    background: color-mix(in srgb, var(--detail-text, #1d1d1f) 6%, transparent);
    color: var(--detail-text, #1d1d1f);
  }

  .attach.disabled {
    opacity: 0.45;
    cursor: default;
  }

  .attach input {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    opacity: 0;
    pointer-events: none;
  }

  .file-chip {
    min-width: 0;
    max-width: min(260px, 42vw);
    padding: 0 5px 0 8px;
    background: color-mix(in srgb, var(--detail-text, #1d1d1f) 6%, transparent);
    color: var(--detail-muted, #686e78);
  }

  .file-chip > span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file-chip small,
  .file-hint {
    color: var(--detail-faint, #8b929c);
    font-size: 11px;
    font-weight: 650;
    white-space: nowrap;
  }

  .file-chip button {
    width: 22px;
    height: 22px;
    flex: 0 0 22px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: inherit;
    padding: 0;
    cursor: pointer;
  }

  .file-chip button:hover:not(:disabled) {
    background: color-mix(in srgb, var(--detail-text, #1d1d1f) 9%, transparent);
    color: var(--detail-text, #1d1d1f);
  }

  .fill {
    flex: 1 1 24px;
  }

  .file-error {
    color: #b42318;
    font-size: 12px;
    font-weight: 650;
  }

  .submit {
    min-width: 84px;
    justify-content: center;
    border: 0.5px solid var(--detail-accent, #173b68);
    background: var(--detail-accent, #173b68);
    color: var(--detail-accent-contrast, #fff);
    padding: 0 12px;
    cursor: pointer;
  }

  .submit:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .submit:active:not(:disabled) {
    transform: scale(0.98);
  }

  .submit:disabled,
  .file-chip button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  @media (max-width: 620px) {
    .file-chip {
      max-width: calc(100vw - 80px);
    }

    .fill {
      flex-basis: 100%;
    }

    .submit {
      margin-left: auto;
    }
  }
</style>
