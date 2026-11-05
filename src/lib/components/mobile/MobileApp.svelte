<script lang="ts">
  /**
   * The phone layout's shell: the page area, a five-slot bottom bar, and the
   * "More" sheet for everything else.
   *
   * The open page is App.svelte's own `currentPage`, bound in, so every
   * "take me to Generate" in App (lightbox Img2Img / Upscale / Make video,
   * artist and character inserts, the done toast) lands on phones too. The
   * shell used to keep its own tab state, and those actions left the phone
   * on whatever page it was showing.
   */
  import type { Snippet } from "svelte";
  import MobileTabBar from "./MobileTabBar.svelte";
  import MobileMoreSheet from "./MobileMoreSheet.svelte";
  import MobileGeneratePage from "./MobileGeneratePage.svelte";
  import MobileSettingsPage from "./MobileSettingsPage.svelte";
  import MusicPage from "../music/MusicPage.svelte";
  import MusicBottomPlayer from "../music/MusicBottomPlayer.svelte";
  import Model3DPage from "../model3d/Model3DPage.svelte";
  import ScenePage from "../scene/ScenePage.svelte";
  import GalleryPage from "../gallery/GalleryPage.svelte";
  import { ArtistGalleryPage } from "../../artist-gallery/index.js";
  import ModelHubPage from "../modelhub/ModelHubPage.svelte";
  import DownloadBanner from "../downloads/DownloadBanner.svelte";
  import UpdateNotification from "../updater/UpdateNotification.svelte";
  import NotificationBell from "../ui/NotificationBell.svelte";
  import { music } from "../../stores/music.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { model3d } from "../../stores/model3d.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { startup } from "../../stores/startup.svelte.js";
  import type { AnimadexCharacter } from "../../animadex/types.js";
  import type { ArtistPreviewStatus, ArtistPreviewVariant } from "../../artist-gallery/previewRecipe.js";
  import type { OutputImage } from "../../types/index.js";
  import {
    allDestinations,
    PRIMARY_DESTINATIONS,
    type MobileNavId,
    type MobilePage,
  } from "./mobileNav.js";

  interface Props {
    page: MobilePage;
    canUseModelhub?: boolean;
    userRole?: "admin" | "moderator" | "user" | "anonymous";
    logoUrl: string;
    appVersion: string;
    /** Startup / ComfyUI status line, shared with the desktop layout (`true` = phone styling). */
    banner?: Snippet<[boolean]>;
    onOpenWorkspace: (workspace: "image" | "video") => void;
    onInterrogate: () => void;
    onReportBug: () => void;
    onInsertArtistTag: (tag: string) => void;
    onInsertCharacter: (character: AnimadexCharacter) => void;
    onGenerateArtistPreview: (slug: string, tag: string, variant: ArtistPreviewVariant) => void;
    artistPreviewStatus: (slug: string, variant: ArtistPreviewVariant) => ArtistPreviewStatus;
    onMake3D?: (image: OutputImage) => void;
  }

  let {
    page = $bindable(),
    canUseModelhub = false,
    userRole = "admin",
    logoUrl,
    appVersion,
    banner,
    onOpenWorkspace,
    onInterrogate,
    onReportBug,
    onInsertArtistTag,
    onInsertCharacter,
    onGenerateArtistPreview,
    artistPreviewStatus,
    onMake3D,
  }: Props = $props();

  let moreOpen = $state(false);

  const navOptions = $derived({
    showModelhub: canUseModelhub,
    showModel3d: model3d.enabled,
  });
  const primary = PRIMARY_DESTINATIONS;
  const overflow = $derived(allDestinations(navOptions).filter((id) => !primary.includes(id)));

  const current = $derived<MobileNavId>(
    page === "generate" && generation.mode === "video" ? "video" : page,
  );

  function go(id: MobileNavId) {
    moreOpen = false;
    if (id === "generate" || id === "video") {
      onOpenWorkspace(id === "video" ? "video" : "image");
      return;
    }
    page = id;
  }

  // A page can disappear under the phone (3D switched off, Model Hub access
  // revoked); fall back to Generate rather than a blank screen.
  $effect(() => {
    if ((page === "model3d" && !model3d.enabled) || (page === "modelhub" && !canUseModelhub)) {
      page = "generate";
    }
  });
</script>

<div class="mobile-shell flex h-full w-full flex-col overflow-hidden bg-neutral-950 pt-[env(safe-area-inset-top)] text-neutral-100 tap-highlight-none">
  {@render banner?.(true)}
  <UpdateNotification {userRole} />
  <DownloadBanner />
  <main class="min-h-0 flex-1 overflow-hidden" inert={startup.locked}>
    {#if page === "generate"}
      <MobileGeneratePage />
    {:else if page === "music"}
      <MusicPage {userRole} />
    {:else if page === "model3d" && model3d.enabled}
      <Model3DPage {userRole} />
    {:else if page === "scene"}
      <ScenePage onOpenSettings={() => go("settings")} />
    {:else if page === "gallery"}
      <GalleryPage onSwitchToGenerate={() => (page = "generate")} {onMake3D} />
    {:else if page === "modelhub" && canUseModelhub}
      <div class="h-full overflow-hidden">
        <ModelHubPage />
      </div>
    {:else if page === "artists" || page === "characters"}
      {#key page}
        <div class="h-full overflow-hidden">
          <ArtistGalleryPage
            manifestUrl={connection.artistGalleryManifestUrl}
            initialTab={page}
            oninsertTag={onInsertArtistTag}
            oninsertCharacter={onInsertCharacter}
            ongeneratePreview={onGenerateArtistPreview}
            previewStatus={artistPreviewStatus}
          />
        </div>
      {/key}
    {:else if page === "settings"}
      <MobileSettingsPage {userRole} />
    {/if}
  </main>
  <MusicBottomPlayer onOpen={() => { music.view = "generate"; go("music"); }} />
  <MobileTabBar
    items={primary}
    {current}
    {moreOpen}
    onChange={go}
    onMore={() => (moreOpen = !moreOpen)}
  />
</div>

{#if moreOpen}
  <MobileMoreSheet
    items={overflow}
    {current}
    {logoUrl}
    {appVersion}
    onNavigate={go}
    {onInterrogate}
    {onReportBug}
    onclose={() => (moreOpen = false)}
  />
{/if}

<NotificationBell sheet onOpenSettings={() => go("settings")} />
