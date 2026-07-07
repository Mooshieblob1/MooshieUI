<script lang="ts">
  import { locale } from "../stores/locale.svelte.js";
  import { generation } from "../stores/generation.svelte.js";
  import { loadGalleryImagePng, saveToGalleryBytes, readImageMetadata } from "../utils/api.js";
  import type { OutputImage } from "../types/index.js";

  interface Props {
    open: boolean;
    /** Source gallery image. Its gallery_filename must be resolved before opening. */
    image: OutputImage | null;
    onclose: () => void;
    onsaved?: (galleryFilename: string) => void;
  }

  let { open, image, onclose, onsaved }: Props = $props();

  const PHOTOPEA_ORIGIN = "https://www.photopea.com";

  // Hash config: empty document list, and replace File>Save (Ctrl+S) with a
  // script that flattens the document to PNG and posts it back to us as an
  // ArrayBuffer instead of triggering a browser download.
  const photopeaUrl =
    `${PHOTOPEA_ORIGIN}/#` +
    encodeURIComponent(
      JSON.stringify({
        files: [],
        environment: { customIO: { save: 'app.activeDocument.saveToOE("png");' } },
      }),
    );

  let iframeEl = $state<HTMLIFrameElement | null>(null);
  // boot: waiting for Photopea's initial "done"; opening: waiting for the source
  // image to finish opening; ready: user can edit and save.
  let phase = $state<"boot" | "opening" | "ready">("boot");
  let saving = $state(false);
  let error = $state("");

  async function sendImageToPhotopea() {
    if (!image?.gallery_filename || !iframeEl?.contentWindow) return;
    try {
      // Always PNG bytes — the backend transcodes JXL sources on the way out.
      const bytes = await loadGalleryImagePng(image.gallery_filename);
      const buffer = new Uint8Array(bytes).buffer;
      iframeEl.contentWindow.postMessage(buffer, PHOTOPEA_ORIGIN);
    } catch (e) {
      error = locale.t("photopea.load_failed");
      console.error("Photopea: failed to load source image:", e);
    }
  }

  async function handleSavedBuffer(buffer: ArrayBuffer) {
    if (saving) return; // serialize rapid Ctrl+S presses
    saving = true;
    error = "";
    try {
      const bytes = Array.from(new Uint8Array(buffer));
      // Post-edit PNG bytes carry no metadata, so forward the source's.
      let metadata: Record<string, string> | undefined;
      if (image?.gallery_filename) {
        metadata = (await readImageMetadata(image.gallery_filename)) ?? undefined;
      }
      const baseName = (image?.filename ?? "image.png").replace(/\.(jxl|webp|jpe?g)$/i, ".png");
      const saved = await saveToGalleryBytes(
        bytes,
        `edit_${baseName}`,
        `photopea_${Date.now()}`, // unique promptId → unique gallery filename per save
        image?.generation_mode,
        metadata,
        generation.metadataMode,
      );
      onsaved?.(saved);
    } catch (e) {
      error = locale.t("photopea.save_failed");
      console.error("Photopea: failed to save edited image:", e);
    } finally {
      saving = false;
    }
  }

  // Bridge Photopea's postMessage protocol. The listener is global, so filter
  // strictly by origin AND by this iframe's contentWindow.
  $effect(() => {
    if (!open) return;
    phase = "boot";
    error = "";
    saving = false;

    const onMessage = (e: MessageEvent) => {
      if (e.origin !== PHOTOPEA_ORIGIN) return;
      if (!iframeEl || e.source !== iframeEl.contentWindow) return;
      if (e.data instanceof ArrayBuffer) {
        void handleSavedBuffer(e.data);
        return;
      }
      if (e.data === "done") {
        if (phase === "boot") {
          phase = "opening";
          void sendImageToPhotopea();
        } else if (phase === "opening") {
          phase = "ready";
        }
        // Later "done"s just acknowledge completed save round-trips.
      }
    };

    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  });
</script>

{#if open}
  <div
    class="fixed inset-0 z-[230] flex flex-col bg-black/90 backdrop-blur-sm p-3"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("photopea.title")}
  >
    <div class="flex items-center gap-3 pb-2 shrink-0">
      <h2 class="text-sm font-semibold text-neutral-100 shrink-0">
        {locale.t("photopea.title")}
      </h2>
      <p class="text-xs text-neutral-400 truncate min-w-0 flex-1">
        {locale.t("photopea.save_hint")}
      </p>
      {#if saving}
        <span class="text-xs text-indigo-300 shrink-0">{locale.t("common.saving")}</span>
      {:else if error}
        <span class="text-xs text-red-400 shrink-0">{error}</span>
      {/if}
      <button
        type="button"
        class="text-neutral-400 hover:text-neutral-100 text-xl leading-none shrink-0"
        onclick={onclose}
        aria-label={locale.t("common.cancel")}
      >×</button>
    </div>
    <iframe
      bind:this={iframeEl}
      src={photopeaUrl}
      title={locale.t("photopea.title")}
      class="flex-1 w-full rounded-lg border border-neutral-700 bg-neutral-900"
    ></iframe>
  </div>
{/if}
