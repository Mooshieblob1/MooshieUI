<script lang="ts">
  import { onDestroy } from "svelte";
  import PromptTextarea from "./PromptTextarea.svelte";
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptPresets } from "../../stores/promptPresets.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";

  interface Props {
    id: string;
    name: string;
    content: string;
    index: number;
    side: "positive" | "negative";
  }

  let { id, name, content, index, side }: Props = $props();

  // Extra boxes are concatenated into the same prompt the main box feeds, so
  // they get the same treatment: no danbooru tag assistance on H3 prose.
  const isVideoMode = $derived(generation.mode === "video");

  // svelte-ignore state_referenced_locally
  // Intentional: seed the local editing copy from the prop's initial value.
  // Later prop changes are adopted by the $effect below.
  let localContent = $state(content);
  // Plain (non-reactive) mirror of the last value we synced in either direction.
  // The effects read it but do NOT track it, so they only fire on real prop /
  // input changes — never reverting an in-flight keystroke.
  // svelte-ignore state_referenced_locally
  let lastSynced = content;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

  // Adopt external content changes (server prefs, settings reload) into the
  // local editing state. Tracks only `content`, so typing never triggers it.
  $effect(() => {
    if (content !== lastSynced) {
      lastSynced = content;
      localContent = content;
    }
  });

  function commitContent(value: string) {
    lastSynced = value;
    if (side === "positive") generation.updatePositiveBox(id, { content: value });
    else generation.updateNegativeBox(id, { content: value });
  }

  // Debounce store writes (each triggers a settings save) while typing.
  $effect(() => {
    const next = localContent;
    if (next === lastSynced) return;
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => commitContent(next), 400);
  });

  onDestroy(() => clearTimeout(debounceTimer));

  function commitName(value: string) {
    if (side === "positive") generation.updatePositiveBox(id, { name: value });
    else generation.updateNegativeBox(id, { name: value });
  }

  function removeBox() {
    if (side === "positive") generation.removePositiveBox(id);
    else generation.removeNegativeBox(id);
  }

  function saveAsPreset() {
    const trimmed = localContent.trim();
    if (!trimmed) {
      gallery.showToast(locale.t("generation.prompts.extra_box_empty"), "warning");
      return;
    }
    const presetName =
      name.trim() || locale.t("generation.prompts.extra_box_default_name", { index: String(index) });
    promptPresets.create(presetName, trimmed);
    gallery.showToast(
      locale.t("generation.prompts.extra_box_saved", { name: presetName }),
      "success",
    );
  }
</script>

<div class="mt-2 rounded-lg border border-neutral-700/60 bg-neutral-900/40 p-2 space-y-1.5">
  <div class="flex items-center gap-1.5">
    <span class="shrink-0 text-[10px] text-neutral-500 tabular-nums">#{index}</span>
    <input
      type="text"
      value={name}
      onchange={(e) => commitName(e.currentTarget.value)}
      placeholder={locale.t("generation.prompts.extra_box_name_placeholder")}
      class="min-w-0 flex-1 border-b border-neutral-700 bg-transparent px-0.5 py-0 text-xs text-neutral-300 placeholder:text-neutral-600 transition-colors focus:border-indigo-500 focus:outline-none"
    />
    <button
      type="button"
      onclick={saveAsPreset}
      title={locale.t("generation.prompts.extra_box_save_as_preset")}
      class="shrink-0 rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 transition-colors hover:border-indigo-500 hover:text-indigo-200"
    >
      {locale.t("generation.prompts.extra_box_save_as_preset")}
    </button>
    <button
      type="button"
      onclick={removeBox}
      title={locale.t("common.remove")}
      aria-label={locale.t("common.remove")}
      class="shrink-0 rounded-lg border border-neutral-700 px-1.5 py-0.5 text-[11px] leading-none text-neutral-400 transition-colors hover:border-red-500/60 hover:text-red-300"
    >
      ×
    </button>
  </div>
  <PromptTextarea
    bind:value={localContent}
    placeholder={side === "positive"
      ? locale.t("generation.prompts.extra_box_positive_placeholder")
      : locale.t("generation.prompts.extra_box_negative_placeholder")}
    rows={3}
    minHeight="min-h-16"
    storageKey={`mooshieui.promptHeight.extra.${side}.${id}`}
    tagAssist={!isVideoMode}
  />
</div>
