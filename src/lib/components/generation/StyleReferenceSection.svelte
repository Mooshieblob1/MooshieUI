<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import {
    uploadImage,
    uploadImageBytes,
    readClipboardImageSafe,
    checkNodeAvailable,
    installCustomNode,
    downloadModel,
  } from "../../utils/api.js";
  import { ipcListen } from "../../utils/ipc.js";
  import { onMount, onDestroy } from "svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  const IPADAPTER_CLASS = "IPAdapterUnifiedLoader";
  const IPADAPTER_GIT_URL = "https://github.com/cubiq/ComfyUI_IPAdapter_plus.git";
  const IPADAPTER_PACK_NAME = "ComfyUI_IPAdapter_plus";

  const SIGCLIP_URL =
    "https://huggingface.co/Comfy-Org/sigclip_vision_384/resolve/main/sigclip_vision_patch14_384.safetensors";
  const CLIP_VIT_H_URL =
    "https://huggingface.co/Comfy-Org/CLIP-ViT-H-14-laion2B-s32B-b79K_repackaged/resolve/main/split_files/clip_vision/CLIP-ViT-H-14-laion2B-s32B-b79K.safetensors";
  const IPA_SD15_URL =
    "https://huggingface.co/h94/IP-Adapter/resolve/main/models/ip-adapter-plus_sd15.safetensors";
  const IPA_SDXL_URL =
    "https://huggingface.co/h94/IP-Adapter/resolve/main/sdxl_models/ip-adapter-plus_sdxl_vit-h.safetensors";

  // Anima Reference Adapter v2. AnimaRefAdapter loads it by this exact filename
  // (ANIMA_REF_ADAPTER_LORA_FILENAME in src-tauri/src/comfyui/nodes.rs). Pinned
  // to one Hugging Face revision and checked against its SHA-256.
  const animaRefLora = {
    url: "https://huggingface.co/ElectronGirl/anima-ref-adapter/resolve/e05a8d21ad3f3aeb68875c5ed0e85787cfba79b0/anima_ref_adapter_v2_r64.safetensors",
    category: "loras",
    filename: "anima_ref_adapter_v2_r64.safetensors",
    sha256: "71dda6079e18d354e16e3347bc49a9b710afd9495c378efbb833d30cac9cb81b",
  };
  const snaprefModels = [
    {
      filename: "snapref_v2a.safetensors",
      category: "loras",
      url: "https://huggingface.co/0ppxnhximxr/snapref-v2a/resolve/e6d6a21e4d56faae4454a1cc9bfd065d87358bf2/snapref_v2a.safetensors",
      sha256: "8a22fc28d060447544eda283b36efe2dcdbad7422cbc0b8d42828d55a8a61ab4",
    },
    {
      filename: "snapref_v1a.safetensors",
      category: "loras",
      url: "https://huggingface.co/0ppxnhximxr/snapref-v1a/resolve/bc3ca7091c08acce8f20af8f1cd7ed8643261023/snapref_v1a.safetensors",
      sha256: "0b8c6897845bb721c46977eae034a95492aa39cf22342b345f7e0315f4bd81fd",
    },
  ];
  let downloadingModel = $state(animaRefLora.filename);
  let snaprefAvailable = $state<boolean | null>(null);
  const isSnapref = $derived(generation.styleRefIsAnima && generation.animaRefMethod === "snapref");
  const snaprefWeights = $derived(models.loras.filter((name) => /snapref/i.test(name)));
  $effect(() => {
    if (!isSnapref || generation.snaprefAdapter || !snaprefWeights.length) return;
    generation.snaprefAdapter = snaprefModels.flatMap((m) => snaprefWeights.filter((name) => name.split(/[\\/]/).pop() === m.filename))[0] ?? snaprefWeights[0];
    generation.saveSettings();
  });
  $effect(() => {
    void models.cacheScope;
    if (!isSnapref) return;
    let current = true;
    checkNodeAvailable("SnapRefSimple", ["reference_8", "strength_8", "fidelity"])
      .then((ready) => { if (current) snaprefAvailable = ready; })
      .catch(() => { if (current) snaprefAvailable = null; });
    return () => { current = false; };
  });
  const animaRefCorrections = ["apg", "rescale", "none"];

  let ipadapterAvailable = $state<boolean | null>(null);
  let installing = $state(false);
  let installError = $state<string | null>(null);
  // Slot 0 is the reference every family uses; slot 1 is Anima's second character.
  let uploadingSlot = $state<number | null>(null);
  let previews = $state<Record<string, string>>({});
  let mounted = true;
  onDestroy(() => {
    mounted = false;
    Object.values(previews).forEach((url) => URL.revokeObjectURL(url));
  });
  let pasteSlot = $state<number | null>(null);

  let loraDownloading = $state(false);
  let loraDownloadError = $state<string | null>(null);
  let dlBytes = $state(0);
  let dlTotal = $state(0);
  const dlPercent = $derived(dlTotal > 0 ? Math.round((dlBytes / dlTotal) * 100) : 0);

  const isRedux = $derived(generation.styleRefIsRedux);
  const isIPAdapter = $derived(generation.styleRefIsIPAdapter);
  const isAnima = $derived(generation.styleRefIsAnima);
  const supported = $derived(generation.supportsStyleRef);
  // endsWith so a file nested in a subfolder still counts as installed.
  const animaLoraInstalled = $derived(
    models.loras.some((m) => m.endsWith(animaRefLora.filename)),
  );
  const slots = $derived(isSnapref ? generation.snaprefReferences.map((_, i) => i) : isAnima ? [0, 1] : [0]);

  function slotImage(slot: number): string | null {
    if (isSnapref) return generation.snaprefReferences[slot]?.image ?? null;
    return slot === 0 ? generation.styleRefImage : generation.styleRefImage2;
  }

  function setSlotImage(slot: number, name: string | null, preview: string | null) {
    const old = slotImage(slot);
    if (old && previews[old]) {
      URL.revokeObjectURL(previews[old]);
      delete previews[old];
    }
    if (isSnapref) generation.snaprefReferences = generation.snaprefReferences.map((r, i) => i === slot ? { ...r, image: name } : r);
    else if (slot === 0) generation.styleRefImage = name;
    else generation.styleRefImage2 = name;
    if (name && preview) previews[name] = preview;
    generation.saveSettings();
  }

  // Keep an asynchronous upload bound to the original slot and settings snapshot.
  function imageWriter(slot: number) {
    const snapref = isSnapref;
    const reference = snapref ? generation.snaprefReferences[slot] : null;
    const image = slotImage(slot);
    return (name: string, preview: string | null) => {
      const index = reference ? generation.snaprefReferences.indexOf(reference) : slot;
      if (!mounted || snapref !== isSnapref || index < 0 || slotImage(index) !== image) {
        if (preview) URL.revokeObjectURL(preview);
        return;
      }
      setSlotImage(index, name, preview);
    };
  }

  function uploadName(name: string) {
    return isSnapref ? `snapref_${crypto.randomUUID?.() ?? `${Date.now()}_${Math.random().toString(36).slice(2)}`}_${name.split(/[\\/]/).pop()}` : name;
  }

  function inputName(result: { name: string; subfolder: string }) {
    return result.subfolder ? `${result.subfolder}/${result.name}` : result.name;
  }

  const ipadapterWeightTypes = [
    "linear",
    "style transfer",
    "composition",
    "strong style transfer",
  ];
  // StyleModelApply's strength_type choices in ComfyUI core.
  const reduxWeightTypes = ["multiply", "attn_bias"];

  async function probeIpadapter() {
    if (!isIPAdapter) {
      ipadapterAvailable = null;
      return;
    }
    ipadapterAvailable = await checkNodeAvailable(IPADAPTER_CLASS).catch(() => null);
  }

  onMount(() => {
    probeIpadapter();
  });

  onMount(async () => {
    await ipcListen("download:progress", (event: any) => {
      if (!loraDownloading) return;
      const data = event.payload as { downloaded: number; total: number; done: boolean };
      if (data.done) {
        dlBytes = 0;
        dlTotal = 0;
      } else {
        dlBytes = data.downloaded;
        dlTotal = data.total;
      }
    });
  });

  async function downloadReferenceModel(model = animaRefLora) {
    const scope = models.cacheScope;
    const selectedAdapter = generation.snaprefAdapter;
    downloadingModel = model.filename;
    loraDownloadError = null;
    loraDownloading = true;
    try {
      await downloadModel(
        model.url,
        model.category,
        model.filename,
        undefined,
        model.sha256,
      );
      await models.refresh();
      if (snaprefModels.includes(model) && models.cacheScope === scope && generation.snaprefAdapter === selectedAdapter) {
        generation.snaprefAdapter = model.filename;
      }
      generation.saveSettings();
    } catch (e) {
      loraDownloadError = locale.t("generation.style_ref.anima_lora_error", { error: `${e}` });
    } finally {
      loraDownloading = false;
    }
  }

  $effect(() => {
    // Re-probe when the model family changes
    void generation.modelFamily;
    probeIpadapter();
  });

  // Desktop file drops: GenerationPage finds the [data-drop-zone] under the
  // cursor and sends it a "tauri-file-drop" event carrying the file path.
  function tauriDropTarget(el: HTMLElement, slot: number) {
    const handler = (event: Event) => {
      const e = event as CustomEvent<{ path: string }>;
      const path = e.detail?.path;
      if (!path) return;
      handleTauriDrop(path, slot);
    };
    el.addEventListener("tauri-file-drop", handler);
    return { destroy: () => el.removeEventListener("tauri-file-drop", handler) };
  }

  $effect(() => {
    const handler = async (event: ClipboardEvent) => {
      const slot = pasteSlot;
      if (slot === null || slotImage(slot)) return;
      const target = event.target as HTMLElement;
      if (
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable
      )
        return;
      event.preventDefault();
      event.stopImmediatePropagation();
      await pasteImage(slot);
    };
    window.addEventListener("paste", handler, { capture: true });
    return () => window.removeEventListener("paste", handler, { capture: true });
  });

  async function handleTauriDrop(path: string, slot: number) {
    if (uploadingSlot !== null) return;
    const write = imageWriter(slot);
    const name = uploadName(path);
    uploadingSlot = slot;
    try {
      if (isSnapref) {
        const { readFile } = await import("@tauri-apps/plugin-fs");
        const bytes = await readFile(path);
        const result = await uploadImageBytes(Array.from(bytes), name);
        write(inputName(result), URL.createObjectURL(new Blob([bytes])));
      } else {
        const result = await uploadImage(path);
        write(inputName(result), null);
      }
    } catch (e) {
      installError = locale.t("generation.style_ref.upload_error");
      console.error("style ref drop failed:", e);
    } finally { uploadingSlot = null; }
  }

  async function pasteImage(slot: number) {
    if (uploadingSlot !== null) return;
    const write = imageWriter(slot);
    const name = uploadName("pasted_style_ref.png");
    uploadingSlot = slot;
    try {
      const bytes = await readClipboardImageSafe();
      if (!bytes?.length) return;
      const result = await uploadImageBytes(bytes, name);
      write(inputName(result), URL.createObjectURL(new Blob([new Uint8Array(bytes)])));
    } catch (e) {
      installError = locale.t("generation.style_ref.upload_error");
      console.error("style ref paste failed:", e);
    } finally { uploadingSlot = null; }
  }

  function handleFileBrowse(slot: number) {
    const write = imageWriter(slot);
    const snapref = isSnapref;
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    input.onchange = () => {
      const file = input.files?.[0];
      if (file && snapref === isSnapref) void uploadFile(file, slot, write);
    };
    input.click();
  }

  async function uploadFile(file: File, slot: number, write = imageWriter(slot)) {
    if (uploadingSlot !== null) return;
    const name = uploadName(file.name);
    uploadingSlot = slot;
    installError = null;
    try {
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      const result = await uploadImageBytes(bytes, name);
      write(inputName(result), URL.createObjectURL(file));
    } catch (e) {
      installError = locale.t("generation.style_ref.upload_error");
      console.error("style ref upload failed:", e);
    } finally { uploadingSlot = null; }
  }

  function handleWebDrop(event: DragEvent, slot: number) {
    event.preventDefault();
    const file = event.dataTransfer?.files?.[0];
    if (file && file.type.startsWith("image/")) {
      uploadFile(file, slot);
    }
  }

  function clearImage(slot: number) {
    setSlotImage(slot, null, null);
  }

  function slotLabel(slot: number): string {
    if (isSnapref) return locale.t("generation.style_ref.snapref_reference", { number: slot + 1 });
    if (!isAnima) return locale.t("generation.style_ref.image");
    return slot === 0
      ? locale.t("generation.style_ref.anima_character")
      : locale.t("generation.style_ref.anima_second_character");
  }

  async function handleInstall() {
    installing = true;
    installError = null;
    try {
      await installCustomNode(IPADAPTER_GIT_URL, IPADAPTER_PACK_NAME);
      ipadapterAvailable = await checkNodeAvailable(IPADAPTER_CLASS).catch(() => null);
    } catch (e) {
      installError = `${e}`;
    } finally {
      installing = false;
    }
  }
</script>

<div class="space-y-3 text-xs">
  <!-- Enable toggle -->
  <label class="flex items-center gap-2 text-neutral-400 select-none cursor-pointer">
    <input
      type="checkbox"
      class="accent-indigo-500"
      checked={generation.styleRefEnabled}
      onchange={(e) => {
        generation.styleRefEnabled = (e.target as HTMLInputElement).checked;
        generation.saveSettings();
      }}
      title={locale.t("generation.style_ref.toggle")}
    />
    {locale.t("generation.style_ref.title")}
    <InfoTip text={locale.t("generation.style_ref.tip")} />
  </label>

  {#if generation.styleRefEnabled}
    {#if !supported}
      <!-- Unsupported family hint -->
      <div
        class="rounded-lg border border-amber-600/30 bg-amber-600/10 px-3 py-2 text-[11px] text-amber-300"
      >
        {locale.t("generation.style_ref.unsupported")}
      </div>
    {:else}
      {#if isAnima}
        <label class="block text-neutral-400">
          {locale.t("generation.style_ref.method")}
          <select class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-neutral-100 transition-colors focus:border-indigo-500 focus:outline-none" aria-label={locale.t("generation.style_ref.method")} bind:value={generation.animaRefMethod} onchange={() => generation.saveSettings()}>
            <option value="anima_ref_adapter">{locale.t("generation.style_ref.anima_mode")}</option>
            <option value="snapref">SnapRef</option>
          </select>
        </label>
      {/if}
      {#if isSnapref}
        <label class="block text-neutral-400">
          {locale.t("generation.style_ref.snapref_weights")}
          <select class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-neutral-100 transition-colors focus:border-indigo-500 focus:outline-none" aria-label={locale.t("generation.style_ref.snapref_weights")} bind:value={generation.snaprefAdapter} onchange={() => generation.saveSettings()}>
            {#if !snaprefWeights.length && !generation.snaprefAdapter}
              <option value={null} disabled>{locale.t("generation.style_ref.snapref_select")}</option>
            {/if}
            {#if generation.snaprefAdapter && !snaprefWeights.includes(generation.snaprefAdapter)}
              <option value={generation.snaprefAdapter}>{generation.snaprefAdapter} — {locale.t("generation.style_ref.snapref_missing")}</option>
            {/if}
            {#each snaprefWeights as weight}<option value={weight}>{weight}</option>{/each}
          </select>
        </label>
        {#if snaprefAvailable === false}<p class="text-[11px] text-amber-400">{locale.t("generation.style_ref.snapref_nodes")}</p>{/if}
        {#if models.remote}
          <p class="text-[11px] text-neutral-500">{locale.t("generation.style_ref.snapref_remote")}</p>
        {:else}
          {#each snaprefModels as model}
            {#if !snaprefWeights.some((name) => name.split(/[\\/]/).pop() === model.filename)}
              <button class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-xs text-neutral-100 hover:border-indigo-500 disabled:opacity-50" disabled={loraDownloading} onclick={() => downloadReferenceModel(model)}>{locale.t("generation.style_ref.snapref_download", { model: model.filename })}</button>
            {/if}
          {/each}
        {/if}
        <label class="block text-neutral-400">
          {locale.t("generation.style_ref.snapref_mode")}
          <select class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-neutral-100 transition-colors focus:border-indigo-500 focus:outline-none" aria-label={locale.t("generation.style_ref.snapref_mode")} bind:value={generation.snaprefMode} onchange={() => generation.saveSettings()}>
            <option value="Character & Style">{locale.t("generation.style_ref.snapref_cs")}</option>
            <option value="Character">{locale.t("generation.style_ref.anima_character")}</option>
          </select>
        </label>
        <label for="snapref-fidelity" class="block text-neutral-400">
          {locale.t("generation.style_ref.snapref_fidelity")} <span class="text-neutral-300">{generation.snaprefFidelity.toFixed(2)}</span>
          <InfoTip text={locale.t("generation.style_ref.snapref_fidelity_tip")} />
          <input id="snapref-fidelity" type="range" min="0" max="1" step="0.05" class="w-full" bind:value={generation.snaprefFidelity} onchange={() => generation.saveSettings()} />
        </label>
      {/if}
      {#if installError}<p class="text-[11px] text-red-400">{installError}</p>{/if}

      <!-- Family badge -->
      <div class="text-[10px] font-medium uppercase tracking-wide text-neutral-500">
        {isRedux
          ? locale.t("generation.style_ref.redux_mode")
          : isAnima
            ? (isSnapref ? "SnapRef" : locale.t("generation.style_ref.anima_mode"))
            : locale.t("generation.style_ref.ipadapter_mode")}
        {#if isAnima}<InfoTip text={locale.t(isSnapref ? "generation.style_ref.snapref_note" : "generation.style_ref.anima_note")} />{/if}
      </div>

      {#if isAnima}
        {#if !isSnapref && !animaLoraInstalled}
          <div class="space-y-1">
            <button
              class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-xs text-neutral-100 transition-colors hover:border-indigo-500 disabled:opacity-50"
              disabled={loraDownloading}
              onclick={() => downloadReferenceModel()}
              title={locale.t("generation.style_ref.anima_lora_auto")}
            >
              {locale.t("generation.style_ref.anima_lora_download")}
            </button>
          </div>
        {/if}
        {#if loraDownloading}
          <div class="rounded-lg bg-neutral-800/80 px-3 py-2">
            <div class="mb-1 flex items-center justify-between text-[11px] text-neutral-400">
              <span class="mr-2 truncate">
                {locale.t("generation.upscale.downloading", { model: downloadingModel })}
              </span>
              {#if dlTotal > 0}
                <span class="shrink-0 tabular-nums">
                  {locale.formatBytes(dlBytes)} / {locale.formatBytes(dlTotal)} ({dlPercent}%)
                </span>
              {/if}
            </div>
            {#if dlTotal > 0}
              <div class="h-1.5 w-full overflow-hidden rounded-full bg-neutral-700">
                <div
                  class="h-full rounded-full bg-indigo-400 transition-[width] duration-300 ease-out"
                  style="width: {dlPercent}%"
                ></div>
              </div>
            {/if}
          </div>
        {/if}
        {#if loraDownloadError}
          <p class="text-[11px] text-red-400">{loraDownloadError}</p>
        {/if}
        {#if generation.animaTeacacheEnabled}
          <p class="text-[11px] text-amber-400/90">
            {locale.t("generation.style_ref.anima_teacache_off")}
          </p>
        {/if}
      {/if}

      <!-- IP-Adapter install prompt (SD1.5 / SDXL only) -->
      {#if isIPAdapter && ipadapterAvailable === false}
        <div
          class="rounded-lg border border-amber-600/30 bg-amber-600/10 px-3 py-2 text-[11px] text-amber-300"
        >
          {locale.t("generation.style_ref.nodes_install")}
        </div>
        <div>
          <button
            class="text-indigo-400 transition-colors hover:text-indigo-300 disabled:opacity-50"
            disabled={installing}
            onclick={handleInstall}
          >
            {installing
              ? locale.t("generation.style_ref.installing")
              : locale.t("generation.style_ref.install")}
          </button>
          {#if installError}
            <div class="mt-1 text-[11px] text-red-400">
              {locale.t("generation.style_ref.install_error", { error: installError })}
            </div>
          {/if}
        </div>
      {/if}

      <!-- Image picker(s): one reference, plus Anima's optional second character -->
      {#each slots as slot (slot)}
        {@const image = slotImage(slot)}
        <div>
          <div class="mb-1 text-neutral-400">
            {slotLabel(slot)}{#if isAnima && !isSnapref && slot === 1}<InfoTip text={locale.t("generation.style_ref.anima_two_characters_tip")} />{/if}
          </div>
          {#if image && previews[image]}
            <div class="relative mb-2">
              <img
                src={previews[image]}
                alt={slotLabel(slot)}
                class="max-h-32 w-full rounded-lg object-cover"
              />
              <button
                class="absolute right-1 top-1 rounded bg-neutral-900/80 px-1.5 py-0.5 text-[10px] text-neutral-300 hover:text-white"
                onclick={() => clearImage(slot)}
              >
                {locale.t("generation.style_ref.clear")}
              </button>
            </div>
          {:else if image}
            <div class="mb-2 truncate text-[11px] text-neutral-400">
              {image}
              <button
                class="ml-1 text-neutral-500 hover:text-neutral-300"
                onclick={() => clearImage(slot)}
              >
                [x]
              </button>
            </div>
          {/if}

          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            role="button"
            tabindex="0"
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") { e.preventDefault(); handleFileBrowse(slot); }
            }}
            use:tauriDropTarget={slot}
            data-drop-zone="style-ref-{slot}"
            class="cursor-pointer select-none rounded-lg border border-dashed border-neutral-700 px-3 py-3 text-center text-neutral-500 transition-colors hover:border-neutral-500"
            onmouseenter={() => (pasteSlot = slot)}
            onmouseleave={() => (pasteSlot = null)}
            ondragover={(e) => e.preventDefault()}
            ondrop={(e) => handleWebDrop(e, slot)}
            onclick={() => handleFileBrowse(slot)}
          >
            {#if uploadingSlot === slot}
              <span class="text-indigo-400">{locale.t("generation.style_ref.snapref_uploading")}</span>
            {:else}
              {isAnima
                ? locale.t("generation.style_ref.anima_drop_image")
                : locale.t("generation.style_ref.drop_image")}
              <span class="cursor-pointer text-indigo-400 underline"
                >{locale.t("generation.style_ref.upload")}</span
              >
            {/if}
          </div>
          {#if isSnapref && image}
            <label for="snapref-strength-{slot}" class="mt-2 block text-neutral-400">
              {locale.t("generation.style_ref.strength")} <span class="text-neutral-300">{generation.snaprefReferences[slot].strength.toFixed(2)}</span>
              <InfoTip text={locale.t("generation.style_ref.snapref_strength_tip")} />
              <input id="snapref-strength-{slot}" type="range" min="0" max="2" step="0.05" class="w-full" bind:value={generation.snaprefReferences[slot].strength} onchange={() => generation.saveSettings()} />
            </label>
          {/if}
          {#if isSnapref && slot > 1}
            <button class="touch-target text-neutral-500 hover:text-neutral-300" disabled={uploadingSlot !== null} onclick={() => {
              clearImage(slot);
              generation.snaprefReferences = generation.snaprefReferences.filter((_, i) => i !== slot);
              generation.saveSettings();
            }}>{locale.t("generation.style_ref.snapref_remove")}</button>
          {/if}
        </div>
      {/each}
      {#if isSnapref && slots.length < 8}
        <button class="touch-target w-full rounded-lg border border-neutral-700 px-3 py-2 text-indigo-400 hover:border-indigo-500" disabled={uploadingSlot !== null} onclick={() => {
          generation.snaprefReferences = [...generation.snaprefReferences, { image: null, strength: 1 }];
          generation.saveSettings();
        }}>{locale.t("generation.style_ref.snapref_add")}</button>
      {/if}

      {#if isAnima && !isSnapref}
        <!-- Reference guidance -->
        <div>
          <div class="mb-1 text-neutral-400">
            {locale.t("generation.style_ref.anima_guidance")}
            <InfoTip text={locale.t("generation.style_ref.anima_guidance_tip")} />
            <span class="ml-1 text-neutral-300">{generation.animaRefGuidance.toFixed(1)}</span>
          </div>
          <input
            type="range"
            min="1"
            max="10"
            step="0.1"
            value={generation.animaRefGuidance}
            oninput={(e) => {
              generation.animaRefGuidance = parseFloat((e.target as HTMLInputElement).value);
            }}
            onchange={() => generation.saveSettings()}
            class="w-full"
          />
        </div>

        <!-- Colour correction -->
        <div>
          <div class="mb-1 text-neutral-400">
            {locale.t("generation.style_ref.anima_correction")}
            <InfoTip text={locale.t("generation.style_ref.anima_correction_tip")} />
          </div>
          <select
            value={generation.animaRefCorrection}
            onchange={(e) => {
              generation.animaRefCorrection = (e.target as HTMLSelectElement).value;
              generation.saveSettings();
            }}
            class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-neutral-100 transition-colors focus:border-indigo-500 focus:outline-none"
          >
            {#each animaRefCorrections as c}
              <option value={c}>{c}</option>
            {/each}
          </select>
        </div>

        <!-- Reference guidance end -->
        <div>
          <div class="mb-1 text-neutral-400">
            {locale.t("generation.style_ref.end")}
            <InfoTip text={locale.t("generation.style_ref.anima_end_tip")} />
            <span class="ml-1 text-neutral-300">{generation.styleRefEnd.toFixed(2)}</span>
          </div>
          <input
            type="range"
            min="0"
            max="1"
            step="0.05"
            value={generation.styleRefEnd}
            oninput={(e) => {
              generation.styleRefEnd = parseFloat((e.target as HTMLInputElement).value);
            }}
            onchange={() => generation.saveSettings()}
            class="w-full"
          />
        </div>
      {/if}

      {#if !isAnima}
      <!-- Strength -->
      <div>
        <div class="mb-1 text-neutral-400">
          {locale.t("generation.style_ref.strength")}
          <InfoTip text={locale.t("generation.style_ref.strength_tip")} />
          <span class="ml-1 text-neutral-300">{generation.styleRefStrength.toFixed(2)}</span>
        </div>
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={generation.styleRefStrength}
          oninput={(e) => {
            generation.styleRefStrength = parseFloat((e.target as HTMLInputElement).value);
          }}
          onchange={() => generation.saveSettings()}
          class="w-full"
        />
      </div>

      <!-- Weight type -->
      <div>
        <div class="mb-1 text-neutral-400">
          {locale.t("generation.style_ref.weight_type")}
          <InfoTip
            text={isRedux
              ? locale.t("generation.style_ref.weight_type_tip_redux")
              : locale.t("generation.style_ref.weight_type_tip_ipadapter")}
          />
        </div>
        <select
          value={generation.styleRefWeightType}
          onchange={(e) => {
            generation.styleRefWeightType = (e.target as HTMLSelectElement).value;
            generation.saveSettings();
          }}
          class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-neutral-100 transition-colors focus:border-indigo-500 focus:outline-none"
        >
          {#each isRedux ? reduxWeightTypes : ipadapterWeightTypes as wt}
            <option value={wt}>{wt}</option>
          {/each}
        </select>
      </div>
      {/if}

      <!-- Start / End (IP-Adapter only) -->
      {#if isIPAdapter}
        <div class="grid grid-cols-2 gap-2">
          <div>
            <div class="mb-1 text-neutral-400">
              {locale.t("generation.style_ref.start")}
              <InfoTip text={locale.t("generation.style_ref.start_end_tip")} />
              <span class="ml-1 text-neutral-300">{generation.styleRefStart.toFixed(2)}</span>
            </div>
            <input
              type="range"
              min="0"
              max="1"
              step="0.05"
              value={generation.styleRefStart}
              oninput={(e) => {
                generation.styleRefStart = parseFloat((e.target as HTMLInputElement).value);
              }}
              onchange={() => generation.saveSettings()}
              class="w-full"
            />
          </div>
          <div>
            <div class="mb-1 text-neutral-400">
              {locale.t("generation.style_ref.end")}
              <span class="ml-1 text-neutral-300">{generation.styleRefEnd.toFixed(2)}</span>
            </div>
            <input
              type="range"
              min="0"
              max="1"
              step="0.05"
              value={generation.styleRefEnd}
              oninput={(e) => {
                generation.styleRefEnd = parseFloat((e.target as HTMLInputElement).value);
              }}
              onchange={() => generation.saveSettings()}
              class="w-full"
            />
          </div>
        </div>
      {/if}

      <!-- Flux Redux model hints -->
      {#if isRedux}
        <div class="space-y-1.5 text-[11px] text-neutral-500">
          <div>
            {locale.t("generation.style_ref.redux_hf_gated")}
          </div>
          <div>
            {locale.t("generation.style_ref.model_clip_vision_missing_redux")}
            <a href={SIGCLIP_URL} target="_blank" rel="noreferrer" class="ml-1 text-indigo-400 underline">
              {locale.t("generation.style_ref.download_clip_vision_redux")}
            </a>
          </div>
        </div>
      {/if}

      <!-- IP-Adapter model hints -->
      {#if isIPAdapter}
        <div class="space-y-1.5 text-[11px] text-neutral-500">
          <div>
            {locale.t(
              generation.modelFamily === "sd15"
                ? "generation.style_ref.model_ipadapter_sd15_missing"
                : "generation.style_ref.model_ipadapter_sdxl_missing",
            )}
            <a
              href={generation.modelFamily === "sd15" ? IPA_SD15_URL : IPA_SDXL_URL}
              target="_blank"
              rel="noreferrer"
              class="ml-1 text-indigo-400 underline"
            >
              {locale.t(
                generation.modelFamily === "sd15"
                  ? "generation.style_ref.download_ipadapter_sd15"
                  : "generation.style_ref.download_ipadapter_sdxl",
              )}
            </a>
          </div>
          <div>
            {locale.t("generation.style_ref.model_clip_vision_missing_ipadapter")}
            <a href={CLIP_VIT_H_URL} target="_blank" rel="noreferrer" class="ml-1 text-indigo-400 underline">
              {locale.t("generation.style_ref.download_clip_vision_ipadapter")}
            </a>
          </div>
        </div>
      {/if}
    {/if}
  {/if}
</div>
