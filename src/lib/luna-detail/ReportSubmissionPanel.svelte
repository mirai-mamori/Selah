<script lang="ts">
  import Icon from "../Icon.svelte";

  interface Props {
    reportType: string;
    loading: boolean;
    unavailable: string;
    text: string;
    file: File | null;
    busy: boolean;
    submitted: boolean;
    progress: number;
    status: string;
    canSubmit: boolean;
    ontextchange: (value: string) => void;
    onfilechange: (file: File | null) => void;
    onclearfile: () => void;
    onsubmit: () => void | Promise<void>;
  }

  let {
    reportType,
    loading,
    unavailable,
    text,
    file,
    busy,
    submitted,
    progress,
    status,
    canSubmit,
    ontextchange,
    onfilechange,
    onclearfile,
    onsubmit,
  }: Props = $props();

  let dragging = $state(false);
  const allowsText = $derived(reportType === "text" || reportType === "both");
  const allowsFile = $derived(reportType === "file" || reportType === "both");
  const disabled = $derived(busy || submitted);

  function formatFileSize(bytes: number): string {
    if (bytes < 1024) return `${bytes}B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
  }

  function selectFile(event: Event): void {
    const input = event.currentTarget as HTMLInputElement;
    onfilechange(input.files?.[0] || null);
    input.value = "";
  }

  function dropFile(event: DragEvent): void {
    event.preventDefault();
    dragging = false;
    if (disabled) return;
    onfilechange(event.dataTransfer?.files?.[0] || null);
  }
</script>

<section class="panel">
  <h2>課題提出</h2>

  {#if loading}
    <div class="state"><span class="spinner"></span>提出フォームを確認中...</div>
  {:else if unavailable}
    <div class="unavailable">{unavailable}</div>
  {:else if reportType}
    <div class="form">
      {#if allowsText}
        <label class="field">
          <span>テキスト入力</span>
          <textarea
            rows="6"
            placeholder="ここに提出テキストを入力..."
            value={text}
            oninput={(event) => ontextchange((event.currentTarget as HTMLTextAreaElement).value)}
            {disabled}
          ></textarea>
        </label>
      {/if}

      {#if allowsText && allowsFile}
        <div class="divider"><span>または</span></div>
      {/if}

      {#if allowsFile}
        {#if file}
          <div class="file-selected">
            <Icon name="paperclip" size={15} />
            <span>{file.name}</span>
            <small>{formatFileSize(file.size)}</small>
            <button type="button" title="添付を削除" aria-label={`${file.name}を削除`} {disabled} onclick={onclearfile}>
              <Icon name="xmark" size={12} />
            </button>
          </div>
        {:else}
          <label
            class:dragging
            class="drop"
            ondragover={(event) => { event.preventDefault(); if (!disabled) dragging = true; }}
            ondragleave={() => dragging = false}
            ondrop={dropFile}
          >
            <Icon name="paperclip" size={19} />
            <span>ファイルを選択またはドラッグ&ドロップ</span>
            <small>PDF, Word, Excel, PPT, 画像, ZIP (最大100MB)</small>
            <input
              type="file"
              accept=".pdf,.doc,.docx,.ppt,.pptx,.xls,.xlsx,.txt,.zip,.jpg,.jpeg,.png"
              {disabled}
              onchange={selectFile}
            />
          </label>
        {/if}
      {/if}

      <div class="actions">
        {#if status}<span class:error={status.includes("エラー")}>{status}</span>{/if}
        <button type="button" disabled={!canSubmit} onclick={() => void onsubmit()}>
          <Icon name="square.and.arrow.up" size={15} />
          <span>{busy ? "提出中..." : submitted ? "提出済み" : "提出する"}</span>
        </button>
      </div>

      {#if progress > 0}
        <div class="progress" aria-label={`提出進捗 ${progress}%`}>
          <span style={`width:${progress}%`}></span>
        </div>
      {/if}
    </div>
  {:else}
    <div class="state">提出フォームを利用できません。</div>
  {/if}
</section>

<style>
  .panel {
    display: grid;
    gap: 12px;
    padding: 16px;
    border: 0.5px solid var(--detail-border, rgba(0,0,0,0.08));
    border-radius: 8px;
    background: var(--detail-card, rgba(255,255,255,0.82));
    box-shadow: var(--detail-shadow, 0 1px 2px rgba(0,0,0,0.035));
  }

  h2 {
    margin: 0;
    color: var(--detail-text, #1d1d1f);
    font-size: 17px;
    letter-spacing: 0;
  }

  .state {
    min-height: 92px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 9px;
    color: var(--detail-muted, #686e78);
    font-size: 13px;
  }

  .spinner {
    width: 17px;
    height: 17px;
    border: 2px solid color-mix(in srgb, var(--detail-accent, #173b68) 14%, transparent);
    border-top-color: var(--detail-accent, #173b68);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .form {
    display: grid;
    gap: 12px;
  }

  .field {
    display: grid;
    gap: 7px;
    color: var(--detail-muted, #475569);
    font-size: 12px;
    font-weight: 800;
  }

  textarea {
    width: 100%;
    min-height: 126px;
    box-sizing: border-box;
    border: 0.5px solid var(--detail-border, rgba(0,0,0,0.12));
    border-radius: 8px;
    background: var(--detail-surface, #fff);
    color: var(--detail-text, #1d1d1f);
    padding: 10px 11px;
    font: inherit;
    line-height: 1.55;
    resize: vertical;
    outline: none;
  }

  textarea:focus {
    border-color: color-mix(in srgb, var(--detail-accent, #173b68) 42%, transparent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--detail-accent, #173b68) 12%, transparent);
  }

  textarea::placeholder {
    color: var(--detail-faint, #a3a7ae);
  }

  .divider {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--detail-faint, #94a3b8);
    font-size: 12px;
    font-weight: 750;
  }

  .divider::before,
  .divider::after {
    content: "";
    height: 0.5px;
    flex: 1;
    background: var(--detail-border, rgba(0,0,0,0.1));
  }

  .drop {
    position: relative;
    min-height: 104px;
    display: grid;
    place-items: center;
    align-content: center;
    gap: 7px;
    border: 1px dashed color-mix(in srgb, var(--detail-accent, #173b68) 25%, transparent);
    border-radius: 8px;
    background: color-mix(in srgb, var(--detail-surface, #fff) 76%, transparent);
    color: var(--detail-muted, #334155);
    text-align: center;
    cursor: pointer;
  }

  .drop.dragging {
    border-color: var(--detail-accent, #173b68);
    background: color-mix(in srgb, var(--detail-accent, #173b68) 8%, transparent);
  }

  .drop span {
    font-size: 13px;
    font-weight: 800;
  }

  .drop small,
  .file-selected small {
    color: var(--detail-faint, #64748b);
    font-size: 11px;
    font-weight: 600;
  }

  .drop input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }

  .file-selected {
    min-height: 40px;
    display: flex;
    align-items: center;
    gap: 9px;
    border-radius: 8px;
    background: color-mix(in srgb, var(--detail-text, #1d1d1f) 5%, transparent);
    padding: 8px 10px;
  }

  .file-selected > span {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--detail-text, #1d1d1f);
    font-size: 13px;
    font-weight: 750;
  }

  .file-selected button {
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--detail-muted, #475569);
    padding: 0;
    cursor: pointer;
  }

  .file-selected button:hover:not(:disabled) {
    background: color-mix(in srgb, var(--detail-text, #1d1d1f) 9%, transparent);
    color: var(--detail-text, #1d1d1f);
  }

  .actions {
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
  }

  .actions > span {
    min-width: 0;
    margin-right: auto;
    color: var(--detail-muted, #64748b);
    font-size: 12px;
    font-weight: 650;
  }

  .actions > span.error {
    color: var(--detail-danger, #b42318);
  }

  .actions button {
    min-width: 104px;
    min-height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    border: 0.5px solid var(--detail-accent, #173b68);
    border-radius: 7px;
    background: var(--detail-accent, #173b68);
    color: var(--detail-accent-contrast, #fff);
    padding: 0 12px;
    font: inherit;
    font-size: 12px;
    font-weight: 750;
    cursor: pointer;
  }

  .actions button:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .actions button:disabled,
  .file-selected button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .progress {
    height: 5px;
    overflow: hidden;
    border-radius: 999px;
    background: color-mix(in srgb, var(--detail-text, #1d1d1f) 10%, transparent);
  }

  .progress span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--detail-accent, #173b68);
    transition: width 0.18s ease;
  }

  .unavailable {
    border-radius: 8px;
    background: rgba(249,115,22,0.1);
    color: var(--detail-warn, #9a3412);
    padding: 10px 12px;
    font-size: 13px;
    font-weight: 700;
  }
</style>
