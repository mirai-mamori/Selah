<!--
  Brand mark for SenseA: a gradient tile carrying a heartbeat/ECG pulse. The
  pulse is the product signature — continuous monitoring that beats while active
  — and keeps it distinct from a plain monochrome icon.
-->
<script lang="ts" module>
  let seq = 0;
</script>

<script lang="ts">
  interface Props {
    size?: number;
    // When true the mark beats (lub-dub), signalling SenseA is live.
    beat?: boolean;
  }
  let { size = 16, beat = false }: Props = $props();
  // Unique gradient id per instance so multiple marks never collide.
  const gid = `senseamark-${(seq += 1)}`;
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  class="sensea-mark"
  class:beat
  role="img"
  aria-label="自動検知"
>
  <defs>
    <linearGradient id={gid} x1="2" y1="2" x2="22" y2="22" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#8b5cf6" />
      <stop offset="0.5" stop-color="#4f7cff" />
      <stop offset="1" stop-color="#22c1e6" />
    </linearGradient>
  </defs>
  <rect x="1.5" y="1.5" width="21" height="21" rx="6.5" fill={`url(#${gid})`} />
  <!-- heartbeat / ECG pulse = continuous monitoring that reacts to change. -->
  <path
    d="M4 12.4h3.4l1.7-4.2 2.2 7.6 2-9 1.7 5.6 1.1-2h2.9"
    stroke="#ffffff"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    fill="none"
  />
</svg>

<style>
  .sensea-mark {
    display: inline-block;
    flex-shrink: 0;
    vertical-align: middle;
    border-radius: 6.5px;
    box-shadow: 0 1px 3px color-mix(in srgb, #4f7cff 35%, transparent);
    transform-origin: center;
  }

  /* Signature lub-dub beat while SenseA is live. */
  .sensea-mark.beat {
    animation: sensea-beat 2.6s ease-in-out infinite;
  }
  @keyframes sensea-beat {
    0%, 18%, 100% { transform: scale(1); }
    6% { transform: scale(1.13); }
    12% { transform: scale(1.04); }
    16% { transform: scale(1.1); }
  }
  @media (prefers-reduced-motion: reduce) {
    .sensea-mark.beat { animation: none; }
  }
</style>
