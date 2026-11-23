<script lang="ts">
  import { enhanceChat } from "../../stores/enhanceChat.svelte.js";
  import { enhanceRecipes } from "../../stores/enhanceRecipes.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import {
    parseRecipesImport,
    slugifyRecipe,
    type EnhanceRecipe,
    type EnhanceRecipeDraft,
    type RecipeApplyMode,
    type RecipeScope,
  } from "../../utils/enhanceRecipes.js";
  import { isLlmCancelled, saveTextFile } from "../../utils/api.js";
  import { mapLlmError } from "../../utils/llmError.js";

  let mode = $state<"list" | "edit">("list");
  let editingId = $state<string | null>(null);
  let name = $state("");
  let slug = $state("");
  let slugTouched = $state(false);
  let scope = $state<RecipeScope>("any");
  let applyMode = $state<RecipeApplyMode>("ai");
  let intent = $state("");
  let include = $state("");
  let avoid = $state("");
  let neverUc = $state("");
  let context = $state("");
  let triggersText = $state("");

  let search = $state("");
  let distilling = $state(false);
  let creating = $state(false);
  let createPrompt = $state("");
  let refining = $state(false);
  let refineFeedback = $state("");
  let errorText = $state<string | null>(null);
  let importInput = $state<HTMLInputElement | null>(null);
  let activeAbort: AbortController | null = null;

  function resetForm() {
    editingId = null;
    name = "";
    slug = "";
    slugTouched = false;
    scope = "any";
    applyMode = "ai";
    intent = "";
    include = "";
    avoid = "";
    neverUc = "";
    context = "";
    triggersText = "";
    createPrompt = "";
    refineFeedback = "";
    errorText = null;
  }

  function applyDraftFields(draft: Partial<EnhanceRecipeDraft>) {
    if (draft.name) {
      name = draft.name;
      if (!slugTouched) slug = draft.slug || slugifyRecipe(draft.name);
    }
    if (draft.slug && !slugTouched) slug = draft.slug;
    if (draft.scope) scope = draft.scope;
    if (draft.applyMode) applyMode = draft.applyMode;
    if (draft.intent !== undefined) intent = draft.intent;
    if (draft.include !== undefined) include = draft.include;
    if (draft.avoid !== undefined) avoid = draft.avoid;
    if (draft.neverUc !== undefined) neverUc = draft.neverUc;
    if (draft.context !== undefined) context = draft.context;
    if (draft.triggers) triggersText = draft.triggers.join(", ");
  }

  function startNew() {
    activeAbort?.abort();
    resetForm();
    mode = "edit";
  }

  function startEdit(recipe: EnhanceRecipe) {
    activeAbort?.abort();
    editingId = recipe.id;
    name = recipe.name;
    slug = recipe.slug;
    slugTouched = true;
    scope = recipe.scope;
    applyMode = recipe.applyMode ?? "ai";
    intent = recipe.intent;
    include = recipe.include;
    avoid = recipe.avoid;
    neverUc = recipe.neverUc;
    context = recipe.context;
    triggersText = recipe.triggers.join(", ");
    refineFeedback = "";
    errorText = null;
    mode = "edit";
  }

  function duplicateRecipe(id: string) {
    const copy = enhanceRecipes.duplicate(id);
    if (copy) startEdit(copy);
  }

  function cancelEdit() {
    activeAbort?.abort();
    activeAbort = null;
    distilling = false;
    creating = false;
    refining = false;
    resetForm();
    mode = "list";
  }

  async function runCreateFromPrompt() {
    if (!createPrompt.trim() || creating || distilling) return;
    if (!promptAssistant.isAvailable) {
      errorText = locale.t("prompt_assistant.no_model");
      return;
    }
    activeAbort?.abort();
    const ctrl = new AbortController();
    activeAbort = ctrl;
    creating = true;
    errorText = null;
    try {
      const draft = await promptAssistant.createEnhanceRecipe(createPrompt, [], ctrl.signal);
      if (ctrl.signal.aborted) return;
      if (!draft) {
        errorText = locale.t("prompt_assistant.recipe_distill_failed");
        return;
      }
      slugTouched = false;
      applyDraftFields(draft);
      createPrompt = "";
      gallery.showToast(locale.t("prompt_assistant.recipe_generated"), "success");
    } catch (e) {
      if (ctrl.signal.aborted || isLlmCancelled(e)) return;
      errorText = mapLlmError(String(e));
    } finally {
      if (activeAbort === ctrl) activeAbort = null;
      creating = false;
    }
  }

  async function runDistill(exchanges: { user: string; reply: string; prompt: string }[]) {
    activeAbort?.abort();
    resetForm();
    mode = "edit";
    if (!promptAssistant.isAvailable) {
      errorText = locale.t("prompt_assistant.no_model");
      return;
    }
    const ctrl = new AbortController();
    activeAbort = ctrl;
    distilling = true;
    errorText = null;
    try {
      const draft = await promptAssistant.distillEnhanceRecipe(exchanges, ctrl.signal);
      if (ctrl.signal.aborted) return;
      if (!draft) {
        errorText = locale.t("prompt_assistant.recipe_distill_failed");
        return;
      }
      applyDraftFields(draft);
    } catch (e) {
      if (ctrl.signal.aborted || isLlmCancelled(e)) return;
      errorText = mapLlmError(String(e));
    } finally {
      if (activeAbort === ctrl) activeAbort = null;
      distilling = false;
    }
  }

  async function runRefine() {
    if (!editingId || !refineFeedback.trim() || refining) return;
    const current = enhanceRecipes.getById(editingId);
    if (!current) return;
    if (!promptAssistant.isAvailable) {
      errorText = locale.t("prompt_assistant.no_model");
      return;
    }
    activeAbort?.abort();
    const ctrl = new AbortController();
    activeAbort = ctrl;
    refining = true;
    errorText = null;
    const snapshot: EnhanceRecipe = {
      ...current,
      name: name.trim() || current.name,
      slug: slugifyRecipe(slug) || current.slug,
      scope,
      applyMode,
      intent: intent.trim(),
      include: include.trim(),
      avoid: avoid.trim(),
      neverUc: neverUc.trim(),
      context: context.trim(),
      triggers: triggersText
        .split(",")
        .map((t) => t.trim().toLowerCase())
        .filter(Boolean),
    };
    try {
      const patch = await promptAssistant.refineEnhanceRecipe(snapshot, refineFeedback, ctrl.signal);
      if (ctrl.signal.aborted) return;
      if (!patch) {
        errorText = locale.t("prompt_assistant.recipe_refine_failed");
        return;
      }
      slugTouched = false;
      applyDraftFields(patch);
      slugTouched = true;
      refineFeedback = "";
      gallery.showToast(locale.t("prompt_assistant.recipe_refined"), "success");
    } catch (e) {
      if (ctrl.signal.aborted || isLlmCancelled(e)) return;
      errorText = mapLlmError(String(e));
    } finally {
      if (activeAbort === ctrl) activeAbort = null;
      refining = false;
    }
  }

  function saveRecipe() {
    const cleanName = name.trim();
    const cleanIntent = intent.trim();
    if (!cleanName || !cleanIntent) return;
    const draft: EnhanceRecipeDraft = {
      name: cleanName,
      slug: slugifyRecipe(slug || cleanName) || "recipe",
      scope,
      applyMode,
      intent: cleanIntent,
      include: include.trim(),
      avoid: avoid.trim(),
      neverUc: neverUc.trim(),
      context: context.trim(),
      triggers: triggersText
        .split(",")
        .map((t) => t.trim().toLowerCase())
        .filter(Boolean),
    };
    if (editingId) {
      enhanceRecipes.update(editingId, draft);
    } else {
      const created = enhanceRecipes.add(draft);
      enhanceRecipes.setActive(created.id, true);
    }
    gallery.showToast(locale.t("prompt_assistant.recipe_saved"), "success");
    resetForm();
    mode = "list";
  }

  async function exportRecipes() {
    const count = enhanceRecipes.recipes.length;
    if (count === 0) return;
    const content = enhanceRecipes.exportJson();
    const filename = "enhance-recipes.json";
    try {
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const path = await save({ defaultPath: filename, filters: [{ name: "JSON", extensions: ["json"] }] });
        if (!path) return;
        await saveTextFile(content, path);
      } else {
        const url = URL.createObjectURL(new Blob([content], { type: "application/json" }));
        const a = document.createElement("a");
        a.href = url;
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
      }
      gallery.showToast(locale.t("prompt_assistant.recipe_exported", { count }), "success");
    } catch (e) {
      console.error("Recipe export failed:", e);
      gallery.showToast(String(e), "error");
    }
  }

  async function onImportFile(e: Event) {
    const el = e.currentTarget as HTMLInputElement;
    const file = el.files?.[0];
    el.value = "";
    if (!file) return;
    try {
      const text = await file.text();
      const parsed = parseRecipesImport(text);
      if (!parsed) {
        gallery.showToast(locale.t("prompt_assistant.recipe_import_invalid"), "error");
        return;
      }
      const count = enhanceRecipes.importRecipes(parsed);
      gallery.showToast(locale.t("prompt_assistant.recipe_imported", { count }), "success");
    } catch {
      gallery.showToast(locale.t("prompt_assistant.recipe_import_invalid"), "error");
    }
  }

  $effect(() => {
    const seed = enhanceChat.recipeDraftSeed;
    if (!seed) return;
    enhanceChat.clearRecipeDraftSeed();
    void runDistill(seed.exchanges);
  });

  $effect(() => {
    const targetId = enhanceChat.editingRecipeId;
    if (!targetId) return;
    enhanceChat.clearEditingRecipeId();
    const target = enhanceRecipes.getById(targetId);
    if (target) startEdit(target);
  });

  $effect(() => {
    return () => {
      activeAbort?.abort();
    };
  });

  const filteredRecipes = $derived.by(() => {
    const q = search.trim().toLowerCase();
    if (!q) return enhanceRecipes.recipes;
    return enhanceRecipes.recipes.filter(
      (r) =>
        r.name.toLowerCase().includes(q) ||
        r.slug.includes(q) ||
        r.intent.toLowerCase().includes(q) ||
        r.triggers.some((t) => t.includes(q)),
    );
  });

  const btn =
    "rounded-lg border border-neutral-600 px-2.5 py-1 text-xs text-neutral-300 hover:bg-neutral-800 disabled:opacity-40";
  const inputCls =
    "w-full rounded-lg border border-neutral-700 bg-neutral-950 px-2.5 py-1.5 text-xs text-neutral-100 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none";
</script>

<div class="flex min-h-0 flex-1 flex-col p-3">
  {#if mode === "list"}
    <div class="mb-3 flex flex-wrap items-center gap-1.5">
      <button
        class="rounded-lg bg-indigo-600 px-2.5 py-1 text-xs font-medium text-white hover:bg-indigo-500"
        onclick={startNew}
      >
        + {locale.t("prompt_assistant.recipe_new")}
      </button>
      <button class={btn} onclick={() => importInput?.click()}>
        {locale.t("prompt_assistant.recipe_import")}
      </button>
      <button class={btn} disabled={enhanceRecipes.recipes.length === 0} onclick={() => void exportRecipes()}>
        {locale.t("prompt_assistant.recipe_export")}
      </button>
      {#if enhanceRecipes.recipes.length >= 6}
        <input
          type="search"
          class="ml-auto w-44 rounded-lg border border-neutral-700 bg-neutral-950 px-2.5 py-1 text-xs text-neutral-100 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
          placeholder={locale.t("prompt_assistant.recipe_search_placeholder")}
          aria-label={locale.t("prompt_assistant.recipe_search_placeholder")}
          bind:value={search}
        />
      {/if}
      <input
        bind:this={importInput}
        type="file"
        accept=".json,application/json"
        class="hidden"
        onchange={onImportFile}
      />
    </div>

    {#if enhanceRecipes.recipes.length === 0}
      <div class="flex flex-1 items-center justify-center rounded-xl border border-dashed border-neutral-800 p-6 text-center text-xs text-neutral-500">
        {locale.t("prompt_assistant.recipe_empty")}
      </div>
    {:else}
      <div class="min-h-0 flex-1 space-y-2 overflow-y-auto">
        {#each filteredRecipes as recipe (recipe.id)}
          {@const active = enhanceRecipes.isActive(recipe.id)}
          <div
            class="flex items-start gap-2.5 rounded-xl border p-2.5 {active
              ? 'border-indigo-500/60 bg-indigo-500/10'
              : 'border-neutral-800 bg-neutral-950/70'}"
          >
            <input
              type="checkbox"
              class="mt-0.5 shrink-0 accent-indigo-500"
              checked={active}
              title={locale.t("prompt_assistant.recipe_active_tip")}
              aria-label={recipe.name}
              onchange={() => enhanceRecipes.toggleActive(recipe.id)}
            />
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="text-xs font-medium text-neutral-100">{recipe.name}</span>
                <span class="rounded bg-neutral-800 px-1.5 py-px font-mono text-[10px] text-neutral-400">#{recipe.slug}</span>
                <span class="rounded bg-neutral-800/80 px-1.5 py-px text-[10px] text-neutral-400">
                  {locale.t(`prompt_assistant.recipe_scope_${recipe.scope}`)}
                </span>
                <span class="rounded bg-neutral-800/80 px-1.5 py-px text-[10px] text-neutral-400">
                  {locale.t(recipe.applyMode === "tags" ? "prompt_assistant.recipe_mode_tags" : "prompt_assistant.recipe_mode_ai")}
                </span>
              </div>
              {#if recipe.intent}
                <div class="mt-1 line-clamp-2 text-[11px] text-neutral-400">{recipe.intent}</div>
              {/if}
            </div>
            <div class="flex shrink-0 items-center gap-1">
              <button class={btn} onclick={() => startEdit(recipe)}>
                {locale.t("common.edit")}
              </button>
              <button
                class={btn}
                title={locale.t("prompt_assistant.recipe_duplicate")}
                aria-label={locale.t("prompt_assistant.recipe_duplicate")}
                onclick={() => duplicateRecipe(recipe.id)}
              >⧉</button>
              <button
                class="rounded-lg border border-neutral-700 px-2 py-1 text-xs text-neutral-400 hover:border-red-500/50 hover:bg-red-500/10 hover:text-red-300"
                title={locale.t("common.delete")}
                aria-label={locale.t("common.delete")}
                onclick={() => enhanceRecipes.remove(recipe.id)}
              >✕</button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <div class="min-h-0 flex-1 space-y-2.5 overflow-y-auto pr-0.5">
      {#if distilling}
        <div class="flex items-center gap-2 rounded-lg border border-indigo-500/30 bg-indigo-500/10 px-2.5 py-2 text-xs text-indigo-200">
          <span class="inline-block animate-spin">⟳</span>
          {locale.t("prompt_assistant.recipe_distilling")}
        </div>
      {/if}

      {#if errorText}
        <div class="flex items-start gap-2 rounded-lg border border-red-500/40 bg-red-500/10 px-2.5 py-2 text-xs text-red-200" role="alert">
          <span class="flex-1">{errorText}</span>
          <button class="text-red-300 hover:text-white" aria-label={locale.t("common.close")} onclick={() => (errorText = null)}>✕</button>
        </div>
      {/if}

      {#if !editingId && !distilling}
        <div class="rounded-lg border border-neutral-800 bg-neutral-950/70 p-2.5">
          <div class="mb-1.5 text-[11px] font-medium text-neutral-300">
            {locale.t("prompt_assistant.recipe_create_ai_title")}
          </div>
          <div class="flex items-start gap-1.5">
            <textarea
              rows="2"
              class="{inputCls} flex-1 resize-y"
              placeholder={locale.t("prompt_assistant.recipe_create_ai_placeholder")}
              bind:value={createPrompt}
              onkeydown={(e) => {
                if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
                  e.preventDefault();
                  void runCreateFromPrompt();
                }
              }}
            ></textarea>
            <button
              class={btn}
              disabled={!createPrompt.trim() || creating || distilling}
              onclick={() => void runCreateFromPrompt()}
            >
              {creating ? locale.t("prompt_assistant.recipe_creating") : locale.t("prompt_assistant.recipe_create_ai_btn")}
            </button>
          </div>
        </div>
      {/if}

      <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <label class="block sm:col-span-2">
          <span class="mb-1 block text-[11px] font-medium text-neutral-300">{locale.t("prompt_assistant.recipe_name")}</span>
          <input
            type="text"
            class={inputCls}
            placeholder={locale.t("prompt_assistant.recipe_name_placeholder")}
            value={name}
            oninput={(e) => {
              name = (e.currentTarget as HTMLInputElement).value;
              if (!slugTouched) slug = slugifyRecipe(name);
            }}
          />
        </label>
        <label class="block">
          <span class="mb-1 block text-[11px] font-medium text-neutral-300">{locale.t("prompt_assistant.recipe_scope")}</span>
          <select class={inputCls} bind:value={scope}>
            <option value="any">{locale.t("prompt_assistant.recipe_scope_any")}</option>
            <option value="character">{locale.t("prompt_assistant.recipe_scope_character")}</option>
            <option value="scene">{locale.t("prompt_assistant.recipe_scope_scene")}</option>
          </select>
        </label>
      </div>

      <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <label class="block">
          <span
            class="mb-1 block text-[11px] font-medium text-neutral-300"
            title={locale.t("prompt_assistant.recipe_slug_tip")}
          >
            {locale.t("prompt_assistant.recipe_slug")}
          </span>
          <input
            type="text"
            class="{inputCls} font-mono"
            placeholder="linea-alba"
            value={slug}
            oninput={(e) => {
              slugTouched = true;
              slug = slugifyRecipe((e.currentTarget as HTMLInputElement).value);
            }}
          />
        </label>
        <label class="block">
          <span
            class="mb-1 block text-[11px] font-medium text-neutral-300"
            title={locale.t("prompt_assistant.recipe_apply_mode_tip")}
          >
            {locale.t("prompt_assistant.recipe_apply_mode")}
          </span>
          <select class={inputCls} bind:value={applyMode}>
            <option value="ai">{locale.t("prompt_assistant.recipe_mode_ai")}</option>
            <option value="tags">{locale.t("prompt_assistant.recipe_mode_tags")}</option>
          </select>
        </label>
        <label class="block">
          <span
            class="mb-1 block text-[11px] font-medium text-neutral-300"
            title={locale.t("prompt_assistant.recipe_triggers_tip")}
          >
            {locale.t("prompt_assistant.recipe_triggers")}
          </span>
          <input
            type="text"
            class={inputCls}
            placeholder={locale.t("prompt_assistant.recipe_triggers_placeholder")}
            bind:value={triggersText}
          />
        </label>
      </div>

      <label class="block">
        <span
          class="mb-1 block text-[11px] font-medium text-neutral-300"
          title={locale.t("prompt_assistant.recipe_intent_tip")}
        >
          {locale.t("prompt_assistant.recipe_intent")}
        </span>
        <textarea
          rows="3"
          class="{inputCls} resize-y"
          placeholder={locale.t("prompt_assistant.recipe_intent_placeholder")}
          bind:value={intent}
        ></textarea>
      </label>

      <label class="block">
        <span
          class="mb-1 block text-[11px] font-medium text-neutral-300"
          title={locale.t("prompt_assistant.recipe_include_tip")}
        >
          {locale.t("prompt_assistant.recipe_include")}
        </span>
        <textarea
          rows="2"
          class="{inputCls} resize-y"
          placeholder={locale.t("prompt_assistant.recipe_include_placeholder")}
          bind:value={include}
        ></textarea>
      </label>

      <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
        <label class="block">
          <span
            class="mb-1 block text-[11px] font-medium text-neutral-300"
            title={locale.t("prompt_assistant.recipe_avoid_tip")}
          >
            {locale.t("prompt_assistant.recipe_avoid")}
          </span>
          <textarea
            rows="2"
            class="{inputCls} resize-y"
            placeholder={locale.t("prompt_assistant.recipe_avoid_placeholder")}
            bind:value={avoid}
          ></textarea>
        </label>
        <label class="block">
          <span
            class="mb-1 block text-[11px] font-medium text-neutral-300"
            title={locale.t("prompt_assistant.recipe_never_uc_tip")}
          >
            {locale.t("prompt_assistant.recipe_never_uc")}
          </span>
          <textarea
            rows="2"
            class="{inputCls} resize-y"
            placeholder={locale.t("prompt_assistant.recipe_never_uc_placeholder")}
            bind:value={neverUc}
          ></textarea>
        </label>
      </div>

      <label class="block">
        <span
          class="mb-1 block text-[11px] font-medium text-neutral-300"
          title={locale.t("prompt_assistant.recipe_context_tip")}
        >
          {locale.t("prompt_assistant.recipe_context")}
        </span>
        <textarea
          rows="2"
          class="{inputCls} resize-y"
          placeholder={locale.t("prompt_assistant.recipe_context_placeholder")}
          bind:value={context}
        ></textarea>
      </label>

      {#if editingId}
        <div class="rounded-lg border border-neutral-800 bg-neutral-950/70 p-2.5">
          <div class="mb-1.5 text-[11px] font-medium text-neutral-300">
            {locale.t("prompt_assistant.recipe_refine_title")}
          </div>
          <div class="flex items-center gap-1.5">
            <input
              type="text"
              class="{inputCls} flex-1"
              placeholder={locale.t("prompt_assistant.recipe_refine_placeholder")}
              bind:value={refineFeedback}
              onkeydown={(e) => {
                if (e.key === "Enter" && !e.isComposing) {
                  e.preventDefault();
                  void runRefine();
                }
              }}
            />
            <button
              class={btn}
              disabled={!refineFeedback.trim() || refining || distilling}
              onclick={() => void runRefine()}
            >
              {refining ? locale.t("prompt_assistant.recipe_refining") : locale.t("prompt_assistant.recipe_refine_btn")}
            </button>
          </div>
        </div>
      {/if}
    </div>

    <div class="mt-3 flex items-center justify-end gap-2 border-t border-neutral-800 pt-2.5">
      <button class={btn} onclick={cancelEdit}>
        {locale.t("common.cancel")}
      </button>
      <button
        class="rounded-lg bg-indigo-600 px-3 py-1 text-xs font-medium text-white hover:bg-indigo-500 disabled:opacity-40"
        disabled={!name.trim() || !intent.trim() || distilling || creating || refining}
        onclick={saveRecipe}
      >
        {locale.t("common.save")}
      </button>
    </div>
  {/if}
</div>
