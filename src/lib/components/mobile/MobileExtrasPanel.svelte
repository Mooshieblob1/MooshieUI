<script lang="ts">
  import MobileBottomSheet from "./MobileBottomSheet.svelte";
  import LoraGallery from "../generation/LoraGallery.svelte";
  import CheckpointGallery from "../generation/CheckpointGallery.svelte";
  import CompareGrid from "../generation/CompareGrid.svelte";
  import StyleManager from "../generation/StyleManager.svelte";
  import ScheduleBuilder from "../generation/ScheduleBuilder.svelte";
  import { models } from "../../stores/models.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { lazyThumbnail } from "../../utils/lazyThumbnail.js";

  type Tool = "loras" | "checkpoints" | "images" | "compare" | "styles" | "schedule";

  let openTool = $state<Tool | null>(null);
  let sheetSnap = $state<"half" | "full">("full");

  function tt(key: string, fb: string) {
    const v = locale.t(key);
    return v === key ? fb : v;
  }

  const showCheckpoints = $derived(
    models.checkpoints.length > 10 || generation.devMode,
  );
  const activeLoraCount = $derived(
    generation.loras.filter((l) => l.enabled && l.name).length,
  );
  const sessionImageCount = $derived(gallery.sessionImages.length);

  interface ToolDef {
    id: Tool;
    label: string;
    badge?: number;
    visible?: boolean;
    icon: string;
  }

  const tools: ToolDef[] = $derived(
    ([
      {
        id: "loras" as Tool,
        label: tt("bottom_panel.tab.loras", "LoRAs"),
        badge: activeLoraCount,
        icon:
          '<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/>',
      },
      {
        id: "checkpoints" as Tool,
        label: tt("bottom_panel.tab.checkpoints", "Checkpoints"),
        visible: showCheckpoints,
        icon:
          '<path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.27 6.96 12 12.01 20.73 6.96"/><line x1="12" y1="22.08" x2="12" y2="12"/>',
      },
      {
        id: "images" as Tool,
        label: tt("bottom_panel.tab.images", "Session"),
        badge: sessionImageCount,
        icon:
          '<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/>',
      },
      {
        id: "styles" as Tool,
        label: tt("bottom_panel.tab.styles", "Styles"),
        icon:
          '<path d="M19 11H5a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-6a2 2 0 0 0-2-2z"/><path d="M17 11V7a5 5 0 0 0-10 0v4"/>',
      },
      {
        id: "schedule" as Tool,
        label: tt("bottom_panel.tab.schedule", "Schedule"),
        icon:
          '<rect x="3" y="4" width="18" height="18" rx="2"/><line x1="16" y1="2" x2="16" y2="6"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/>',
      },
      {
        id: "compare" as Tool,
        label: tt("bottom_panel.tab.compare", "Compare"),
        icon:
          '<rect x="3" y="3" width="8" height="18" rx="1"/><rect x="13" y="3" width="8" height="18" rx="1"/>',
      },
    ] as ToolDef[]).filter((t) => t.visible !== false),
  );

  function open(tool: Tool) {
    openTool = tool;
    sheetSnap = "full";
  }
</script>

<div
  class="shrink-0 w-12 border-l border-neutral-800 bg-neutral-900/60 overflow-y-auto no-scroll-chain"
>
  <div class="flex flex-col items-stretch py-1.5 gap-0.5">
    {#each tools as tool}
      <button
        type="button"
        title={tool.label}
        aria-label={tool.label}
        onclick={() => open(tool.id)}
        class="relative flex items-center justify-center w-full h-11 text-neutral-400 hover:text-neutral-100 hover:bg-neutral-800/70 rounded transition-colors"
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          class="w-5 h-5"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round">{@html tool.icon}</svg
        >
        {#if tool.badge && tool.badge > 0}
          <span
            class="absolute top-1 right-1 min-w-[16px] h-4 px-1 rounded-full bg-indigo-600 text-white text-[9px] font-semibold flex items-center justify-center leading-none"
          >{tool.badge > 99 ? "99+" : tool.badge}</span>
        {/if}
      </button>
    {/each}
  </div>
</div>

<MobileBottomSheet
  open={openTool !== null}
  bind:snap={sheetSnap}
  snaps={["half", "full"]}
  onClose={() => (openTool = null)}
  title={openTool ? (tools.find((t) => t.id === openTool)?.label ?? "") : ""}
>
  {#if openTool === "loras"}
    <LoraGallery />
  {:else if openTool === "checkpoints"}
    <CheckpointGallery />
  {:else if openTool === "images"}
    {#if gallery.sessionImages.length === 0}
      <p class="text-xs text-neutral-500 text-center py-8">
        {tt("bottom_panel.no_session_images", "No session images yet.")}
      </p>
    {:else}
      <div class="grid grid-cols-3 gap-1.5">
        {#each gallery.sessionImages as image}
          <button
            type="button"
            class="aspect-square rounded-md overflow-hidden border border-neutral-800 hover:border-indigo-500 transition-colors"
            onclick={() => {
              gallery.openLightbox(image);
              openTool = null;
            }}
          >
            <img
              use:lazyThumbnail={{ image, size: 256 }}
              alt={image.filename}
              class="w-full h-full object-cover"
            />
          </button>
        {/each}
      </div>
    {/if}
  {:else if openTool === "styles"}
    <StyleManager />
  {:else if openTool === "schedule"}
    <ScheduleBuilder />
  {:else if openTool === "compare"}
    <CompareGrid />
  {/if}
</MobileBottomSheet>
