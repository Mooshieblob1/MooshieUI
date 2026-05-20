<script lang="ts">
  import { LOCALE_OPTIONS, locale, type Locale } from "../../stores/locale.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import type { AppConfig } from "../../types/index.js";
  import { getConfig, updateConfig } from "../../utils/api.js";
  import { clearAuthToken } from "../../utils/ipc.js";
  import { applyTheme, THEME_PALETTES } from "../../utils/theme.js";
  import {
    getForceDesktopOverride,
    setForceDesktopOverride,
  } from "../../utils/device.js";
  import MobileTopBar from "./MobileTopBar.svelte";

  declare const __APP_VERSION__: string;
  const appVersion = __APP_VERSION__ ?? "dev";

  let forceDesktop = $state(getForceDesktopOverride());
  let config = $state<AppConfig | null>(null);

  function tt(key: string, fb: string) {
    const v = locale.t(key);
    return v === key ? fb : v;
  }

  function setLocale(value: Locale) {
    locale.current = value;
    try {
      localStorage.setItem(
        "locale-settings",
        JSON.stringify({ locale: value }),
      );
    } catch {}
  }

  function applyForceDesktop(v: boolean) {
    forceDesktop = v;
    setForceDesktopOverride(v);
    // Reload to swap UI shells.
    window.location.reload();
  }

  async function loadConfig() {
    try {
      config = await getConfig();
    } catch {
      config = null;
    }
  }

  async function saveTheme() {
    if (!config) return;
    applyTheme(config.theme, config.theme_palette);
    try {
      await updateConfig(config);
    } catch {}
  }

  async function logout() {
    if (!confirm(tt("settings.confirm_logout", "Sign out?"))) return;
    clearAuthToken();
    window.location.reload();
  }

  $effect(() => {
    void loadConfig();
  });
</script>

<div class="h-full flex flex-col bg-neutral-950">
  <MobileTopBar title={tt("nav.settings", "Settings")} />

  <div class="flex-1 min-h-0 overflow-y-auto p-3 space-y-3 no-scroll-chain">
    <!-- Display -->
    <section class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-3 space-y-3">
      <h2 class="text-xs font-semibold uppercase tracking-wide text-neutral-400">
        {tt("settings.display", "Display")}
      </h2>
      <label class="block">
        <span class="text-sm text-neutral-200">{tt("settings.language", "Language")}</span>
        <select
          name="locale"
          value={locale.current}
          onchange={(e) => setLocale((e.target as HTMLSelectElement).value as Locale)}
          class="mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2.5 text-sm text-neutral-100"
        >
          {#each LOCALE_OPTIONS as opt}
            <option value={opt.value}>{opt.label}</option>
          {/each}
        </select>
      </label>
      {#if config}
        <div class="grid grid-cols-2 gap-2">
          <label for="mobile-theme-mode" class="block">
            <span class="text-sm text-neutral-200">{tt("settings.appearance.theme", "Theme")}</span>
            <select
              id="mobile-theme-mode"
              name="theme-mode"
              bind:value={config.theme}
              onchange={saveTheme}
              class="mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2.5 text-sm text-neutral-100"
            >
              <option value="dark">{tt("settings.appearance.theme_dark", "Dark")}</option>
              <option value="light">{tt("settings.appearance.theme_light", "Light")}</option>
            </select>
          </label>
          <label for="mobile-theme-palette" class="block">
            <span class="text-sm text-neutral-200">{tt("settings.appearance.palette", "Palette")}</span>
            <select
              id="mobile-theme-palette"
              name="theme-palette"
              bind:value={config.theme_palette}
              onchange={saveTheme}
              class="mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2.5 text-sm text-neutral-100"
            >
              {#each THEME_PALETTES as palette}
                <option value={palette.value}>{palette.label}</option>
              {/each}
            </select>
          </label>
        </div>
      {/if}
      <div class="flex items-center justify-between gap-3">
        <div class="flex-1">
          <p class="text-sm text-neutral-200">
            {tt("settings.use_desktop_layout", "Use desktop layout")}
          </p>
          <p class="text-xs text-neutral-500 mt-0.5">
            {tt(
              "settings.use_desktop_layout_hint",
              "Show the full desktop UI on this device. Reloads the app.",
            )}
          </p>
        </div>
        <button
          type="button"
          role="switch"
          aria-label={tt("settings.use_desktop_layout", "Use desktop layout")}
          aria-checked={forceDesktop}
          onclick={() => applyForceDesktop(!forceDesktop)}
          class="relative shrink-0 w-12 h-7 rounded-full transition-colors {forceDesktop ? 'bg-indigo-600' : 'bg-neutral-700'}"
          style="touch-action: manipulation;"
        >
          <span
            class="absolute top-0.5 left-0.5 w-6 h-6 rounded-full bg-white transition-transform"
            style="transform: translateX({forceDesktop ? '20px' : '0'});"
          ></span>
        </button>
      </div>
    </section>

    <!-- Account -->
    <section class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-3 space-y-3">
      <h2 class="text-xs font-semibold uppercase tracking-wide text-neutral-400">
        {tt("settings.account", "Account")}
      </h2>
      <button
        type="button"
        onclick={logout}
        class="touch-target w-full px-3 py-2.5 rounded-lg border border-red-700/50 bg-red-900/20 text-red-300 text-sm font-medium hover:bg-red-900/30"
      >
        {tt("settings.logout", "Sign out")}
      </button>
    </section>

    <!-- Generation defaults -->
    <section class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-3 space-y-3">
      <h2 class="text-xs font-semibold uppercase tracking-wide text-neutral-400">
        {tt("settings.generation_defaults", "Generation defaults")}
      </h2>
      <label class="block">
        <span class="text-sm text-neutral-200">{tt("generation.steps.title", "Steps")}: {generation.steps}</span>
        <input
          type="range"
          min="1"
          max="100"
          step="1"
          bind:value={generation.steps}
          class="w-full accent-indigo-500"
        />
      </label>
      <label class="block">
        <span class="text-sm text-neutral-200">CFG: {locale.formatDecimal(generation.cfg, 1)}</span>
        <input
          type="range"
          min="1"
          max="20"
          step="0.5"
          bind:value={generation.cfg}
          class="w-full accent-indigo-500"
        />
      </label>
    </section>

    <!-- About -->
    <section class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-3 space-y-2">
      <h2 class="text-xs font-semibold uppercase tracking-wide text-neutral-400">
        {tt("settings.about", "About")}
      </h2>
      <p class="text-sm text-neutral-300">
        MooshieUI <span class="text-neutral-500">v{appVersion}</span>
      </p>
      <p class="text-xs text-neutral-500">
        {tt("nav.connected", "Connected")}: <span class={connection.connected ? "text-green-400" : "text-red-400"}>{connection.connected ? tt("common.yes", "yes") : tt("common.no", "no")}</span>
      </p>
      {#if connection.serverUrl}
        <p class="text-xs text-neutral-500 break-all">{connection.serverUrl}</p>
      {/if}
      <a
        href="https://github.com/Mooshieblob1/MooshieUI"
        target="_blank"
        rel="noopener noreferrer"
        class="inline-block text-xs text-indigo-400 hover:text-indigo-300 mt-1"
      >GitHub →</a>
    </section>
  </div>
</div>
