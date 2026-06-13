<script lang="ts">
  import Icon, { type IconName } from "./Icon.svelte";

  let {
    icon,
    size = 15,
    title,
    disabled = false,
    onclick,
  }: {
    icon: IconName;
    size?: number;
    title: string;
    disabled?: boolean;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<button
  type="button"
  {title}
  aria-label={title}
  {disabled}
  {onclick}
>
  <Icon name={icon} {size} />
</button>

<style>
  button {
    width: 28px;
    height: 28px;
    flex: 0 0 28px;
    display: inline-grid;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: 10px;
    color: var(--agent-text-3, var(--text-tertiary));
    background: transparent;
    cursor: pointer;
    transition: background 0.15s, color 0.15s, opacity 0.15s, transform 0.1s;
    -webkit-app-region: no-drag;
  }

  button:hover:not(:disabled) {
    color: var(--agent-text, var(--text-primary));
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  button:active:not(:disabled) {
    transform: scale(0.96);
  }

  button:disabled {
    opacity: 0.35;
    cursor: default;
  }
</style>
