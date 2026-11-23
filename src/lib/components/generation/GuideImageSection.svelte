<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { uploadImage, uploadImageBytes, readClipboardImageSafe } from "../../utils/api.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import EditableValue from "../ui/EditableValue.svelte";

  let uploading = $state(false);
  let previewUrl = $state<string | null>(null);
  let dropZone = $state<HTMLElement | null>(null);
  let pasteActive = $state(false);

  function setImage(name: string | null, preview: string | null = null) {
    if (previewUrl && previewUrl !== preview) URL.revokeObjectURL(previewUrl);
    previewUrl = preview;
    generation.guideImage = name;
    generation.saveSettings();
  }

  $effect(() => {
    const el = dropZone;
    if (!el) return;
    const handler = async (event: Event) => {
      const path = (event as CustomEvent<{ paths: string[] }>).detail?.paths?.[0];
      if (!path) return;
      uploading = true;
      try {
        setImage((await uploadImage(path)).name);
      } catch (e) {
        console.error("guide image drop failed:", e);
      } finally {
        uploading = false;
      }
    };
    el.addEventListener("tauri-file-drop", handler);
    return () => el.removeEventListener("tauri-file-drop", handler);
  });

  $effect(() => {
    const handler = async (event: ClipboardEvent) => {
      if (!pasteActive) return;
      const target = event.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)
        return;
      event.preventDefault();
      event.stopImmediatePropagation();
      const bytes = await readClipboardImageSafe().catch(() => null);
      if (!bytes || !bytes.length) return;
      uploading = true;
      try {
        const result = await uploadImageBytes(bytes, "pasted_guide.png");
        const blob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
        setImage(result.name, URL.createObjectURL(blob));
      } catch (e) {
        console.error("guide image paste failed:", e);
      } finally {
        uploading = false;
      }
    };
    window.addEventListener("paste", handler, { capture: true });
    return () => window.removeEventListener("paste", handler, { capture: true });
  });

  async function uploadFile(file: File) {
    if (!file.type.startsWith("image/")) return;
    uploading = true;
    try {
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      const result = await uploadImageBytes(bytes, file.name);
      setImage(result.name, URL.createObjectURL(file));
    } catch (e) {
      console.error("guide image upload failed:", e);
    } finally {
      uploading = false;
    }
  }

  function browse() {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    input.onchange = () => {
      const file = input.files?.[0];
      if (file) uploadFile(file);
    };
    input.click();
  }
</script>

<div class="space-y-3 text-xs">
  <label class="flex items-center gap-2 text-neutral-400 select-none cursor-pointer">
    <input
      type="checkbox"
      class="accent-indigo-500"
      checked={generation.guideImageEnabled}
      onchange={(e) => {
        generation.guideImageEnabled = (e.target as HTMLInputElement).checked;
        generation.saveSettings();
      }}
    />
    {locale.t("generation.guide_image.title")}
    <InfoTip text={locale.t("generation.guide_image.tip")} />
  </label>

  {#if generation.guideImageEnabled}
    <div>
      {#if generation.guideImage}
        <div class="relative mb-2">
          {#if previewUrl}
            <img src={previewUrl} alt="" class="max-h-32 w-full rounded-lg object-cover" />
          {:else}
            <p class="truncate pr-16 text-[11px] text-neutral-400">{generation.guideImage}</p>
          {/if}
          <button
            class="absolute right-1 top-1 rounded bg-neutral-900/80 px-1.5 py-0.5 text-[10px] text-neutral-300 hover:text-white"
            onclick={() => setImage(null)}
          >
            {locale.t("common.remove")}
          </button>
        </div>
      {/if}

      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        bind:this={dropZone}
        role="button"
        tabindex="0"
        class="cursor-pointer select-none rounded-lg border border-dashed border-neutral-700 px-3 py-3 text-center text-neutral-500 transition-colors hover:border-neutral-500"
        onmouseenter={() => (pasteActive = true)}
        onmouseleave={() => (pasteActive = false)}
        ondragover={(e) => e.preventDefault()}
        ondrop={(e) => {
          e.preventDefault();
          const file = e.dataTransfer?.files?.[0];
          if (file) uploadFile(file);
        }}
        onclick={browse}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            browse();
          }
        }}
      >
        {#if uploading}
          <span class="text-indigo-400">{locale.t("generation.guide_image.uploading")}</span>
        {:else}
          {locale.t("generation.image_edit.drop_hint")}
          <span class="text-indigo-400 underline">{locale.t("generation.image_edit.upload_prompt")}</span>
        {/if}
      </div>
    </div>

    <div>
      <label class="flex items-center justify-between text-neutral-400 mb-1">
        <span>
          {locale.t("generation.guide_image.strength")}
          <InfoTip text={locale.t("generation.guide_image.strength_tip")} />
        </span>
        <EditableValue
          value={generation.guideStrength}
          min={0}
          max={1}
          step={0.05}
          decimals={2}
          onchange={(v) => {
            generation.guideStrength = v;
            generation.saveSettings();
          }}
        />
      </label>
      <input
        type="range"
        bind:value={generation.guideStrength}
        onchange={() => generation.saveSettings()}
        min="0"
        max="1"
        step="0.05"
        class="w-full accent-indigo-500"
      />
    </div>

    <div>
      <label class="flex items-center justify-between text-neutral-400 mb-1">
        <span>
          {locale.t("generation.guide_image.until")}
          <InfoTip text={locale.t("generation.guide_image.until_tip")} />
        </span>
        <EditableValue
          value={generation.guideEndPercent}
          min={0}
          max={1}
          step={0.05}
          decimals={2}
          onchange={(v) => {
            generation.guideEndPercent = v;
            generation.saveSettings();
          }}
        />
      </label>
      <input
        type="range"
        bind:value={generation.guideEndPercent}
        onchange={() => generation.saveSettings()}
        min="0"
        max="1"
        step="0.05"
        class="w-full accent-indigo-500"
      />
    </div>
  {/if}
</div>
