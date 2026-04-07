<script lang="ts">
  import { getModelInstallDirs, openDirectory, type ModelInstallDir } from "../../utils/api.js";
  import { locale } from "../../stores/locale.svelte.js";

  interface ModelCategory {
    id: string;
    label: () => string;
    category: string;
  }

  const categories: ModelCategory[] = [
    { id: "checkpoints", label: () => locale.t('settings.paths.open_folder.checkpoints'), category: "checkpoints" },
    { id: "loras", label: () => locale.t('settings.paths.open_folder.loras'), category: "loras" },
    { id: "vae", label: () => locale.t('settings.paths.open_folder.vae'), category: "vae" },
    { id: "upscale", label: () => locale.t('settings.paths.open_folder.upscale'), category: "upscale_models" },
    { id: "ultralytics", label: () => locale.t('settings.paths.open_folder.facefix'), category: "ultralytics" },
    { id: "embeddings", label: () => locale.t('settings.paths.open_folder.embeddings'), category: "embeddings" },
    { id: "controlnet", label: () => locale.t('settings.paths.open_folder.controlnet'), category: "controlnet" },
    { id: "clip", label: () => locale.t('settings.paths.open_folder.clip'), category: "text_encoders" },
    { id: "diffusion", label: () => locale.t('settings.paths.open_folder.diffusion'), category: "diffusion_models" },
  ];

  let pickerDirs = $state<ModelInstallDir[]>([]);
  let pickerOpen = $state(false);
  let pickerLabel = $state("");

  async function handleOpen(cat: ModelCategory) {
    try {
      const dirs = await getModelInstallDirs(cat.category);
      if (dirs.length === 0) return;
      if (dirs.length === 1) {
        await openDirectory(dirs[0].path);
      } else {
        pickerDirs = dirs;
        pickerLabel = cat.label();
        pickerOpen = true;
      }
    } catch (e) {
      console.error("Failed to open model folder:", e);
    }
  }

  async function pickDir(dir: ModelInstallDir) {
    pickerOpen = false;
    try {
      await openDirectory(dir.path);
    } catch (e) {
      console.error("Failed to open directory:", e);
    }
  }
</script>

<div>
  <label class="block text-xs text-neutral-400 mb-2">{locale.t('settings.paths.open_folder.title')}</label>
  <div class="grid grid-cols-3 gap-1.5">
    {#each categories as cat}
      <button
        onclick={() => handleOpen(cat)}
        class="flex items-center gap-1.5 px-2.5 py-2 rounded-lg border border-neutral-700 bg-neutral-800 text-xs text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
        {cat.label()}
      </button>
    {/each}
  </div>
</div>

{#if pickerOpen}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
    onmousedown={(e) => { if (e.target === e.currentTarget) pickerOpen = false; }}
  >
    <div class="bg-neutral-900 border border-neutral-700 rounded-xl p-5 w-96 max-w-[90vw] shadow-2xl">
      <h3 class="text-sm font-medium text-neutral-200 mb-1">{pickerLabel}</h3>
      <p class="text-xs text-neutral-500 mb-3">{locale.t('settings.paths.open_folder.pick_directory')}</p>
      <div class="space-y-1.5">
        {#each pickerDirs as dir}
          <button
            onclick={() => pickDir(dir)}
            class="w-full text-left px-3 py-2.5 rounded-lg border border-neutral-700 bg-neutral-800 hover:border-indigo-500 hover:bg-neutral-750 transition-colors"
          >
            <p class="text-sm text-neutral-200">{dir.label}</p>
            <p class="text-[10px] text-neutral-500 truncate mt-0.5">{dir.path}</p>
          </button>
        {/each}
      </div>
      <button
        onclick={() => (pickerOpen = false)}
        class="mt-3 w-full py-2 rounded-lg border border-neutral-700 text-xs text-neutral-400 hover:text-neutral-200 hover:border-neutral-600 transition-colors"
      >
        {locale.t('common.cancel')}
      </button>
    </div>
  </div>
{/if}
