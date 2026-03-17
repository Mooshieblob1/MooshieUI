<script lang="ts">
  interface Props {
    text: string;
  }
  let { text }: Props = $props();

  let visible = $state(false);
  let buttonEl: HTMLButtonElement | undefined = $state();
  let tipStyle = $state("");

  function reposition() {
    if (!buttonEl) return;
    const rect = buttonEl.getBoundingClientRect();
    const tipWidth = 280;
    const tipHeight = 100; // generous estimate
    const pad = 8;

    // Prefer above, fall back to below
    let top: number;
    let anchorAbove: boolean;
    if (rect.top - tipHeight - pad > 0) {
      top = rect.top - 4; // bottom of tooltip aligns just above the button
      anchorAbove = true;
    } else {
      top = rect.bottom + 4;
      anchorAbove = false;
    }

    // Center horizontally, clamp to viewport
    let left = rect.left + rect.width / 2 - tipWidth / 2;
    left = Math.max(pad, Math.min(left, window.innerWidth - tipWidth - pad));

    // If above, use bottom anchoring so tooltip grows upward
    if (anchorAbove) {
      tipStyle = `position:fixed;bottom:${window.innerHeight - top}px;left:${left}px;width:${tipWidth}px;z-index:9999;`;
    } else {
      tipStyle = `position:fixed;top:${top}px;left:${left}px;width:${tipWidth}px;z-index:9999;`;
    }
  }

  function show() {
    reposition();
    visible = true;
  }

  function hide() {
    visible = false;
  }

  let hideTimeout: ReturnType<typeof setTimeout> | null = null;

  function delayedHide() {
    hideTimeout = setTimeout(hide, 100);
  }

  function cancelHide() {
    if (hideTimeout) {
      clearTimeout(hideTimeout);
      hideTimeout = null;
    }
  }
</script>

<span class="inline-flex items-center ml-1">
  <button
    bind:this={buttonEl}
    class="w-3.5 h-3.5 rounded-full border border-neutral-600 text-neutral-500 hover:border-neutral-400 hover:text-neutral-300 transition-colors inline-flex items-center justify-center text-[9px] leading-none font-medium cursor-help"
    onmouseenter={() => { cancelHide(); show(); }}
    onmouseleave={delayedHide}
    onclick={(e) => { e.preventDefault(); visible ? hide() : show(); }}
    tabindex={-1}
  >?</button>
</span>

{#if visible}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    style={tipStyle}
    class="px-3 py-2 rounded-lg bg-neutral-800 border border-neutral-700 shadow-2xl text-[11px] text-neutral-300 leading-relaxed"
    onmouseenter={cancelHide}
    onmouseleave={delayedHide}
  >
    {text}
  </div>
{/if}
