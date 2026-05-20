<script lang="ts">
  import { locale } from "../stores/locale.svelte.js";
  import { killPortProcess } from "../utils/api.js";
  import { ipcInvoke, ipcListen } from "../utils/ipc.js";
  import {
    isNodeLoadFailurePayload,
    type ComfyServerErrorPayload,
  } from "../utils/comfyStartup.js";

  interface Props {
    open: boolean;
    payload: ComfyServerErrorPayload;
    serverUrl?: string;
    onclose: () => void;
    onrestarted?: () => void;
  }

  let { open, payload, serverUrl = "http://127.0.0.1:8188", onclose, onrestarted }: Props =
    $props();

  let busy = $state(false);
  let localError = $state("");

  const isNodeLoadFailure = $derived(isNodeLoadFailurePayload(payload));

  const title = $derived(
    isNodeLoadFailure
      ? locale.t("app.external_comfy.title_missing_nodes")
      : locale.t("app.external_comfy.title_already_running"),
  );

  const port = $derived(payload.port ?? 8188);

  function waitForComfyReady(timeoutMs = 120_000): Promise<void> {
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        cleanup();
        reject(new Error(locale.t("generation.controlnet.install_timeout")));
      }, timeoutMs);

      let unlistenReady: (() => void) | undefined;
      let unlistenError: (() => void) | undefined;

      const cleanup = () => {
        clearTimeout(timeout);
        unlistenReady?.();
        unlistenError?.();
      };

      ipcListen("comfyui:server_ready", () => {
        cleanup();
        resolve();
      }).then((fn) => {
        unlistenReady = fn;
      });

      ipcListen("comfyui:server_error", (event: { payload?: unknown }) => {
        cleanup();
        const err =
          event.payload &&
          typeof event.payload === "object" &&
          event.payload !== null &&
          "error" in event.payload &&
          typeof (event.payload as { error?: unknown }).error === "string"
            ? (event.payload as { error: string }).error
            : locale.t("app.status.unknown_error");
        reject(new Error(err));
      }).then((fn) => {
        unlistenError = fn;
      });
    });
  }

  async function killAndRestart() {
    busy = true;
    localError = "";
    try {
      await killPortProcess();
      const result = await ipcInvoke<string>("start_comfyui");
      if (result === "already_running" || result === "skipped") {
        onrestarted?.();
        onclose();
        return;
      }
      if (result !== "spawned") {
        localError = locale.t("app.status.failed_to_start", {
          message: result,
        });
        return;
      }
      onrestarted?.();
      await waitForComfyReady();
      onclose();
    } catch (e) {
      localError = String(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <div
    class="fixed inset-0 z-[220] flex items-center justify-center bg-black/80 backdrop-blur-sm p-4"
    role="dialog"
    aria-modal="true"
    aria-labelledby="external-comfy-title"
  >
    <button
      type="button"
      class="absolute inset-0 h-full w-full cursor-default"
      aria-label={locale.t("common.cancel")}
      onclick={onclose}
    ></button>

    <div
      class="relative z-10 w-full max-w-lg max-h-[90vh] overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl"
    >
      <div class="mb-3 flex items-start justify-between gap-3">
        <h2 id="external-comfy-title" class="text-base font-semibold text-neutral-100">
          {title}
        </h2>
        <button
          type="button"
          class="text-neutral-500 hover:text-neutral-200 text-lg leading-none shrink-0"
          onclick={onclose}
          aria-label={locale.t("common.cancel")}
        >×</button>
      </div>

      <p class="text-sm text-neutral-300">
        {locale.t("app.external_comfy.found_server_prefix")}
        <span class="font-mono text-indigo-300">{serverUrl}</span>
        {locale.t("app.external_comfy.found_server_suffix")}
      </p>

      <p class="mt-3 text-sm text-neutral-400">
        {#if isNodeLoadFailure}
          {locale.t("app.external_comfy.missing_nodes_body")}
        {:else}
          {locale.t("app.external_comfy.already_running_body", { port })}
        {/if}
      </p>

      {#if payload.missing_nodes && payload.missing_nodes.length > 0}
        <p class="mt-2 text-xs font-mono text-amber-200/90 break-words">
          {payload.missing_nodes.join(", ")}
        </p>
      {/if}

      <div class="mt-4 rounded-lg border border-neutral-800 bg-neutral-950/60 p-3">
        <p class="text-xs font-semibold text-neutral-300">
          {locale.t("app.external_comfy.what_to_do")}
        </p>
        <ol class="mt-2 list-decimal list-inside space-y-1 text-xs text-neutral-400">
          <li>{locale.t("app.external_comfy.step_close_apps")}</li>
          <li>{locale.t("app.external_comfy.step_kill_process")}</li>
          <li>{locale.t("app.external_comfy.step_try_again")}</li>
        </ol>
        <p class="mt-3 text-xs text-neutral-500">
          {locale.t("app.external_comfy.external_hint")}
        </p>
      </div>

      {#if payload.log_excerpt}
        <details class="mt-4 rounded border border-neutral-800 bg-black/40 p-2">
          <summary class="cursor-pointer text-xs text-neutral-400 select-none">
            {locale.t("app.external_comfy.technical_details")}
          </summary>
          <pre
            class="mt-2 max-h-40 overflow-auto text-[10px] text-neutral-400 whitespace-pre-wrap break-words"
          >{payload.log_excerpt}</pre>
        </details>
      {/if}

      {#if payload.error}
        <details class="mt-2 rounded border border-neutral-800/80 bg-black/30 p-2">
          <summary class="cursor-pointer text-xs text-neutral-500 select-none">
            {locale.t("app.external_comfy.technical_details")}
          </summary>
          <pre
            class="mt-2 max-h-32 overflow-auto text-[10px] text-neutral-500 whitespace-pre-wrap break-words"
          >{payload.error}</pre>
        </details>
      {/if}

      {#if localError}
        <p class="mt-3 text-sm text-red-400">{localError}</p>
      {/if}

      <div class="mt-5 flex flex-wrap gap-2 justify-end">
        <button
          type="button"
          class="px-4 py-2 rounded-lg border border-neutral-600 text-neutral-300 text-sm hover:bg-neutral-800 transition-colors cursor-pointer"
          onclick={onclose}
          disabled={busy}
        >
          {locale.t("common.cancel")}
        </button>
        <button
          type="button"
          class="px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-sm transition-colors cursor-pointer disabled:opacity-50"
          onclick={killAndRestart}
          disabled={busy}
        >
          {busy
            ? locale.t("app.external_comfy.kill_busy")
            : locale.t("app.external_comfy.kill_and_restart")}
        </button>
      </div>
    </div>
  </div>
{/if}
