<script lang="ts">
  interface Props {
    summary: string;
    renderMd: (text: string) => string;
    onClose: () => void;
  }

  let { summary, renderMd, onClose }: Props = $props();
</script>

{#if summary}
  <section class="overall-summary-card" aria-label="現在までの全体要約">
    <header>
      <span class="badge">現在までの全体要約</span>
      <button type="button" onclick={onClose} aria-label="全体要約を閉じる">閉じる</button>
    </header>
    <div class="md">{@html renderMd(summary)}</div>
  </section>
{/if}

<style>
  .overall-summary-card {
    margin-bottom: 14px;
    padding: 12px 16px;
    border: 0.5px solid rgba(52, 199, 89, 0.24);
    border-radius: 14px;
    background: color-mix(in srgb, var(--bg-card) 92%, rgba(52, 199, 89, 0.08));
    box-shadow: 0 2px 16px rgba(52, 199, 89, 0.07);
    color: var(--text-primary);
    animation: summary-enter 0.28s cubic-bezier(0.22, 1, 0.36, 1) both;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 6px;
  }
  .badge {
    font-size: 12px;
    font-weight: 700;
    color: color-mix(in srgb, var(--green, #34c759) 74%, var(--text-primary));
  }
  button {
    all: unset;
    cursor: pointer;
    padding: 2px 6px;
    border-radius: 5px;
    color: var(--text-tertiary);
    font-size: 10px;
  }
  button:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 6%, transparent);
  }
  .md {
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 1.65;
  }
  .md :global(h3) {
    margin: 10px 0 4px;
    color: var(--text-primary);
    font-size: 13px;
  }
  .md :global(h3:first-child) {
    display: none;
  }
  .md :global(p),
  .md :global(ul) {
    margin: 4px 0;
  }
  @keyframes summary-enter {
    from { opacity: 0; transform: translateY(-6px); }
    to { opacity: 1; transform: translateY(0); }
  }
</style>
