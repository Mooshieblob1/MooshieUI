<script lang="ts">
  import MobileTabBar, { type MobileTab } from "./MobileTabBar.svelte";
  import MobileGeneratePage from "./MobileGeneratePage.svelte";
  import MobileSettingsPage from "./MobileSettingsPage.svelte";
  import GalleryPage from "../gallery/GalleryPage.svelte";
  import { ArtistGalleryPage } from "../../artist-gallery/index.js";
  import ModelHubPage from "../modelhub/ModelHubPage.svelte";
  import DownloadBanner from "../downloads/DownloadBanner.svelte";
  import { connection } from "../../stores/connection.svelte.js";
  import { characterInsert } from "../../stores/characterInsert.svelte.js";
  import CharacterInsertModal from "../../animadex/components/CharacterInsertModal.svelte";
  import type { AnimadexCharacter } from "../../animadex/types.js";

  interface Props {
    canUseModelhub?: boolean;
    userRole?: string;
    navigationTarget?: MobileTab | null;
    navigationVersion?: number;
    onTabChange?: (tab: MobileTab) => void;
  }
  let {
    canUseModelhub = false,
    userRole = "admin",
    navigationTarget = null,
    navigationVersion = 0,
    onTabChange,
  }: Props = $props();

  let currentTab = $state<MobileTab>("generate");
  let lastNavigationVersion = $state(navigationVersion);

  function go(tab: MobileTab) {
    currentTab = tab;
    onTabChange?.(tab);
  }

  function handleCharacterInsert(character: AnimadexCharacter) {
    characterInsert.request(character);
    if (!characterInsert.pending) {
      go("generate");
    }
  }

  function finishCharacterInsert() {
    characterInsert.dismiss();
    go("generate");
  }

  $effect(() => {
    if (navigationVersion === lastNavigationVersion) return;
    lastNavigationVersion = navigationVersion;
    if (navigationTarget) go(navigationTarget);
  });
</script>

<div class="mobile-shell flex flex-col h-full w-full bg-neutral-950 text-neutral-100 overflow-hidden tap-highlight-none">
  <DownloadBanner />
  <main class="flex-1 min-h-0 overflow-hidden">
    {#if currentTab === "generate"}
      <MobileGeneratePage />
    {:else if currentTab === "gallery"}
      <GalleryPage onSwitchToGenerate={() => go("generate")} />
    {:else if currentTab === "modelhub" && canUseModelhub}
      <div class="h-full overflow-hidden">
        <ModelHubPage />
      </div>
    {:else if currentTab === "artists"}
      <div class="h-full overflow-hidden">
        <ArtistGalleryPage
          manifestUrl={connection.artistGalleryManifestUrl}
          initialTab="artists"
          oninsertCharacter={handleCharacterInsert}
        />
      </div>
    {:else if currentTab === "characters"}
      <div class="h-full overflow-hidden">
        <ArtistGalleryPage
          manifestUrl={connection.artistGalleryManifestUrl}
          initialTab="characters"
          oninsertCharacter={handleCharacterInsert}
        />
      </div>
    {:else if currentTab === "settings"}
      <MobileSettingsPage {userRole} />
    {/if}
  </main>
  <CharacterInsertModal onapplied={finishCharacterInsert} />
  <MobileTabBar
    current={currentTab}
    onChange={go}
    showModelhub={canUseModelhub}
  />
</div>
