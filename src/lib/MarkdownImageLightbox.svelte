<script lang="ts">
  interface Props {
    src: string;
    alt?: string;
    onclose: () => void;
  }

  let { src, alt = "", onclose }: Props = $props();
  let scale = $state(1);
  let x = $state(0);
  let y = $state(0);
  let dragging = $state(false);
  let dragStart = { clientX: 0, clientY: 0, x: 0, y: 0 };

  function setScale(next: number): void {
    scale = Math.min(8, Math.max(1, next));
    if (scale === 1) {
      x = 0;
      y = 0;
    }
  }

  function handleWheel(event: WheelEvent): void {
    event.preventDefault();
    setScale(scale + (event.deltaY < 0 ? 0.2 : -0.2));
  }

  function handlePointerDown(event: PointerEvent): void {
    if (scale <= 1) return;
    event.preventDefault();
    event.stopPropagation();
    dragging = true;
    dragStart = { clientX: event.clientX, clientY: event.clientY, x, y };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function handlePointerMove(event: PointerEvent): void {
    if (!dragging) return;
    x = dragStart.x + event.clientX - dragStart.clientX;
    y = dragStart.y + event.clientY - dragStart.clientY;
  }

  function handlePointerUp(event: PointerEvent): void {
    dragging = false;
    (event.currentTarget as HTMLElement).releasePointerCapture?.(event.pointerId);
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") onclose();
    else if (event.key === "+" || event.key === "=") setScale(scale + 0.3);
    else if (event.key === "-") setScale(scale - 0.3);
    else if (event.key === "0") setScale(1);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="lightbox"
  role="dialog"
  aria-modal="true"
  aria-label="画像プレビュー"
  tabindex="-1"
  onwheel={handleWheel}
  onclick={(event) => {
    if (event.currentTarget === event.target && !dragging) onclose();
  }}
>
  <button type="button" aria-label="閉じる" title="閉じる" onclick={onclose}>×</button>
  <img
    {src}
    {alt}
    class:dragging
    style={`transform: translate(${x}px, ${y}px) scale(${scale});`}
    onpointerdown={handlePointerDown}
    onpointermove={handlePointerMove}
    onpointerup={handlePointerUp}
    onpointercancel={handlePointerUp}
    ondblclick={(event) => {
      event.stopPropagation();
      setScale(scale > 1 ? 1 : 2);
    }}
  />
</div>

<style>
  .lightbox {
    position: fixed;
    inset: 0;
    z-index: 9000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: rgba(0, 0, 0, 0.82);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    cursor: zoom-out;
    animation: lightbox-in 0.18s ease;
  }

  button {
    position: absolute;
    top: 14px;
    right: 14px;
    z-index: 2;
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: 50%;
    color: #fff;
    background: rgba(255, 255, 255, 0.15);
    font: 18px/1 -apple-system, BlinkMacSystemFont, sans-serif;
    cursor: pointer;
    -webkit-app-region: no-drag;
  }

  button:hover {
    background: rgba(255, 255, 255, 0.28);
  }

  img {
    max-width: calc(100vw - 64px);
    max-height: calc(100vh - 64px);
    width: auto;
    height: auto;
    border-radius: 10px;
    object-fit: contain;
    box-shadow: 0 32px 96px rgba(0, 0, 0, 0.6);
    transform-origin: center center;
    user-select: none;
    -webkit-user-drag: none;
    cursor: default;
    transition: transform 0.08s ease-out;
    animation: lightbox-zoom 0.18s cubic-bezier(0.2, 0.8, 0.2, 1);
  }

  img.dragging {
    cursor: grabbing;
    transition: none;
  }

  @keyframes lightbox-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes lightbox-zoom {
    from { opacity: 0; }
    to { opacity: 1; }
  }
</style>
