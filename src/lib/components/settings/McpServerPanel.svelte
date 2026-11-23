<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { getMcpStatus, setMcpSettings, regenerateMcpToken } from "../../utils/api.js";
  import type { McpStatus } from "../../types/index.js";

  /**
   * Settings for the local MCP server (src-tauri/src/mcp). Saved through its
   * own commands rather than the config object, so a stale Settings save can
   * never switch it off or bring back an old token.
   */

  let status = $state<McpStatus | null>(null);
  let loadError = $state<string | null>(null);
  let busy = $state(false);
  let portInput = $state(3210);
  let publicUrlInput = $state("");
  let showToken = $state(false);
  let copied = $state<string | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;

  function apply(next: McpStatus) {
    status = next;
    portInput = next.port;
    publicUrlInput = next.public_url;
  }

  async function load() {
    try {
      apply(await getMcpStatus());
      loadError = null;
    } catch (e) {
      loadError = String(e);
    }
  }

  $effect(() => {
    void load();
  });

  // The tunnel gets its address in the background: poll quickly while it
  // starts, then slowly so a restart's new address shows up.
  const tunnelState = $derived(status?.enabled && status.tunnel ? status.tunnel_status.state : "off");
  $effect(() => {
    if (tunnelState === "off") return;
    const ms = tunnelState === "running" || tunnelState === "error" ? 10000 : 2000;
    const timer = setInterval(() => {
      if (!busy) void load();
    }, ms);
    return () => clearInterval(timer);
  });

  async function save(enabled: boolean, extra: { tunnel?: boolean; publicUrl?: string } = {}) {
    if (busy) return;
    busy = true;
    try {
      apply(await setMcpSettings(enabled, Math.round(Number(portInput)) || 3210, extra));
      loadError = null;
    } catch (e) {
      loadError = String(e);
      await load();
    } finally {
      busy = false;
    }
  }

  async function regenerate() {
    if (busy) return;
    busy = true;
    try {
      apply(await regenerateMcpToken());
    } catch (e) {
      loadError = String(e);
    } finally {
      busy = false;
    }
  }

  async function copy(key: string, value: string) {
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      return;
    }
    copied = key;
    clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => { copied = null; }, 1500);
  }

  /** Hide the token inside commands and URLs until "Show" is pressed. */
  function mask(text: string): string {
    return showToken || !status?.token ? text : text.replaceAll(status.token, "•".repeat(12));
  }

  const claudeCodeCommand = $derived(
    status ? `claude mcp add --transport http mooshieui ${status.url} --header "Authorization: Bearer ${status.token}"` : "",
  );
  const jsonConfig = $derived(
    status
      ? JSON.stringify(
          { mcpServers: { mooshieui: { type: "http", url: status.url, headers: { Authorization: `Bearer ${status.token}` } } } },
          null,
          2,
        )
      : "",
  );
</script>

{#snippet urlRow(key: string, label: string, url: string)}
  <div class="space-y-1">
    <div class="flex items-center justify-between">
      <span class="text-xs text-neutral-400">{label}</span>
      <button
        class="px-2 py-0.5 text-[10px] rounded bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors"
        onclick={() => copy(key, url)}
      >{copied === key ? locale.t("common.copied") : locale.t("common.copy")}</button>
    </div>
    <pre class="bg-neutral-950 border border-neutral-800 rounded-lg px-3 py-2 text-[11px] text-neutral-300 font-mono whitespace-pre-wrap break-all">{mask(url)}</pre>
  </div>
{/snippet}

<section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden mb-4">
  <div class="p-5 space-y-3">
    <div>
      <h3 class="text-sm font-medium text-neutral-200">{locale.t("settings.mcp.title")}</h3>
      <p class="text-xs text-neutral-500 mt-0.5">{locale.t("settings.mcp.desc")}</p>
    </div>

    {#if loadError}
      <p class="text-xs text-red-400">{locale.t("settings.mcp.load_failed", { error: loadError })}</p>
    {/if}

    {#if status}
      <div class="flex items-center justify-between pt-2 border-t border-neutral-800">
        <label for="settings-mcp-enabled" class="text-xs text-neutral-300 font-medium">{locale.t("settings.mcp.enable")}</label>
        <label class="relative inline-flex items-center cursor-pointer">
          <input
            id="settings-mcp-enabled"
            type="checkbox"
            checked={status.enabled}
            disabled={busy}
            onchange={(e) => void save((e.currentTarget as HTMLInputElement).checked)}
            class="sr-only peer"
          />
          <div class="w-9 h-5 bg-neutral-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-0.5 after:left-0.5 after:bg-white after:border-neutral-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-indigo-600"></div>
        </label>
      </div>

      <div class="flex items-center gap-3">
        <label for="settings-mcp-port" class="text-xs text-neutral-400">{locale.t("settings.mcp.port")}</label>
        <input
          id="settings-mcp-port"
          type="number"
          min="1024"
          max="65535"
          bind:value={portInput}
          disabled={busy}
          onchange={() => { if (portInput !== status?.port) void save(status?.enabled ?? false); }}
          class="w-28 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-1.5 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
        />
        {#if status.enabled && status.running}
          <span class="text-xs text-green-400 truncate">{locale.t("settings.mcp.running", { url: status.url })}</span>
        {:else if status.enabled && status.error}
          <span class="text-xs text-red-400">{locale.t("settings.mcp.error", { error: status.error })}</span>
        {:else}
          <span class="text-xs text-neutral-500">{locale.t("settings.mcp.stopped")}</span>
        {/if}
      </div>

      {#if status.enabled}
        <div class="bg-neutral-800 rounded-lg px-3 py-2 space-y-2">
          <div class="flex items-center gap-2 min-w-0">
            <span class="text-xs text-neutral-400 shrink-0">{locale.t("settings.mcp.token")}</span>
            <input
              type={showToken ? "text" : "password"}
              readonly
              value={status.token}
              class="flex-1 min-w-0 bg-neutral-900 border border-neutral-700 rounded px-2 py-1 text-xs text-neutral-200 font-mono"
            />
            <button
              class="shrink-0 px-2 py-0.5 text-[10px] rounded bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors"
              onclick={() => { showToken = !showToken; }}
            >{showToken ? locale.t("settings.mcp.hide_token") : locale.t("settings.mcp.show_token")}</button>
            <button
              class="shrink-0 px-2 py-0.5 text-[10px] rounded bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors"
              onclick={() => copy("token", status?.token ?? "")}
            >{copied === "token" ? locale.t("common.copied") : locale.t("common.copy")}</button>
            <button
              class="shrink-0 px-2 py-0.5 text-[10px] rounded bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors disabled:opacity-50"
              disabled={busy}
              title={locale.t("settings.mcp.regenerate_hint")}
              onclick={regenerate}
            >{locale.t("settings.mcp.regenerate")}</button>
          </div>
          <p class="text-[10px] text-amber-400">{locale.t("settings.mcp.token_warning")}</p>
        </div>

        <div class="space-y-1">
          <div class="flex items-center justify-between">
            <span class="text-xs text-neutral-400">{locale.t("settings.mcp.claude_code")}</span>
            <button
              class="px-2 py-0.5 text-[10px] rounded bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors"
              onclick={() => copy("claude", claudeCodeCommand)}
            >{copied === "claude" ? locale.t("common.copied") : locale.t("common.copy")}</button>
          </div>
          <pre class="bg-neutral-950 border border-neutral-800 rounded-lg px-3 py-2 text-[11px] text-neutral-300 font-mono whitespace-pre-wrap break-all">{mask(claudeCodeCommand)}</pre>
        </div>

        <div class="space-y-1">
          <div class="flex items-center justify-between">
            <span class="text-xs text-neutral-400">{locale.t("settings.mcp.other_apps")}</span>
            <button
              class="px-2 py-0.5 text-[10px] rounded bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors"
              onclick={() => copy("json", jsonConfig)}
            >{copied === "json" ? locale.t("common.copied") : locale.t("common.copy")}</button>
          </div>
          <pre class="bg-neutral-950 border border-neutral-800 rounded-lg px-3 py-2 text-[11px] text-neutral-300 font-mono whitespace-pre-wrap break-all">{mask(jsonConfig)}</pre>
        </div>

        {@render urlRow("local", locale.t("settings.mcp.local_url"), status.local_connector_url)}

        <div class="pt-3 mt-1 border-t border-neutral-800 space-y-3">
          <div>
            <h4 class="text-xs font-medium text-neutral-200">{locale.t("settings.mcp.remote_title")}</h4>
            <p class="text-xs text-neutral-500 mt-0.5">{locale.t("settings.mcp.remote_desc")}</p>
          </div>

          <div class="flex items-start justify-between gap-3">
            <div class="min-w-0">
              <label for="settings-mcp-tunnel" class="text-xs text-neutral-300 font-medium">{locale.t("settings.mcp.tunnel")}</label>
              <p class="text-[11px] text-neutral-500 mt-0.5">{locale.t("settings.mcp.tunnel_desc")}</p>
            </div>
            <label class="relative inline-flex items-center cursor-pointer shrink-0 mt-0.5">
              <input
                id="settings-mcp-tunnel"
                type="checkbox"
                checked={status.tunnel}
                disabled={busy}
                onchange={(e) => void save(true, { tunnel: (e.currentTarget as HTMLInputElement).checked })}
                class="sr-only peer"
              />
              <div class="w-9 h-5 bg-neutral-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-0.5 after:left-0.5 after:bg-white after:border-neutral-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-indigo-600"></div>
            </label>
          </div>

          {#if status.tunnel}
            {#if tunnelState === "downloading"}
              <p class="text-xs text-neutral-400">{locale.t("settings.mcp.tunnel_downloading")}</p>
            {:else if tunnelState === "starting"}
              <p class="text-xs text-neutral-400">{locale.t("settings.mcp.tunnel_starting")}</p>
            {:else if tunnelState === "error"}
              <p class="text-xs text-red-400">{locale.t("settings.mcp.tunnel_error", { error: status.tunnel_status.error ?? "" })}</p>
            {/if}
            {#if status.tunnel_connector_url}
              <p class="text-xs text-green-400">{locale.t("settings.mcp.tunnel_running")}</p>
              {@render urlRow("tunnel", locale.t("settings.mcp.connector_url"), status.tunnel_connector_url)}
            {/if}
          {/if}

          <div class="space-y-1">
            <label for="settings-mcp-public-url" class="text-xs text-neutral-300 font-medium">{locale.t("settings.mcp.public_url")}</label>
            <p class="text-[11px] text-neutral-500">{locale.t("settings.mcp.public_url_desc", { local: `http://127.0.0.1:${status.port}` })}</p>
            <div class="flex items-center gap-2">
              <input
                id="settings-mcp-public-url"
                type="url"
                placeholder="https://mooshie.example.com"
                bind:value={publicUrlInput}
                disabled={busy}
                class="flex-1 min-w-0 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-1.5 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
              />
              <button
                class="shrink-0 px-3 py-1.5 text-xs rounded-lg bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors disabled:opacity-50"
                disabled={busy || publicUrlInput.trim() === status.public_url}
                onclick={() => void save(true, { publicUrl: publicUrlInput })}
              >{locale.t("common.save")}</button>
            </div>
            {#if status.public_connector_url}
              {@render urlRow("public", locale.t("settings.mcp.connector_url"), status.public_connector_url)}
            {/if}
          </div>

          {#if status.tunnel_connector_url || status.public_connector_url}
            <p class="text-[10px] text-amber-400">{locale.t("settings.mcp.connector_warning")}</p>
          {/if}
        </div>

        <p class="text-xs text-neutral-500">
          {status.setups > 0
            ? locale.t("settings.mcp.setups", { count: status.setups })
            : locale.t("settings.mcp.setups_none")}
        </p>
      {/if}
    {/if}
  </div>
</section>
