<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import PromptTextarea from "./PromptTextarea.svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import InterrogateModal from "./InterrogateModal.svelte";
  import { interrogateImage, interrogateImagePath, interrogateClipboard } from "../../utils/api.js";
  import { open } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { readFile } from "@tauri-apps/plugin-fs";
  import type { InterrogationResult } from "../../types/index.js";

  interface Props {
    showHistory?: boolean;
  }

  let { showHistory = true }: Props = $props();

  // Interrogation state
  let showInterrogateModal = $state(false);
  let interrogateResult = $state<InterrogationResult | null>(null);
  let interrogateLoading = $state(false);
  let interrogateStage = $state<string | null>(null);
  let interrogateDownloadProgress = $state<{ downloaded: number; total: number; filename: string } | null>(null);
  let interrogateImageUrl = $state<string | null>(null);
  let interrogateError = $state<string | null>(null);
  let interrogateOpen = $state(false);
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

    const unlistenDownload = await listen<{ downloaded: number; total: number; filename: string; done: boolean }>(
      "interrogator:download_progress",
      (event) => {
        if (event.payload.done) {
          interrogateDownloadProgress = null;
        } else {
          interrogateDownloadProgress = event.payload;
        }
      }
    );

    const unlistenStage = await listen<string>("interrogator:stage", (event) => {
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
    try {
      const bytes = await readFile(path);
      const blob = new Blob([bytes]);
      interrogateImageUrl = URL.createObjectURL(blob);
    } catch {
      interrogateImageUrl = null;
    }

    const unlistenDownload = await listen<{ downloaded: number; total: number; filename: string; done: boolean }>(
      "interrogator:download_progress",
      (event) => {
        if (event.payload.done) {
          interrogateDownloadProgress = null;
        } else {
          interrogateDownloadProgress = event.payload;
        }
      }
    );

    const unlistenStage = await listen<string>("interrogator:stage", (event) => {
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
    showInterrogateModal = true;
    interrogateLoading = true;
    interrogateResult = null;
    interrogateStage = null;
    interrogateDownloadProgress = null;
    interrogateImageUrl = null;
    interrogateError = null;

    const unlistenDownload = await listen<{ downloaded: number; total: number; filename: string; done: boolean }>(
      "interrogator:download_progress",
      (event) => {
        if (event.payload.done) {
          interrogateDownloadProgress = null;
        } else {
          interrogateDownloadProgress = event.payload;
        }
      }
    );

    const unlistenStage = await listen<string>("interrogator:stage", (event) => {
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

  // Listen for Ctrl+V when interrogate section is open — uses native clipboard (bypasses WebView restrictions)
  $effect(() => {
    if (!interrogateOpen) return;
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

  const sortedPromptHistory = $derived(
    [...generation.promptHistory].sort((a, b) => {
      if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
      return b.createdAt - a.createdAt;
    }).slice(0, 12)
  );
  let historySectionOpen = $state(true);

  function historyLabel(ts: number): string {
    return new Date(ts).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
</script>

<div class="space-y-2">
  {#if generation.stylePresetsEnabled}
    <div>
      <label class="block text-xs text-neutral-400 mb-1">Style Preset<InfoTip text="Fooocus-style presets that automatically inject prompt modifiers. Great for fast starts: pick a style first, then write your subject prompt." /></label>
      <select
        bind:value={generation.stylePreset}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#each generation.stylePresetOptions as preset}
          <option value={preset.id}>{preset.label}</option>
        {/each}
      </select>
    </div>
  {/if}

  <div class="rounded-lg border border-neutral-800 bg-neutral-900/50">
    <button
      class="w-full flex items-center justify-between px-3 py-2 text-xs text-neutral-400 hover:text-neutral-200 transition-colors"
      onclick={() => (interrogateOpen = !interrogateOpen)}
    >
      <span class="flex items-center gap-1.5">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 text-amber-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z"/><circle cx="12" cy="12" r="3"/></svg>
        Interrogate Image
        <InfoTip text="Analyze an image to extract tags (character, artist, general). Drop an image, browse your files, or paste from clipboard. Tags can be applied directly to your prompt." />
      </span>
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {interrogateOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
    </button>
    {#if interrogateOpen}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        bind:this={dropZoneEl}
        data-drop-zone="interrogate-image"
        class="mx-3 mb-3 border-2 border-dashed rounded-lg p-4 text-center transition-colors {isDragOver ? 'border-indigo-500 bg-indigo-500/10' : 'border-neutral-700 hover:border-neutral-600'}"
        ondragover={(e) => { e.preventDefault(); isDragOver = true; }}
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
            Browse or drop image
          </label>
          <span class="text-neutral-700">|</span>
          <button
            type="button"
            onclick={interrogateFromClipboard}
            class="text-xs text-emerald-500/70 hover:text-emerald-400 transition-colors flex items-center gap-1"
          >
            <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" /></svg>
            Ctrl+V Paste
          </button>
          <span class="text-neutral-700">|</span>
          <button
            type="button"
            onclick={handleInterrogateFromFile}
            class="text-xs text-neutral-500 hover:text-neutral-300 transition-colors flex items-center gap-1"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z"/><polyline points="14 2 14 8 20 8"/></svg>
            Select Image
          </button>
        </div>
        <p class="text-[10px] text-neutral-600 mt-2">Drag & drop, paste from clipboard, or browse to select an image</p>
      </div>
    {/if}
  </div>

  <div>
    <div class="flex items-center justify-between mb-1">
      <div class="flex items-center gap-1.5">
        <label class="text-xs text-neutral-400">Positive Prompt<InfoTip text="Describe what you want to see in the image. Use commas to separate concepts. More specific prompts give better results — include style, subject, lighting, and quality tags." /></label>
      </div>
      {#if generation.isAnima || generation.isIllustrious}
        <span class="shrink-0 text-[10px] px-2 py-0.5 rounded-full bg-emerald-600/20 text-emerald-400 border border-emerald-600/30">Quality prompts applied</span>
      {/if}
    </div>
    {#if generation.isAnima}
      <div class="text-[10px] text-amber-400/80 mb-1">Tip: Start artist tags with @ (e.g. @artist_name)</div>
    {/if}
    <PromptTextarea
      bind:value={generation.positivePrompt}
      placeholder={generation.isAnima ? "1girl, long hair, @artist_name, ..." : "A beautiful landscape, golden hour lighting, ..."}
      rows={4}
      minHeight="min-h-25"
    />
  </div>

  <div>
    <label class="block text-xs text-neutral-400 mb-1">Negative Prompt<InfoTip text="Describe what you don't want in the image. Common negatives include 'lowres', 'bad anatomy', 'blurry', 'worst quality'. Helps steer the AI away from common artifacts." /></label>
    <PromptTextarea
      bind:value={generation.negativePrompt}
      placeholder="blurry, cropped, extra fingers, ..."
      rows={3}
      minHeight="min-h-18"
    />
  </div>

  {#if showHistory && sortedPromptHistory.length > 0}
    <div class="rounded-lg border border-neutral-800 bg-neutral-900/50 p-2.5 space-y-2">
      <div class="flex items-center justify-between">
        <button
          class="w-full text-left flex items-center justify-between text-xs text-neutral-400 hover:text-neutral-200 transition-colors"
          onclick={() => (historySectionOpen = !historySectionOpen)}
          title={historySectionOpen ? "Collapse Prompt History & Favorites" : "Expand Prompt History & Favorites"}
        >
          <span>Prompt History & Favorites<InfoTip text="Recent prompts are auto-saved when you generate. Click to reload, star favorites to pin them to the top." /></span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {historySectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if historySectionOpen}
        <div class="space-y-1.5 max-h-56 overflow-y-auto pr-1">
          {#each sortedPromptHistory as entry}
            <div class="rounded border border-neutral-800 bg-neutral-900/80 p-2">
              <button
                class="w-full text-left"
                onclick={() => generation.applyPromptHistoryEntry(entry.id)}
                title="Load prompt"
              >
                <p class="text-[11px] text-neutral-200 max-h-8 overflow-hidden">{entry.positivePrompt || "(empty positive prompt)"}</p>
                {#if entry.negativePrompt}
                  <p class="text-[10px] text-neutral-500 mt-0.5 whitespace-nowrap overflow-hidden text-ellipsis">Negative: {entry.negativePrompt}</p>
                {/if}
              </button>
              <div class="mt-1.5 flex items-center justify-between gap-2">
                <span class="text-[10px] text-neutral-500">{historyLabel(entry.createdAt)}</span>
                <div class="flex items-center gap-1">
                  <button
                    class="px-1.5 py-0.5 text-[10px] rounded border transition-colors {entry.favorite ? 'border-amber-500 text-amber-300 bg-amber-500/10' : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-300'}"
                    onclick={() => generation.togglePromptFavorite(entry.id)}
                    title={entry.favorite ? "Unfavorite" : "Favorite"}
                  >
                    ★
                  </button>
                  <button
                    class="px-1.5 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300 transition-colors"
                    onclick={() => generation.removePromptHistoryEntry(entry.id)}
                    title="Remove"
                  >
                    Remove
                  </button>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
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
