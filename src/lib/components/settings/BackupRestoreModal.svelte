<script lang="ts">
  import { Download, Upload, X } from "@lucide/svelte";
  import { locale } from "../../stores/locale.svelte.js";
  import { wildcards } from "../../stores/wildcards.svelte.js";
  import {
    applyBackupAdminData,
    applyBackupAdminDataFile,
    exportBackupZip,
    exportBackupZipBlob,
    inspectBackupZip,
    inspectBackupZipFile,
    type BackupInspection,
  } from "../../utils/api.js";
  import {
    applyBackupUserAndExtras,
    BACKUP_CATEGORY_SPECS,
    categoryPresentInInspection,
    categorySummaryCount,
    collectBackupBundle,
    type BackupCategoryId,
  } from "../../utils/backupService.js";
  import { ipcOpenFileDialog, isTauri } from "../../utils/ipc.js";
  import { portal } from "../../utils/portal.js";
  import InfoTip from "../ui/InfoTip.svelte";

  interface Props {
    open: boolean;
    isAdmin: boolean;
    onclose: () => void;
    onConfigRestored?: () => void | Promise<void>;
  }

  let { open, isAdmin, onclose, onConfigRestored }: Props = $props();

  let includeAdminExport = $state(true);
  let exporting = $state(false);
  let exportStatus = $state<string | null>(null);
  let exportError = $state<string | null>(null);

  let inspecting = $state(false);
  let restoring = $state(false);
  let selectedPath = $state<string | null>(null);
  let selectedFile = $state<File | null>(null);
  let selectedLabel = $state<string | null>(null);
  let inspection = $state<BackupInspection | null>(null);
  let restoreMode = $state<"merge" | "replace">("merge");
  let confirmReplace = $state(false);

  let selectedCategories = $state<Record<BackupCategoryId, boolean>>({
    characters: true,
    prompts: true,
    styles: true,
    lora_presets: true,
    generation: true,
    gallery_boards: true,
    comics: true,
    ui_notes: true,
  });

  let applyAppConfig = $state(true);
  let includeMachineLocalPaths = $state(false);
  let applyWildcards = $state(true);

  let restoreStatus = $state<string | null>(null);
  let restoreError = $state<string | null>(null);

  function close() {
    if (exporting || inspecting || restoring) return;
    confirmReplace = false;
    onclose();
  }

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }

  function resetInspectionState() {
    selectedPath = null;
    selectedFile = null;
    selectedLabel = null;
    inspection = null;
    confirmReplace = false;
    restoreStatus = null;
    restoreError = null;
  }

  function initSelectionsFromInspection(data: BackupInspection) {
    const next = { ...selectedCategories };
    for (const spec of BACKUP_CATEGORY_SPECS) {
      next[spec.id] = categoryPresentInInspection(data, spec);
    }
    selectedCategories = next;
    applyAppConfig = isAdmin && data.has_app_config;
    // Machine-local paths stay opt-in (unchecked by default) to avoid clobbering host paths.
    includeMachineLocalPaths = false;
    applyWildcards = isAdmin && data.wildcard_paths.length > 0;
    confirmReplace = false;
  }

  async function handleExport() {
    exporting = true;
    exportStatus = null;
    exportError = null;
    try {
      const bundle = await collectBackupBundle();
      const withAdmin = isAdmin && includeAdminExport;
      const date = new Date().toISOString().slice(0, 10);
      const defaultName = `mooshieui-backup-${date}.zip`;

      if (isTauri) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const dest = await save({
          title: locale.t("settings.backup.export_dialog_title"),
          defaultPath: defaultName,
          filters: [{ name: "ZIP", extensions: ["zip"] }],
        });
        if (!dest) return;
        await exportBackupZip(dest, bundle, withAdmin);
      } else {
        const { blob, filename } = await exportBackupZipBlob(bundle, withAdmin);
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        a.remove();
        URL.revokeObjectURL(url);
      }
      exportStatus = locale.t("settings.backup.export_success");
    } catch (e) {
      exportError = e instanceof Error ? e.message : String(e);
    } finally {
      exporting = false;
    }
  }

  async function handleChooseFile() {
    restoreStatus = null;
    restoreError = null;
    confirmReplace = false;
    try {
      if (isTauri) {
        const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
        const picked = await openDialog({
          title: locale.t("settings.backup.import_dialog_title"),
          multiple: false,
          filters: [{ name: "ZIP", extensions: ["zip"] }],
        });
        if (typeof picked !== "string" || !picked) return;
        inspecting = true;
        resetInspectionState();
        selectedPath = picked;
        selectedLabel = picked.split(/[\\/]/).pop() ?? picked;
        const data = await inspectBackupZip(picked);
        inspection = data;
        initSelectionsFromInspection(data);
      } else {
        const file = await ipcOpenFileDialog({ accept: ".zip" });
        if (!file) return;
        inspecting = true;
        resetInspectionState();
        selectedFile = file;
        selectedLabel = file.name;
        const data = await inspectBackupZipFile(file);
        inspection = data;
        initSelectionsFromInspection(data);
      }
    } catch (e) {
      resetInspectionState();
      restoreError = e instanceof Error ? e.message : String(e);
    } finally {
      inspecting = false;
    }
  }

  const anySelected = $derived.by(() => {
    if (!inspection) return false;
    const hasUser = BACKUP_CATEGORY_SPECS.some(
      (s) => selectedCategories[s.id] && categoryPresentInInspection(inspection!, s),
    );
    const hasAdmin =
      isAdmin &&
      ((inspection.has_app_config && applyAppConfig) ||
        (inspection.wildcard_paths.length > 0 && applyWildcards));
    return hasUser || hasAdmin;
  });

  async function handleRestore() {
    if (!inspection || !anySelected) return;
    if (restoreMode === "replace" && !confirmReplace) {
      confirmReplace = true;
      return;
    }
    restoring = true;
    confirmReplace = false;
    restoreStatus = null;
    restoreError = null;

    try {
      const chosen = new Set<BackupCategoryId>(
        BACKUP_CATEGORY_SPECS.filter(
          (s) => selectedCategories[s.id] && categoryPresentInInspection(inspection!, s),
        ).map((s) => s.id),
      );

      await applyBackupUserAndExtras(inspection, chosen, restoreMode);

      const wantConfig = isAdmin && inspection.has_app_config && applyAppConfig;
      const wantWildcards = isAdmin && inspection.wildcard_paths.length > 0 && applyWildcards;
      if (wantConfig || wantWildcards) {
        const opts = {
          apply_app_config: wantConfig,
          include_machine_local_paths: wantConfig && includeMachineLocalPaths,
          apply_wildcards: wantWildcards,
          replace_wildcards: restoreMode === "replace",
        };
        if (isTauri && selectedPath) {
          await applyBackupAdminData(selectedPath, opts);
        } else if (selectedFile) {
          await applyBackupAdminDataFile(selectedFile, opts);
        }
        if (wantConfig) await onConfigRestored?.();
        if (wantWildcards) await wildcards.load(true);
      }

      restoreStatus = locale.t("settings.backup.restore_success");
    } catch (e) {
      restoreError = e instanceof Error ? e.message : String(e);
    } finally {
      restoring = false;
    }
  }
</script>

<svelte:window onkeydown={open ? onKeydown : undefined} />

{#if open}
  <div
    use:portal
    data-modal-open="true"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
    onclick={(e) => {
      if (e.currentTarget === e.target) close();
    }}
    role="presentation"
  >
    <div
      class="flex max-h-[90vh] w-full max-w-lg flex-col overflow-hidden rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      aria-labelledby="backup-restore-title"
    >
      <!-- Header -->
      <div class="flex items-center justify-between border-b border-neutral-800 px-5 py-4">
        <div class="flex items-center gap-1.5">
          <h3 id="backup-restore-title" class="text-base font-semibold text-neutral-100">
            {locale.t("settings.backup.title")}
          </h3>
          <InfoTip text={locale.t("settings.backup.overview_tip")} />
        </div>
        <button
          type="button"
          onclick={close}
          disabled={exporting || inspecting || restoring}
          class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-lg text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200 disabled:opacity-50"
          aria-label={locale.t("common.close")}
        >
          <X class="h-4 w-4" />
        </button>
      </div>

      <!-- Body -->
      <div class="flex-1 space-y-5 overflow-y-auto p-5">
        <!-- Export -->
        <div class="space-y-3 rounded-lg border border-neutral-800 bg-neutral-950/60 p-4">
          <div class="flex items-center justify-between gap-2">
            <div class="flex items-center gap-1.5">
              <span class="text-xs font-semibold uppercase tracking-wide text-neutral-300">
                {locale.t("settings.backup.export_section")}
              </span>
              <InfoTip text={locale.t("settings.backup.export_tip")} />
            </div>
            <button
              type="button"
              onclick={handleExport}
              disabled={exporting || restoring}
              class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Download class="h-3.5 w-3.5" />
              {exporting ? locale.t("settings.backup.exporting") : locale.t("settings.backup.export_button")}
            </button>
          </div>

          {#if isAdmin}
            <label class="flex cursor-pointer items-center gap-2 text-xs text-neutral-300">
              <input
                type="checkbox"
                bind:checked={includeAdminExport}
                class="h-3.5 w-3.5 rounded border-neutral-600 bg-neutral-800 accent-indigo-500"
              />
              <span>{locale.t("settings.backup.include_admin")}</span>
              <InfoTip text={locale.t("settings.backup.include_admin_tip")} />
            </label>
          {/if}

          {#if exportStatus}
            <div class="rounded border border-emerald-700/50 bg-emerald-950/40 px-2.5 py-1.5 text-xs text-emerald-300">
              {exportStatus}
            </div>
          {/if}
          {#if exportError}
            <div class="rounded border border-red-700/50 bg-red-950/40 px-2.5 py-1.5 text-xs text-red-300">
              {exportError}
            </div>
          {/if}
        </div>

        <!-- Restore -->
        <div class="space-y-3 rounded-lg border border-neutral-800 bg-neutral-950/60 p-4">
          <div class="flex items-center justify-between gap-2">
            <div class="flex items-center gap-1.5">
              <span class="text-xs font-semibold uppercase tracking-wide text-neutral-300">
                {locale.t("settings.backup.restore_section")}
              </span>
              <InfoTip text={locale.t("settings.backup.restore_tip")} />
            </div>
            <button
              type="button"
              onclick={handleChooseFile}
              disabled={inspecting || restoring}
              class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs font-medium text-neutral-200 transition-colors hover:bg-neutral-700 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Upload class="h-3.5 w-3.5" />
              {inspecting ? locale.t("settings.backup.inspecting") : locale.t("settings.backup.choose_file")}
            </button>
          </div>

          {#if selectedLabel && inspection}
            <!-- Manifest preview -->
            <div class="flex flex-wrap items-center justify-between gap-2 rounded border border-neutral-800 bg-neutral-900 px-3 py-2 text-[11px] text-neutral-400">
              <span class="truncate font-medium text-neutral-200" title={selectedLabel}>
                {selectedLabel}
              </span>
              <div class="flex items-center gap-2 shrink-0">
                <span>v{inspection.manifest.app_version}</span>
                <span>·</span>
                <span>{inspection.manifest.username}</span>
                <span>·</span>
                <span>{locale.formatDateTime(inspection.manifest.exported_at)}</span>
              </div>
            </div>

            <!-- Merge vs Replace -->
            <div class="flex items-center justify-between gap-2">
              <div class="flex items-center gap-1">
                <span class="text-xs text-neutral-300">{locale.t("settings.backup.mode_label")}</span>
                <InfoTip text={locale.t("settings.backup.mode_tip")} />
              </div>
              <div class="inline-flex rounded-lg border border-neutral-700 bg-neutral-900 p-0.5 text-xs">
                <button
                  type="button"
                  onclick={() => {
                    restoreMode = "merge";
                    confirmReplace = false;
                  }}
                  class="cursor-pointer rounded-md px-2.5 py-1 transition-colors {restoreMode === 'merge'
                    ? 'bg-indigo-600 text-white'
                    : 'text-neutral-400 hover:text-neutral-200'}"
                >
                  {locale.t("settings.backup.mode_merge")}
                </button>
                <button
                  type="button"
                  onclick={() => {
                    restoreMode = "replace";
                    confirmReplace = false;
                  }}
                  class="cursor-pointer rounded-md px-2.5 py-1 transition-colors {restoreMode === 'replace'
                    ? 'bg-amber-600 text-white'
                    : 'text-neutral-400 hover:text-neutral-200'}"
                >
                  {locale.t("settings.backup.mode_replace")}
                </button>
              </div>
            </div>

            <!-- Category checkboxes -->
            <div class="space-y-1.5 pt-1">
              {#each BACKUP_CATEGORY_SPECS as spec (spec.id)}
                {@const present = categoryPresentInInspection(inspection, spec)}
                {@const count = categorySummaryCount(inspection, spec.id)}
                <label
                  class="flex items-center justify-between gap-2 rounded border border-neutral-800/80 px-2.5 py-1.5 text-xs {present
                    ? 'cursor-pointer bg-neutral-900/70 text-neutral-200 hover:border-neutral-700'
                    : 'cursor-not-allowed bg-neutral-900/30 text-neutral-600'}"
                >
                  <span class="flex items-center gap-2">
                    <input
                      type="checkbox"
                      disabled={!present || restoring}
                      checked={present && selectedCategories[spec.id]}
                      onchange={(e) => {
                        selectedCategories = {
                          ...selectedCategories,
                          [spec.id]: e.currentTarget.checked,
                        };
                        confirmReplace = false;
                      }}
                      class="h-3.5 w-3.5 rounded border-neutral-600 bg-neutral-800 accent-indigo-500"
                    />
                    <span>{locale.t(spec.labelKey)}</span>
                  </span>
                  {#if count !== null && count > 0}
                    <span class="rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-400">
                      {count}
                    </span>
                  {/if}
                </label>
              {/each}

              {#if isAdmin && inspection.has_app_config}
                <div class="space-y-1.5 rounded border border-neutral-800/80 bg-neutral-900/70 px-2.5 py-1.5">
                  <label class="flex cursor-pointer items-center justify-between gap-2 text-xs text-neutral-200">
                    <span class="flex items-center gap-2">
                      <input
                        type="checkbox"
                        disabled={restoring}
                        bind:checked={applyAppConfig}
                        onchange={() => (confirmReplace = false)}
                        class="h-3.5 w-3.5 rounded border-neutral-600 bg-neutral-800 accent-indigo-500"
                      />
                      <span>{locale.t("settings.backup.cat.app_config")}</span>
                      <InfoTip text={locale.t("settings.backup.app_config_tip")} />
                    </span>
                  </label>
                  {#if inspection.machine_local_paths.length > 0}
                    <label
                      class="ml-5 flex items-center gap-2 text-[11px] {applyAppConfig
                        ? 'cursor-pointer text-neutral-400'
                        : 'cursor-not-allowed text-neutral-600'}"
                    >
                      <input
                        type="checkbox"
                        disabled={!applyAppConfig || restoring}
                        bind:checked={includeMachineLocalPaths}
                        onchange={() => (confirmReplace = false)}
                        class="h-3 w-3 rounded border-neutral-600 bg-neutral-800 accent-indigo-500"
                      />
                      <span>{locale.t("settings.backup.include_local_paths")}</span>
                      <InfoTip
                        text={locale.t("settings.backup.include_local_paths_tip", {
                          paths: inspection.machine_local_paths.join(", "),
                        })}
                      />
                    </label>
                  {/if}
                </div>
              {/if}

              {#if isAdmin && inspection.wildcard_paths.length > 0}
                <label class="flex cursor-pointer items-center justify-between gap-2 rounded border border-neutral-800/80 bg-neutral-900/70 px-2.5 py-1.5 text-xs text-neutral-200 hover:border-neutral-700">
                  <span class="flex items-center gap-2">
                    <input
                      type="checkbox"
                      disabled={restoring}
                      bind:checked={applyWildcards}
                      onchange={() => (confirmReplace = false)}
                      class="h-3.5 w-3.5 rounded border-neutral-600 bg-neutral-800 accent-indigo-500"
                    />
                    <span>{locale.t("settings.backup.cat.wildcards")}</span>
                  </span>
                  <span class="rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-400">
                    {inspection.wildcard_paths.length}
                  </span>
                </label>
              {/if}
            </div>

            <!-- Restore action -->
            <div class="flex items-center justify-end gap-2 pt-2">
              {#if confirmReplace}
                <button
                  type="button"
                  onclick={() => (confirmReplace = false)}
                  disabled={restoring}
                  class="cursor-pointer rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-300 hover:bg-neutral-700"
                >
                  {locale.t("common.cancel")}
                </button>
              {/if}
              <button
                type="button"
                onclick={handleRestore}
                disabled={restoring || !anySelected}
                class="cursor-pointer rounded-lg px-3.5 py-1.5 text-xs font-medium text-white transition-colors disabled:cursor-not-allowed disabled:opacity-50 {confirmReplace
                  ? 'bg-amber-600 hover:bg-amber-500'
                  : 'bg-indigo-600 hover:bg-indigo-500'}"
              >
                {#if restoring}
                  {locale.t("settings.backup.restoring")}
                {:else if confirmReplace}
                  {locale.t("settings.backup.confirm_replace")}
                {:else}
                  {locale.t("settings.backup.restore_button")}
                {/if}
              </button>
            </div>
          {/if}

          {#if restoreStatus}
            <div class="rounded border border-emerald-700/50 bg-emerald-950/40 px-2.5 py-1.5 text-xs text-emerald-300">
              {restoreStatus}
            </div>
          {/if}
          {#if restoreError}
            <div class="rounded border border-red-700/50 bg-red-950/40 px-2.5 py-1.5 text-xs text-red-300">
              {restoreError}
            </div>
          {/if}
        </div>
      </div>
    </div>
  </div>
{/if}
