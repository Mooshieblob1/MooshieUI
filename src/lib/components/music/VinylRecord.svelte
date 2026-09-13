<script lang="ts" module>
  // One continuous groove winds towards the run-out, outside the paper label.
  // Its winding direction makes clockwise rotation appear to travel inward.
  const segments = 12 * 96;
  const groove = Array.from({ length: segments + 1 }, (_, index) => {
    const fraction = index / segments;
    const radius = 91 - 47 * fraction;
    const angle = -Math.PI / 2 - fraction * 12 * Math.PI * 2;
    return `${index === 0 ? "M" : "L"}${(100 + radius * Math.cos(angle)).toFixed(3)},${(100 + radius * Math.sin(angle)).toFixed(3)}`;
  }).join(" ");
</script>

<script lang="ts">
  let { progress, spinning }: { progress: number; spinning: boolean } = $props();
  const played = $derived(Number.isFinite(progress) ? Math.max(0, Math.min(100, progress)) : 0);
</script>

<svg class="h-28 w-28 shrink-0 sm:h-36 sm:w-36" viewBox="0 0 200 200" fill="none" aria-hidden="true">
  <circle cx="100" cy="100" r="99" class="fill-neutral-950" />
  <circle cx="100" cy="100" r="95" class="stroke-neutral-800" stroke-width="0.7" />
  <g class="origin-center motion-safe:animate-[spin_24s_linear_infinite]" style:animation-play-state={spinning ? "running" : "paused"}>
    <path d={groove} class="stroke-neutral-700/75" stroke-width="0.9" />
    {#if played > 0}
      <path d={groove} pathLength="100" stroke-dasharray={`${played} 100`} class="stroke-indigo-400/40" stroke-width="1" />
      <path d={groove} pathLength="100" stroke-dasharray="0.8 100" stroke-dashoffset={-Math.max(0, played - 0.8)} class="stroke-indigo-300" stroke-width="1.8" stroke-linecap="round" />
    {/if}
    <circle cx="100" cy="100" r="39" class="stroke-neutral-800" stroke-width="0.8" />
    <circle cx="100" cy="100" r="35" class="fill-indigo-400" />
    <circle cx="100" cy="100" r="31" stroke="var(--theme-accent-foreground)" stroke-width="0.5" opacity="0.3" />
    <text x="100" y="87" text-anchor="middle" font-size="9" letter-spacing="0.3" fill="var(--theme-accent-foreground)">MOOSHIEUI</text>
    <path d="M86 115H114M92 119H108" stroke="var(--theme-accent-foreground)" stroke-width="1" opacity="0.45" />
  </g>
  <circle cx="100" cy="100" r="3.5" class="fill-neutral-950" />
</svg>
