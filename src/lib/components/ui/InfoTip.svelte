<script lang="ts">
  interface Props {
    text: string;
  }

  let { text }: Props = $props();
</script>

<span class="inline-flex items-center ml-1">
  <button
    type="button"
    class="w-3.5 h-3.5 rounded-full border border-neutral-600 text-neutral-500 hover:border-neutral-400 hover:text-neutral-300 transition-colors inline-flex items-center justify-center text-[9px] leading-none font-medium cursor-help"
    title={text}
    aria-label={text}
    tabindex={0}
  >?</button>
</span>

