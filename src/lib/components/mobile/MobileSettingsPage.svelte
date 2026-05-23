<script lang="ts">
  import SettingsPage from "../settings/SettingsPage.svelte";

  interface Props {
    userRole?: string;
  }

  let { userRole = "admin" }: Props = $props();
</script>

<div class="h-full min-h-0 w-full bg-neutral-950 overflow-hidden">
  <SettingsPage {userRole} mobileFriendly />
</div>
