<script lang="ts">
  import { styleCreator } from "../../stores/styleCreator.svelte.js";
  import { styles } from "../../stores/styles.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { novelai } from "../../stores/novelai.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { estimateCurrentNovelAiCost } from "../../utils/novelaiCurrentCost.js";
  import { stripArtistSigil } from "../../utils/artistTag.js";
  import type { StyleArtist } from "../../stores/styles.svelte.js";

  let anchorInput = $state("");
  let anchorUnresolved = $state(false);

  const hasAnchor = $derived(styleCreator.anchor.length > 0);
  const indexReady = $derived(gallery.artistIndexReady);
  // NovelAI generates server-side, so a round is impossible without a key.
  const canStart = $derived(
    indexReady && (!generation.isNovelAi || novelai.apiKeyConfigured),
  );
  const choosing = $derived(styleCreator.phase === "choosing");

  /** Per-round Anlas: the single-image estimate times the number of cards. */
  const anlas = $derived.by(() => {
    const one = estimateCurrentNovelAiCost(1);
    return one === null ? null : one * styleCreator.cardsPerRound;
  });

  // The badge needs the subscription tier, same as the Generate button.
  $effect(() => {
    if (generation.isNovelAi && novelai.apiKeyConfigured) void novelai.ensureSubscription();
  });

  function chipLabel(artist: StyleArtist): string {
    const tag = generation.isNovelAi ? stripArtistSigil(artist.tag) : artist.tag;
    return artist.weight === 1 ? tag : `${tag}:${artist.weight}`;
  }

  function addAnchor() {
    const text = anchorInput.trim();
    if (!text) return;
    anchorUnresolved = !styleCreator.addAnchorTag(text);
    anchorInput = "";
  }

  function onAnchorSelect(e: Event) {
    const select = e.currentTarget as HTMLSelectElement;
    const id = select.value;
    if (!id) return;
    styleCreator.loadAnchorFromStyle(id);
    anchorUnresolved = false;
  }
</script>

<div class="flex h-full flex-col gap-3 overflow-y-auto p-3">
  <p class="text-[11px] leading-relaxed text-neutral-500">{locale.t("style_creator.hint")}</p>

  <!-- Anchor artists -->
  <section class="rounded-lg border border-neutral-800 bg-neutral-950/50 p-2">
    <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">
      {locale.t("style_creator.anchor")}
    </p>
    {#if hasAnchor}
      <div class="mb-2 flex flex-wrap gap-1">
        {#each styleCreator.anchor as artist, i (artist.tag + i)}
          <span class="inline-flex items-center gap-1 rounded bg-neutral-800 px-1.5 py-0.5 text-[11px] text-neutral-200">
            {chipLabel(artist)}
            {#if !artist.slug}
              <!-- Hand-typed and not found in the index: kept, but flagged. -->
              <span class="text-amber-400" title={locale.t("style_creator.anchor_unresolved")}
                >?</span
              >
            {/if}
            <button
              type="button"
              class="text-neutral-500 hover:text-red-400"
              aria-label={locale.t("common.remove")}
              title={locale.t("common.remove")}
              onclick={() => styleCreator.removeAnchor(i)}>x</button
            >
          </span>
        {/each}
      </div>
    {/if}
    <div class="flex flex-wrap items-center gap-2">
      <input
        type="text"
        bind:value={anchorInput}
        placeholder={locale.t("style_creator.anchor_placeholder")}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            addAnchor();
          }
        }}
        class="min-w-[12rem] flex-1 rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
      />
      <button
        type="button"
        class="rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500"
        onclick={addAnchor}>{locale.t("style_creator.anchor_add")}</button
      >
      <select
        value={styleCreator.anchorStyleId ?? ""}
        onchange={onAnchorSelect}
        class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-xs text-neutral-200 focus:border-indigo-500 focus:outline-none"
        title={locale.t("style_creator.anchor_from_style")}
      >
        <option value="">{locale.t("style_creator.anchor_from_style_placeholder")}</option>
        {#each styles.styles as style (style.id)}
          <option value={style.id}>{style.name}</option>
        {/each}
      </select>
      {#if hasAnchor}
        <button
          type="button"
          class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 hover:text-indigo-200"
          onclick={() => {
            styleCreator.clearAnchor();
            anchorUnresolved = false;
          }}>{locale.t("style_creator.anchor_clear")}</button
        >
      {/if}
    </div>
    {#if anchorUnresolved}
      <p class="mt-1 text-[10px] text-amber-400">{locale.t("style_creator.anchor_unresolved")}</p>
    {/if}
  </section>

  <!-- Draw settings -->
  <section class="flex flex-wrap items-center gap-4 rounded-lg border border-neutral-800 bg-neutral-950/50 p-2">
    <label class="flex items-center gap-2 text-[11px] text-neutral-300">
      {hasAnchor ? locale.t("style_creator.extra_count") : locale.t("style_creator.count")}
      <input
        type="number"
        min="1"
        max="10"
        value={styleCreator.count}
        onchange={(e) => {
          const el = e.currentTarget as HTMLInputElement;
          styleCreator.setCount(Number(el.value));
          // setCount clamps to 1..10; write the stored value back so a
          // rejected entry cannot sit in the field misreporting the setting.
          el.value = String(styleCreator.count);
        }}
        class="w-16 rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-sm text-neutral-100 focus:border-indigo-500 focus:outline-none"
      />
    </label>
    <label class="flex items-center gap-2 text-[11px] text-neutral-300">
      <input
        type="checkbox"
        checked={styleCreator.favouritesOnly}
        onchange={(e) => styleCreator.setFavouritesOnly((e.currentTarget as HTMLInputElement).checked)}
        class="accent-indigo-500"
      />
      {locale.t("style_creator.favourites_only")}
    </label>
    <label class="flex items-center gap-2 text-[11px] text-neutral-300">
      <input
        type="checkbox"
        checked={styleCreator.varyWeights}
        onchange={(e) => styleCreator.setVaryWeights((e.currentTarget as HTMLInputElement).checked)}
        class="accent-indigo-500"
      />
      {locale.t("style_creator.vary_weights")}
    </label>
    {#if generation.isNovelAi}
      <label class="flex items-center gap-2 text-[11px] text-neutral-300">
        <input
          type="checkbox"
          checked={styleCreator.pairInNai}
          onchange={(e) => styleCreator.setPairInNai((e.currentTarget as HTMLInputElement).checked)}
          class="accent-indigo-500"
        />
        {locale.t("style_creator.pair_in_nai")}
      </label>
      {#if anlas !== null}
        <span class="rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-400">
          {locale.t("style_creator.cost_badge", { anlas })}
        </span>
      {/if}
    {/if}
  </section>

  <!-- Run controls -->
  <section class="flex flex-wrap items-center gap-3">
    {#if styleCreator.running}
      <button
        type="button"
        class="rounded bg-neutral-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-neutral-600"
        onclick={() => styleCreator.stop()}>{locale.t("style_creator.stop")}</button
      >
    {:else}
      <button
        type="button"
        class="rounded bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
        disabled={!canStart}
        onclick={() => styleCreator.start()}>{locale.t("style_creator.start")}</button
      >
    {/if}
    <span class="text-[11px] text-neutral-500">
      {locale.t("style_creator.history_count", { count: styleCreator.historyCount })}
    </span>
    <button
      type="button"
      class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 hover:text-indigo-200"
      onclick={() => styleCreator.clearHistory()}>{locale.t("style_creator.clear_history")}</button
    >
  </section>

  {#if styleCreator.error}
    <p class="text-[11px] text-red-400">{styleCreator.error}</p>
  {/if}
  {#if styleCreator.note}
    <p class="text-[11px] text-amber-400">{styleCreator.note}</p>
  {/if}

  <!-- Round -->
  {#if styleCreator.round}
    <div class="flex gap-3">
      {#each styleCreator.round.cards as card, i (i)}
        <div class="flex min-w-0 flex-1 flex-col gap-2 rounded-lg border border-neutral-800 bg-neutral-950/50 p-2">
          <div class="flex flex-wrap gap-1">
            {#each card.artists as artist, j (artist.tag + j)}
              <span class="rounded bg-neutral-800 px-1.5 py-0.5 text-[11px] text-neutral-200">
                {chipLabel(artist)}
              </span>
            {/each}
          </div>
          <div class="relative aspect-square w-full overflow-hidden rounded border border-neutral-800 bg-neutral-900">
            {#if card.image}
              <img src={card.image.url} alt="" class="h-full w-full object-contain" />
            {:else}
              <div class="absolute inset-0 flex items-center justify-center">
                <div class="w-5 h-5 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin"></div>
              </div>
            {/if}
          </div>
          {#if card.saveable}
            <input
              type="text"
              value={card.name}
              placeholder={locale.t("style_creator.name")}
              oninput={(e) => styleCreator.setCardName(i, (e.currentTarget as HTMLInputElement).value)}
              class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
            />
            <div class="flex gap-2">
              <button
                type="button"
                class="flex-1 rounded bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
                disabled={!choosing}
                onclick={() => void styleCreator.pick(i)}>{locale.t("style_creator.pick")}</button
              >
              <button
                type="button"
                class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 hover:text-indigo-200 disabled:opacity-50"
                disabled={!choosing}
                onclick={() => void styleCreator.pick(i, true)}>{locale.t("style_creator.pick_edit")}</button
              >
            </div>
          {:else}
            <p class="text-center text-[11px] text-neutral-500">{locale.t("style_creator.anchor_alone")}</p>
            <button
              type="button"
              class="rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500 disabled:opacity-50"
              disabled={!choosing}
              onclick={() => void styleCreator.pick(i)}>{locale.t("style_creator.pick")}</button
            >
          {/if}
        </div>
      {/each}
    </div>
    <button
      type="button"
      class="self-start rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500 disabled:opacity-50"
      disabled={!choosing}
      onclick={() => styleCreator.skip()}>{locale.t("style_creator.skip")}</button
    >
  {:else if !indexReady}
    <div class="rounded border border-dashed border-neutral-800 bg-neutral-950/50 p-4 text-center text-[11px] text-neutral-500">
      {locale.t("style_creator.index_loading")}
    </div>
  {/if}
</div>
