<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import InterrogateModal from "./InterrogateModal.svelte";
  import { interrogateImage, interrogateImagePath, interrogateClipboard, readClipboardImage } from "../../utils/api.js";
  import { ipcListen, isTauri, isBrowserMode } from "../../utils/ipc.js";
  import type { InterrogationResult } from "../../types/index.js";

  // Interrogation state
  let showInterrogateModal = $state(false);
  let interrogateResult = $state<InterrogationResult | null>(null);
  let interrogateLoading = $state(false);
  let interrogateStage = $state<string | null>(null);
  let interrogateDownloadProgress = $state<{ downloaded: number; total: number; filename: string } | null>(null);
  let interrogateImageUrl = $state<string | null>(null);
  let interrogateError = $state<string | null>(null);
  let isDragOver = $state(false);

  async function interrogateBytes(base64: string, previewUrl?: string) {
    showInterrogateModal = true;
    interrogateLoading = true;
    interrogateResult = null;
    interrogateStage = null;
    interrogateDownloadProgress = null;
    if (previewUrl) {
      interrogateImageUrl = previewUrl;
    }
    interrogateError = null;

    const unlistenDownload = await ipcListen(
      "interrogator:download_progress",
      (event: any) => {
        if (event.payload.done) {
          interrogateDownloadProgress = null;
        } else {
          interrogateDownloadProgress = event.payload;
        }
      }
    );

    const unlistenStage = await ipcListen("interrogator:stage", (event: any) => {
      interrogateStage = event.payload;
    });

    try {
      const result = await interrogateImage(base64);
      interrogateResult = result;
    } catch (e) {
      console.error("Interrogation failed:", e);
      interrogateError = e instanceof Error ? e.message : String(e);
    } finally {
      interrogateLoading = false;
      interrogateStage = null;
      unlistenDownload();
      unlistenStage();
    }
  }

  async function interrogatePath(path: string) {
    showInterrogateModal = true;
    interrogateLoading = true;
    interrogateResult = null;
    interrogateStage = null;
    interrogateDownloadProgress = null;
    interrogateError = null;
    // Read file via Tauri fs plugin and create a blob URL for preview
    if (isTauri) {
      try {
        const { readFile } = await import("@tauri-apps/plugin-fs");
        const bytes = await readFile(path);
        const blob = new Blob([bytes]);
        if (interrogateImageUrl?.startsWith("blob:")) URL.revokeObjectURL(interrogateImageUrl);
        interrogateImageUrl = URL.createObjectURL(blob);
      } catch {
        interrogateImageUrl = null;
      }
    } else {
      interrogateImageUrl = null;
    }

    const unlistenDownload = await ipcListen(
      "interrogator:download_progress",
      (event: any) => {
        if (event.payload.done) {
          interrogateDownloadProgress = null;
        } else {
          interrogateDownloadProgress = event.payload;
        }
      }
    );

    const unlistenStage = await ipcListen("interrogator:stage", (event: any) => {
      interrogateStage = event.payload;
    });

    try {
      const result = await interrogateImagePath(path);
      interrogateResult = result;
    } catch (e) {
      console.error("Interrogation failed:", e);
      interrogateError = e instanceof Error ? e.message : String(e);
    } finally {
      interrogateLoading = false;
      interrogateStage = null;
      unlistenDownload();
      unlistenStage();
    }
  }

  async function interrogateFromClipboard() {
    // In browser mode, try the Web Clipboard API first (requires HTTPS),
    // then fall back to the server-side native clipboard read.
    if (isBrowserMode) {
      let imageBlob: Blob | null = null;

      // Try browser Clipboard API first
      if (navigator.clipboard?.read) {
        try {
          const items = await navigator.clipboard.read();
          for (const item of items) {
            for (const type of item.types) {
              if (type.startsWith("image/")) {
                imageBlob = await item.getType(type);
                break;
              }
            }
            if (imageBlob) break;
          }
        } catch {
          // Clipboard API blocked — fall through to server fallback
        }
      }

      // Fallback: ask server to read from host OS clipboard
      if (!imageBlob) {
        try {
          const bytes = await readClipboardImage();
          if (!bytes || bytes.length === 0) {
            showInterrogateModal = true;
            interrogateError = locale.t('common.no_clipboard_image');
            return;
          }
          imageBlob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
        } catch (e) {
          showInterrogateModal = true;
          interrogateError = e instanceof Error ? e.message : String(e);
          return;
        }
      }

      try {
        // Revoke previous blob URL to prevent memory leak
        if (interrogateImageUrl?.startsWith("blob:")) URL.revokeObjectURL(interrogateImageUrl);
        const previewUrl = URL.createObjectURL(imageBlob);
        // Use FileReader for efficient base64 conversion
        const base64 = await new Promise<string>((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => {
            const dataUrl = reader.result as string;
            resolve(dataUrl.split(",")[1]);
          };
          reader.onerror = () => reject(reader.error);
          reader.readAsDataURL(imageBlob!);
        });
        await interrogateBytes(base64, previewUrl);
        return;
      } catch (e) {
        showInterrogateModal = true;
        interrogateError = e instanceof Error ? e.message : String(e);
        return;
      }
    }

    showInterrogateModal = true;
    interrogateLoading = true;
    interrogateResult = null;
    interrogateStage = null;
    interrogateDownloadProgress = null;
    interrogateImageUrl = null;
    interrogateError = null;

    const unlistenDownload = await ipcListen(
      "interrogator:download_progress",
      (event: any) => {
        if (event.payload.done) {
          interrogateDownloadProgress = null;
        } else {
          interrogateDownloadProgress = event.payload;
        }
      }
    );

    const unlistenStage = await ipcListen("interrogator:stage", (event: any) => {
      interrogateStage = event.payload;
    });

    try {
      const result = await interrogateClipboard();
      interrogateResult = result;
    } catch (e) {
      console.error("Clipboard interrogation failed:", e);
      interrogateError = e instanceof Error ? e.message : String(e);
    } finally {
      interrogateLoading = false;
      interrogateStage = null;
      unlistenDownload();
      unlistenStage();
    }
  }

  async function handleInterrogateFromFile() {
    if (!isTauri) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      multiple: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (!selected) return;

    await interrogatePath(selected);
  }

  function handleInterrogateUpload(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    input.value = "";
    readFileAsBase64(file);
  }

  async function handleInterrogateDrop(event: DragEvent) {
    event.preventDefault();
    event.stopPropagation();
    isDragOver = false;
    const file = event.dataTransfer?.files?.[0];
    if (!file || !file.type.startsWith("image/")) return;
    await readFileAsBase64(file);
  }

  let dropZoneEl: HTMLDivElement | undefined = $state();

  /** Handle Tauri native drag-drop via custom event dispatched from parent. */
  async function handleTauriFileDrop(e: Event) {
    const { path } = (e as CustomEvent).detail as { path: string; filename: string };
    await interrogatePath(path);
  }

  $effect(() => {
    const el = dropZoneEl;
    if (!el) return;
    el.addEventListener("tauri-file-drop", handleTauriFileDrop);
    return () => el.removeEventListener("tauri-file-drop", handleTauriFileDrop);
  });

  // Listen for Ctrl+V — uses native clipboard (bypasses WebView restrictions)
  $effect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === "v") {
        // Don't intercept if user is typing in an input/textarea
        const active = document.activeElement;
        if (active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement) return;
        e.preventDefault();
        interrogateFromClipboard();
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  });

  async function readFileAsBase64(file: File) {
    const previewUrl = URL.createObjectURL(file);
    const buffer = await file.arrayBuffer();
    const bytes = new Uint8Array(buffer);
    let binary = "";
    for (let i = 0; i < bytes.length; i++) {
      binary += String.fromCharCode(bytes[i]);
    }
    await interrogateBytes(btoa(binary), previewUrl);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={dropZoneEl}
  data-drop-zone="interrogate-image"
  class="border-2 border-dashed rounded-lg p-4 text-center transition-colors {isDragOver ? 'border-indigo-500 bg-indigo-500/10' : 'border-neutral-700 hover:border-neutral-600'}"
  ondragenter={(e) => { e.preventDefault(); e.stopPropagation(); isDragOver = true; }}
  ondragover={(e) => { e.preventDefault(); e.stopPropagation(); isDragOver = true; }}
  ondragleave={() => { isDragOver = false; }}
  ondrop={handleInterrogateDrop}
>
  <div class="flex items-center justify-center gap-3 flex-wrap">
    <label class="cursor-pointer text-xs text-neutral-500 hover:text-neutral-300 transition-colors">
      <input
        type="file"
        accept="image/png,image/jpeg,image/webp"
        onchange={handleInterrogateUpload}
        class="hidden"
      />
      {locale.t('generation.interrogate.browse_or_drop')}
    </label>
    <span class="text-neutral-700">|</span>
    <button
      type="button"
      onclick={interrogateFromClipboard}
      class="text-xs text-emerald-500/70 hover:text-emerald-400 transition-colors flex items-center gap-1"
    >
      <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" /></svg>
      {locale.t('generation.interrogate.paste')}
    </button>
    <span class="text-neutral-700">|</span>
    <button
      type="button"
      onclick={handleInterrogateFromFile}
      class="text-xs text-neutral-500 hover:text-neutral-300 transition-colors flex items-center gap-1"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z"/><polyline points="14 2 14 8 20 8"/></svg>
      {locale.t('generation.interrogate.select_image')}
    </button>
  </div>
  <p class="text-[10px] text-neutral-600 mt-2">{locale.t('generation.interrogate.drop_hint')}</p>
</div>

{#if showInterrogateModal}
  <InterrogateModal
    result={interrogateResult}
    loading={interrogateLoading}
    stage={interrogateStage}
    downloadProgress={interrogateDownloadProgress}
    imagePreviewUrl={interrogateImageUrl}
    error={interrogateError}
    onclose={() => {
      showInterrogateModal = false;
      interrogateResult = null;
      interrogateError = null;
      if (interrogateImageUrl?.startsWith("blob:")) URL.revokeObjectURL(interrogateImageUrl);
      interrogateImageUrl = null;
    }}
  />
{/if}
