<script lang="ts">
  import { untrack } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  let dialog: HTMLDialogElement | undefined = $state();
  let title = $state("");
  const song = $derived(music.results.find(item => item.prompt_id === music.editingSong));
  $effect(() => {
    const id = music.editingSong;
    const element = dialog;
    if (!element) return;
    untrack(() => {
      if (id && song) { title = song.title ?? ""; if (!element.open) element.showModal(); }
      else element.close();
    });
  });
  function close() { music.editingSong = null; dialog?.close(); }
  async function save() {
    if (song && await music.updateSong(song.prompt_id, { title })) close();
  }
</script>

<dialog bind:this={dialog} class="m-auto max-h-[85vh] w-[calc(100%-2rem)] max-w-md overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 text-neutral-200 shadow-2xl backdrop:bg-black/70" aria-labelledby="music-song-edit-title" onclose={() => { music.editingSong = null; }}>
  <form class="space-y-4" onsubmit={(event) => { event.preventDefault(); void save(); }}>
    <h2 id="music-song-edit-title" class="text-lg font-semibold">{locale.t("music.edit_song")}</h2>
    <label class="block space-y-2 text-sm"><span>{locale.t("music.song_title")}</span><input class="min-h-11 w-full rounded-md bg-neutral-800 px-3" maxlength="200" bind:value={title} placeholder={song ? music.resultTitle(song) : ""} disabled={music.librarySaving} /></label>
    {#if song}
      <div class="flex flex-wrap gap-2">
        <button type="button" class="touch-target px-3 text-xs text-indigo-300" onclick={() => { const id = song.prompt_id; close(); music.reviewingSong = id; }}>{locale.t("music.review")}</button>
        <button type="button" class="touch-target px-3 text-xs text-indigo-300 disabled:opacity-40" disabled={music.busy} onclick={() => { music.useVersion(song); close(); }}>{locale.t("music.version_use")}</button>
        <button type="button" class="touch-target px-3 text-xs text-indigo-300 disabled:opacity-40" disabled={music.busy || !song.abc.trim()} onclick={() => { const id = song.prompt_id; close(); music.editingComposition = id; }}>{locale.t("music.edit_composition")}</button>
        <button type="button" class="touch-target px-3 text-xs text-indigo-300" onclick={() => { music.compareIds = [song.prompt_id]; close(); music.comparing = true; }}>{locale.t("music.versions")}</button>
      </div>
      <fieldset class="space-y-1">
        <legend class="mb-2 text-sm text-neutral-400">{locale.t("music.playlists")}</legend>
        {#each music.playlists as playlist (playlist.id)}
          <label class="touch-target flex items-center gap-2 text-sm"><input type="checkbox" class="accent-indigo-400" checked={playlist.songIds.includes(song.prompt_id)} disabled={music.librarySaving || !song.saved} onchange={(event) => music.setPlaylistSong(playlist.id, song!.prompt_id, event.currentTarget.checked)} />{playlist.name}</label>
        {:else}
          <button type="button" class="touch-target text-sm text-indigo-300" onclick={() => { music.view = "library"; close(); }}>{locale.t("music.new_playlist")}</button>
        {/each}
      </fieldset>
    {/if}
    {#if music.libraryError}<p class="text-xs text-red-300" role="alert">{music.libraryError}</p>{/if}
    <div class="flex justify-end gap-2">
      <button type="button" class="touch-target rounded-md px-4 text-sm text-neutral-400" disabled={music.librarySaving} onclick={close}>{locale.t("common.cancel")}</button>
      <button type="submit" class="touch-target rounded-md bg-indigo-500 px-4 text-sm text-[var(--theme-accent-foreground)] disabled:opacity-40" disabled={music.librarySaving}>{locale.t("common.save")}</button>
    </div>
  </form>
</dialog>
