<script lang="ts">
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { audioTime } from "../../utils/musicAudio.js";
  let selectedPlaylist = $state("");
  let query = $state("");
  let sort = $state("added");
  let dialog: HTMLDialogElement | undefined = $state();
  let mode = $state<"create" | "rename" | "delete">("create");
  let playlistName = $state("");
  const playlist = $derived(music.playlists.find(list => list.id === selectedPlaylist));
  const songs = $derived.by(() => {
    const source = playlist ? playlist.songIds.flatMap(id => {
      const song = music.results.find(song => song.prompt_id === id);
      return song ? [song] : [];
    }) : music.results;
    const search = query.trim().toLocaleLowerCase();
    const filtered = source.filter(song => !search || `${music.resultTitle(song)} ${song.params.style}`.toLocaleLowerCase().includes(search));
    return sort === "title" ? [...filtered].sort((a, b) => music.resultTitle(a).localeCompare(music.resultTitle(b), locale.intlTag)) : filtered;
  });
  const title = $derived(playlist?.name ?? locale.t("music.all_songs"));
  const duration = $derived(songs.reduce((sum, song) => sum + (song.duration ?? 0), 0));
  const button = "touch-target rounded-full px-4 text-sm transition-colors focus-visible:outline-2 focus-visible:outline-indigo-400 disabled:opacity-40";
  function open(modeValue: typeof mode) {
    mode = modeValue;
    playlistName = modeValue === "create" ? "" : playlist?.name ?? "";
    dialog?.showModal();
  }
  async function submit() {
    const ok = mode === "delete" && playlist
      ? await music.deletePlaylist(playlist.id)
      : await music.savePlaylist(playlistName, mode === "rename" ? playlist?.id : undefined);
    if (ok) {
      if (mode === "create") selectedPlaylist = music.playlists.at(-1)?.id ?? "";
      if (mode === "delete") selectedPlaylist = "";
      query = "";
      dialog?.close();
    }
  }
</script>

<dialog bind:this={dialog} class="m-auto w-[calc(100%-2rem)] max-w-md rounded-xl border border-neutral-700 bg-neutral-900 p-5 text-neutral-200 shadow-2xl backdrop:bg-black/70" aria-labelledby="music-playlist-dialog-title">
  <form class="space-y-4" onsubmit={(event) => { event.preventDefault(); void submit(); }}>
    <h2 id="music-playlist-dialog-title" class="text-lg font-semibold">{locale.t(mode === "create" ? "music.new_playlist" : mode === "delete" ? "music.delete_playlist" : "music.rename_playlist")}</h2>
    {#if mode === "delete"}<p class="text-sm text-neutral-400">{locale.t("music.delete_playlist_help", { name: playlistName })}</p>
    {:else}<label class="block space-y-2 text-sm"><span>{locale.t("music.playlist_name")}</span><input class="min-h-11 w-full rounded-md bg-neutral-800 px-3" maxlength="100" required bind:value={playlistName} disabled={music.librarySaving} /></label>{/if}
    {#if music.libraryError}<p class="text-xs text-red-300" role="alert">{music.libraryError}</p>{/if}
    <div class="flex justify-end gap-2"><button type="button" class={button} disabled={music.librarySaving} onclick={() => dialog?.close()}>{locale.t("common.cancel")}</button><button type="submit" class={`${button} bg-indigo-500 text-[var(--theme-accent-foreground)]`} disabled={music.librarySaving || (mode !== "delete" && !playlistName.trim())}>{locale.t(mode === "delete" ? "common.delete" : "common.save")}</button></div>
  </form>
</dialog>

<div class="flex min-h-0 flex-1 flex-col overflow-y-auto lg:flex-row lg:overflow-hidden">
  <aside class="shrink-0 space-y-2 border-b border-neutral-800 p-4 lg:w-56 lg:overflow-y-auto lg:border-b-0 lg:border-r" aria-label={locale.t("music.playlists")}>
    <div class="flex items-center justify-between gap-2"><h2 class="text-xs font-semibold uppercase tracking-widest text-neutral-500">{locale.t("music.playlists")}</h2><button type="button" class="touch-target w-11 rounded-full text-xl text-neutral-300 hover:bg-neutral-800" onclick={() => open("create")} disabled={music.libraryLoading} aria-label={locale.t("music.new_playlist")}>+</button></div>
    <div class="flex gap-1 overflow-x-auto lg:flex-col">
      <button type="button" class={`touch-target shrink-0 rounded-md px-3 text-left text-sm ${!playlist ? 'bg-indigo-500/10 text-indigo-300' : 'text-neutral-400 hover:bg-neutral-800'}`} aria-current={!playlist ? "page" : undefined} onclick={() => { selectedPlaylist = ""; query = ""; }}>{locale.t("music.all_songs")}</button>
      {#each music.playlists as list (list.id)}
        <button type="button" class={`touch-target flex shrink-0 items-center gap-2 rounded-md px-3 text-left text-sm ${playlist?.id === list.id ? 'bg-indigo-500/10 text-indigo-300' : 'text-neutral-400 hover:bg-neutral-800'}`} aria-current={playlist?.id === list.id ? "page" : undefined} onclick={() => { selectedPlaylist = list.id; query = ""; }}><span aria-hidden="true">♫</span><span class="max-w-48 truncate">{list.name}</span></button>
      {/each}
    </div>
  </aside>
  <section class="min-w-0 flex-1 lg:overflow-y-auto" aria-label={locale.t("music.library")}>
    <div class="flex items-end gap-5 bg-linear-to-b from-indigo-500/20 to-neutral-950/0 px-5 pb-7 pt-7 md:px-8 md:pt-10">
      <div class="flex h-24 w-24 shrink-0 items-center justify-center rounded-lg bg-linear-to-br from-indigo-400/70 via-indigo-600/40 to-neutral-900 text-5xl text-indigo-100 shadow-xl md:h-40 md:w-40 md:text-7xl" aria-hidden="true">♫</div>
      <div class="min-w-0 space-y-2"><p class="text-[10px] font-medium uppercase tracking-[0.2em] text-neutral-400">{locale.t(playlist ? "music.playlists" : "music.library")}</p><h1 class="break-words text-3xl font-bold tracking-tight text-neutral-100 md:text-5xl">{title}</h1><p class="text-xs text-neutral-400">{locale.t("music.track_count", { count: songs.length })}{duration > 0 ? ` · ${audioTime(duration)}` : ""}</p></div>
    </div>
    <div class="space-y-5 px-4 pb-8 md:px-8">
      <div class="flex flex-wrap items-center gap-2">
        <button type="button" class={`${button} bg-indigo-500 font-medium text-[var(--theme-accent-foreground)] hover:bg-indigo-400`} disabled={!songs.length || music.loadingAudio} onclick={() => music.playAll(songs)}>{locale.t("music.play_all")}</button>
        <button type="button" class={`${button} border border-neutral-700 text-neutral-300 hover:bg-neutral-800`} disabled={!songs.length || music.loadingAudio} onclick={() => music.playAll(songs, true)}>{locale.t("music.shuffle")}</button>
        {#if playlist}<button type="button" class={`${button} text-neutral-400 hover:text-neutral-200`} onclick={() => open("rename")}>{locale.t("music.rename_playlist")}</button><button type="button" class={`${button} text-neutral-500 hover:text-red-300`} onclick={() => open("delete")}>{locale.t("common.delete")}</button>{/if}
        <div class="ml-auto flex w-full items-center gap-2 pt-2 md:w-auto md:pt-0">
          <input type="search" class="min-h-11 min-w-0 flex-1 rounded-full bg-neutral-800/70 px-4 text-sm text-neutral-200 placeholder:text-neutral-500 md:w-56" bind:value={query} placeholder={locale.t("music.search_songs")} aria-label={locale.t("music.search_songs")} />
          <select class="min-h-11 max-w-40 rounded-md bg-neutral-900 px-2 text-xs text-neutral-400" bind:value={sort} aria-label={locale.t("music.sort")}><option value="added">{locale.t(playlist ? "music.playlist_order" : "music.date_added")}</option><option value="title">{locale.t("music.song_title")}</option></select>
        </div>
      </div>
      {#if music.libraryLoading}<p class="text-sm text-neutral-400" role="status">{locale.t("common.loading")}</p>{/if}
      {#if music.libraryError}<p class="text-sm text-red-300" role="alert">{music.libraryError} <button type="button" class="touch-target px-2 underline" onclick={() => music.loadLibrary()}>{locale.t("common.retry")}</button></p>{/if}
      {#if music.error}<p class="text-xs text-red-300" role="alert">{music.error}</p>{/if}
      <div class="overflow-x-auto">
        <table class="w-full table-fixed border-collapse text-left text-sm">
          <thead class="border-b border-neutral-800 text-xs text-neutral-500"><tr><th class="w-12 px-2 py-3" scope="col">#</th><th class="px-2 py-3 font-normal" scope="col">{locale.t("music.song_title")}</th><th class="hidden w-28 px-2 py-3 font-normal md:table-cell" scope="col">{locale.t("music.date_added")}</th><th class="w-16 px-2 py-3 text-right font-normal" scope="col">{locale.t("music.track_duration")}</th><th class="w-12" scope="col"><span class="sr-only">{locale.t("music.edit_song")}</span></th></tr></thead>
          <tbody>
            {#each songs as song, index (song.prompt_id)}
              {@const selected = music.selectedResult?.prompt_id === song.prompt_id}
              <tr class={`group border-b border-neutral-800/30 hover:bg-neutral-800/60 ${selected ? 'bg-indigo-500/5' : ''}`}>
                <td class="px-1 py-2"><button type="button" class="touch-target w-11 rounded-md font-mono text-xs text-neutral-500 group-hover:text-neutral-100" aria-label={locale.t(selected && music.playing ? "music.pause" : "music.play")} onclick={() => selected && !music.loadingAudio ? music.togglePlayback() : music.play(song, true, songs)}><span class={selected ? "text-indigo-300" : ""} aria-hidden="true">{selected && music.playing ? "Ⅱ" : String(index + 1).padStart(2, "0")}</span></button></td>
                <td class="min-w-0 px-2 py-3"><button type="button" class="min-h-11 w-full text-left focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={() => music.play(song, true, songs)}><span class={`block truncate font-medium ${selected ? 'text-indigo-300' : 'text-neutral-200'}`}>{music.resultTitle(song)}</span><span class="mt-1 block truncate text-xs text-neutral-500">{song.params.style}</span></button>{#if !song.saved}<span class="text-[10px] text-amber-300">{locale.t("music.not_saved")}</span>{/if}</td>
                <td class="hidden px-2 text-xs text-neutral-500 md:table-cell">{song.createdAt ? new Date(song.createdAt).toLocaleDateString(locale.intlTag) : "—"}</td>
                <td class="px-2 text-right font-mono text-xs text-neutral-500">{song.duration ? audioTime(song.duration) : "—"}</td>
                <td><button type="button" class="touch-target w-11 rounded-full text-lg text-neutral-500 hover:bg-neutral-700 hover:text-neutral-200" aria-label={locale.t("music.edit_song")} onclick={() => { music.editingSong = song.prompt_id; }}>···</button></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if !songs.length && !music.libraryLoading}<div class="space-y-3 py-12 text-center"><p class="text-lg text-neutral-300">{locale.t(query ? "music.no_songs_found" : playlist ? "music.playlist_empty" : "music.library_empty")}</p><button type="button" class={`${button} text-indigo-300 hover:bg-neutral-800`} onclick={() => { if (playlist) { selectedPlaylist = ""; query = ""; } else if (query) query = ""; else music.view = "generate"; }}>{locale.t(playlist || query ? "music.all_songs" : "music.generate")}</button></div>{/if}
      <p class="text-xs leading-relaxed text-neutral-500">{locale.t("music.library_local")}</p>
    </div>
  </section>
</div>
