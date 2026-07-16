<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { isTauri } from "../../utils/ipc.js";

  interface Props {
    onclose: () => void;
    /** Interrogate whatever image is currently on the clipboard. */
    onpaste: () => void;
    /** Tauri-only: open a native file dialog and interrogate the picked image. */
    onbrowse: () => void;
    /** Browser file input / drag-drop: interrogate the given image file. */
    onfile: (file: File) => void;
  }

  let { onclose, onpaste, onbrowse, onfile }: Props = $props();

  let isDragOver = $state(false);
  let fileInput: HTMLInputElement | undefined = $state();

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }

  function handleBrowse() {
    // Tauri routes through the native dialog (parent handles the path); the
    // browser has no path access, so fall back to a hidden file input.
    if (isTauri) {
      onbrowse();
    } else {
      fileInput?.click();
    }
  }

  function handleFileChange(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (file) onfile(file);
  }

  // Browser-mode drop handling. In Tauri the OS drag-drop is intercepted natively
  // by the App-level onDragDropEvent listener, so these HTML5 events never fire.
  function handleDrop(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    isDragOver = false;
    const file = e.dataTransfer?.files?.[0];
    if (file && file.type.startsWith("image/")) onfile(file);
  }

  /** Teleport element to document.body so it escapes overflow/transform containers. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
<div
  use:portal
  data-interrogate-quick-modal
  class="fixed inset-0 bg-black/70 z-50 flex items-center justify-center p-4"
  onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}
>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="bg-neutral-900 border rounded-2xl shadow-2xl w-80 max-w-full flex flex-col transition-colors {isDragOver ? 'border-indigo-500 bg-indigo-500/5' : 'border-neutral-700'}"
    ondragenter={(e) => { e.preventDefault(); e.stopPropagation(); isDragOver = true; }}
    ondragover={(e) => { e.preventDefault(); e.stopPropagation(); isDragOver = true; }}
    ondragleave={() => { isDragOver = false; }}
    ondrop={handleDrop}
  >
    <div class="flex items-center gap-3 px-5 py-3 border-b border-neutral-700">
      <h2 class="text-base font-semibold text-neutral-100 flex-1">{locale.t('generation.interrogate.title')}</h2>
      <button
        onclick={onclose}
        class="text-neutral-400 hover:text-neutral-200 transition-colors"
        aria-label={locale.t('common.close')}
        title={locale.t('common.close')}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
      </button>
    </div>

    <div class="p-4 space-y-2.5">
      <button
        onclick={onpaste}
        class="w-full flex items-center gap-3 px-4 py-3 rounded-xl bg-neutral-800 hover:bg-neutral-700 border border-neutral-700 hover:border-neutral-600 text-sm text-neutral-200 hover:text-white transition-colors"
      >
        <svg class="w-4.5 h-4.5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" /></svg>
        {locale.t('generation.interrogate.paste_clipboard')}
      </button>
      <button
        onclick={handleBrowse}
        class="w-full flex items-center gap-3 px-4 py-3 rounded-xl bg-neutral-800 hover:bg-neutral-700 border border-neutral-700 hover:border-neutral-600 text-sm text-neutral-200 hover:text-white transition-colors"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4.5 h-4.5 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z"/><polyline points="14 2 14 8 20 8"/></svg>
        {locale.t('generation.interrogate.browse_file')}
      </button>
      <input
        bind:this={fileInput}
        type="file"
        accept="image/png,image/jpeg,image/webp"
        onchange={handleFileChange}
        class="hidden"
      />
    </div>

    <p class="px-4 pb-4 text-[10px] text-neutral-600 text-center">{locale.t('generation.interrogate.drop_hint')}</p>
  </div>
</div>
