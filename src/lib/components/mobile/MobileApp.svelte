<script lang="ts">
  import MobileTabBar, { type MobileTab } from "./MobileTabBar.svelte";
  import MobileGeneratePage from "./MobileGeneratePage.svelte";
  import MobileGalleryPage from "./MobileGalleryPage.svelte";
  import MobileSettingsPage from "./MobileSettingsPage.svelte";
  import { ArtistGalleryPage } from "../../artist-gallery/index.js";
  import ModelHubPage from "../modelhub/ModelHubPage.svelte";
  import DownloadBanner from "../downloads/DownloadBanner.svelte";
  import { connection } from "../../stores/connection.svelte.js";

  interface Props {
    canUseModelhub?: boolean;
  }
  let { canUseModelhub = false }: Props = $props();

  let currentTab = $state<MobileTab>("generate");

  function go(tab: MobileTab) {
    currentTab = tab;
  }
</script>

<div class="flex flex-col h-full w-full bg-neutral-950 text-neutral-100 overflow-hidden tap-highlight-none">
  <DownloadBanner />
  <main class="flex-1 min-h-0 overflow-hidden">
    {#if currentTab === "generate"}
      <MobileGeneratePage />
    {:else if currentTab === "gallery"}
      <MobileGalleryPage onSwitchToGenerate={() => go("generate")} />
    {:else if currentTab === "modelhub" && canUseModelhub}
      <div class="h-full overflow-hidden">
        <ModelHubPage />
      </div>
    {:else if currentTab === "artists"}
      <div class="h-full overflow-hidden">
        <ArtistGalleryPage manifestUrl={connection.artistGalleryManifestUrl} />
      </div>
    {:else if currentTab === "settings"}
      <MobileSettingsPage />
    {/if}
  </main>
  <MobileTabBar
    current={currentTab}
    onChange={go}
    showModelhub={canUseModelhub}
  />
</div>
