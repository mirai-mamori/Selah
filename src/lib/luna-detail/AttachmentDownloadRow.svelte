<script lang="ts">
  import Icon from "../Icon.svelte";
  import type { DownloadMark } from "./types";

  interface Props {
    name: string;
    mark: DownloadMark;
    iconSize?: number;
    onopen: () => void | Promise<void>;
    onredownload: () => void | Promise<void>;
  }

  let { name, mark, iconSize = 15, onopen, onredownload }: Props = $props();
</script>

<div class="attachment-download-row">
  <button class="attachment-main" type="button" disabled={mark.loading} onclick={onopen}>
    <Icon name="paperclip" size={iconSize} />
    <span>{mark.loading ? "ダウンロード中..." : name}</span>
    {#if mark.path}<Icon name="checkmark.circle" size={iconSize} />{/if}
    {#if !mark.path && !mark.loading}<Icon name="arrow.down.circle" size={iconSize} />{/if}
  </button>
  {#if mark.path}
    <button
      class="attachment-redownload"
      type="button"
      title="再ダウンロード"
      aria-label={`${name}を再ダウンロード`}
      onclick={onredownload}
    >
      <Icon name="arrow.clockwise" size={iconSize} />
    </button>
  {/if}
</div>
