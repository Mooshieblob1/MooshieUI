<script lang="ts">
  import { onMount } from "svelte";
  import { model3d, ENGINES, GAUSSIAN_CHOICES, MESH_DETAILS } from "../../stores/model3d.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import GalleryPickerModal from "../gallery/GalleryPickerModal.svelte";
  import Model3DViewer from "./Model3DViewer.svelte";
  import type { OutputImage } from "../../types/index.js";

  let { userRole = "admin" }: { userRole?: string } = $props();
  const canDownload = $derived(userRole === "admin" || userRole === "moderator");

  let fileInput: HTMLInputElement | undefined = $state();
  let viewer: Model3DViewer | undefined = $state();
  let pickerOpen = $state(false);
  let dragging = $state(false);
  let autoRotate = $state(true);
  let titleDraft = $state("");

  const inputClass = "w-full min-w-0 rounded-md border border-transparent bg-neutral-900 px-3 py-2 text-sm text-neutral-200 placeholder:text-neutral-500 transition-colors hover:border-indigo-400/20 focus:border-indigo-400/50 focus:outline-none disabled:opacity-50";

  let now = $state(Date.now());
  const missing = $derived(model3d.missingFiles);
  const missingBytes = $derived(missing.reduce((sum, f) => sum + f.sizeBytes, 0));
  const selected = $derived(model3d.selected);
  const pixal = $derived(model3d.engine === "pixal3d");
  const elapsed = $derived(model3d.busy && model3d.startedAt ? Math.max(0, Math.round((now - model3d.startedAt) / 1000)) : 0);

  $effect(() => {
    if (!model3d.busy) return;
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  $effect(() => {
    titleDraft = selected?.title ?? "";
  });

  onMount(() => {
    void model3d.loadLibrary();
    if (connection.connected) void model3d.refresh();
  });

  $effect(() => {
    if (connection.connected && !model3d.capabilities && !model3d.refreshing) void model3d.refresh();
  });

  const size = (bytes: number) => locale.formatBytes(bytes);

  function takeFile(file: File | undefined | null) {
    if (file) model3d.setInput(file, file.name);
  }

  function ondrop(event: DragEvent) {
    event.preventDefault();
    dragging = false;
    takeFile(event.dataTransfer?.files?.[0]);
  }

  function onpaste(event: ClipboardEvent) {
    const item = [...(event.clipboardData?.items ?? [])].find((i) => i.type.startsWith("image/"));
    const file = item?.getAsFile();
    if (file) { event.preventDefault(); takeFile(file); }
  }

  function pick(images: OutputImage[]) {
    pickerOpen = false;
    if (images[0]) void model3d.useGalleryImage(images[0]);
  }

  function stageLabel() {
    if (model3d.phase === "submitting") return locale.t("model3d.stage.submitting");
    if (model3d.phase === "queued") {
      return model3d.queuePosition ? locale.t("model3d.stage.queued_at", { position: model3d.queuePosition }) : locale.t("model3d.stage.queued");
    }
    return locale.t(`model3d.stage.${model3d.stage}`);
  }

  function commitTitle() {
    if (selected && titleDraft.trim() !== selected.title) void model3d.rename(selected, titleDraft);
  }
</script>

<svelte:window {onpaste} />

<div class="h-full min-h-0 overflow-y-auto overscroll-contain px-4 py-5 pb-[max(env(safe-area-inset-bottom),1rem)] md:px-6">
  <div class="mx-auto max-w-6xl">
    <header class="mb-6 flex items-center justify-between gap-4">
      <div class="flex items-baseline gap-3 border-l-[3px] border-indigo-400 pl-3">
        <h1 class="text-xl font-semibold tracking-tight text-neutral-100 sm:text-2xl">{locale.t("model3d.title")}</h1>
        <span class="rounded-sm border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-amber-300">{locale.t("model3d.experimental")}</span>
        <span class="text-xs text-neutral-500">{pixal ? "Pixal3D · TRELLIS.2" : "Idol · TripoSplat"}</span>
      </div>
      <button class="touch-target inline-flex shrink-0 items-center justify-center gap-2 rounded-sm px-2 text-xs text-neutral-400 hover:bg-neutral-900 hover:text-neutral-200 focus-visible:outline-2 focus-visible:outline-indigo-400 disabled:opacity-40" title={locale.t("model3d.refresh")} disabled={!connection.connected || model3d.refreshing} onclick={() => model3d.refresh()}>
        <svg class={`h-3.5 w-3.5 ${model3d.refreshing ? 'motion-safe:animate-spin' : ''}`} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 7v5h-5M4 17v-5h5M6 6a8 8 0 0 1 13 2l1 4M4 12l1 4a8 8 0 0 0 13 2"/></svg>
        <span>{locale.t("model3d.refresh")}</span>
      </button>
    </header>

    <p class="-mt-3 mb-5 text-xs text-neutral-500">{locale.t(pixal ? "model3d.experimental_note_pixal3d" : "model3d.experimental_note")}</p>

    {#if !connection.connected}
      <p class="mb-5 text-sm text-neutral-400">{locale.t("model3d.connect")}</p>
    {:else if model3d.engineCapabilities && !model3d.nodesReady}
      <p class="mb-5 border-l-2 border-amber-500 pl-3 text-sm text-amber-200">{locale.t("model3d.needs_comfyui", { nodes: model3d.engineCapabilities.missing_nodes.join(", ") })}</p>
    {:else if model3d.engineCapabilities && missing.length > 0}
      <div class="mb-5 space-y-2 border-l-2 border-indigo-400 pl-3 text-sm text-neutral-300">
        <p>{locale.t(pixal ? "model3d.needs_models_pixal3d" : "model3d.needs_models", { size: size(missingBytes) })}</p>
        <ul class="list-inside list-disc text-xs text-neutral-500">
          {#each missing as file (file.filename)}<li>{file.category}/{file.filename}</li>{/each}
        </ul>
        {#if canDownload}
          <button class="touch-target rounded-sm bg-indigo-500 px-4 text-sm text-[var(--theme-accent-foreground)] hover:bg-indigo-400 disabled:opacity-40" disabled={model3d.downloading} onclick={() => model3d.downloadMissing()}>
            {model3d.downloading ? locale.t("model3d.downloading", { file: model3d.downloadingFile }) : locale.t(pixal ? "model3d.download_pixal3d" : "model3d.download", { size: size(missingBytes) })}
          </button>
        {:else}
          <p class="text-xs text-neutral-500">{locale.t(pixal ? "model3d.ask_admin_pixal3d" : "model3d.ask_admin")}</p>
        {/if}
      </div>
    {/if}

    <div class="grid items-start gap-7 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] lg:gap-8">
      <form class="min-w-0 space-y-4" onsubmit={(event) => { event.preventDefault(); void model3d.generate(); }} onchange={() => model3d.saveSettings()}>
        <fieldset class="space-y-1.5" disabled={model3d.busy}>
          <legend class="mb-1.5 text-sm font-medium text-neutral-200">{locale.t("model3d.engine")}</legend>
          <div class="grid gap-2 sm:grid-cols-2">
            {#each ENGINES as engine (engine)}
              <label class="flex cursor-pointer items-start gap-2.5 rounded-md border p-2.5 text-sm transition-colors has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-indigo-400 {model3d.engine === engine ? 'border-indigo-400/60 bg-indigo-500/10 text-neutral-100' : 'border-neutral-800 bg-neutral-900 text-neutral-300 hover:border-neutral-700'}">
                <input type="radio" name="model3d-engine" class="mt-1 accent-indigo-500" value={engine} checked={model3d.engine === engine} onchange={() => model3d.setEngine(engine)} />
                <span>{locale.t(`model3d.engine_${engine}`)}<span class="block text-xs text-neutral-500">{locale.t(`model3d.engine_${engine}_help`)}</span></span>
              </label>
            {/each}
          </div>
        </fieldset>

        <div>
          <p class="mb-2 text-sm font-medium text-neutral-200">{locale.t("model3d.input")}</p>
          <div
            class="relative flex aspect-square w-full items-center justify-center overflow-hidden rounded-md border border-dashed transition-colors {dragging ? 'border-indigo-400 bg-indigo-500/10' : 'border-neutral-700 bg-neutral-900'}"
            role="group"
            aria-label={locale.t("model3d.input")}
            ondragover={(event) => { event.preventDefault(); dragging = true; }}
            ondragleave={() => (dragging = false)}
            {ondrop}
          >
            {#if model3d.inputUrl}
              <img src={model3d.inputUrl} alt={model3d.inputName} class="h-full w-full object-contain" />
              <button type="button" class="absolute right-2 top-2 rounded bg-black/70 px-2 py-1 text-xs text-neutral-200 hover:bg-black/90" disabled={model3d.busy} onclick={() => model3d.clearInput()}>{locale.t("model3d.clear_input")}</button>
            {:else}
              <div class="space-y-1 px-6 text-center">
                <p class="text-sm text-neutral-300">{model3d.loadingInput ? locale.t("model3d.loading_input") : locale.t("model3d.drop_hint")}</p>
                <p class="text-xs text-neutral-500">{locale.t("model3d.input_tip")}</p>
              </div>
            {/if}
          </div>
          <div class="mt-2 flex flex-wrap gap-2">
            <button type="button" class="touch-target rounded-sm bg-neutral-800 px-3 text-sm text-neutral-200 hover:bg-neutral-700 disabled:opacity-40" disabled={model3d.busy} onclick={() => fileInput?.click()}>{locale.t("model3d.choose_file")}</button>
            <button type="button" class="touch-target rounded-sm bg-neutral-800 px-3 text-sm text-neutral-200 hover:bg-neutral-700 disabled:opacity-40" disabled={model3d.busy || model3d.loadingInput} onclick={() => (pickerOpen = true)}>{locale.t("model3d.from_gallery")}</button>
            <input bind:this={fileInput} type="file" accept="image/png,image/jpeg,image/webp" class="hidden" onchange={(event) => { takeFile((event.currentTarget as HTMLInputElement).files?.[0]); (event.currentTarget as HTMLInputElement).value = ""; }} />
          </div>
        </div>

        {#if pixal}
          {#if model3d.models.length > 1}
            <label class="block space-y-1.5 text-sm text-neutral-200"><span>{locale.t("model3d.model")}</span>
              <select class={inputClass} bind:value={model3d.meshParams.model} disabled={model3d.busy}>
                {#each model3d.models as name (name)}<option value={name}>{name}</option>{/each}
              </select>
            </label>
          {/if}

          <label class="flex items-start gap-3 text-sm text-neutral-200">
            <input type="checkbox" class="mt-1 accent-indigo-500" bind:checked={model3d.meshParams.remove_background} disabled={model3d.busy} />
            <span>{locale.t("model3d.remove_background")}<span class="block text-xs text-neutral-500">{locale.t("model3d.remove_background_help")}</span></span>
          </label>

          <label class="block space-y-1.5 text-sm text-neutral-200"><span>{locale.t("model3d.detail")}</span>
            <select class={inputClass} bind:value={model3d.meshParams.detail} disabled={model3d.busy}>
              {#each MESH_DETAILS as detail (detail.value)}
                <option value={detail.value}>{locale.t(`model3d.mesh_detail_${detail.value}`, { faces: locale.formatInteger(detail.faces), size: detail.texture })}</option>
              {/each}
            </select>
          </label>

          <details class="text-sm text-neutral-300">
            <summary class="touch-target cursor-pointer content-center text-xs text-neutral-400">{locale.t("model3d.advanced")}</summary>
            <label class="block max-w-48 space-y-1 pt-2 text-xs text-neutral-400"><span>{locale.t("model3d.seed")}</span><input class={inputClass} type="text" inputmode="numeric" pattern="-?[0-9]*" bind:value={model3d.meshParams.seed} disabled={model3d.busy} /></label>
            <p class="pt-2 text-xs text-neutral-500">{locale.t("model3d.advanced_help_pixal3d")}</p>
          </details>
        {:else}
        {#if model3d.models.length > 1}
          <label class="block space-y-1.5 text-sm text-neutral-200"><span>{locale.t("model3d.model")}</span>
            <select class={inputClass} bind:value={model3d.params.model} disabled={model3d.busy}>
              {#each model3d.models as name (name)}<option value={name}>{name}</option>{/each}
            </select>
          </label>
        {/if}

        <label class="flex items-start gap-3 text-sm text-neutral-200">
          <input type="checkbox" class="mt-1 accent-indigo-500" bind:checked={model3d.params.remove_background} disabled={model3d.busy} />
          <span>{locale.t("model3d.remove_background")}<span class="block text-xs text-neutral-500">{locale.t("model3d.remove_background_help")}</span></span>
        </label>

        <label class="block space-y-1.5 text-sm text-neutral-200"><span>{locale.t("model3d.detail")}</span>
          <select class={inputClass} bind:value={model3d.params.num_gaussians} disabled={model3d.busy}>
            {#each GAUSSIAN_CHOICES as count (count)}
              <option value={count}>{locale.t(`model3d.detail_${count}`, { count: locale.formatInteger(count) })}</option>
            {/each}
          </select>
        </label>

        <details class="text-sm text-neutral-300">
          <summary class="touch-target cursor-pointer content-center text-xs text-neutral-400">{locale.t("model3d.advanced")}</summary>
          <div class="grid grid-cols-3 gap-3 pt-2">
            <label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("model3d.steps")}</span><input class={inputClass} type="number" min="4" max="100" step="1" bind:value={model3d.params.steps} disabled={model3d.busy} /></label>
            <label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("model3d.guidance")}</span><input class={inputClass} type="number" min="1" max="10" step="0.5" bind:value={model3d.params.cfg} disabled={model3d.busy} /></label>
            <label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("model3d.seed")}</span><input class={inputClass} type="text" inputmode="numeric" pattern="-?[0-9]*" bind:value={model3d.params.seed} disabled={model3d.busy} /></label>
          </div>
          <p class="pt-2 text-xs text-neutral-500">{locale.t("model3d.advanced_help")}</p>
        </details>
        {/if}

        <div class="flex items-center gap-3">
          <button type="submit" class="touch-target inline-flex min-h-12 flex-1 items-center justify-center gap-2.5 rounded-md bg-indigo-500 px-4 text-sm font-semibold text-[var(--theme-accent-foreground)] transition-colors hover:bg-indigo-400 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 disabled:opacity-40" disabled={!connection.connected || !model3d.ready || !model3d.inputUrl || model3d.busy}>
            <svg class={`h-4 w-4 ${model3d.busy ? 'motion-safe:animate-pulse' : ''}`} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 2 3 7v10l9 5 9-5V7z"/><path d="M3 7l9 5 9-5M12 12v10"/></svg>
            {model3d.busy ? locale.t("model3d.generating") : locale.t("model3d.generate")}
          </button>
          {#if model3d.busy}<button type="button" class="touch-target rounded-sm px-3 text-sm text-neutral-400 hover:bg-neutral-900 disabled:opacity-40" disabled={model3d.cancelling} onclick={() => model3d.cancel()}>{locale.t("common.cancel")}</button>{/if}
        </div>
        {#if model3d.busy}
          <div class="space-y-1.5" role="status">
            <div class="flex justify-between text-xs text-neutral-400"><span>{stageLabel()}</span><span>{elapsed} s</span></div>
            <div class="h-1 overflow-hidden rounded-full bg-neutral-800">
              <div class="h-full bg-indigo-400 transition-[width]" style="width: {model3d.progressPercent}%"></div>
            </div>
          </div>
        {/if}
        {#if model3d.error}<p class="text-sm text-red-300" role="alert">{model3d.error}</p>{/if}
        <p class="text-xs leading-relaxed text-neutral-500">{locale.t(pixal ? "model3d.about_pixal3d" : "model3d.about")}</p>
      </form>

      <section class="min-w-0 space-y-4" aria-label={locale.t("model3d.result")}>
        <div class="relative h-[min(70vh,40rem)] min-h-72">
          <Model3DViewer bind:this={viewer} data={model3d.current} bind:autoRotate />
          {#if !model3d.current}
            <p class="pointer-events-none absolute inset-0 flex items-center justify-center px-8 text-center text-sm text-neutral-500">{model3d.loadingResult ? locale.t("model3d.loading_result") : locale.t("model3d.empty")}</p>
          {/if}
        </div>
        {#if selected && model3d.current}
          <div class="flex flex-wrap items-center gap-2">
            <input class="{inputClass} max-w-64 flex-1" type="text" maxlength="120" bind:value={titleDraft} onblur={commitTitle} onkeydown={(event) => { if (event.key === "Enter") (event.currentTarget as HTMLInputElement).blur(); }} aria-label={locale.t("model3d.name")} />
            <button type="button" class="touch-target rounded-sm px-2 text-xs text-neutral-300 hover:bg-neutral-900" onclick={() => viewer?.resetView()}>{locale.t("model3d.reset_view")}</button>
            <button type="button" class="touch-target rounded-sm px-2 text-xs hover:bg-neutral-900 {autoRotate ? 'text-indigo-300' : 'text-neutral-300'}" aria-pressed={autoRotate} onclick={() => (autoRotate = !autoRotate)}>{locale.t("model3d.spin")}</button>
            {#if selected.kind === "mesh"}
              <button type="button" class="touch-target rounded-sm bg-neutral-800 px-3 text-xs text-neutral-200 hover:bg-neutral-700" onclick={() => model3d.exportResult(selected, "glb")}>{locale.t("model3d.export_glb")}</button>
            {:else}
              <button type="button" class="touch-target rounded-sm bg-neutral-800 px-3 text-xs text-neutral-200 hover:bg-neutral-700" onclick={() => model3d.exportResult(selected, "ply")}>{locale.t("model3d.export_ply")}</button>
              <button type="button" class="touch-target rounded-sm bg-neutral-800 px-3 text-xs text-neutral-200 hover:bg-neutral-700" onclick={() => model3d.exportResult(selected, "splat")}>{locale.t("model3d.export_splat")}</button>
            {/if}
          </div>
          <p class="text-xs text-neutral-500">
            {locale.t(selected.kind === "mesh" ? "model3d.details_mesh" : "model3d.details", { count: locale.formatInteger(selected.count), seed: selected.seed, model: selected.model })}
            {#if model3d.exportStatus}<span class="ml-2 text-emerald-300">{model3d.exportStatus}</span>{/if}
          </p>
          <p class="text-xs text-neutral-500">{locale.t("model3d.controls")}</p>
        {/if}

        <div>
          <h2 class="mb-2 text-sm font-medium text-neutral-200">{locale.t("model3d.library")}</h2>
          {#if model3d.results.length === 0}
            <p class="text-xs text-neutral-500">{locale.t("model3d.library_empty")}</p>
          {:else}
            <ul class="grid grid-cols-3 gap-2 sm:grid-cols-4 lg:grid-cols-5">
              {#each model3d.results as result (result.id)}
                <li class="group relative">
                  <button type="button" class="block aspect-square w-full overflow-hidden rounded-md border bg-neutral-900 {model3d.selectedId === result.id ? 'border-indigo-400' : 'border-transparent hover:border-neutral-600'}" aria-pressed={model3d.selectedId === result.id} title={result.title} onclick={() => model3d.select(result)}>
                    {#if result.thumbnail}<img src={result.thumbnail} alt={result.title} class="h-full w-full object-contain" />{:else}<span class="text-xs text-neutral-500">{result.title}</span>{/if}
                  </button>
                  <button type="button" class="absolute right-1 top-1 hidden h-6 w-6 items-center justify-center rounded bg-black/70 text-xs text-neutral-200 hover:bg-red-700 group-hover:flex group-focus-within:flex pointer-coarse:flex" title={locale.t("model3d.delete")} aria-label={locale.t("model3d.delete")} onclick={() => { if (confirm(locale.t("model3d.delete_confirm"))) void model3d.remove(result); }}>×</button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>
    </div>
  </div>
</div>

<GalleryPickerModal open={pickerOpen} title={locale.t("model3d.pick_title")} onselect={pick} onclose={() => (pickerOpen = false)} />
