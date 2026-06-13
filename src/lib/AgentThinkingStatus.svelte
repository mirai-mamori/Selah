<script lang="ts">
  let { text = "" }: { text?: string } = $props();
</script>

<div class="thinking-status" aria-live="polite" aria-label={text || "計画中"}>
  {#if text}
    <span class="thinking-text">{text}</span>
  {:else}
    <span class="thinking-dots" aria-hidden="true">
      <span></span>
      <span></span>
      <span></span>
    </span>
  {/if}
</div>

<style>
  .thinking-status {
    --thinking-accent: var(--agent-accent, var(--accent, #2f6598));
    --thinking-text: var(--agent-text-2, var(--text-secondary, #636366));
    width: 100%;
    min-width: 0;
    min-height: 30px;
    display: flex;
    align-items: center;
    justify-content: flex-start;
    color: var(--thinking-text);
    text-align: left;
  }

  .thinking-text {
    width: 100%;
    min-width: 0;
    overflow: hidden;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    color: var(--thinking-text);
    background: linear-gradient(
      105deg,
      var(--thinking-text) 0%,
      var(--thinking-text) 38%,
      color-mix(in srgb, var(--thinking-accent) 45%, white) 48%,
      var(--thinking-text) 58%,
      var(--thinking-text) 100%
    );
    background-size: 240% 100%;
    background-position: 130% 0;
    background-clip: text;
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    font-size: 12px;
    font-weight: 600;
    line-height: 1.45;
    overflow-wrap: anywhere;
    animation: thinking-scan 2.2s ease-in-out infinite;
  }

  .thinking-dots {
    height: 18px;
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .thinking-dots span {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--thinking-accent);
    animation: thinking-dot 1.25s ease-in-out infinite;
  }

  .thinking-dots span:nth-child(2) {
    animation-delay: 0.16s;
  }

  .thinking-dots span:nth-child(3) {
    animation-delay: 0.32s;
  }

  @keyframes thinking-scan {
    0%, 18% { background-position: 130% 0; }
    78%, 100% { background-position: -45% 0; }
  }

  @keyframes thinking-dot {
    0%, 70%, 100% {
      opacity: 0.28;
      transform: translateY(0);
    }
    35% {
      opacity: 0.95;
      transform: translateY(-2px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .thinking-text {
      background-position: 0 0;
      animation: none;
    }

    .thinking-dots span {
      animation: none;
      opacity: 0.65;
    }
  }
</style>
