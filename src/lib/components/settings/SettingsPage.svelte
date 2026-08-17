<script lang="ts">
  import type { AppConfig, LlmProviderState, QueueInfo } from "../../types/index.js";
  import { getConfig, updateConfig, stopComfyui, startComfyui, fetchReleaseNotes, importImageDirectory, exportLogs, exportLogsContent, getGalleryPath, setGalleryPath, setStorageLimit, installAttentionBackend, checkAttentionBackend, clearAllQueues, getQueue, getGpuStats, getComfyuiVersion, updateComfyui } from "../../utils/api.js";
  import type { ReleaseNote, ImportResult, AttentionBackendStatus, BackendSupport, ComfyUiVersionInfo } from "../../utils/api.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { autocomplete } from "../../stores/autocomplete.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { accessibility } from "../../stores/accessibility.svelte.js";
  import { locale, LOCALE_OPTIONS } from "../../stores/locale.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import PromptAssistantSetupModal from "../generation/PromptAssistantSetupModal.svelte";
  import OpenModelFolders from "./OpenModelFolders.svelte";
  import ModelManagerModal from "./ModelManagerModal.svelte";
  import GpuStatusPanel from "./GpuStatusPanel.svelte";
  import ModelRequestsPanel from "./ModelRequestsPanel.svelte";
  import QualityTagsEditor from "./QualityTagsEditor.svelte";
  import LlmProviderPanel from "./LlmProviderPanel.svelte";
  import { ipcInvoke, ipcListen, isTauri, isBrowserMode, authHeaders, clearAuthToken } from "../../utils/ipc.js";
  import { useMobileLayout, isMobileUA, setForceDesktopOverride } from "../../utils/device.js";
  import {
    applyTheme,
    THEME_PALETTES,
    THEME_TONE_FIELDS,
    DEFAULT_THEME_TONE_DARK,
    DEFAULT_THEME_TONE_LIGHT,
  } from "../../utils/theme.js";
  import type { ThemeProfile, ThemeTone } from "../../utils/theme.js";
  import { onMount, onDestroy } from "svelte";
  import { marked } from "marked";
  import DOMPurify from "dompurify";
  import { clearArtistImageCache, getArtistImageCacheCount } from "../../artist-gallery/imageCache.js";
  import { appVersion as getAppVersion } from "../../utils/platformInfo.js";
  import type { DownloadEvent } from "@tauri-apps/plugin-updater";

  interface Props {
    userRole?: string;
    mobileFriendly?: boolean;
  }

  let { userRole = "admin", mobileFriendly = false }: Props = $props();

  // Layout override: only meaningful on a mobile-capable device in browser mode,
  // where the mobile shell exists. The control lets the user flip between the
  // touch layout and the full desktop interface (resolved at module load).
  const deviceSupportsMobileLayout = isBrowserMode && isMobileUA();
  function switchLayout() {
    // Currently mobile -> force desktop; currently desktop -> clear the override.
    setForceDesktopOverride(useMobileLayout);
    location.reload();
  }

  // Reopen the first-run setup wizard. check_setup auto-recovers the completion
  // marker (main.py present), so we can't clear backend state to force it;
  // instead a one-shot flag survives the reload and App.svelte shows the wizard.
  // Reloading also avoids re-running initApp() in the live session.
  let showRerunSetupConfirm = $state(false);
  function rerunSetup() {
    localStorage.setItem("mooshieui_force_setup", "1");
    location.reload();
  }

  let settingsScrollEl = $state<HTMLDivElement | null>(null);
  let showScrollToTop = $state(false);

  function onSettingsScroll() {
    showScrollToTop = (settingsScrollEl?.scrollTop ?? 0) > 240;
  }

  function scrollSettingsToTop() {
    settingsScrollEl?.scrollTo({ top: 0, behavior: "smooth" });
  }
  const isAdmin = $derived(userRole === "admin");
  const canManageServer = $derived(userRole === "admin" || userRole === "moderator");

  const activeThemeProfile = $derived.by(() => {
    if (!config?.theme_profile_id) return null;
    const profiles = config.theme_profiles ?? [];
    return profiles.find((profile) => profile.id === config!.theme_profile_id) ?? null;
  });

  /** Open a directory picker. Returns path string or null. */
  async function openDirectoryDialog(title: string): Promise<string | null> {
    if (!isTauri) return null;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({ directory: true, multiple: false, title });
    return typeof selected === "string" ? selected : null;
  }

  // Configure marked for safe rendering (no raw HTML passthrough)
  marked.setOptions({ breaks: true, gfm: true });

  const appVersion = getAppVersion();

  let config = $state<AppConfig | null>(null);
  let showPromptAssistantSetup = $state(false);
  let loading = $state(true);
  let saving = $state(false);
  let saved = $state(false);
  let error = $state<string | null>(null);
  let restartNeeded = $state(false);
  let restarting = $state(false);
  let search = $state("");

  let tagUrlInput = $state("");
  let tagFileLoading = $state(false);
  let showQualityTagsWarning = $state(false);
  let showAdvancedModeWarning = $state(false);

  // Attention backend state
  let attentionInstalling = $state(false);
  let attentionError = $state<string | null>(null);
  let attentionStatus = $state<AttentionBackendStatus | null>(null);
  let attentionStatusLoading = $state(false);
  let attentionStatusError = $state<string | null>(null);
  let attentionStatusVenvPath = $state<string | null>(null);
  // ComfyUI version / in-app updater state (desktop only)
  let comfyuiVersion = $state<ComfyUiVersionInfo | null>(null);
  let comfyuiUpdating = $state(false);
  let comfyuiUpdateError = $state<string | null>(null);
  let comfyuiUpdateProgress = $state<string | null>(null);
  let workersDetecting = $state(false);
  let newThemeName = $state("");
  let themeImportError = $state<string | null>(null);
  let themeImportDone = $state(false);
  let themeExportDone = $state(false);
  let themeExportError = $state<string | null>(null);
  let showThemeCreatorModal = $state(false);
  let draftEditingProfileId = $state<string | null>(null);
  let settingsLoadError = $state<string | null>(null);
  let draftThemeName = $state("");
  let draftThemeDark = $state<ThemeTone>({ ...DEFAULT_THEME_TONE_DARK });
  let draftThemeLight = $state<ThemeTone>({ ...DEFAULT_THEME_TONE_LIGHT });
  let draftThemeBackgroundImage = $state<string | null>(null);
  let draftThemeBackgroundFade = $state(0.65);
  let draftThemeHideBranding = $state(false);
  let draftThemeLogoImage = $state<string | null>(null);
  let draftToneLinked = $state<Record<keyof ThemeTone, boolean>>({
    main: true,
    sub: true,
    trim: true,
    background: true,
    text: true,
  });
  let showLogoCropModal = $state(false);
  let pendingLogoDataUrl = $state<string | null>(null);
  let logoCropZoom = $state(1);
  let logoCropPanX = $state(0);
  let logoCropPanY = $state(0);
  let logoCropTarget = $state<"draft" | "active">("draft");

  // Gallery import state
  let importBusy = $state(false);
  let importResult = $state<ImportResult | null>(null);
  let importError = $state<string | null>(null);

  // Log export state
  let exportingLogs = $state(false);
  let logExportDone = $state(false);
  let logExportError = $state<string | null>(null);

  // Clear queue state (mod/admin only)
  let clearQueueBusy = $state(false);
  let clearQueueDone = $state(false);
  let clearQueueError = $state<string | null>(null);
  let showClearQueueConfirm = $state(false);

  function ensureGpuWorkers() {
    if (!config) return;
    if (!Array.isArray(config.gpu_workers)) config.gpu_workers = [];
  }

  function addGpuWorker() {
    if (!config) return;
    ensureGpuWorkers();
    config.gpu_workers = [
      ...config.gpu_workers,
      {
        gpu_index: config.gpu_workers.length,
        port: null,
        enabled: true,
        label: null,
        vram_mode: null,
      },
    ];
    autoSave();
  }

  function removeGpuWorker(index: number) {
    if (!config) return;
    ensureGpuWorkers();
    config.gpu_workers = config.gpu_workers.filter((_, i) => i !== index);
    autoSave();
  }

  async function autoDetectGpuWorkers() {
    if (!config) return;
    workersDetecting = true;
    try {
      const stats = await getGpuStats();
      if (stats.length > 0) {
        config.gpu_workers = stats.map((gpu) => ({
          gpu_index: gpu.index,
          port: gpu.worker?.port ?? config!.server_port + gpu.index,
          enabled: true,
          label: gpu.name,
          vram_mode: null,
        }));
        autoSave();
      }
    } finally {
      workersDetecting = false;
    }
  }

  async function handleClearQueue() {
    clearQueueBusy = true;
    clearQueueError = null;
    try {
      await clearAllQueues();
      clearQueueDone = true;
      showClearQueueConfirm = false;
      setTimeout(() => (clearQueueDone = false), 3000);
    } catch (e: any) {
      clearQueueError = e?.message ?? String(e);
    } finally {
      clearQueueBusy = false;
    }
  }

  // Mode switching state
  let switchingMode = $state(false);
  /** After a successful switch, which mode we landed in (for status text). */
  let modeSwitchResult = $state<"browser" | "app" | null>(null);
  let showModelManager = $state(false);

  // Queue viewer state (live-polling when settings page is open)
  let queueData = $state<QueueInfo | null>(null);
  let queuePollInterval: ReturnType<typeof setInterval> | null = null;

  async function refreshQueue() {
    try {
      queueData = await getQueue();
    } catch {
      // non-critical
    }
  }

  function startQueuePolling() {
    void refreshQueue();
    queuePollInterval = setInterval(refreshQueue, 3000);
  }

  function stopQueuePolling() {
    if (queuePollInterval !== null) {
      clearInterval(queuePollInterval);
      queuePollInterval = null;
    }
  }

  /** Returns the set of prompt_ids currently in the running list. */
  function runningIds(info: QueueInfo): Set<string> {
    const ids = new Set<string>();
    for (const entry of info.queue_running) {
      const arr = entry as unknown[];
      if (arr.length >= 2 && typeof arr[1] === "string") ids.add(arr[1]);
    }
    return ids;
  }

  let lanAccounts = $state<{ username: string; role: string; online: boolean; created_at: string; last_online: string | null; storage_limit_bytes: number; can_use_modelhub: boolean }[]>([]);
  let lanNewUser = $state("");
  let lanNewPass = $state("");
  let lanAuthError = $state<string | null>(null);
  let lanAuthBusy = $state(false);
  let lanAddresses = $state<string[]>([]);
  let showAddAccountModal = $state(false);

  // Account list: search, sort, and delete modal
  let accountSearch = $state("");
  let accountSort = $state<"name" | "joined" | "last_online">("name");
  let accountSortAsc = $state(true);
  let showDeleteModal = $state(false);
  let deleteTargetUser = $state("");
  let deleteKeepData = $state(true);

  // Account actions modal (per-user)
  let showAccountActionsModal = $state(false);
  let actionsTargetAccount = $state<{ username: string; role: string; online: boolean; created_at: string; last_online: string | null; storage_limit_bytes: number; can_use_modelhub: boolean } | null>(null);

  // Storage limit modal
  let showStorageModal = $state(false);
  let storageTargetUser = $state("");
  let storageInputGB = $state("1");
  let storageError = $state<string | null>(null);
  let storageBusy = $state(false);


  async function applyStorageLimit() {
    storageBusy = true;
    storageError = null;
    try {
      const gb = locale.parseDecimal(storageInputGB);
      if (isNaN(gb) || gb < 0.1 || gb > 100) {
        storageError = locale.t("settings.lan.storage_range_error");
        return;
      }
      const limitBytes = Math.round(gb * 1024 * 1024 * 1024);
      await setStorageLimit(storageTargetUser, limitBytes);
      showStorageModal = false;
      await loadLanAccounts();
    } catch (e: any) {
      storageError = e.message || String(e);
    } finally {
      storageBusy = false;
    }
  }

  function relativeTime(iso: string | null): string {
    if (!iso) return locale.t("settings.lan.time.never");
    const diff = Date.now() - new Date(iso).getTime();
    if (diff < 0 || isNaN(diff)) return locale.t("settings.lan.time.unknown");
    const sec = Math.floor(diff / 1000);
    if (sec < 60) return locale.t("notifications.time.just_now");
    const min = Math.floor(sec / 60);
    if (min < 60) return locale.t("notifications.time.minutes_ago", { min: String(min) });
    const hrs = Math.floor(min / 60);
    if (hrs < 24) return locale.t("notifications.time.hours_ago", { hrs: String(hrs) });
    const days = Math.floor(hrs / 24);
    if (days < 7) return locale.t("notifications.time.days_ago", { days: String(days) });
    const weeks = Math.floor(days / 7);
    if (weeks < 5) return locale.t("settings.lan.time.weeks_ago", { weeks: String(weeks) });
    const months = Math.floor(days / 30);
    if (months < 12) return locale.t("settings.lan.time.months_ago", { months: String(months) });
    return locale.t("settings.lan.time.years_ago", { years: String(Math.floor(days / 365)) });
  }

  const sortedAccounts = $derived.by(() => {
    // Filter by search
    const query = accountSearch.toLowerCase();
    const filtered = query
      ? lanAccounts.filter((a) => a.username.toLowerCase().includes(query))
      : lanAccounts;

    // Partition: online first
    const online = filtered.filter((a) => a.online);
    const offline = filtered.filter((a) => !a.online);

    // Sort helper
    const cmp = (a: typeof lanAccounts[0], b: typeof lanAccounts[0]): number => {
      let v = 0;
      if (accountSort === "name") {
        v = a.username.localeCompare(b.username);
      } else if (accountSort === "joined") {
        v = (a.created_at || "").localeCompare(b.created_at || "");
      } else {
        v = (a.last_online || "").localeCompare(b.last_online || "");
      }
      return accountSortAsc ? v : -v;
    };

    return [...online.sort(cmp), ...offline.sort(cmp)];
  });

  // User self-service password change
  let showChangePasswordForm = $state(false);
  let cpCurrentPass = $state("");
  let cpNewPass1 = $state("");
  let cpNewPass2 = $state("");
  let cpError = $state<string | null>(null);
  let cpSuccess = $state(false);
  let cpBusy = $state(false);

  let usesLegacyPassword = $state(false);
  let legacyPasswordExpired = $state(false);
  let legacyPasswordDeadline = $state<string | null>(null);
  let upgradePass = $state("");
  let upgradeBusy = $state(false);
  let upgradeError = $state<string | null>(null);
  let upgradeSuccess = $state(false);

  function formatLegacyDeadline(iso: string | null): string {
    if (!iso) return "";
    try {
      return new Date(iso).toLocaleDateString(undefined, {
        year: "numeric",
        month: "long",
        day: "numeric",
      });
    } catch {
      return iso;
    }
  }

  async function refreshPasswordSecurityStatus() {
    if (!isBrowserMode) return;
    try {
      const resp = await fetch("/internal-api/_auth/status", {
        headers: authHeaders(),
      });
      if (!resp.ok) return;
      const data = await resp.json();
      usesLegacyPassword = data.uses_legacy_password === true;
      legacyPasswordExpired = data.legacy_password_expired === true;
      legacyPasswordDeadline = data.legacy_password_deadline ?? null;
    } catch (e) {
      console.warn("[settings] failed to read password security status:", e);
    }
  }

  async function upgradePasswordEncryption() {
    if (!upgradePass) {
      upgradeError = locale.t("auth.password_placeholder");
      return;
    }
    upgradeBusy = true;
    upgradeError = null;
    upgradeSuccess = false;
    try {
      const resp = await fetch("/internal-api/_auth/upgrade_password_encryption", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ password: upgradePass }),
      });
      const data = await resp.json();
      if (!resp.ok) {
        upgradeError = data.error ?? locale.t("auth.upgrade_password_failed");
        return;
      }
      upgradePass = "";
      upgradeSuccess = true;
      usesLegacyPassword = false;
      legacyPasswordExpired = false;
      setTimeout(() => (upgradeSuccess = false), 4000);
    } catch (e) {
      upgradeError = String(e);
    } finally {
      upgradeBusy = false;
    }
  }

  async function changeOwnPassword() {
    if (cpNewPass1.length < 4) { cpError = locale.t("auth.password_min_length"); return; }
    if (cpNewPass1 !== cpNewPass2) { cpError = locale.t("auth.passwords_mismatch"); return; }
    cpBusy = true;
    cpError = null;
    cpSuccess = false;
    try {
      const resp = await fetch("/internal-api/_auth/change_password", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ current_password: cpCurrentPass, new_password: cpNewPass1 }),
      });
      const data = await resp.json();
      if (!resp.ok) { cpError = data.error ?? locale.t("auth.change_password_failed"); return; }
      cpCurrentPass = "";
      cpNewPass1 = "";
      cpNewPass2 = "";
      cpSuccess = true;
      setTimeout(() => (cpSuccess = false), 4000);
    } catch (e) {
      cpError = String(e);
    } finally {
      cpBusy = false;
    }
  }

  // Admin reset password modal
  let showResetPasswordModal = $state(false);
  let resetTargetUser = $state("");
  let resetTempPass = $state("");
  let resetError = $state<string | null>(null);
  let resetSuccess = $state(false);
  let resetBusy = $state(false);

  async function adminResetPassword() {
    if (resetTempPass.length < 4) { resetError = locale.t("settings.lan.temp_password_min"); return; }
    resetBusy = true;
    resetError = null;
    resetSuccess = false;
    try {
      const resp = await fetch("/internal-api/_auth/reset_password", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ username: resetTargetUser, temp_password: resetTempPass }),
      });
      const data = await resp.json();
      if (!resp.ok) { resetError = data.error ?? locale.t("settings.lan.reset_failed"); return; }
      resetSuccess = true;
      resetTempPass = "";
    } catch (e) {
      resetError = String(e);
    } finally {
      resetBusy = false;
    }
  }

  // About modal & issue report modal
  let showAboutModal = $state(false);
  let showReportModal = $state(false);
  let reportName = $state("");
  let reportEmail = $state("");
  let reportMessage = $state("");

  async function openExternalUrl(url: string) {
    if (isTauri) {
      const { open } = await import("@tauri-apps/plugin-shell");
      await open(url);
    } else {
      window.open(url, "_blank", "noopener,noreferrer");
    }
  }

  function openReportInMail() {
    const subject = encodeURIComponent(locale.t("settings.about.issue_report_subject"));
    const bodyParts = [];
    if (reportName.trim()) bodyParts.push(`Name: ${reportName.trim()}`);
    if (reportEmail.trim()) bodyParts.push(`Email: ${reportEmail.trim()}`);
    if (bodyParts.length) bodyParts.push("");
    bodyParts.push(reportMessage.trim());
    const mailtoUrl = `mailto:blob@mooshieblob.com?subject=${subject}&body=${encodeURIComponent(bodyParts.join("\n"))}`;
    openExternalUrl(mailtoUrl);
    showReportModal = false;
    reportName = "";
    reportEmail = "";
    reportMessage = "";
  }

  async function loadLanAccounts() {
    try {
      const resp = await fetch("/internal-api/_auth/accounts", { headers: authHeaders() });
      const data = await resp.json();
      const raw = data.accounts ?? [];
      // Normalise: backend now returns {username, role, online, created_at, last_online}
      lanAccounts = raw.map((a: any) =>
        typeof a === "string"
          ? { username: a, role: "user", online: false, created_at: "", last_online: null, storage_limit_bytes: 1024 * 1024 * 1024 }
          : { username: a.username, role: a.role ?? "user", online: !!a.online, created_at: a.created_at ?? "", last_online: a.last_online ?? null, storage_limit_bytes: a.storage_limit_bytes ?? 1024 * 1024 * 1024 }
      );
    } catch {
      lanAccounts = [];
    }
  }

  async function loadLanInfo() {
    try {
      const resp = await fetch("/internal-api/_auth/lan_info", { headers: authHeaders() });
      const data = await resp.json();
      lanAddresses = data.addresses ?? [];
    } catch {
      lanAddresses = [];
    }
  }

  async function createLanAccount() {
    if (!lanNewUser.trim() || lanNewPass.length < 4) {
      lanAuthError = locale.t("settings.lan.account_validation");
      return;
    }
    lanAuthBusy = true;
    lanAuthError = null;
    try {
      const resp = await fetch("/internal-api/_auth/register", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ username: lanNewUser.trim(), password: lanNewPass }),
      });
      const data = await resp.json();
      if (!resp.ok) {
        lanAuthError = data.error ?? locale.t("settings.lan.create_failed");
      } else {
        lanNewUser = "";
        lanNewPass = "";
        await loadLanAccounts();
      }
    } catch (e) {
      lanAuthError = String(e);
    } finally {
      lanAuthBusy = false;
    }
  }

  async function deleteLanAccount(username: string, keepData: boolean = false) {
    lanAuthBusy = true;
    lanAuthError = null;
    try {
      const resp = await fetch("/internal-api/_auth/delete", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ username, keep_data: keepData }),
      });
      if (!resp.ok) {
        const data = await resp.json();
        lanAuthError = data.error ?? locale.t("settings.lan.delete_failed");
      } else {
        await loadLanAccounts();
      }
    } catch (e) {
      lanAuthError = String(e);
    } finally {
      lanAuthBusy = false;
    }
  }

  async function toggleAccountRole(username: string, currentRole: string) {
    const newRole = currentRole === "moderator" ? "user" : "moderator";
    lanAuthBusy = true;
    lanAuthError = null;
    try {
      const resp = await fetch("/internal-api/_auth/set_role", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ username, role: newRole }),
      });
      if (!resp.ok) {
        const data = await resp.json();
        lanAuthError = data.error ?? locale.t("settings.lan.role_update_failed");
      } else {
        await loadLanAccounts();
      }
    } catch (e) {
      lanAuthError = String(e);
    } finally {
      lanAuthBusy = false;
    }
  }

  async function toggleModelhubAccess(username: string, currentValue: boolean) {
    lanAuthBusy = true;
    lanAuthError = null;
    try {
      const resp = await fetch("/internal-api/_auth/set_modelhub_access", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ username, allowed: !currentValue }),
      });
      if (!resp.ok) {
        const data = await resp.json();
        lanAuthError = data.error ?? locale.t("settings.lan.modelhub_update_failed");
      } else {
        await loadLanAccounts();
      }
    } catch (e) {
      lanAuthError = String(e);
    } finally {
      lanAuthBusy = false;
    }
  }

  async function switchUiMode() {
    if (!config) return;
    switchingMode = true;
    modeSwitchResult = null;
    const newMode = !config.browser_mode;
    console.log("[switchUiMode] isTauri:", isTauri, "isBrowserMode:", isBrowserMode, "config.browser_mode:", config.browser_mode, "newMode:", newMode);
    try {
      if (isTauri && newMode) {
        // App → Browser: call backend to start web server, open browser, hide window
        console.log("[switchUiMode] calling switch_to_browser_mode via Tauri invoke...");
        await ipcInvoke("switch_to_browser_mode");
        console.log("[switchUiMode] switch_to_browser_mode succeeded");
        config.browser_mode = true;
        modeSwitchResult = "browser";
      } else if (isTauri && !newMode) {
        // App mode, user wants to stay in app mode? Shouldn't happen but log it
        console.warn("[switchUiMode] already in app mode (isTauri=true, newMode=false)");
        switchingMode = false;
      } else if (!isTauri && isBrowserMode && !newMode) {
        // Browser → App: show the native Tauri window
        console.log("[switchUiMode] calling switch_to_app_mode via HTTP...");
        const result = await ipcInvoke("switch_to_app_mode");
        console.log("[switchUiMode] switch_to_app_mode result:", JSON.stringify(result));
        config.browser_mode = false;
        modeSwitchResult = "app";
      } else if (!isTauri && isBrowserMode && newMode) {
        // Already in browser mode wanting browser mode? Shouldn't happen
        console.warn("[switchUiMode] already in browser mode");
        switchingMode = false;
      } else {
        console.warn("[switchUiMode] no branch matched — isTauri:", isTauri, "isBrowserMode:", isBrowserMode, "newMode:", newMode);
        switchingMode = false;
      }
    } catch (e) {
      console.error("[switchUiMode] FAILED:", e);
      switchingMode = false;
      modeSwitchResult = null;
    }
  }

  // Gallery path state
  let galleryPathDisplay = $state("");
  let galleryPathSaving = $state(false);
  let galleryPathMessage = $state<string | null>(null);

  async function handleExportLogs() {
    if (isTauri) {
      const { save: saveDialog } = await import("@tauri-apps/plugin-dialog");
      const destination = await saveDialog({
        title: locale.t('settings.about.save_dialog_title'),
        defaultPath: "mooshieui-diagnostics.log",
        filters: [{ name: locale.t("settings.about.log_files_filter"), extensions: ["log", "txt"] }],
      });
      if (!destination) return;
      exportingLogs = true;
      logExportDone = false;
      logExportError = null;
      try {
        await exportLogs(destination);
        logExportDone = true;
        setTimeout(() => (logExportDone = false), 4000);
      } catch (e) {
        logExportError = String(e);
      } finally {
        exportingLogs = false;
      }
      return;
    }
    // Browser/server mode: fetch the diagnostic text and download it client-side.
    exportingLogs = true;
    logExportDone = false;
    logExportError = null;
    try {
      const content = await exportLogsContent();
      const blob = new Blob([content], { type: "text/plain" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "mooshieui-diagnostics.log";
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
      logExportDone = true;
      setTimeout(() => (logExportDone = false), 4000);
    } catch (e) {
      logExportError = String(e);
    } finally {
      exportingLogs = false;
    }
  }

  async function handleImportDirectory() {
    const selected = await openDirectoryDialog(locale.t('settings.gallery.import_dialog_title'));
    if (!selected) return;
    importBusy = true;
    importResult = null;
    importError = null;
    try {
      importResult = await importImageDirectory(selected as string);
      if (importResult.imported > 0) {
        await gallery.loadFromDisk();
      }
    } catch (e) {
      importError = String(e);
    } finally {
      importBusy = false;
    }
  }

  async function handleBrowseGalleryPath() {
    const selected = await openDirectoryDialog(locale.t('settings.gallery.storage_browse_title'));
    if (!selected) return;
    galleryPathSaving = true;
    galleryPathMessage = null;
    try {
      galleryPathDisplay = await setGalleryPath(selected as string);
      if (config) config.gallery_path = selected as string;
      galleryPathMessage = locale.t('settings.gallery.storage_moved');
      setTimeout(() => (galleryPathMessage = null), 6000);
    } catch (e) {
      galleryPathMessage = String(e);
    } finally {
      galleryPathSaving = false;
    }
  }

  async function handleResetGalleryPath() {
    galleryPathSaving = true;
    galleryPathMessage = null;
    try {
      galleryPathDisplay = await setGalleryPath("");
      if (config) config.gallery_path = null;
      galleryPathMessage = locale.t('settings.gallery.storage_moved');
      setTimeout(() => (galleryPathMessage = null), 6000);
    } catch (e) {
      galleryPathMessage = String(e);
    } finally {
      galleryPathSaving = false;
    }
  }

  // Release notes from GitHub
  let releaseNotes = $state<ReleaseNote[]>([]);
  let releaseNotesLoading = $state(false);
  let releaseNotesError = $state<string | null>(null);

  // Artist image cache
  let cacheClearBusy = $state(false);
  let cacheClearDone = $state(false);
  let cacheClearCount = $state<number | null>(null);

  async function loadCacheCount() {
    const n = await getArtistImageCacheCount();
    cacheClearCount = n >= 0 ? n : null;
  }

  async function handleClearArtistCache() {
    cacheClearBusy = true;
    cacheClearDone = false;
    try {
      await clearArtistImageCache();
      cacheClearCount = 0;
      cacheClearDone = true;
      setTimeout(() => (cacheClearDone = false), 3000);
    } finally {
      cacheClearBusy = false;
    }
  }

  async function loadReleaseNotes() {
    if (releaseNotes.length > 0 || releaseNotesLoading) return;
    releaseNotesLoading = true;
    releaseNotesError = null;
    try {
      releaseNotes = await fetchReleaseNotes();
    } catch (e) {
      releaseNotesError = String(e);
    } finally {
      releaseNotesLoading = false;
    }
  }

  function renderReleaseBody(body: string): string {
    // Strip the repeated installer blurb that appears at the top of every release
    const cleaned = body
      .replace(/\*?\*?One-click installer\*?\*?[\s\S]*?\| \*\*Linux\*\* \| [^\n]+\n?/g, "")
      .replace(/^\s*\|[^\n]*\n?/gm, (match) => {
        // Keep tables that aren't the installer table (already stripped above)
        return match;
      })
      .trim();
    if (!cleaned) return `<p class='text-neutral-500 italic'>${locale.t('settings.about.no_notes_html')}</p>`;
    const html = marked.parse(cleaned, { async: false }) as string;
    return DOMPurify.sanitize(html);
  }

  // Model directory auto-detection
  interface DetectedModelDir {
    path: string;
    tool: string;
    has_checkpoints: boolean;
    has_loras: boolean;
    has_vae: boolean;
  }
  let detectedModelDirs = $state<DetectedModelDir[]>([]);
  let scanningModelDirs = $state(false);

  async function scanForModelDirs() {
    scanningModelDirs = true;
    try {
      const dirs = await ipcInvoke<DetectedModelDir[]>("detect_model_directories");
      // Filter out directories already in config
      const existing = new Set(
        (config?.extra_model_paths ?? "").split("\n").map((p: string) => p.trim()).filter(Boolean)
      );
      detectedModelDirs = dirs.filter((d) => !existing.has(d.path));
    } catch {
      detectedModelDirs = [];
    } finally {
      scanningModelDirs = false;
    }
  }

  // Move installation
  let currentInstallPath = $state("");
  let moveTargetPath = $state("");
  let moving = $state(false);
  let moveProgress = $state("");
  let moveError = $state<string | null>(null);
  let moveSuccess = $state(false);

  async function loadInstallPath() {
    try {
      currentInstallPath = await ipcInvoke<string>("get_install_path");
    } catch {
      currentInstallPath = "";
    }
  }

  async function browseMoveTarget() {
    const selected = await openDirectoryDialog(locale.t('settings.paths.move_dialog_title'));
    if (selected) {
      moveTargetPath = selected;
    }
  }

  async function browseModelDir(i: number) {
    if (!config) return;
    const selected = await openDirectoryDialog(locale.t('settings.paths.model_dir_dialog_title'));
    if (selected) {
      const paths = (config.extra_model_paths ?? "").split("\n");
      paths[i] = selected;
      config.extra_model_paths = paths.join("\n") || null;
      checkRestartNeeded();
    }
  }

  async function browseSaveDir(i: number) {
    const selected = await openDirectoryDialog(locale.t('settings.gallery.browse_save_dir_title'));
    if (selected) {
      const dirs = [...generation.autoSaveDirs];
      dirs[i] = selected;
      generation.autoSaveDirs = dirs;
      generation.saveSettings();
    }
  }

  async function moveInstallation() {
    if (!moveTargetPath.trim()) return;
    moving = true;
    moveError = null;
    moveSuccess = false;
    moveProgress = locale.t("settings.gallery.move_starting");

    const unlisten = await ipcListen("setup:progress", (event: any) => {
      const data = event.payload as { message: string };
      moveProgress = data.message;
    });

    try {
      await ipcInvoke("move_installation", { newPath: moveTargetPath.trim() });
      moveSuccess = true;
      moveProgress = "";
      currentInstallPath = moveTargetPath.trim();
      moveTargetPath = "";
      // Reload config since paths changed
      await loadConfig();
    } catch (e: any) {
      moveError = typeof e === "string" ? e : e.message || locale.t("app.status.unknown_error");
      moveProgress = "";
    } finally {
      moving = false;
      unlisten();
    }
  }

  function addDetectedModelDir(path: string) {
    if (!config) return;
    const current = config.extra_model_paths ?? "";
    const paths = current.split("\n").filter((p: string) => p.trim());
    if (!paths.includes(path)) {
      paths.push(path);
      config.extra_model_paths = paths.join("\n");
      checkRestartNeeded();
    }
    // Remove from detected list
    detectedModelDirs = detectedModelDirs.filter((d) => d.path !== path);
  }

  // Update check state
  type UpdateCheckState = "idle" | "checking" | "available" | "downloading" | "ready" | "up-to-date" | "error";
  type BrowserUpdateMode = "local" | "lan" | "server";
  let updateState = $state<UpdateCheckState>("idle");
  let updateVersion = $state("");
  let updateError = $state("");
  let updateDownloaded = $state(0);
  let updateTotal = $state(0);
  let browserUpdateMode = $state<BrowserUpdateMode>("local");
  let updateObj: any | null = null;

  const updatePercent = $derived(updateTotal > 0 ? Math.round((updateDownloaded / updateTotal) * 100) : 0);

  async function refreshBrowserUpdateMode(): Promise<BrowserUpdateMode> {
    let mode: BrowserUpdateMode = "local";
    try {
      const resp = await fetch("/internal-api/_auth/status", {
        headers: authHeaders(),
      });
      if (resp.ok) {
        const data = await resp.json();
        mode = data.server_mode === true ? "server" : data.lan_enabled === true ? "lan" : "local";
      }
    } catch (e) {
      console.warn("[settings] failed to read browser update mode:", e);
    }
    browserUpdateMode = mode;
    return mode;
  }

  async function checkForUpdates() {
    updateState = "checking";
    updateError = "";
    try {
      if (isBrowserMode) {
        updateObj = null;
        await refreshBrowserUpdateMode();
        const resp = await fetch("/internal-api/_check_update", {
          headers: authHeaders(),
        });
        if (!resp.ok) {
          const message = await resp.text();
          throw new Error(message || `Update check failed (${resp.status})`);
        }
        const data = await resp.json();
        if (data.error) throw new Error(String(data.error));
        if (data.update_available) {
          updateVersion = data.latest_version;
          updateState = "available";
        } else {
          updateState = "up-to-date";
        }
        return;
      }
      if (!isTauri) { updateState = "up-to-date"; return; }
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (update) {
        updateObj = update;
        updateVersion = update.version;
        updateState = "available";
      } else {
        updateState = "up-to-date";
      }
    } catch (e) {
      updateState = "error";
      updateError = String(e);
    }
  }

  async function downloadAndInstallUpdate() {
    if (!updateObj && isBrowserMode && browserUpdateMode === "local") {
      updateState = "checking";
      updateError = "";
      try {
        await ipcInvoke("switch_to_app_mode");
        updateState = "available";
      } catch (e) {
        updateState = "error";
        updateError = String(e);
      }
      return;
    }
    if (!updateObj) return;
    updateState = "downloading";
    try {
      await updateObj.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          updateTotal = event.data.contentLength ?? 0;
          updateDownloaded = 0;
        } else if (event.event === "Progress") {
          updateDownloaded += event.data.chunkLength;
        } else if (event.event === "Finished") {
          updateState = "ready";
        }
      });
      // Record the expected version so UpdateNotification can verify on the
      // next launch that the update actually applied (mirrors the banner path).
      if (updateVersion) localStorage.setItem("mooshieui_pending_update", updateVersion);
      updateState = "ready";
    } catch (e) {
      updateState = "error";
      updateError = String(e);
    }
  }
  let dyslexicFont = $state(localStorage.getItem("mooshieui.dyslexicFont") === "true");

  $effect(() => {
    document.documentElement.classList.toggle("dyslexic-font", dyslexicFont);
    localStorage.setItem("mooshieui.dyslexicFont", String(dyslexicFont));
  });

  // Section collapse state (persisted across tab switches)
  const COLLAPSED_KEY = "mooshieui.settings.collapsed.v1";
  let collapsed: Record<string, boolean> = $state(loadCollapsedState());

  function loadCollapsedState(): Record<string, boolean> {
    const defaults: Record<string, boolean> = {
      connection: false,
      appearance: false,
      performance: false,
      models: false,
      modelRequests: false,
      paths: false,
      autocomplete: false,
      interrogator: false,
      prompt_assistant: false,
      civitai: false,
      about: false,
    };
    try {
      const raw = localStorage.getItem(COLLAPSED_KEY);
      if (!raw) return defaults;
      const saved = JSON.parse(raw);
      return { ...defaults, ...saved };
    } catch {
      return defaults;
    }
  }

  let settingsCollapseSaveTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    const val = JSON.stringify(collapsed);
    if (settingsCollapseSaveTimer) clearTimeout(settingsCollapseSaveTimer);
    settingsCollapseSaveTimer = setTimeout(() => {
      try { localStorage.setItem(COLLAPSED_KEY, val); } catch {}
    }, 300);
  });

  const sections = [
    { key: "appMode", labelKey: "settings.sections.app_mode", keywords: "browser app mode desktop native window web switch ui" },
    { key: "connection", labelKey: "settings.sections.connection", keywords: "server mode url port remote autolaunch" },
    { key: "appearance", labelKey: "settings.sections.appearance", keywords: "theme dark light font scale palette custom create logo background branding import export color" },
    { key: "performance", labelKey: "settings.sections.performance", keywords: "vram mode high low normal keep alive close attention backend sage flash" },
    { key: "quality", labelKey: "settings.sections.quality", keywords: "quality tags auto masterpiece best quality anima illustrious noobai pony nanosaur positive negative prompt" },
    { key: "gpu", labelKey: "settings.sections.gpu", keywords: "gpu vram worker backend multi status utilization temperature power nvidia" },
    { key: "models", labelKey: "settings.sections.models", keywords: "models manage delete move lora checkpoint vae upscaler controlnet" },
    { key: "modelRequests", labelKey: "settings.sections.model_requests", keywords: "model requests approve deny pending download civitai hub" },
    { key: "paths", labelKey: "settings.sections.paths", keywords: "comfyui install venv python cli arguments extra args shared model directory models" },
    { key: "gallery", labelKey: "settings.sections.gallery", keywords: "gallery storage location import images output directory swarmui comfyui external folder manual save mode save directory artist cache clear anima preview upscale pre-upscale before base" },
    { key: "autocomplete", labelKey: "settings.sections.autocomplete", keywords: "tags taglist suggestions results url upload csv json danbooru" },
    { key: "interrogator", labelKey: "settings.sections.interrogator", keywords: "interrogate tags tagger threshold confidence onnx model" },
    { key: "prompt_assistant", labelKey: "settings.sections.prompt_assistant", keywords: "llm prompt enhance compose model gguf ai assistant" },
    { key: "civitai", labelKey: "settings.sections.civitai", keywords: "civitai api key metadata model hub image fetch download authentication" },
    { key: "queue", labelKey: "settings.sections.queue", keywords: "queue position pending running cancel clear jobs users order wait" },
    { key: "about", labelKey: "settings.sections.about", keywords: "version update check updates about troubleshooting logs export diagnostic github report issue" },
  ];

  function sectionVisible(key: string): boolean {
    if (!search.trim()) return true;
    const s = sections.find((sec) => sec.key === key);
    if (!s) return false;
    const q = search.toLowerCase();
    return locale.t(s.labelKey).toLowerCase().includes(q) || s.keywords.includes(q);
  }

  // Track original values for restart-needing settings
  let originalUrl = "";
  let originalPort = 0;
  let originalMode = "";
  let originalVramMode = "";
  let originalAttentionBackend = "";
  let originalExtraArgs = "";
  let originalModelPaths = "";

  async function loadConfig() {
    config = await getConfig();
    if (!Array.isArray(config.theme_profiles)) config.theme_profiles = [];
    config.theme_profile_id ??= null;
    if (
      config.theme_profile_id &&
      !config.theme_profiles.some((profile) => profile.id === config!.theme_profile_id)
    ) {
      config.theme_profile_id = null;
    }
    // Migrate CivitAI API key from ModelHub localStorage if not already in config
    if (!config.civitai_api_key) {
      try {
        const lsKey = localStorage.getItem("mooshieui.civitai.apiKey.v1");
        if (lsKey) {
          config.civitai_api_key = lsKey;
          await updateConfig(config);
        }
      } catch { /* ignore */ }
    }
    snapshotRestartFields();
    try {
      applyTheme(config);
    } catch (e) {
      console.error("Failed to apply theme from config:", e);
    }
  }

  async function refreshAttentionStatus() {
    if (!config) return;
    attentionStatusLoading = true;
    attentionStatusError = null;
    attentionStatusVenvPath = config.venv_path;
    try {
      attentionStatus = await checkAttentionBackend();
    } catch (e: any) {
      attentionStatusError = typeof e === "string" ? e : e.message || locale.t("settings.attention.check_failed");
    } finally {
      attentionStatusLoading = false;
    }
  }

  /** Load the installed ComfyUI version and whether the pinned target is newer. */
  async function refreshComfyuiVersion() {
    try {
      comfyuiVersion = await getComfyuiVersion();
    } catch (e: any) {
      // Non-fatal: leave the card hidden if the version can't be read.
      comfyuiVersion = null;
    }
  }

  /**
   * Update ComfyUI to the version this MooshieUI build was tested against, then
   * restart it. The backend leaves ComfyUI stopped, so we bring it back via
   * startComfyui() (reusing the full websocket wiring).
   */
  async function handleComfyuiUpdate() {
    if (comfyuiUpdating) return;
    comfyuiUpdating = true;
    comfyuiUpdateError = null;
    comfyuiUpdateProgress = locale.t("settings.performance.comfyui_update_starting");
    const unlisten = await ipcListen("setup:progress", (event: any) => {
      const data = event.payload as { message?: string };
      if (data?.message) comfyuiUpdateProgress = data.message;
    });
    try {
      await updateComfyui();
      await startComfyui();
      await refreshComfyuiVersion();
    } catch (e: any) {
      comfyuiUpdateError =
        typeof e === "string" ? e : e.message || locale.t("settings.performance.comfyui_update_failed");
    } finally {
      comfyuiUpdating = false;
      comfyuiUpdateProgress = null;
      unlisten();
    }
  }

  async function initSettings() {
    loading = true;
    settingsLoadError = null;
    try {
      await loadConfig();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      settingsLoadError = message;
      error = `Failed to load config: ${message}`;
      config = null;
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void initSettings().then(() => {
      void refreshAttentionStatus();
    });
    // The in-app ComfyUI updater is desktop-only (hosted/browser ships ComfyUI
    // in the image), so only probe the installed version off-browser.
    if (!isBrowserMode) void refreshComfyuiVersion();
    loadInstallPath();
    getGalleryPath().then(p => { galleryPathDisplay = p; }).catch(() => {});
    void loadCacheCount();
    if (isBrowserMode) {
      loadLanAccounts();
      loadLanInfo();
      void refreshPasswordSecurityStatus();
    }
    startQueuePolling();
  });

  onDestroy(() => {
    stopQueuePolling();
  });

  // Poll account list every 10s to refresh online/offline indicators (admin/mod).
  $effect(() => {
    if (!isBrowserMode || !canManageServer) return;
    const id = setInterval(loadLanAccounts, 10_000);
    return () => clearInterval(id);
  });

  function snapshotRestartFields() {
    if (!config) return;
    originalUrl = config.server_url;
    originalPort = config.server_port;
    originalMode = config.server_mode;
    originalVramMode = config.vram_mode;
    originalAttentionBackend = config.attention_backend;
    originalExtraArgs = config.extra_args.join(" ");
    originalModelPaths = config.extra_model_paths ?? "";
  }

  function checkRestartNeeded() {
    if (!config) return;
    restartNeeded =
      config.server_url !== originalUrl ||
      config.server_port !== originalPort ||
      config.server_mode !== originalMode ||
      config.vram_mode !== originalVramMode ||
      config.attention_backend !== originalAttentionBackend ||
      config.extra_args.join(" ") !== originalExtraArgs ||
      (config.extra_model_paths ?? "") !== originalModelPaths;
  }

  /** Auto-save for sliders, dropdowns, checkboxes — fires immediately on change. */
  async function autoSave() {
    if (!config) return;
    checkRestartNeeded();
    applyTheme(config);
    try {
      await updateConfig(config);
    } catch (e) {
      error = `Failed to save: ${e}`;
    }
  }

  /**
   * Mirror an LLM provider mutation back into the cached config.
   *
   * The provider commands write Rust config directly, so this snapshot goes
   * stale the moment one runs and a later unrelated `autoSave()` would revert
   * provider, base URL and model. (The API key needs no mirroring — Rust's
   * `preserve_secrets` carries a stored key across any full-config save.)
   */
  function onProviderState(next: LlmProviderState) {
    if (!config) return;
    config.llm_provider = next.provider;
    config.llm_external_base_url = next.base_url;
    config.llm_external_model = next.model;
    config.llm_external_enabled = next.enabled;
  }

  /** Install a different attention backend and update config. */
  async function handleAttentionChange(backend: string) {
    if (!config || attentionInstalling) return;
    const previousBackend = config.attention_backend;
    attentionError = null;
    attentionInstalling = true;
    try {
      await installAttentionBackend(backend);
      config.attention_backend = backend;
      checkRestartNeeded();
    } catch (e: any) {
      attentionError = typeof e === "string" ? e : e.message || locale.t("settings.attention.install_failed");
      config.attention_backend = previousBackend;
    } finally {
      attentionInstalling = false;
      void refreshAttentionStatus();
    }
  }

  /** Look up the capability record for a backend value from the current status. */
  function backendSupport(value: string): BackendSupport | null {
    return attentionStatus?.support.find((s) => s.backend === value) ?? null;
  }

  /** Whether a backend option should be disabled (unsupported on this machine). */
  function backendBlocked(value: string): boolean {
    if (value === "default") return false;
    // Until status loads, don't pre-disable — the install preflight is the backstop.
    if (!attentionStatus) return false;
    return backendSupport(value)?.supported === false;
  }

  /** Human-readable display label for a backend value (reuses existing option keys). */
  function backendLabel(value: string): string {
    return locale.t(`settings.performance.attention_${value}`);
  }

  /** Map a BackendSupport reason code to a localized reason string. */
  function backendReason(s: BackendSupport): string {
    switch (s.reason) {
      case "no_nvidia_gpu":
        return locale.t("settings.performance.attention_requires_nvidia");
      case "compute_capability":
        return locale.t("settings.performance.attention_requires_cc", {
          min: s.min_cc != null ? s.min_cc.toFixed(1) : "?",
          detected:
            attentionStatus?.compute_capability != null
              ? attentionStatus.compute_capability.toFixed(1)
              : locale.t("settings.performance.attention_not_detected"),
        });
      case "nvcc_missing":
        return locale.t("settings.performance.attention_requires_nvcc");
      default:
        return "";
    }
  }

  /** Manual save for text inputs — triggered by Save button. */
  async function save() {
    if (!config) return;
    saving = true;
    error = null;
    try {
      await updateConfig(config);
      saved = true;
      snapshotRestartFields();
      checkRestartNeeded();
      void refreshAttentionStatus();
      setTimeout(() => (saved = false), 2000);
    } catch (e) {
      error = `Failed to save: ${e}`;
    } finally {
      saving = false;
    }
  }

  function applyFontScale(scale: number) {
    document.documentElement.style.setProperty("--font-scale", String(scale));
  }

  function normalizeHexColor(input: string): string | null {
    const value = input.trim().replace(/^#/, "");
    if (/^[0-9a-fA-F]{6}$/.test(value)) return `#${value.toLowerCase()}`;
    if (/^[0-9a-fA-F]{3}$/.test(value)) {
      const expanded = value.split("").map((char) => `${char}${char}`).join("");
      return `#${expanded.toLowerCase()}`;
    }
    return null;
  }

  function updateDraftTone(mode: "dark" | "light", key: keyof ThemeTone, value: string) {
    const normalized = normalizeHexColor(value);
    if (!normalized) return;
    if (mode === "dark") {
      draftThemeDark = { ...draftThemeDark, [key]: normalized };
      if (draftToneLinked[key]) draftThemeLight = { ...draftThemeLight, [key]: normalized };
    } else {
      draftThemeLight = { ...draftThemeLight, [key]: normalized };
      if (draftToneLinked[key]) draftThemeDark = { ...draftThemeDark, [key]: normalized };
    }
  }

  function resetThemeCreatorDraft() {
    draftEditingProfileId = null;
    draftThemeName = newThemeName.trim() || locale.t("settings.appearance.custom_theme_default");
    draftThemeDark = { ...DEFAULT_THEME_TONE_DARK };
    draftThemeLight = { ...DEFAULT_THEME_TONE_LIGHT };
    draftThemeBackgroundImage = null;
    draftThemeBackgroundFade = 0.65;
    draftThemeHideBranding = false;
    draftThemeLogoImage = null;
    draftToneLinked = { main: true, sub: true, trim: true, background: true, text: true };
  }

  function openThemeCreatorModal() {
    resetThemeCreatorDraft();
    showThemeCreatorModal = true;
  }

  function openThemeEditorModal() {
    const profile = activeThemeProfile;
    if (!profile) return;
    draftEditingProfileId = profile.id;
    draftThemeName = profile.name;
    draftThemeDark = { ...profile.dark };
    draftThemeLight = { ...profile.light };
    draftThemeBackgroundImage = profile.background_image;
    draftThemeBackgroundFade = profile.background_fade;
    draftThemeHideBranding = profile.hide_branding;
    draftThemeLogoImage = profile.logo_image;
    draftToneLinked = { main: false, sub: false, trim: false, background: false, text: false };
    showThemeCreatorModal = true;
  }

  function createThemeProfileFromDraft() {
    if (!config) return;
    const profile: ThemeProfile = {
      id: draftEditingProfileId ?? `theme_${Date.now()}`,
      name: draftThemeName.trim() || locale.t("settings.appearance.custom_theme_default"),
      palette: "custom",
      dark: { ...draftThemeDark },
      light: { ...draftThemeLight },
      background_image: draftThemeBackgroundImage,
      background_fade: draftThemeBackgroundFade,
      logo_image: draftThemeLogoImage,
      hide_branding: draftThemeHideBranding,
    };

    if (draftEditingProfileId) {
      config.theme_profiles = config.theme_profiles.map((existing) =>
        existing.id === draftEditingProfileId ? profile : existing,
      );
      config.theme_profile_id = draftEditingProfileId;
    } else {
      config.theme_profiles = [...config.theme_profiles, profile];
      config.theme_profile_id = profile.id;
    }

    newThemeName = profile.name;
    showThemeCreatorModal = false;
    draftEditingProfileId = null;
    applyTheme(config);
    void autoSave();
  }

  function openLogoCropper(dataUrl: string, target: "draft" | "active") {
    pendingLogoDataUrl = dataUrl;
    logoCropZoom = 1;
    logoCropPanX = 0;
    logoCropPanY = 0;
    logoCropTarget = target;
    showLogoCropModal = true;
  }

  async function cropSquareLogo(dataUrl: string, zoom: number, panX: number, panY: number): Promise<string> {
    return new Promise((resolve, reject) => {
      const image = new Image();
      image.onload = () => {
        const sourceWidth = image.naturalWidth;
        const sourceHeight = image.naturalHeight;
        const minSide = Math.min(sourceWidth, sourceHeight);
        const safeZoom = Math.max(1, Math.min(3, zoom));
        const cropSize = minSide / safeZoom;
        const maxX = Math.max(0, (sourceWidth - cropSize) / 2);
        const maxY = Math.max(0, (sourceHeight - cropSize) / 2);
        const srcX = (sourceWidth - cropSize) / 2 + panX * maxX;
        const srcY = (sourceHeight - cropSize) / 2 + panY * maxY;

        const canvas = document.createElement("canvas");
        canvas.width = 512;
        canvas.height = 512;
        const context = canvas.getContext("2d");
        if (!context) {
          reject(new Error("Failed to create crop canvas."));
          return;
        }
        context.imageSmoothingEnabled = true;
        context.imageSmoothingQuality = "high";
        context.drawImage(image, srcX, srcY, cropSize, cropSize, 0, 0, 512, 512);
        resolve(canvas.toDataURL("image/png"));
      };
      image.onerror = () => reject(new Error("Failed to decode selected image."));
      image.src = dataUrl;
    });
  }

  async function confirmLogoCrop() {
    if (!pendingLogoDataUrl) return;
    try {
      const cropped = await cropSquareLogo(pendingLogoDataUrl, logoCropZoom, logoCropPanX, logoCropPanY);
      if (logoCropTarget === "draft") {
        draftThemeLogoImage = cropped;
      } else {
        const profile = activeThemeProfile;
        if (!profile) return;
        profile.logo_image = cropped;
        void autoSave();
      }
      showLogoCropModal = false;
      pendingLogoDataUrl = null;
    } catch (err) {
      themeImportError = String((err as Error)?.message ?? err);
    }
  }

  function addThemeProfile() {
    openThemeCreatorModal();
  }

  function removeActiveThemeProfile() {
    if (!config || !config.theme_profile_id) return;
    config.theme_profiles = config.theme_profiles.filter((profile) => profile.id !== config!.theme_profile_id);
    config.theme_profile_id = null;
    void autoSave();
  }

  async function fileToDataUrl(file: File): Promise<string> {
    return await new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result ?? ""));
      reader.onerror = () => reject(reader.error ?? new Error("Failed to read file"));
      reader.readAsDataURL(file);
    });
  }

  async function setProfileImage(kind: "background" | "logo", event: Event) {
    const profile = activeThemeProfile;
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!profile || !file) return;
    const dataUrl = await fileToDataUrl(file);
    if (kind === "background") {
      profile.background_image = dataUrl;
      void autoSave();
    } else {
      openLogoCropper(dataUrl, "active");
    }
    input.value = "";
  }

  async function setDraftThemeImage(kind: "background" | "logo", event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    const dataUrl = await fileToDataUrl(file);
    if (kind === "background") {
      draftThemeBackgroundImage = dataUrl;
    } else {
      openLogoCropper(dataUrl, "draft");
    }
    input.value = "";
  }

  function makeUniqueThemeProfileId(baseId: string, usedIds: Set<string>): string {
    const base = baseId.trim() || `theme_${Date.now()}`;
    if (!usedIds.has(base)) {
      usedIds.add(base);
      return base;
    }

    let suffix = 1;
    let candidate = `${base}_${suffix}`;
    while (usedIds.has(candidate)) {
      suffix += 1;
      candidate = `${base}_${suffix}`;
    }
    usedIds.add(candidate);
    return candidate;
  }

  async function importThemeProfiles(event: Event) {
    if (!config) return;
    themeImportError = null;
    themeImportDone = false;
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    try {
      const parsed = JSON.parse(await file.text()) as { themes?: unknown[] };
      if (!Array.isArray(parsed.themes)) throw new Error(locale.t("settings.appearance.theme_import_invalid"));
      if (parsed.themes.length === 0) throw new Error(locale.t("settings.appearance.theme_import_empty"));
      const existingProfiles = Array.isArray(config.theme_profiles) ? config.theme_profiles : [];
      const usedIds = new Set(existingProfiles.map((profile) => profile.id));
      const imported = parsed.themes.map((theme, index) => {
        const source = (typeof theme === "object" && theme) ? (theme as Partial<ThemeProfile>) : {};
        const dark: Partial<ThemeTone> = source.dark ?? {};
        const light: Partial<ThemeTone> = source.light ?? {};
        return {
          id: makeUniqueThemeProfileId(
            typeof source.id === "string" && source.id.trim() ? source.id : `theme_${Date.now()}_${index}`,
            usedIds,
          ),
          name: typeof source.name === "string" && source.name.trim() ? source.name.trim() : locale.t("settings.appearance.imported_theme", { index: String(index + 1) }),
          palette:
            source.palette === "mooshie" ||
            source.palette === "nord" ||
            source.palette === "solarized" ||
            source.palette === "gruvbox" ||
            source.palette === "catppuccin" ||
            source.palette === "custom"
              ? source.palette
              : "custom",
          dark: {
            main: typeof dark.main === "string" ? dark.main : DEFAULT_THEME_TONE_DARK.main,
            sub: typeof dark.sub === "string" ? dark.sub : DEFAULT_THEME_TONE_DARK.sub,
            trim: typeof dark.trim === "string" ? dark.trim : DEFAULT_THEME_TONE_DARK.trim,
            background: typeof dark.background === "string" ? dark.background : DEFAULT_THEME_TONE_DARK.background,
            text: typeof dark.text === "string" ? dark.text : DEFAULT_THEME_TONE_DARK.text,
          },
          light: {
            main: typeof light.main === "string" ? light.main : DEFAULT_THEME_TONE_LIGHT.main,
            sub: typeof light.sub === "string" ? light.sub : DEFAULT_THEME_TONE_LIGHT.sub,
            trim: typeof light.trim === "string" ? light.trim : DEFAULT_THEME_TONE_LIGHT.trim,
            background: typeof light.background === "string" ? light.background : DEFAULT_THEME_TONE_LIGHT.background,
            text: typeof light.text === "string" ? light.text : DEFAULT_THEME_TONE_LIGHT.text,
          },
          background_image: typeof source.background_image === "string" ? source.background_image : null,
          background_fade:
            typeof source.background_fade === "number" && Number.isFinite(source.background_fade)
              ? Math.max(0, Math.min(1, source.background_fade))
              : 0.65,
          logo_image: typeof source.logo_image === "string" ? source.logo_image : null,
          hide_branding: Boolean(source.hide_branding),
        } satisfies ThemeProfile;
      });
      config.theme_profiles = [...existingProfiles, ...imported];
      config.theme_profile_id = imported[0]?.id ?? null;
      await autoSave();
      themeImportDone = true;
      setTimeout(() => (themeImportDone = false), 2500);
    } catch (error: any) {
      themeImportError = String(error?.message ?? error);
    } finally {
      input.value = "";
    }
  }

  function exportThemeProfiles() {
    if (!config) return;
    themeExportDone = false;
    themeExportError = null;
    try {
      const payload = JSON.stringify({ themes: config.theme_profiles }, null, 2);
      const blob = new Blob([payload], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "mooshie-themes.json";
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(url);
      themeExportDone = true;
      setTimeout(() => (themeExportDone = false), 2500);
    } catch (error: any) {
      themeExportError = String(error?.message ?? error);
    }
  }

  async function restartServer() {
    // Save first so restart picks up latest config
    if (config) {
      try { await updateConfig(config); } catch {}
    }
    restarting = true;
    error = null;
    try {
      connection.connected = false;
      await stopComfyui();
      await startComfyui();
      snapshotRestartFields();
      restartNeeded = false;
    } catch (e) {
      error = `Failed to restart: ${e}`;
    } finally {
      restarting = false;
    }
  }
</script>

<div class="h-full flex flex-col overflow-hidden">
  <!-- Persistent top bar -->
  {#if config}
    <div class="shrink-0 px-6 py-3 bg-neutral-900 border-b border-neutral-800 flex items-center gap-3">
      <h1 class="text-lg font-medium text-neutral-100 shrink-0">{locale.t('settings.title')}</h1>

      <input
        type="text"
        bind:value={search}
        placeholder={locale.t('settings.search_placeholder')}
        class="flex-1 min-w-0 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-1.5 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
      />

      <div class="ml-auto flex items-center gap-3 shrink-0">
      {#if canManageServer}
      {#if restartNeeded}
        <div class="flex items-center gap-1.5 text-amber-200 text-xs mr-2">
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          {locale.t('settings.restart_needed')}
        </div>
      {/if}

      <button
        class="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
        onclick={save}
        disabled={saving}
      >
        {#if saving}
          {locale.t('settings.saving')}
        {:else if saved}
          {locale.t('settings.saved')}
        {:else}
          {locale.t('settings.save')}
        {/if}
      </button>

      <button
        class="px-3 py-1.5 rounded-lg text-sm transition-colors disabled:opacity-50 {restartNeeded
          ? 'bg-red-700 hover:bg-red-600 text-white animate-pulse'
          : 'bg-neutral-700 hover:bg-neutral-600 text-neutral-100'}"
        onclick={restartServer}
        disabled={restarting}
      >
        {#if restarting}
          {locale.t('settings.restarting')}
        {:else}
          {locale.t('settings.restart_comfyui')}
        {/if}
      </button>
      {/if}
      </div>
    </div>
  {/if}

  <!-- Scrollable content -->
  <div
    class="flex-1 overflow-y-auto {mobileFriendly ? 'p-4' : 'p-6'}"
    bind:this={settingsScrollEl}
    onscroll={onSettingsScroll}
  >
    <div class="columns-1 {mobileFriendly ? '' : 'lg:columns-2 xl:columns-3'} gap-4">
      {#if loading}
        <div class="flex flex-col items-center justify-center py-12 text-neutral-500 gap-3">
          <div class="w-6 h-6 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin"></div>
          <p class="text-xs text-neutral-500">{locale.t("common.loading")}</p>
        </div>
      {:else if settingsLoadError}
        <div class="rounded-xl border border-red-800/50 bg-red-950/30 p-5 space-y-3 break-inside-avoid">
          <p class="text-sm text-red-200">{locale.t("settings.load_failed")}</p>
          <p class="text-xs text-red-300/90">{settingsLoadError}</p>
          <button
            type="button"
            class="px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium cursor-pointer"
            onclick={() => void initSettings()}
          >{locale.t("common.retry")}</button>
        </div>
      {:else if config}
        <!-- Browser / App Mode Switch (admin only; hidden on mobile — users are already in browser mode) -->
        {#if isAdmin && sectionVisible("appMode") && !mobileFriendly}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <div class="p-5 space-y-3">
            <div class="flex items-center justify-between">
              <div>
                <h3 class="text-sm font-medium text-neutral-200">
                  {config.browser_mode ? locale.t('settings.app_mode.browser_title') : locale.t('settings.app_mode.app_title')}
                </h3>
                <p class="text-xs text-neutral-500 mt-0.5">
                  {config.browser_mode
                    ? locale.t('settings.app_mode.browser_desc')
                    : locale.t('settings.app_mode.app_desc')}
                </p>
              </div>
              <button
                class="px-4 py-2 text-sm font-medium rounded-lg transition-colors {config.browser_mode
                  ? 'bg-indigo-600 hover:bg-indigo-500 text-white'
                  : 'bg-neutral-700 hover:bg-neutral-600 text-neutral-200'}"
                onclick={switchUiMode}
              >
                {config.browser_mode ? locale.t('settings.app_mode.switch_to_app') : locale.t('settings.app_mode.switch_to_browser')}
              </button>
            </div>
            {#if switchingMode}
              <p class="text-xs text-amber-400">
                {#if modeSwitchResult === "browser"}
                  {locale.t('settings.app_mode.switched_to_browser')}
                {:else if modeSwitchResult === "app"}
                  {locale.t('settings.app_mode.switched_to_app')}
                {:else}
                  {locale.t('settings.app_mode.switching_to_browser')}
                {/if}
              </p>
            {/if}
            {#if config.browser_mode}
              <div class="flex items-center justify-between pt-2 border-t border-neutral-800">
                <div>
                  <label class="text-xs text-neutral-300 font-medium">{locale.t('settings.lan.enable')}</label>
                  <p class="text-xs text-neutral-500 mt-0.5">
                    {locale.t('settings.lan.enable_desc')}
                  </p>
                </div>
                <label class="relative inline-flex items-center cursor-pointer">
                  <input
                    type="checkbox"
                    bind:checked={config.lan_enabled}
                    onchange={() => { checkRestartNeeded(); autoSave(); }}
                    class="sr-only peer"
                  />
                  <div class="w-9 h-5 bg-neutral-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-0.5 after:left-0.5 after:bg-white after:border-neutral-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-indigo-600"></div>
                </label>
              </div>
              {#if config.lan_enabled}
                <div class="space-y-3 pt-2 border-t border-neutral-800">
                  <p class="text-xs text-amber-400">
                    {locale.t('settings.lan.warning')}
                  </p>

                  <!-- LAN address -->
                  {#if lanAddresses.length > 0}
                    <div class="bg-neutral-800 rounded-lg px-3 py-2">
                      <p class="text-xs text-neutral-400 mb-1">{locale.t('settings.lan.access_at')}</p>
                      {#each lanAddresses as addr}
                        <p class="text-sm text-indigo-400 font-mono select-all">{addr}</p>
                      {/each}
                    </div>
                  {/if}

                  <!-- Existing accounts -->
                  {#if lanAccounts.length > 0}
                    <div class="space-y-2">
                      <div class="flex items-center justify-between">
                        <p class="text-xs text-neutral-400 font-medium">{locale.t('settings.lan.accounts')}</p>
                        <p class="text-[10px] text-neutral-500">{locale.t('settings.lan.accounts_count', { shown: sortedAccounts.length, total: lanAccounts.length })}</p>
                      </div>

                      <!-- Search -->
                      <input
                        type="text"
                        placeholder={locale.t('settings.lan.search_accounts')}
                        bind:value={accountSearch}
                        class="w-full px-3 py-1.5 rounded-lg bg-neutral-800 border border-neutral-700 text-xs text-neutral-200 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
                      />

                      <!-- Sort buttons -->
                      <div class="flex gap-1">
                        {#each [["name", "settings.lan.sort_name"], ["joined", "settings.lan.sort_joined"], ["last_online", "settings.lan.sort_last_online"]] as [key, labelKey]}
                          <button
                            class="text-[10px] px-2 py-1 rounded cursor-pointer transition-colors {accountSort === key ? 'bg-indigo-600/30 text-indigo-300' : 'bg-neutral-800 text-neutral-400 hover:text-neutral-300'}"
                            onclick={() => { if (accountSort === key) { accountSortAsc = !accountSortAsc; } else { accountSort = key as typeof accountSort; accountSortAsc = true; } }}
                          >{locale.t(labelKey)} {accountSort === key ? (accountSortAsc ? '↑' : '↓') : ''}</button>
                        {/each}
                      </div>

                      <!-- Scrollable account list (max 6 visible) -->
                      <div class="max-h-72 overflow-y-auto space-y-1 pr-1">
                        {#each sortedAccounts as account}
                          <div class="flex items-center justify-between bg-neutral-800 rounded-lg px-3 py-2">
                            <div class="flex items-center gap-2 min-w-0">
                              <span class="inline-block w-2 h-2 rounded-full shrink-0 {account.online ? 'bg-green-500' : 'bg-neutral-600'}"></span>
                              <span class="text-sm text-neutral-200 truncate" title={account.username}>{account.username}</span>
                              {#if account.role === "moderator"}
                                <span class="text-[10px] px-1.5 py-0.5 rounded bg-indigo-600/30 text-indigo-300 font-medium shrink-0">{locale.t('common.role_mod')}</span>
                              {/if}
                              <span class="text-[10px] text-neutral-500 shrink-0">{locale.formatBytes(account.storage_limit_bytes)}</span>
                              <span class="text-[10px] text-neutral-500 shrink-0" title={account.created_at ? locale.t('settings.lan.joined_title', { date: new Date(account.created_at).toLocaleDateString() }) : ''}>
                                {account.created_at ? relativeTime(account.created_at) : ''}
                              </span>
                              {#if !account.online && account.last_online}
                                <span class="text-[10px] text-neutral-600 shrink-0" title={locale.t('settings.lan.last_online_title', { date: locale.formatDateTime(account.last_online) })}>
                                  · {relativeTime(account.last_online)}
                                </span>
                              {/if}
                            </div>
                            <button
                              class="shrink-0 ml-2 p-1 rounded hover:bg-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors cursor-pointer"
                              title={locale.t('settings.lan.manage_user', { user: account.username })}
                              onclick={() => { actionsTargetAccount = account; showAccountActionsModal = true; }}
                            >
                              <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M11.49 3.17c-.38-1.56-2.6-1.56-2.98 0a1.532 1.532 0 01-2.286.948c-1.372-.836-2.942.734-2.106 2.106.54.886.061 2.042-.947 2.287-1.561.379-1.561 2.6 0 2.978a1.532 1.532 0 01.947 2.287c-.836 1.372.734 2.942 2.106 2.106a1.532 1.532 0 012.287.947c.379 1.561 2.6 1.561 2.978 0a1.533 1.533 0 012.287-.947c1.372.836 2.942-.734 2.106-2.106a1.533 1.533 0 01.947-2.287c1.561-.379 1.561-2.6 0-2.978a1.532 1.532 0 01-.947-2.287c.836-1.372-.734-2.942-2.106-2.106a1.532 1.532 0 01-2.287-.947zM10 13a3 3 0 100-6 3 3 0 000 6z" clip-rule="evenodd"/></svg>
                            </button>
                          </div>
                        {/each}
                      </div>
                    </div>
                  {:else}
                    <p class="text-xs text-neutral-500">{locale.t('settings.lan.no_accounts')}</p>
                  {/if}

                  <!-- Add account button -->
                  <button
                    class="w-full px-3 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {lanAuthBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
                    disabled={lanAuthBusy}
                    onclick={() => { lanNewUser = ''; lanNewPass = ''; lanAuthError = null; showAddAccountModal = true; }}
                  >{locale.t('settings.lan.add_account')}</button>
                </div>
              {/if}
            {/if}
          </div>
        </section>
        {/if}

        <!-- Account Management (moderator in browser mode — admins see this inside the LAN section above) -->
        {#if canManageServer && !isAdmin && isBrowserMode}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <div class="p-5 space-y-3">
            <h3 class="text-sm font-medium text-neutral-200">{locale.t('settings.lan.account_management')}</h3>
            <p class="text-xs text-neutral-500">{locale.t('settings.lan.account_management_desc')}</p>

            {#if lanAccounts.length > 0}
              <div class="space-y-2">
                <div class="flex items-center justify-between">
                  <p class="text-xs text-neutral-400 font-medium">{locale.t('settings.lan.accounts')}</p>
                  <p class="text-[10px] text-neutral-500">{locale.t('settings.lan.accounts_count', { shown: sortedAccounts.length, total: lanAccounts.length })}</p>
                </div>

                <input
                  type="text"
                  placeholder={locale.t('settings.lan.search_accounts')}
                  bind:value={accountSearch}
                  class="w-full px-3 py-1.5 rounded-lg bg-neutral-800 border border-neutral-700 text-xs text-neutral-200 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
                />

                <div class="flex gap-1">
                  {#each [["name", "settings.lan.sort_name"], ["joined", "settings.lan.sort_joined"], ["last_online", "settings.lan.sort_last_online"]] as [key, labelKey]}
                    <button
                      class="text-[10px] px-2 py-1 rounded cursor-pointer transition-colors {accountSort === key ? 'bg-indigo-600/30 text-indigo-300' : 'bg-neutral-800 text-neutral-400 hover:text-neutral-300'}"
                      onclick={() => { if (accountSort === key) { accountSortAsc = !accountSortAsc; } else { accountSort = key as typeof accountSort; accountSortAsc = true; } }}
                    >{locale.t(labelKey)} {accountSort === key ? (accountSortAsc ? '↑' : '↓') : ''}</button>
                  {/each}
                </div>

                <div class="max-h-72 overflow-y-auto space-y-1 pr-1">
                  {#each sortedAccounts as account}
                    <div class="flex items-center justify-between bg-neutral-800 rounded-lg px-3 py-2">
                      <div class="flex items-center gap-2 min-w-0">
                        <span class="inline-block w-2 h-2 rounded-full shrink-0 {account.online ? 'bg-green-500' : 'bg-neutral-600'}"></span>
                        <span class="text-sm text-neutral-200 truncate" title={account.username}>{account.username}</span>
                        {#if account.role === "moderator"}
                          <span class="text-[10px] px-1.5 py-0.5 rounded bg-indigo-600/30 text-indigo-300 font-medium shrink-0">{locale.t('common.role_mod')}</span>
                        {/if}
                        {#if account.role === "admin"}
                          <span class="text-[10px] px-1.5 py-0.5 rounded bg-amber-600/30 text-amber-300 font-medium shrink-0">{locale.t('common.role_admin')}</span>
                        {/if}
                        {#if account.role === "user"}
                          <span class="text-[10px] text-neutral-500 shrink-0">{locale.formatBytes(account.storage_limit_bytes)}</span>
                        {/if}
                        <span class="text-[10px] text-neutral-500 shrink-0" title={account.created_at ? locale.t('settings.lan.joined_title', { date: new Date(account.created_at).toLocaleDateString() }) : ''}>
                          {account.created_at ? relativeTime(account.created_at) : ''}
                        </span>
                        {#if !account.online && account.last_online}
                          <span class="text-[10px] text-neutral-600 shrink-0" title={locale.t('settings.lan.last_online_title', { date: locale.formatDateTime(account.last_online) })}>
                            · {relativeTime(account.last_online)}
                          </span>
                        {/if}
                      </div>
                      {#if account.role === "user"}
                        <button
                          class="shrink-0 ml-2 p-1 rounded hover:bg-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors cursor-pointer"
                          title={locale.t('settings.lan.manage_user', { user: account.username })}
                          onclick={() => { actionsTargetAccount = account; showAccountActionsModal = true; }}
                        >
                          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M11.49 3.17c-.38-1.56-2.6-1.56-2.98 0a1.532 1.532 0 01-2.286.948c-1.372-.836-2.942.734-2.106 2.106.54.886.061 2.042-.947 2.287-1.561.379-1.561 2.6 0 2.978a1.532 1.532 0 01.947 2.287c-.836 1.372.734 2.942 2.106 2.106a1.532 1.532 0 012.287.947c.379 1.561 2.6 1.561 2.978 0a1.533 1.533 0 012.287-.947c1.372.836 2.942-.734 2.106-2.106a1.533 1.533 0 01.947-2.287c1.561-.379 1.561-2.6 0-2.978a1.532 1.532 0 01-.947-2.287c.836-1.372-.734-2.942-2.106-2.106a1.532 1.532 0 01-2.287-.947zM10 13a3 3 0 100-6 3 3 0 000 6z" clip-rule="evenodd"/></svg>
                        </button>
                      {/if}
                    </div>
                  {/each}
                </div>
              </div>
            {:else}
              <p class="text-xs text-neutral-500">{locale.t('settings.lan.no_accounts_found')}</p>
            {/if}

            <!-- Add account button (moderators can create accounts too) -->
            <button
              class="w-full px-3 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {lanAuthBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
              disabled={lanAuthBusy}
              onclick={() => { lanNewUser = ''; lanNewPass = ''; lanAuthError = null; showAddAccountModal = true; }}
            >{locale.t('settings.lan.add_account')}</button>

            {#if lanAuthError}
              <p class="text-xs text-red-400 mt-1">{lanAuthError}</p>
            {/if}
          </div>
        </section>
        {/if}

        <!-- Queue (all users — always shown when section visible) -->
        {#if sectionVisible("queue")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.queue = !collapsed.queue)}
          >
            {locale.t('settings.queue.title')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.queue ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>
          {#if !collapsed.queue}
          <div class="px-5 pb-5 space-y-3">
            {#if queueData === null}
              <p class="text-xs text-neutral-500">{locale.t('settings.queue.loading')}</p>
            {:else}
              {@const rIds = runningIds(queueData)}
              {@const positions = queueData.queue_positions ?? []}
              {#if positions.length === 0}
                <p class="text-xs text-neutral-500">{locale.t('settings.queue.empty')}</p>
              {:else}
                <div class="space-y-1.5">
                  {#each positions as entry (entry.prompt_id)}
                    {@const isRunning = rIds.has(entry.prompt_id)}
                    <div class="flex items-center gap-2 rounded-lg px-3 py-2 text-xs
                      {isRunning ? 'bg-indigo-900/40 border border-indigo-700/50' : 'bg-neutral-800'}">
                      <span class="w-5 text-center font-bold shrink-0
                        {isRunning ? 'text-indigo-300' : 'text-neutral-400'}">
                        {isRunning ? '▶' : `#${entry.position + 1}`}
                      </span>
                      <span class="flex-1 font-mono text-neutral-300 truncate" title={entry.prompt_id}>
                        {entry.prompt_id.slice(0, 20)}{entry.prompt_id.length > 20 ? '…' : ''}
                      </span>
                      {#if canManageServer}
                        <span class="shrink-0 text-neutral-400 italic">
                          {entry.username ?? 'admin'}
                        </span>
                      {/if}
                      <span class="shrink-0 rounded-full px-1.5 py-0.5 text-[10px] font-medium
                        {isRunning ? 'bg-indigo-600 text-white' : 'bg-neutral-700 text-neutral-400'}">
                        {isRunning ? locale.t('settings.queue.status_running') : locale.t('settings.queue.status_pending')}
                      </span>
                    </div>
                  {/each}
                </div>
              {/if}
              <p class="text-[10px] text-neutral-600">{locale.t('settings.queue.auto_refresh')}</p>

              {#if canManageServer}
                <div class="pt-2 border-t border-neutral-800 space-y-2">
                  {#if clearQueueError}
                    <p class="text-xs text-red-400">{clearQueueError}</p>
                  {/if}
                  {#if clearQueueDone}
                    <p class="text-xs text-green-400">{locale.t('settings.queue.cleared')}</p>
                  {/if}
                  {#if showClearQueueConfirm}
                    <p class="text-xs text-amber-300">{locale.t('settings.queue.clear_confirm')}</p>
                    <div class="flex gap-2">
                      <button
                        class="flex-1 py-2 rounded-lg text-xs font-medium bg-neutral-700 hover:bg-neutral-600 text-neutral-300 transition-colors cursor-pointer"
                        onclick={() => (showClearQueueConfirm = false)}
                      >{locale.t('common.cancel')}</button>
                      <button
                        class="flex-1 py-2 rounded-lg text-xs font-medium bg-red-600 hover:bg-red-500 text-white transition-colors cursor-pointer disabled:opacity-50"
                        disabled={clearQueueBusy}
                        onclick={handleClearQueue}
                      >{clearQueueBusy ? locale.t('settings.queue.clearing') : locale.t('settings.queue.clear_confirm_yes')}</button>
                    </div>
                  {:else}
                    <button
                      class="w-full py-2 rounded-lg text-xs font-medium bg-red-600/20 hover:bg-red-600/40 text-red-300 border border-red-800/50 transition-colors cursor-pointer"
                      onclick={() => { clearQueueError = null; showClearQueueConfirm = true; }}
                    >{locale.t('settings.queue.clear_button')}</button>
                  {/if}
                </div>
              {/if}
            {/if}
          </div>
          {/if}
        </section>
        {/if}

        <!-- Queue Management (admin / moderator in browser mode — legacy clear button kept for non-queue-section visibility) -->
        {#if canManageServer && isBrowserMode && !sectionVisible("queue")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <div class="p-5 space-y-3">
            <h3 class="text-sm font-medium text-neutral-200">{locale.t('settings.queue.management_title')}</h3>
            <p class="text-xs text-neutral-500">{locale.t('settings.queue.management_desc')}</p>
            {#if clearQueueError}
              <p class="text-xs text-red-400">{clearQueueError}</p>
            {/if}
            {#if clearQueueDone}
              <p class="text-xs text-green-400">{locale.t('settings.queue.cleared')}</p>
            {/if}
            {#if showClearQueueConfirm}
              <p class="text-xs text-amber-300">{locale.t('settings.queue.clear_confirm')}</p>
              <div class="flex gap-2">
                <button
                  class="flex-1 py-2 rounded-lg text-xs font-medium bg-neutral-700 hover:bg-neutral-600 text-neutral-300 transition-colors cursor-pointer"
                  onclick={() => (showClearQueueConfirm = false)}
                >{locale.t('common.cancel')}</button>
                <button
                  class="flex-1 py-2 rounded-lg text-xs font-medium bg-red-600 hover:bg-red-500 text-white transition-colors cursor-pointer disabled:opacity-50"
                  disabled={clearQueueBusy}
                  onclick={handleClearQueue}
                >{clearQueueBusy ? locale.t('settings.queue.clearing') : locale.t('settings.queue.clear_confirm_yes')}</button>
              </div>
            {:else}
              <button
                class="w-full py-2 rounded-lg text-xs font-medium bg-red-600/20 hover:bg-red-600/40 text-red-300 border border-red-800/50 transition-colors cursor-pointer"
                onclick={() => { clearQueueError = null; showClearQueueConfirm = true; }}
              >{locale.t('settings.queue.clear_button')}</button>
            {/if}
          </div>
        </section>
        {/if}

        <!-- Connection (admin / moderator) -->
        {#if isAdmin && sectionVisible("connection")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.connection = !collapsed.connection)}
          >
            {locale.t('settings.connection.title')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.connection ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.connection}
          <div class="px-5 pb-5 space-y-4">
          <div>
            <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.connection.server_mode')}<span class="text-amber-400">*</span></label>
            <select
              bind:value={config.server_mode}
              onchange={() => { autoSave(); }}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
            >
              <option value="autolaunch">{locale.t('settings.connection.mode_autolaunch')}</option>
              <option value="remote">{locale.t('settings.connection.mode_remote')}</option>
            </select>
          </div>

          {#if config.server_mode === "remote"}
          <p class="text-xs text-neutral-500">{locale.t('settings.connection.mode_remote_desc')}</p>
          {/if}

          {#if config.server_mode === "autolaunch"}
          <div class="flex items-center justify-between">
            <div>
              <p class="text-sm text-neutral-200">{locale.t('settings.connection.auto_start')}</p>
              <p class="text-xs text-neutral-500">{locale.t('settings.connection.auto_start_desc')}</p>
            </div>
            <button
              class="w-10 h-5 rounded-full transition-colors cursor-pointer {config.auto_start !== false ? 'bg-indigo-600' : 'bg-neutral-700'}"
              onclick={() => { config!.auto_start = config!.auto_start === false; autoSave(); }}
              role="switch"
              aria-checked={config.auto_start !== false}
            >
              <div class="w-4 h-4 rounded-full bg-white shadow transition-transform {config.auto_start !== false ? 'translate-x-5' : 'translate-x-0.5'}"></div>
            </button>
          </div>
          {/if}

          <div class="grid grid-cols-3 gap-3">
            <div class="col-span-2">
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.connection.server_url')}<span class="text-amber-400">*</span></label>
              <input
                type="text"
                bind:value={config.server_url}
                oninput={checkRestartNeeded}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                placeholder={locale.t('settings.connection.server_url_placeholder')}
              />
              {#if config.server_mode === "remote"}
              <p class="text-xs text-neutral-500 mt-1">{locale.t('settings.connection.remote_url_hint')}</p>
              {/if}
            </div>
            <div>
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.connection.port')}<span class="text-amber-400">*</span></label>
              <input
                type="number"
                bind:value={config.server_port}
                oninput={checkRestartNeeded}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
                min="1"
                max="65535"
              />
            </div>
            <div>
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.connection.network_proxy')}</label>
              <input
                type="text"
                bind:value={config.network_proxy}
                oninput={checkRestartNeeded}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                placeholder={locale.t('settings.connection.proxy_placeholder')}
              />
              <p class="text-xs text-neutral-500 mt-1">{locale.t('settings.connection.network_proxy_desc')}</p>
            </div>
            <div>
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.connection.pip_index_url')}</label>
              <input
                type="text"
                bind:value={config.pip_index_url}
                oninput={checkRestartNeeded}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                placeholder={locale.t('settings.connection.pip_index_placeholder')}
              />
              <p class="text-xs text-neutral-500 mt-1">{locale.t('settings.connection.pip_index_url_desc')}</p>
            </div>
            <div class="col-span-3">
              <label class="block text-xs text-neutral-400 mb-1">{locale.t("settings.connection.output_filename_template")}</label>
              <input
                type="text"
                bind:value={config.output_filename_template}
                oninput={checkRestartNeeded}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                placeholder={`{prompt_id}__{mode}__{model}__{seed}`}
              />
              <p class="text-xs text-neutral-500 mt-1">
                {locale.t("settings.connection.output_filename_keys", {
                  prompt_id: "{prompt_id}",
                  mode: "{mode}",
                  index: "{index}",
                  date: "{date}",
                  time: "{time}",
                  model: "{model}",
                  seed: "{seed}",
                })}
              </p>
            </div>
            <div class="col-span-3">
              <label class="block text-xs text-neutral-400 mb-1">{locale.t("settings.connection.webhook_url")}</label>
              <input
                type="text"
                bind:value={config.webhook_url}
                oninput={checkRestartNeeded}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                placeholder={locale.t("settings.connection.webhook_url_placeholder")}
              />
              <div class="mt-2 flex flex-wrap gap-4 text-xs text-neutral-400">
                <label class="inline-flex items-center gap-2">
                  <input type="checkbox" bind:checked={config.webhook_include_sensitive} class="accent-indigo-500" />
                  {locale.t("settings.connection.webhook_include_sensitive")}
                </label>
                <label class="inline-flex items-center gap-2">
                  <input type="checkbox" bind:checked={config.webhook_allow_private_targets} class="accent-indigo-500" />
                  {locale.t("settings.connection.webhook_allow_private")}
                </label>
              </div>
              <p class="text-xs text-neutral-500 mt-1">{locale.t("settings.connection.webhook_events")}</p>
            </div>
          </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Appearance -->
        {#if sectionVisible("appearance")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.appearance = !collapsed.appearance)}
          >
            {locale.t('settings.appearance.title')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.appearance ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.appearance}
          <div class="px-5 pb-5 space-y-4">
          {#if deviceSupportsMobileLayout}
            <div class="rounded-lg border border-neutral-700 bg-neutral-950/60 p-4">
              <div class="flex items-center justify-between gap-3">
                <div>
                  <p class="text-xs font-medium text-neutral-200">{locale.t('settings.appearance.layout_title')}</p>
                  <p class="text-[10px] text-neutral-500 mt-1">{locale.t('settings.appearance.layout_desc')}</p>
                </div>
                <button
                  type="button"
                  class="shrink-0 px-3 py-2 text-xs font-medium rounded-lg bg-neutral-800 border border-neutral-700 text-neutral-100 hover:bg-neutral-700 transition-colors cursor-pointer"
                  onclick={switchLayout}
                >
                  {useMobileLayout ? locale.t('settings.appearance.layout_use_desktop') : locale.t('settings.appearance.layout_use_mobile')}
                </button>
              </div>
            </div>
          {/if}
          <p class="text-xs font-medium text-neutral-300">{locale.t("settings.appearance.builtin_theme")}</p>
          <p class="text-[10px] text-neutral-500 -mt-2">{locale.t("settings.appearance.builtin_palette_hint")}</p>
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label for="theme-mode" class="block text-xs text-neutral-400 mb-1">{locale.t('settings.appearance.theme')}</label>
              <select
                id="theme-mode"
                name="theme-mode"
                bind:value={config.theme}
                onchange={() => { if (config) { applyTheme(config); autoSave(); } }}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
              >
                <option value="dark">{locale.t('settings.appearance.theme_dark')}</option>
                <option value="light">{locale.t('settings.appearance.theme_light')}</option>
              </select>
            </div>

            <div>
              <label for="theme-palette" class="block text-xs text-neutral-400 mb-1">{locale.t('settings.appearance.palette')}</label>
              <select
                id="theme-palette"
                name="theme-palette"
                bind:value={config.theme_palette}
                onchange={() => { if (config) { config.theme_profile_id = null; applyTheme(config); autoSave(); } }}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
              >
                {#each THEME_PALETTES as palette}
                  <option value={palette.value}>{palette.label}</option>
                {/each}
              </select>
            </div>

            <div>
              <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
                {locale.t('settings.appearance.font_scale')}
                <span class="text-neutral-300">{Math.round(config.font_scale * 100)}%</span>
              </label>
              <input
                type="range"
                bind:value={config.font_scale}
                onchange={() => { autoSave(); }}
                oninput={() => { if (config) applyFontScale(config.font_scale); }}
                min="0.75"
                max="1.5"
                step="0.05"
                class="w-full accent-indigo-500"
              />
            </div>
          </div>

          {#if config.theme_profile_id}
            <p class="text-[10px] text-amber-400/90">{locale.t("settings.appearance.custom_theme_active_hint")}</p>
          {/if}

          <div class="rounded-lg border border-indigo-500/30 bg-neutral-950/80 p-4 space-y-3">
            <div>
              <h3 class="text-sm font-medium text-neutral-100">{locale.t("settings.appearance.custom_themes_title")}</h3>
              <p class="text-[10px] text-neutral-500 mt-1">{locale.t("settings.appearance.custom_themes_desc")}</p>
            </div>

            {#if (config.theme_profiles ?? []).length === 0}
              <p class="text-xs text-neutral-400">{locale.t("settings.appearance.no_custom_themes")}</p>
            {/if}

            <div class="flex flex-col sm:flex-row items-stretch sm:items-end gap-2">
              <div class="flex-1 min-w-0">
                <label for="custom-theme-name" class="block text-xs text-neutral-400 mb-1">{locale.t("settings.appearance.custom_theme_name")}</label>
                <input
                  id="custom-theme-name"
                  type="text"
                  bind:value={newThemeName}
                  class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100"
                  placeholder={locale.t("settings.appearance.custom_theme_name_placeholder")}
                  onkeydown={(e) => { if (e.key === "Enter") openThemeCreatorModal(); }}
                />
              </div>
              <button
                type="button"
                class="shrink-0 px-4 py-2 text-sm font-medium rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white transition-colors cursor-pointer"
                onclick={openThemeCreatorModal}
              >{locale.t("settings.appearance.create_theme")}</button>
            </div>

            <div>
              <label for="saved-theme-select" class="block text-xs text-neutral-400 mb-1">{locale.t("settings.appearance.saved_themes")}</label>
              <select
                id="saved-theme-select"
                value={config.theme_profile_id ?? "__builtin__"}
                onchange={(event) => {
                  if (!config) return;
                  const value = (event.target as HTMLSelectElement).value;
                  config.theme_profile_id = value === "__builtin__" ? null : value;
                  applyTheme(config);
                  autoSave();
                }}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100"
              >
                <option value="__builtin__">{locale.t("settings.appearance.use_builtin_palette")}</option>
                {#each config.theme_profiles ?? [] as profile}
                  <option value={profile.id}>{profile.name}</option>
                {/each}
              </select>
            </div>

            {#if activeThemeProfile}
              <div class="rounded-lg border border-neutral-800 bg-neutral-900/70 p-2 space-y-2">
                <div class="text-xs text-neutral-300 font-medium">{locale.t("settings.appearance.theme_preview")}</div>
                <div class="grid grid-cols-2 gap-2">
                  <div class="rounded-md border border-neutral-700 p-2 space-y-1" style="background:{activeThemeProfile.dark.background}; color:{activeThemeProfile.dark.text}">
                    <div class="text-[10px] font-semibold">{locale.t("settings.appearance.dark_preview")}</div>
                    <div class="h-2 rounded" style="background:{activeThemeProfile.dark.main}"></div>
                    <div class="h-2 rounded" style="background:{activeThemeProfile.dark.sub}"></div>
                    <div class="h-2 rounded" style="background:{activeThemeProfile.dark.trim}"></div>
                  </div>
                  <div class="rounded-md border border-neutral-700 p-2 space-y-1" style="background:{activeThemeProfile.light.background}; color:{activeThemeProfile.light.text}">
                    <div class="text-[10px] font-semibold">{locale.t("settings.appearance.light_preview")}</div>
                    <div class="h-2 rounded" style="background:{activeThemeProfile.light.main}"></div>
                    <div class="h-2 rounded" style="background:{activeThemeProfile.light.sub}"></div>
                    <div class="h-2 rounded" style="background:{activeThemeProfile.light.trim}"></div>
                  </div>
                </div>
                <div class="flex items-center gap-2 text-[10px] text-neutral-400">
                  <span>{locale.t("settings.appearance.logo_preview")}</span>
                  {#if activeThemeProfile.logo_image}
                    <img src={activeThemeProfile.logo_image} alt={locale.t("settings.appearance.logo_preview_alt")} class="w-6 h-6 rounded object-cover border border-neutral-700" />
                  {:else}
                    <span class="text-neutral-500">-</span>
                  {/if}
                  <span class="ml-2">{locale.t("settings.appearance.background_preview")}</span>
                  {#if activeThemeProfile.background_image}
                    <span class="text-green-400">{locale.t("settings.appearance.status_on")}</span>
                  {:else}
                    <span class="text-neutral-500">{locale.t("settings.appearance.status_off")}</span>
                  {/if}
                </div>
              </div>

              <div class="flex flex-wrap gap-2">
                <button
                  type="button"
                  class="px-3 py-1.5 text-xs rounded bg-indigo-600 hover:bg-indigo-500 text-white transition-colors cursor-pointer"
                  onclick={openThemeEditorModal}
                >{locale.t("settings.appearance.edit_theme")}</button>
                <button
                  type="button"
                  class="px-3 py-1.5 text-xs rounded bg-red-700/70 hover:bg-red-700 text-white transition-colors cursor-pointer"
                  onclick={removeActiveThemeProfile}
                >{locale.t("settings.appearance.delete_theme_profile")}</button>
              </div>
            {/if}

            <div class="flex gap-2">
              <button class="px-3 py-1.5 text-xs rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-100 transition-colors cursor-pointer" onclick={exportThemeProfiles}>
                {locale.t("settings.appearance.export_themes")}
              </button>
              <label class="px-3 py-1.5 text-xs rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-100 transition-colors cursor-pointer">
                {locale.t("settings.appearance.import_themes")}
                <input type="file" accept=".json,application/json" class="hidden" onchange={importThemeProfiles} />
              </label>
            </div>
            {#if themeImportError}<p class="text-xs text-red-400">{themeImportError}</p>{/if}
            {#if themeImportDone}<p class="text-xs text-green-400">{locale.t("settings.appearance.themes_imported")}</p>{/if}
            {#if themeExportError}<p class="text-xs text-red-400">{themeExportError}</p>{/if}
            {#if themeExportDone}<p class="text-xs text-green-400">{locale.t("settings.appearance.themes_exported")}</p>{/if}
          </div>

          <div>
            <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.appearance.color_vision')}</label>
            <select
              bind:value={accessibility.visionSimulatorMode}
              onchange={() => accessibility.saveSettings()}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
            >
              <option value="none">{locale.t('settings.appearance.sim_none')}</option>
              <option value="protanopia">{locale.t('settings.appearance.sim_protanopia')}</option>
              <option value="deuteranopia">{locale.t('settings.appearance.sim_deuteranopia')}</option>
              <option value="tritanopia">{locale.t('settings.appearance.sim_tritanopia')}</option>
            </select>
            <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.appearance.sim_note')}</p>
          </div>

          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="enable-style-presets"
              bind:checked={generation.stylePresetsEnabled}
              onchange={() => {
                generation.saveSettings();
              }}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="enable-style-presets" class="text-sm text-neutral-200">{locale.t('settings.appearance.style_presets')}</label>
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.appearance.style_presets_desc')}</p>
            </div>
          </div>

          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="show-info-tips"
              bind:checked={accessibility.showInfoTips}
              onchange={() => accessibility.saveSettings()}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="show-info-tips" class="text-sm text-neutral-200">{locale.t('settings.appearance.show_info_tips_label')}</label>
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.appearance.show_info_tips_tip')}</p>
            </div>
          </div>

          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="dyslexic-font"
              bind:checked={dyslexicFont}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="dyslexic-font" class="text-sm text-neutral-200">{locale.t('settings.appearance.dyslexic_font')}</label>
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.appearance.dyslexic_font_desc')}</p>
            </div>
          </div>

          <div>
            <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.appearance.language')}</label>
            <select
              bind:value={locale.current}
              onchange={() => locale.saveSettings()}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
            >
              {#each LOCALE_OPTIONS as opt}
                <option value={opt.value}>{opt.label}</option>
              {/each}
            </select>
            <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.appearance.language_desc')}</p>
          </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Performance (admin / moderator) -->
        {#if isAdmin && sectionVisible("performance")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.performance = !collapsed.performance)}
          >
            {locale.t('settings.performance.title')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.performance ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.performance}
          <div class="px-5 pb-5 space-y-4">
          <div>
            <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.performance.vram_mode')}<span class="text-amber-400">*</span></label>
            <select
              bind:value={config.vram_mode}
              onchange={() => { autoSave(); }}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
            >
              <option value="high">{locale.t('settings.performance.vram_high')}</option>
              <option value="normal">{locale.t('settings.performance.vram_normal')}</option>
              <option value="low">{locale.t('settings.performance.vram_low')}</option>
              <option value="none">{locale.t('settings.performance.vram_none')}</option>
            </select>
            <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.performance.vram_note')}</p>
          </div>

          <div>
            <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.performance.attention_backend')}<span class="text-amber-400">*</span></label>
            <select
              value={config.attention_backend}
              onchange={(e) => { handleAttentionChange((e.target as HTMLSelectElement).value); }}
              disabled={attentionInstalling}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors disabled:opacity-50"
            >
              <option value="default">{locale.t('settings.performance.attention_default')}</option>
              <option value="sage_v1" disabled={backendBlocked('sage_v1')}>{locale.t('settings.performance.attention_sage_v1')}</option>
              <option value="sage_v2" disabled={backendBlocked('sage_v2')}>{locale.t('settings.performance.attention_sage_v2')}</option>
              <option value="flash_v1" disabled={backendBlocked('flash_v1')}>{locale.t('settings.performance.attention_flash_v1')}</option>
              <option value="flash_v2" disabled={backendBlocked('flash_v2')}>{locale.t('settings.performance.attention_flash_v2')}</option>
            </select>
            {#if attentionInstalling}
              <p class="text-[10px] text-indigo-400 mt-0.5 flex items-center gap-1">
                <span class="inline-block w-3 h-3 border border-indigo-400 border-t-transparent rounded-full animate-spin"></span>
                {locale.t('settings.performance.attention_installing')}
              </p>
            {:else if attentionError}
              <p class="text-[10px] text-red-400 mt-0.5">{attentionError}</p>
            {:else}
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.performance.attention_note')}</p>
            {/if}

            {#if attentionStatus && attentionStatus.compute_capability == null}
              <p class="text-[10px] text-amber-400/80 mt-1">{locale.t('settings.performance.attention_requires_nvidia')}</p>
            {:else if attentionStatus}
              {#each attentionStatus.support.filter((s) => !s.supported) as s (s.backend)}
                <p class="text-[10px] text-amber-400/80 mt-1">{backendLabel(s.backend)}: {backendReason(s)}</p>
              {/each}
            {/if}

            <div class="rounded-lg border border-neutral-800 bg-neutral-950/50 p-3 space-y-2 mt-2">
              <p class="text-[10px] text-neutral-500">{locale.t('settings.performance.attention_status_desc')}</p>

              <div class="space-y-1.5 text-[11px]">
                <div class="flex items-start gap-2">
                  <span class="w-32 shrink-0 text-neutral-500">{locale.t('settings.paths.venv')}</span>
                  <span class="min-w-0 break-all font-mono text-neutral-300">
                    {attentionStatusVenvPath || locale.t('common.none')}
                  </span>
                </div>

                <div class="flex items-start gap-2">
                  <span class="w-32 shrink-0 text-neutral-500">{locale.t('settings.performance.attention_installed_packages')}</span>
                  <span class="min-w-0 break-all text-neutral-200">
                    {#if attentionStatusLoading && !attentionStatus}
                      {locale.t('common.loading')}
                    {:else if attentionStatus?.venv_packages.length}
                      {attentionStatus.venv_packages.join(", ")}
                    {:else}
                      {locale.t('common.none')}
                    {/if}
                  </span>
                </div>

                <div class="flex items-start gap-2">
                  <span class="w-32 shrink-0 text-neutral-500">{locale.t('settings.performance.attention_compute_capability')}</span>
                  <span class="text-neutral-200">
                    {#if attentionStatusLoading && !attentionStatus}
                      {locale.t('common.loading')}
                    {:else if attentionStatus?.compute_capability != null}
                      {attentionStatus.compute_capability.toFixed(1)}
                    {:else}
                      {locale.t('settings.performance.attention_not_detected')}
                    {/if}
                  </span>
                </div>
              </div>

              {#if attentionStatusError}
                <p class="text-[10px] text-red-400">{attentionStatusError}</p>
              {/if}

              <p class="text-[10px] text-neutral-500">{locale.t('settings.performance.attention_install_target')}</p>
              <p class="text-[10px] text-amber-400/80">{locale.t('settings.performance.attention_external_env')}</p>
              <p class="text-[10px] text-amber-400/80">{locale.t('setup.attention.compile_warning')}</p>
            </div>
          </div>

          {#if !isBrowserMode && comfyuiVersion}
          <div>
            <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.performance.comfyui_version')}</label>
            <div class="rounded-lg border border-neutral-800 bg-neutral-950/50 p-3 space-y-2">
              <div class="space-y-1.5 text-[11px]">
                <div class="flex items-start gap-2">
                  <span class="w-32 shrink-0 text-neutral-500">{locale.t('settings.performance.comfyui_installed')}</span>
                  <span class="min-w-0 break-all font-mono text-neutral-200">
                    {comfyuiVersion.installed || locale.t('settings.performance.comfyui_unknown')}
                  </span>
                </div>
                <div class="flex items-start gap-2">
                  <span class="w-32 shrink-0 text-neutral-500">{locale.t('settings.performance.comfyui_target')}</span>
                  <span class="min-w-0 break-all font-mono text-neutral-200">{comfyuiVersion.target}</span>
                </div>
              </div>

              {#if comfyuiUpdating}
                <p class="text-[10px] text-indigo-400 flex items-center gap-1">
                  <span class="inline-block w-3 h-3 border border-indigo-400 border-t-transparent rounded-full animate-spin"></span>
                  {comfyuiUpdateProgress || locale.t('settings.performance.comfyui_updating')}
                </p>
              {:else if comfyuiUpdateError}
                <p class="text-[10px] text-red-400">{comfyuiUpdateError}</p>
              {/if}

              {#if comfyuiVersion.update_available}
                <p class="text-[10px] text-amber-400/80">{locale.t('settings.performance.comfyui_update_available')}</p>
                <button
                  onclick={handleComfyuiUpdate}
                  disabled={comfyuiUpdating}
                  class="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {locale.t('settings.performance.comfyui_update_button')}
                </button>
              {:else}
                <p class="text-[10px] text-neutral-500">{locale.t('settings.performance.comfyui_up_to_date')}</p>
              {/if}

              <p class="text-[10px] text-neutral-500">{locale.t('settings.performance.comfyui_update_note')}</p>
            </div>
          </div>
          {/if}

          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="keep-alive"
              bind:checked={config.keep_alive}
              onchange={() => { autoSave(); }}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="keep-alive" class="text-sm text-neutral-200">{locale.t('settings.performance.keep_alive')}</label>
              <p class="text-[10px] text-amber-400/80 mt-0.5">{locale.t('settings.performance.keep_alive_warning')}</p>
            </div>
          </div>

          </div>
          {/if}
        </section>
        {/if}

        <!-- Quality Tags (visible to all users) -->
        {#if sectionVisible("quality")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.quality = !collapsed.quality)}
          >
            {locale.t('settings.performance.auto_quality_tags')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.quality ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.quality}
          <div class="px-5 pb-5 space-y-4">
          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="auto-quality-tags"
              checked={generation.autoQualityTags}
              onchange={(e) => {
                const target = e.target as HTMLInputElement;
                if (!target.checked) {
                  // Revert — let the popup decide
                  target.checked = true;
                  showQualityTagsWarning = true;
                } else {
                  generation.autoQualityTags = true;
                  generation.saveSettings();
                }
              }}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="auto-quality-tags" class="text-sm text-neutral-200">{locale.t('settings.performance.auto_quality_tags')}</label>
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.performance.auto_quality_tags_desc')}</p>
            </div>
          </div>

          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="advanced-mode"
              checked={generation.advancedMode}
              onchange={(e) => {
                const target = e.target as HTMLInputElement;
                if (target.checked) {
                  // Revert — confirm via popup before enabling
                  target.checked = false;
                  showAdvancedModeWarning = true;
                } else {
                  generation.advancedMode = false;
                  generation.saveSettings();
                }
              }}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="advanced-mode" class="text-sm text-neutral-200">{locale.t('settings.advanced_mode.label')}</label>
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.advanced_mode.desc')}</p>
            </div>
          </div>

          {#if generation.autoQualityTags}
          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="custom-quality-tags"
              checked={generation.customQualityTagsEnabled}
              onchange={(e) => {
                generation.customQualityTagsEnabled = (e.target as HTMLInputElement).checked;
                generation.saveSettings();
              }}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="custom-quality-tags" class="text-sm text-neutral-200">{locale.t('settings.performance.custom_quality_tags')}</label>
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.performance.custom_quality_tags_desc')}</p>
            </div>
          </div>

          {#if generation.customQualityTagsEnabled}
          <QualityTagsEditor />
          {/if}
          {/if}
          </div>
          {/if}
        </section>
        {/if}

        <!-- GPU Workers (visible to all users) -->
        {#if sectionVisible("gpu")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.gpu = !collapsed.gpu)}
          >
            {locale.t('settings.sections.gpu')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.gpu ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.gpu}
          <div class="px-5 pb-5 space-y-4">
            {#if config}
              <div class="rounded-lg border border-neutral-800 bg-neutral-950/50 p-3 space-y-3">
                <div class="flex items-center justify-between">
                  <p class="text-xs text-neutral-300">{locale.t("settings.gpu.worker_config")}</p>
                  <div class="flex items-center gap-2">
                    <button
                      type="button"
                      class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs text-neutral-200 hover:border-indigo-500"
                      onclick={addGpuWorker}
                    >
                      {locale.t("common.add")}
                    </button>
                    <button
                      type="button"
                      class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs text-neutral-200 hover:border-indigo-500 disabled:opacity-50"
                      onclick={autoDetectGpuWorkers}
                      disabled={workersDetecting}
                    >
                      {workersDetecting ? locale.t("settings.gpu.detecting") : locale.t("settings.gpu.auto_detect")}
                    </button>
                  </div>
                </div>
                {#if !config.gpu_workers || config.gpu_workers.length === 0}
                  <p class="text-[11px] text-neutral-500">{locale.t("settings.gpu.no_workers")}</p>
                {:else}
                  <div class="space-y-2">
                    {#each config.gpu_workers as worker, idx}
                      <div class="grid grid-cols-12 gap-2 items-center rounded border border-neutral-800 bg-neutral-900/60 p-2">
                        <label class="col-span-2 text-[10px] text-neutral-400">
                          {locale.t("settings.gpu.field.gpu")}
                          <input type="number" bind:value={worker.gpu_index} oninput={() => autoSave()} class="mt-1 w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs text-neutral-100" />
                        </label>
                        <label class="col-span-2 text-[10px] text-neutral-400">
                          {locale.t("settings.gpu.field.port")}
                          <input type="number" bind:value={worker.port} oninput={() => autoSave()} class="mt-1 w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs text-neutral-100" />
                        </label>
                        <label class="col-span-4 text-[10px] text-neutral-400">
                          {locale.t("settings.gpu.field.label")}
                          <input type="text" bind:value={worker.label} oninput={() => autoSave()} class="mt-1 w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs text-neutral-100" />
                        </label>
                        <label class="col-span-2 text-[10px] text-neutral-400">
                          {locale.t("settings.gpu.vram")}
                          <select bind:value={worker.vram_mode} onchange={() => autoSave()} class="mt-1 w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs text-neutral-100">
                            <option value="">{locale.t("settings.gpu.vram_mode.default")}</option>
                            <option value="high">{locale.t("settings.gpu.vram_mode.high")}</option>
                            <option value="normal">{locale.t("settings.gpu.vram_mode.normal")}</option>
                            <option value="low">{locale.t("settings.gpu.vram_mode.low")}</option>
                            <option value="none">{locale.t("settings.gpu.vram_mode.none")}</option>
                          </select>
                        </label>
                        <div class="col-span-2 flex items-center justify-end gap-2">
                          <label class="inline-flex items-center gap-1 text-[10px] text-neutral-300">
                            <input type="checkbox" bind:checked={worker.enabled} onchange={() => autoSave()} class="accent-indigo-500" />
                            {locale.t("settings.gpu.enabled_label")}
                          </label>
                          <button type="button" class="text-xs text-red-300 hover:text-red-200" onclick={() => removeGpuWorker(idx)}>{locale.t("common.delete")}</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {/if}
            <GpuStatusPanel />
          </div>
          {/if}
        </section>
        {/if}

        <!-- Model Management (mods/admins) -->
        {#if canManageServer && sectionVisible("models")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.models = !collapsed.models)}
          >
            {locale.t('settings.models.manage')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.models ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.models}
          <div class="px-5 pb-5">
            <button
              type="button"
              onclick={() => (showModelManager = true)}
              class="w-full flex items-center justify-between rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-left text-sm text-neutral-200 transition-colors hover:border-indigo-500 hover:text-indigo-300"
            >
              <span>
                <span class="block text-sm font-medium">{locale.t('settings.models.title')}</span>
                <span class="block text-[10px] text-neutral-500">{locale.t('settings.models.manage_desc')}</span>
              </span>
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/><path d="M9 15h6"/><path d="M12 12v6"/></svg>
            </button>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Model Requests (mods/admins) -->
        {#if canManageServer && sectionVisible("modelRequests")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.modelRequests = !collapsed.modelRequests)}
          >
            {locale.t('settings.sections.model_requests')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.modelRequests ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.modelRequests}
          <div class="px-5 pb-5">
            <ModelRequestsPanel {userRole} />
          </div>
          {/if}
        </section>
        {/if}

        <!-- Paths (admin only) -->
        {#if isAdmin && sectionVisible("paths")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.paths = !collapsed.paths)}
          >
            {locale.t('settings.paths.title')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.paths ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.paths}
          <div class="px-5 pb-5 space-y-4">

          <!-- Open Model Folders -->
          <OpenModelFolders />

          <!-- Move Installation -->
          <div class="rounded-lg border border-neutral-800 bg-neutral-950/50 p-3 space-y-2">
            <div class="flex items-center justify-between">
              <p class="text-xs text-neutral-400">{locale.t('settings.paths.data_location')}</p>
            </div>
            {#if currentInstallPath}
              <p class="text-xs text-neutral-500 font-mono truncate" title={currentInstallPath}>{currentInstallPath}</p>
            {/if}

            {#if moveSuccess}
              <div class="rounded border border-green-800/50 bg-green-900/20 px-2 py-1.5 text-[11px] text-green-300">
                {locale.t('settings.paths.move_success')}
              </div>
            {/if}

            {#if !moving}
              <div class="flex gap-1.5">
                <input
                  type="text"
                  bind:value={moveTargetPath}
                  class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-1.5 text-sm text-neutral-100 placeholder-neutral-500"
                  placeholder={locale.t('settings.paths.new_location_placeholder')}
                />
                <button
                  onclick={browseMoveTarget}
                  class="px-2 py-1.5 rounded-lg border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors text-xs"
                >
                  {locale.t('common.browse')}
                </button>
              </div>
              {#if moveTargetPath.trim()}
                <button
                  onclick={moveInstallation}
                  class="w-full px-3 py-2 text-xs rounded bg-amber-600 hover:bg-amber-500 text-white transition-colors"
                >
                  {locale.t('settings.paths.move_button')}
                </button>
                <p class="text-[10px] text-amber-400/70">{locale.t('settings.paths.move_warning')}</p>
              {/if}
            {:else}
              <div class="flex items-center gap-2 text-xs text-neutral-400">
                <div class="w-3.5 h-3.5 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin shrink-0"></div>
                <span>{moveProgress}</span>
              </div>
            {/if}

            {#if moveError}
              <div class="rounded border border-red-800/50 bg-red-900/20 px-2 py-1.5 text-[11px] text-red-300">{moveError}</div>
            {/if}
          </div>

          <!-- Rerun Setup Wizard (desktop host only; setup installs locally) -->
          {#if isTauri}
          <div class="rounded-lg border border-neutral-800 bg-neutral-950/50 p-3 space-y-2">
            <p class="text-xs text-neutral-400">{locale.t('settings.paths.rerun_setup')}</p>
            <p class="text-[10px] text-neutral-500">{locale.t('settings.paths.rerun_setup_desc')}</p>
            {#if showRerunSetupConfirm}
              <p class="text-[11px] text-amber-300">{locale.t('settings.paths.rerun_setup_confirm')}</p>
              <div class="flex gap-2">
                <button
                  class="flex-1 px-3 py-2 text-xs rounded bg-neutral-700 hover:bg-neutral-600 text-neutral-300 transition-colors"
                  onclick={() => (showRerunSetupConfirm = false)}
                >{locale.t('common.cancel')}</button>
                <button
                  class="flex-1 px-3 py-2 text-xs rounded bg-amber-600 hover:bg-amber-500 text-white transition-colors"
                  onclick={rerunSetup}
                >{locale.t('settings.paths.rerun_setup')}</button>
              </div>
            {:else}
              <button
                class="w-full px-3 py-2 text-xs rounded bg-amber-600 hover:bg-amber-500 text-white transition-colors"
                onclick={() => (showRerunSetupConfirm = true)}
              >{locale.t('settings.paths.rerun_setup')}</button>
            {/if}
          </div>
          {/if}

          <div>
            <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.paths.comfyui_install')}</label>
            <input
              type="text"
              bind:value={config.comfyui_path}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              placeholder={locale.t('settings.paths.comfyui_placeholder')}
            />
          </div>

          <div>
            <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.paths.venv')}</label>
            <input
              type="text"
              bind:value={config.venv_path}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              placeholder={locale.t('settings.paths.venv_placeholder')}
            />
          </div>

          <div>
            <div class="flex items-center justify-between mb-1">
              <label class="block text-xs text-neutral-400">{locale.t('settings.paths.shared_model_dirs')}<span class="text-amber-400">*</span></label>
              <div class="flex gap-1.5">
                <button
                  class="px-2 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-400 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                  onclick={scanForModelDirs}
                  disabled={scanningModelDirs}
                >
                  {scanningModelDirs ? locale.t('settings.paths.scanning') : locale.t('settings.paths.auto_detect')}
                </button>
                <button
                  class="px-2 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-400 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                  onclick={() => {
                    if (config) {
                      const current = config.extra_model_paths ?? "";
                      config.extra_model_paths = current ? current + "\n" : "";
                      checkRestartNeeded();
                    }
                  }}
                  title={locale.t('settings.paths.add_model_dir_title')}
                >
                  {locale.t('settings.paths.add_directory')}
                </button>
              </div>
            </div>
            {#each (config.extra_model_paths ?? "").split("\n") as dirPath, i}
              <div class="flex gap-1.5 mb-1.5">
                <input
                  type="text"
                  value={dirPath}
                  oninput={(e) => {
                    if (config) {
                      const paths = (config.extra_model_paths ?? "").split("\n");
                      paths[i] = (e.target as HTMLInputElement).value;
                      config.extra_model_paths = paths.join("\n") || null;
                      checkRestartNeeded();
                    }
                  }}
                  class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                  placeholder={locale.t('settings.paths.extra_model_placeholder')}
                />
                <button
                  class="px-2 py-2 rounded-lg border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors text-xs"
                  onclick={() => browseModelDir(i)}
                  title={locale.t('settings.paths.browse_model_dir_title')}
                >
                  {locale.t('common.browse')}
                </button>
                {#if (config.extra_model_paths ?? "").split("\n").length > 1}
                  <button
                    class="px-2 py-2 rounded-lg border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300 transition-colors text-xs"
                    onclick={() => {
                      if (config) {
                        const paths = (config.extra_model_paths ?? "").split("\n");
                        paths.splice(i, 1);
                        config.extra_model_paths = paths.join("\n") || null;
                        checkRestartNeeded();
                      }
                    }}
                    title={locale.t('settings.paths.remove_model_dir_title')}
                  >
                    &times;
                  </button>
                {/if}
              </div>
            {/each}
            <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.paths.model_dirs_desc')}</p>

            {#if detectedModelDirs.length > 0}
              <div class="mt-2 space-y-1">
                <p class="text-[10px] text-neutral-500">{locale.t('settings.paths.found_dirs')}</p>
                {#each detectedModelDirs as dir}
                  <div class="flex items-center gap-1.5">
                    <button
                      class="flex-1 text-left px-2 py-1.5 rounded border border-neutral-700/50 bg-neutral-800/50 hover:border-indigo-500/50 transition-colors"
                      onclick={() => addDetectedModelDir(dir.path)}
                      title={locale.t('settings.paths.click_to_add')}
                    >
                      <p class="text-[11px] text-neutral-300 truncate">{dir.path}</p>
                      <p class="text-[10px] text-neutral-500">
                        {dir.tool}
                        {#if dir.has_checkpoints} · {locale.t('settings.paths.checkpoints')}{/if}
                        {#if dir.has_loras} · {locale.t('settings.paths.loras')}{/if}
                        {#if dir.has_vae} · {locale.t('settings.paths.vaes')}{/if}
                      </p>
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>

          <div>
            <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.paths.extra_args')}<span class="text-amber-400">*</span></label>
            <input
              type="text"
              value={config.extra_args.join(" ")}
              oninput={(e) => {
                if (config) {
                  const val = (e.target as HTMLInputElement).value;
                  config.extra_args = val ? val.split(/\s+/) : [];
                  checkRestartNeeded();
                }
              }}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              placeholder={locale.t('settings.paths.extra_args_placeholder')}
            />
            <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.paths.extra_args_desc')}</p>
          </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Gallery -->
        {#if sectionVisible("gallery")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.gallery = !collapsed.gallery)}
          >
            {locale.t('settings.gallery.title')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.gallery ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.gallery}
          <div class="px-5 pb-5 space-y-4">

            {#if isAdmin}
            <!-- Gallery storage location -->
            <div>
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.gallery.storage_label')}</label>
              <p class="text-[10px] text-neutral-500 mb-2">{locale.t('settings.gallery.storage_desc')}</p>
              <div class="flex gap-1.5 items-center">
                <div class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-300 truncate select-text" title={galleryPathDisplay}>
                  {#if config?.gallery_path}
                    <span class="text-indigo-400 text-[10px] uppercase mr-1.5">{locale.t('settings.gallery.storage_custom')}</span>
                  {:else}
                    <span class="text-neutral-500 text-[10px] uppercase mr-1.5">{locale.t('settings.gallery.storage_default')}</span>
                  {/if}
                  {galleryPathDisplay}
                </div>
                <button
                  class="px-3 py-2 rounded-lg border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors text-xs whitespace-nowrap"
                  disabled={galleryPathSaving}
                  onclick={handleBrowseGalleryPath}
                >
                  {locale.t('common.browse')}
                </button>
                {#if config?.gallery_path}
                  <button
                    class="px-2 py-2 rounded-lg border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300 transition-colors text-xs whitespace-nowrap"
                    disabled={galleryPathSaving}
                    onclick={handleResetGalleryPath}
                    title={locale.t('settings.gallery.storage_reset_title')}
                  >
                    {locale.t('settings.gallery.storage_reset')}
                  </button>
                {/if}
              </div>
              {#if galleryPathMessage}
                <p class="mt-1.5 text-[11px] text-amber-400">{galleryPathMessage}</p>
              {/if}
            </div>

            <!-- Never expire gallery images -->
            <div>
              <label class="flex items-center gap-2 cursor-pointer select-none">
                <input
                  type="checkbox"
                  class="w-4 h-4 rounded accent-indigo-500"
                  bind:checked={config.gallery_never_expire}
                  onchange={() => { autoSave(); }}
                />
                <span class="text-sm text-neutral-200">{locale.t('settings.gallery.never_expire_label')}</span>
              </label>
              <p class="text-[10px] text-neutral-500 mt-1 ml-6">{locale.t('settings.gallery.never_expire_desc')}</p>
            </div>
            {/if}

            <!-- Manual save mode -->
            <div>
              <label class="flex items-center gap-2 cursor-pointer select-none">
                <input
                  type="checkbox"
                  class="w-4 h-4 rounded accent-indigo-500"
                  checked={generation.manualSaveMode}
                  onchange={(e) => {
                    generation.manualSaveMode = (e.target as HTMLInputElement).checked;
                    generation.saveSettings();
                  }}
                />
                <span class="text-sm text-neutral-200">{locale.t('settings.gallery.manual_save_label')}</span>
              </label>
              <p class="text-[10px] text-neutral-500 mt-1 ml-6">{locale.t('settings.gallery.manual_save_desc')}</p>
            </div>

            <!-- Save pre-upscale image (advanced) -->
            <div>
              <label class="flex items-center gap-2 cursor-pointer select-none">
                <input
                  type="checkbox"
                  class="w-4 h-4 rounded accent-indigo-500"
                  checked={generation.savePreUpscaleImage}
                  onchange={(e) => {
                    generation.savePreUpscaleImage = (e.target as HTMLInputElement).checked;
                    generation.saveSettings();
                  }}
                />
                <span class="text-sm text-neutral-200">{locale.t('settings.gallery.save_pre_upscale_label')}</span>
              </label>
              <p class="text-[10px] text-neutral-500 mt-1 ml-6">{locale.t('settings.gallery.save_pre_upscale_desc')}</p>
            </div>

            {#if generation.manualSaveMode && isAdmin}
            <!-- Save directories -->
            <div>
              <div class="flex items-center justify-between mb-1.5">
                <label class="text-xs text-neutral-400">{locale.t('settings.gallery.save_dirs_label')}</label>
                <button
                  class="px-2 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-400 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                  onclick={() => {
                    generation.autoSaveDirs = [...generation.autoSaveDirs, ""];
                    generation.saveSettings();
                  }}
                  title={locale.t('settings.gallery.add_save_dir_title')}
                >
                  {locale.t('settings.gallery.add_save_dir')}
                </button>
              </div>
              {#each generation.autoSaveDirs as dir, i}
                <div class="flex gap-1.5 mb-1.5">
                  <input
                    type="text"
                    value={dir}
                    oninput={(e) => {
                      const dirs = [...generation.autoSaveDirs];
                      dirs[i] = (e.target as HTMLInputElement).value;
                      generation.autoSaveDirs = dirs;
                      generation.saveSettings();
                    }}
                    class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                    placeholder={locale.t('settings.gallery.save_dir_placeholder')}
                  />
                  <button
                    class="px-2 py-2 rounded-lg border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors text-xs"
                    onclick={() => browseSaveDir(i)}
                    title={locale.t('settings.gallery.browse_save_dir_title')}
                  >
                    {locale.t('common.browse')}
                  </button>
                  {#if generation.autoSaveDirs.length > 1}
                    <button
                      class="px-2 py-2 rounded-lg border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300 transition-colors text-xs"
                      onclick={() => {
                        generation.autoSaveDirs = generation.autoSaveDirs.filter((_, j) => j !== i);
                        generation.saveSettings();
                      }}
                      title={locale.t('settings.gallery.remove_save_dir_title')}
                    >
                      &times;
                    </button>
                  {/if}
                </div>
              {/each}
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.gallery.save_dirs_desc')}</p>
            </div>
            {/if}

            {#if isAdmin}
            <!-- Import from external directory -->
            <div>
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.gallery.import_label')}</label>
              <p class="text-[10px] text-neutral-500 mb-2">{locale.t('settings.gallery.import_desc')}</p>
              <button
                class="px-4 py-2 text-sm font-medium rounded-lg transition-colors {importBusy ? 'bg-neutral-700 text-neutral-400 cursor-not-allowed' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
                disabled={importBusy}
                onclick={handleImportDirectory}
              >
                {#if importBusy}
                  {locale.t('settings.gallery.importing')}
                {:else}
                  {locale.t('settings.gallery.choose_directory')}
                {/if}
              </button>

              {#if importResult}
                <div class="mt-2 p-3 rounded-lg bg-neutral-800 border border-neutral-700 text-sm">
                  <p class="text-green-400">{locale.t('settings.gallery.imported_count', { count: importResult.imported })}</p>
                  {#if importResult.skipped > 0}
                    <p class="text-neutral-400">{locale.t('settings.gallery.skipped_count', { count: importResult.skipped })}</p>
                  {/if}
                  {#if importResult.failed > 0}
                    <p class="text-red-400">{locale.t('settings.gallery.failed_count', { count: importResult.failed })}</p>
                  {/if}
                </div>
              {/if}

              {#if importError}
                <p class="mt-2 text-sm text-red-400">{importError}</p>
              {/if}
            </div>

            <div class="rounded-lg bg-neutral-800/50 border border-neutral-700/50 p-3">
              <p class="text-[11px] text-neutral-400 leading-relaxed">
                <strong class="text-neutral-300">{locale.t('settings.gallery.supported_sources').split(':')[0]}:</strong> {locale.t('settings.gallery.supported_sources').split(':').slice(1).join(':').trim()}
              </p>
              <p class="text-[11px] text-neutral-400 mt-1.5 leading-relaxed">
                <strong class="text-neutral-300">{locale.t('settings.gallery.metadata_support').split(':')[0]}:</strong> {locale.t('settings.gallery.metadata_support').split(':').slice(1).join(':').trim()}
              </p>
            </div>
            {/if}

            <!-- Artist image cache (visible to all users) -->
            <div>
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.gallery.artist_cache_label')}</label>
              <p class="text-[10px] text-neutral-500 mb-2">{locale.t('settings.gallery.artist_cache_desc')}</p>
              <div class="flex items-center gap-3">
                <button
                  class="px-3 py-1.5 text-xs rounded-lg border transition-colors {cacheClearBusy ? 'border-neutral-700 text-neutral-500 cursor-not-allowed' : 'border-red-800/60 text-red-400 hover:border-red-600 hover:text-red-300'}"
                  disabled={cacheClearBusy}
                  onclick={handleClearArtistCache}
                >
                  {cacheClearBusy ? locale.t('settings.gallery.artist_cache_clearing') : locale.t('settings.gallery.artist_cache_clear')}
                </button>
                {#if cacheClearCount !== null}
                  <span class="text-[10px] text-neutral-500">
                    {cacheClearCount === 0
                      ? locale.t('settings.gallery.artist_cache_empty')
                      : locale.t('settings.gallery.artist_cache_count', { count: String(cacheClearCount) })}
                  </span>
                {/if}
                {#if cacheClearDone}
                  <span class="text-[10px] text-green-400">{locale.t('settings.gallery.artist_cache_cleared')}</span>
                {/if}
              </div>
            </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Autocomplete -->
        {#if sectionVisible("autocomplete")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.autocomplete = !collapsed.autocomplete)}
          >
            {locale.t('settings.autocomplete.title')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.autocomplete ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.autocomplete}
          <div class="px-5 pb-5 space-y-4">
            <!-- Enabled toggle -->
            <label class="flex items-center justify-between gap-3 cursor-pointer">
              <div>
                <p class="text-sm text-neutral-200">{locale.t('settings.autocomplete.enabled')}</p>
                <p class="text-[11px] text-neutral-500 mt-0.5">{locale.t('settings.autocomplete.enabled_desc')}</p>
              </div>
              <button
                class="relative w-10 h-5 rounded-full transition-colors shrink-0 {autocomplete.enabled ? 'bg-indigo-600' : 'bg-neutral-700'}"
                onclick={() => { autocomplete.enabled = !autocomplete.enabled; autocomplete.saveSettings(); }}
                role="switch"
                aria-checked={autocomplete.enabled}
                aria-label={locale.t('settings.autocomplete.enabled')}
                title={locale.t('settings.autocomplete.enabled')}
              >
                <span class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {autocomplete.enabled ? 'translate-x-5' : ''}"></span>
              </button>
            </label>

            <label class="flex items-center justify-between gap-3 cursor-pointer">
              <div>
                <p class="text-sm text-neutral-200">{locale.t('settings.autocomplete.clickable_overlay')}</p>
                <p class="text-[11px] text-neutral-500 mt-0.5">{locale.t('settings.autocomplete.clickable_overlay_desc')}</p>
              </div>
              <button
                class="relative w-10 h-5 rounded-full transition-colors shrink-0 {autocomplete.clickableOverlayEnabled ? 'bg-indigo-600' : 'bg-neutral-700'}"
                onclick={() => { autocomplete.clickableOverlayEnabled = !autocomplete.clickableOverlayEnabled; autocomplete.saveSettings(); }}
                role="switch"
                aria-checked={autocomplete.clickableOverlayEnabled}
                aria-label={locale.t('settings.autocomplete.clickable_overlay')}
                title={locale.t('settings.autocomplete.clickable_overlay')}
              >
                <span class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {autocomplete.clickableOverlayEnabled ? 'translate-x-5' : ''}"></span>
              </button>
            </label>

            <label class="flex items-center justify-between gap-3 cursor-pointer">
              <div>
                <p class="text-sm text-neutral-200">{locale.t('settings.autocomplete.spellcheck')}</p>
                <p class="text-[11px] text-neutral-500 mt-0.5">{locale.t('settings.autocomplete.spellcheck_desc')}</p>
              </div>
              <button
                class="relative w-10 h-5 rounded-full transition-colors shrink-0 {autocomplete.spellcheckEnabled ? 'bg-indigo-600' : 'bg-neutral-700'}"
                onclick={() => { autocomplete.spellcheckEnabled = !autocomplete.spellcheckEnabled; autocomplete.saveSettings(); }}
                role="switch"
                aria-checked={autocomplete.spellcheckEnabled}
                aria-label={locale.t('settings.autocomplete.spellcheck')}
                title={locale.t('settings.autocomplete.spellcheck')}
              >
                <span class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {autocomplete.spellcheckEnabled ? 'translate-x-5' : ''}"></span>
              </button>
            </label>

            {#if isAdmin}
            <!-- Current source -->
            <div>
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.autocomplete.tag_source')}</label>
              <div class="flex items-center gap-2 text-sm text-neutral-300">
                {#if autocomplete.sourceMode === "builtin"}
                  <span class="inline-block w-2 h-2 rounded-full bg-indigo-400"></span>
                  {locale.t('settings.autocomplete.source_builtin')} ({locale.formatInteger(autocomplete.tags.length)} {locale.t('settings.autocomplete.tags_count')})
                {:else if autocomplete.sourceMode === "url"}
                  <span class="inline-block w-2 h-2 rounded-full bg-green-400"></span>
                  URL: <span class="text-neutral-400 truncate max-w-xs">{autocomplete.sourceUrl}</span>
                  ({locale.formatInteger(autocomplete.tags.length)} tags)
                {:else if autocomplete.sourceMode === "file"}
                  <span class="inline-block w-2 h-2 rounded-full bg-green-400"></span>
                  File: {autocomplete.sourceFileName}
                  ({locale.formatInteger(autocomplete.tags.length)} tags)
                {/if}
              </div>
            </div>

            <!-- Load from URL -->
            <div>
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.autocomplete.load_url')}</label>
              <div class="flex gap-2">
                <input
                  type="text"
                  bind:value={tagUrlInput}
                  placeholder={locale.t('settings.autocomplete.url_placeholder')}
                  class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                />
                <button
                  class="px-3 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
                  disabled={!tagUrlInput.trim() || autocomplete.loading}
                  onclick={() => autocomplete.loadFromUrl(tagUrlInput.trim())}
                >
                  {autocomplete.loading ? locale.t('settings.autocomplete.fetching') : locale.t('settings.autocomplete.fetch')}
                </button>
              </div>
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.autocomplete.format_hint')}</p>
            </div>

            <!-- Upload file -->
            <div>
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.autocomplete.upload_file')}</label>
              <label
                class="inline-flex items-center gap-2 px-3 py-2 bg-neutral-800 border border-neutral-700 rounded-lg text-sm text-neutral-300 hover:border-indigo-500 transition-colors cursor-pointer"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                {tagFileLoading ? locale.t('settings.autocomplete.reading_file') : locale.t('settings.autocomplete.choose_file')}
                <input
                  type="file"
                  accept=".json,.csv,.txt"
                  class="hidden"
                  onchange={async (e) => {
                    const input = e.target as HTMLInputElement;
                    const file = input.files?.[0];
                    if (!file) return;
                    tagFileLoading = true;
                    try {
                      const text = await file.text();
                      await autocomplete.loadFromFile(text, file.name);
                    } finally {
                      tagFileLoading = false;
                      input.value = "";
                    }
                  }}
                />
              </label>
            </div>

            <!-- Reset to built-in -->
            {#if autocomplete.sourceMode !== "builtin"}
            <button
              class="text-xs text-neutral-400 hover:text-neutral-200 underline transition-colors"
              onclick={() => autocomplete.resetToBuiltin()}
            >
              {locale.t('settings.autocomplete.reset_builtin')}
            </button>
            {/if}

            <!-- Error -->
            {#if autocomplete.error}
              <div class="px-3 py-2 bg-red-900/30 border border-red-800/50 rounded-lg text-red-200 text-xs">
                {autocomplete.error}
              </div>
            {/if}
            {/if}

            <!-- Max results -->
            <div>
              <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
                {locale.t('settings.autocomplete.max_suggestions')}
                <span class="text-neutral-300">{autocomplete.maxResults}</span>
              </label>
              <input
                type="range"
                value={autocomplete.maxResults}
                oninput={(e) => { autocomplete.setMaxResults(parseInt((e.target as HTMLInputElement).value)); }}
                min="3"
                max="30"
                step="1"
                class="w-full accent-indigo-500"
              />
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.autocomplete.results_hint')}</p>
            </div>

            <!-- Undo/redo hint -->
            <div class="px-3 py-2 bg-neutral-800/50 border border-neutral-700/50 rounded-lg text-[10px] text-neutral-500">
              {locale.t('settings.autocomplete.undo_redo_tip')}
            </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Interrogator -->
        {#if sectionVisible("interrogator")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.interrogator = !collapsed.interrogator)}
          >
            <span class="flex items-center gap-2">
              <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 text-amber-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
              {locale.t('settings.interrogator.title')}
            </span>
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 text-neutral-500 transition-transform {collapsed.interrogator ? '' : 'rotate-180'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.interrogator}
          <div class="px-5 pb-5 space-y-4">
            <p class="text-[10px] text-neutral-500">
              {locale.t('settings.interrogator.thresholds_desc')}
            </p>

            <div>
              <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
                {locale.t('settings.interrogator.general_threshold')}
                <span class="text-neutral-300">{locale.formatDecimal(config.interrogator_general_threshold, 2)}</span>
              </label>
              <input
                type="range"
                bind:value={config.interrogator_general_threshold}
                onchange={() => { autoSave(); }}
                min="0.05"
                max="0.95"
                step="0.05"
                class="w-full accent-indigo-500"
              />
              <div class="flex justify-between text-[10px] text-neutral-600 mt-0.5">
                <span>{locale.t('settings.interrogator.more_tags')}</span>
                <span>{locale.t('settings.interrogator.fewer_tags')}</span>
              </div>
            </div>

            <div>
              <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
                {locale.t('settings.interrogator.character_threshold')}
                <span class="text-neutral-300">{locale.formatDecimal(config.interrogator_character_threshold, 2)}</span>
              </label>
              <input
                type="range"
                bind:value={config.interrogator_character_threshold}
                onchange={() => { autoSave(); }}
                min="0.05"
                max="0.95"
                step="0.05"
                class="w-full accent-indigo-500"
              />
              <div class="flex justify-between text-[10px] text-neutral-600 mt-0.5">
                <span>{locale.t('settings.interrogator.more_tags')}</span>
                <span>{locale.t('settings.interrogator.fewer_tags')}</span>
              </div>
            </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Prompt Assistant -->
        {#if sectionVisible("prompt_assistant")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.prompt_assistant = !collapsed.prompt_assistant)}
          >
            <span class="flex items-center gap-2">
              <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 text-violet-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m12 3-1.912 5.813a2 2 0 0 1-1.275 1.275L3 12l5.813 1.912a2 2 0 0 1 1.275 1.275L12 21l1.912-5.813a2 2 0 0 1 1.275-1.275L21 12l-5.813-1.912a2 2 0 0 1-1.275-1.275L12 3Z"/></svg>
              {locale.t('settings.prompt_assistant.title')}
            </span>
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 text-neutral-500 transition-transform {collapsed.prompt_assistant ? '' : 'rotate-180'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.prompt_assistant}
          <div class="px-5 pb-5 space-y-4">
            <p class="text-[10px] text-neutral-500">
              {locale.t('settings.prompt_assistant.desc')}
            </p>

            <div class="text-sm text-neutral-300">
              {#if promptAssistant.status && promptAssistant.status.installed_models.length > 0}
                {locale.t('settings.prompt_assistant.installed_label')}: {promptAssistant.status.installed_models.join(", ")}
              {:else}
                {locale.t('settings.prompt_assistant.none_installed')}
              {/if}
            </div>

            <div class="flex flex-wrap gap-2">
              <button
                class="rounded-lg border border-neutral-600 px-3 py-1 text-sm text-neutral-200 hover:bg-neutral-800 cursor-pointer"
                onclick={() => (showPromptAssistantSetup = true)}
              >
                {locale.t('settings.prompt_assistant.manage_models')}
              </button>
              {#each promptAssistant.status?.installed_models ?? [] as id}
                <button
                  class="rounded-lg border border-red-700 px-3 py-1 text-sm text-red-300 hover:bg-red-900/30 cursor-pointer"
                  onclick={() => promptAssistant.deleteModel(id)}
                >
                  {locale.t('settings.prompt_assistant.delete')} {id}
                </button>
              {/each}
              <button
                class="rounded-lg border border-neutral-600 px-3 py-1 text-sm text-neutral-200 hover:bg-neutral-800 cursor-pointer"
                onclick={() => promptAssistant.unload()}
              >
                {locale.t('settings.prompt_assistant.unload_now')}
              </button>
            </div>

            <div>
              <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
                {locale.t('settings.prompt_assistant.idle_timeout')}
                <span class="text-neutral-300">{config.prompt_assistant_idle_timeout_secs}s</span>
              </label>
              <input
                type="range"
                bind:value={config.prompt_assistant_idle_timeout_secs}
                onchange={() => { autoSave(); }}
                min="30"
                max="600"
                step="30"
                class="w-full accent-indigo-500"
              />
            </div>

            <!-- External LLM provider (#389). Its own component: every field
                 but the enable toggle is written by a dedicated Rust command,
                 not by this page's full-config autosave. -->
            <LlmProviderPanel
              bind:enabled={config.llm_external_enabled}
              {autoSave}
              onstate={onProviderState}
            />
          </div>
          {/if}
        </section>
        {/if}

        <!-- CivitAI (admin / moderator) -->
        {#if canManageServer && sectionVisible("civitai")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.civitai = !collapsed.civitai)}
          >
            <span class="flex items-center gap-2">
              <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 text-blue-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>
              {locale.t('settings.civitai.title')}
            </span>
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 text-neutral-500 transition-transform {collapsed.civitai ? '' : 'rotate-180'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.civitai}
          <div class="px-5 pb-5 space-y-3">
            <p class="text-[10px] text-neutral-500">{locale.t('settings.civitai.api_key_desc')}</p>
            <div>
              <label class="text-xs text-neutral-400 block mb-1">{locale.t('settings.civitai.api_key')}</label>
              <input
                type="password"
                value={config.civitai_api_key ?? ""}
                oninput={(e) => {
                  if (config) {
                    const v = (e.target as HTMLInputElement).value.trim();
                    config.civitai_api_key = v || null;
                  }
                }}
                onchange={() => { autoSave(); }}
                placeholder={locale.t('settings.civitai.api_key_placeholder')}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors font-mono"
              />
              <p class="text-[10px] text-neutral-500 mt-1">{locale.t('settings.civitai.api_key_link')}</p>
            </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Legacy password encryption upgrade (browser mode) -->
        {#if isBrowserMode && usesLegacyPassword}
        <section class="bg-neutral-900 rounded-xl border border-amber-800/50 overflow-hidden break-inside-avoid mb-4">
          <div class="p-5 space-y-3">
            <h3 class="text-sm font-medium text-amber-200">{locale.t('auth.legacy_password_migration_title')}</h3>
            <p class="text-xs text-neutral-400">
              {locale.t('auth.legacy_password_migration_desc', {
                deadline: formatLegacyDeadline(legacyPasswordDeadline),
              })}
            </p>
            {#if legacyPasswordExpired}
              <p class="text-xs text-red-400">{locale.t('auth.legacy_password_expired')}</p>
            {/if}
            <p class="text-[10px] text-neutral-500">{locale.t('auth.upgrade_password_encryption_desc')}</p>
            <input
              type="password"
              bind:value={upgradePass}
              placeholder={locale.t('settings.lan.current_password')}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-amber-500 transition-colors"
              onkeydown={(e) => { if (e.key === "Enter") upgradePasswordEncryption(); }}
            />
            {#if upgradeError}
              <p class="text-xs text-red-400">{upgradeError}</p>
            {/if}
            {#if upgradeSuccess}
              <p class="text-xs text-green-400">{locale.t('auth.upgrade_password_success')}</p>
            {/if}
            <button
              class="w-full py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer border border-amber-700/60 {upgradeBusy ? 'bg-neutral-800 text-neutral-500' : 'bg-amber-600/20 hover:bg-amber-600/30 text-amber-200'}"
              disabled={upgradeBusy}
              onclick={upgradePasswordEncryption}
            >
              {upgradeBusy ? locale.t('common.saving') : locale.t('auth.upgrade_password_encryption')}
            </button>
          </div>
        </section>
        {/if}

        <!-- Account / Change Password (browser mode non-admin users) -->
        {#if isBrowserMode && !isAdmin}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <div class="p-5 space-y-3">
            <h3 class="text-sm font-medium text-neutral-200">{locale.t('settings.account')}</h3>
            <button
              class="w-full py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer bg-neutral-800 hover:bg-neutral-700 text-neutral-300 border border-neutral-700"
              onclick={() => {
                showChangePasswordForm = !showChangePasswordForm;
                cpError = null;
                cpSuccess = false;
              }}
            >
              {showChangePasswordForm ? locale.t('common.cancel') : locale.t('auth.change_password')}
            </button>
            {#if showChangePasswordForm}
            <div class="space-y-2">
              <input
                type="password"
                bind:value={cpCurrentPass}
                placeholder={locale.t('settings.lan.current_password')}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              />
              <input
                type="password"
                bind:value={cpNewPass1}
                placeholder={locale.t('auth.new_password_placeholder')}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              />
              <input
                type="password"
                bind:value={cpNewPass2}
                placeholder={locale.t('auth.confirm_password_placeholder')}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                onkeydown={(e) => { if (e.key === "Enter") changeOwnPassword(); }}
              />
              {#if cpError}
                <p class="text-xs text-red-400">{cpError}</p>
              {/if}
              {#if cpSuccess}
                <p class="text-xs text-green-400">{locale.t('settings.lan.password_changed')}</p>
              {/if}
              <button
                class="w-full py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {cpBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
                disabled={cpBusy}
                onclick={changeOwnPassword}
              >
                {cpBusy ? locale.t('common.saving') : locale.t('auth.confirm_change')}
              </button>
            </div>
            {/if}
            <hr class="border-neutral-800" />
            <button
              class="w-full py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer bg-red-600/20 hover:bg-red-600/40 text-red-300 border border-red-800/50"
              onclick={async () => {
                try { await fetch("/internal-api/_auth/logout", { method: "POST", headers: authHeaders() }); } catch {}
                clearAuthToken();
                window.location.reload();
              }}
            >
              {locale.t('settings.account.logout')}
            </button>
          </div>
        </section>
        {/if}

        <!-- About & Updates -->
        {#if sectionVisible("about")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.about = !collapsed.about)}
          >
            {locale.t('settings.about.title')}
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.about ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.about}
          <div class="px-5 pb-5 space-y-4">
            <div class="flex items-center justify-between">
              <div>
                <p class="text-sm text-neutral-200">{locale.t("app.brand_name")}</p>
                <p class="text-xs text-neutral-500">{locale.t('settings.about.version')} {appVersion}</p>
              </div>
              <button
                onclick={() => (showAboutModal = true)}
                class="px-3 py-1.5 text-xs bg-neutral-800 hover:bg-neutral-700 text-neutral-300 rounded-lg border border-neutral-700 transition-colors cursor-pointer"
              >
                {locale.t('settings.about.about_button')}
              </button>
            </div>

            <!-- What's New -->
            <details class="rounded-lg border border-neutral-800 bg-neutral-950 overflow-hidden" ontoggle={(e) => { if ((e.target as HTMLDetailsElement).open) loadReleaseNotes(); }}>
              <summary class="px-3 py-2 text-xs font-medium text-neutral-300 hover:text-neutral-100 cursor-pointer select-none transition-colors">
                {locale.t('settings.about.whats_new').replace('{version}', appVersion)}
              </summary>
              <div class="px-3 pb-3 pt-1 text-xs text-neutral-400 space-y-2 max-h-64 overflow-y-auto">
                {#if releaseNotesLoading}
                  <div class="flex items-center gap-2 py-2">
                    <div class="w-3.5 h-3.5 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
                    <span>{locale.t('settings.about.fetching_notes')}</span>
                  </div>
                {:else if releaseNotesError}
                  <p class="text-red-400">{locale.t('settings.about.release_notes_error').replace('{error}', releaseNotesError ?? '')}</p>
                {:else if releaseNotes.length > 0}
                  {#each releaseNotes as release, i}
                    <p class="text-neutral-300 font-medium {i > 0 ? 'mt-3 pt-3 border-t border-neutral-800' : ''}">{release.version}</p>
                    <div class="release-body">
                      {@html renderReleaseBody(release.body)}
                    </div>
                  {/each}
                {:else}
                  <p class="text-neutral-500">{locale.t('settings.about.no_notes')}</p>
                {/if}
              </div>
            </details>

            <div class="space-y-2">
              {#if updateState === "idle"}
                <button
                  onclick={checkForUpdates}
                  class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors cursor-pointer"
                >
                  {locale.t('settings.about.check_updates')}
                </button>

              {:else if updateState === "checking"}
                <div class="flex items-center gap-2 text-sm text-neutral-400">
                  <div class="w-4 h-4 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
                  {locale.t('settings.about.checking_updates')}
                </div>

              {:else if updateState === "up-to-date"}
                <div class="flex items-center gap-2 text-sm text-emerald-400">
                  <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" /></svg>
                  {locale.t('settings.about.up_to_date')}
                </div>
                <button
                  onclick={checkForUpdates}
                  class="text-xs text-neutral-500 hover:text-neutral-300 transition-colors cursor-pointer"
                >
                  {locale.t('settings.about.check_again')}
                </button>

              {:else if updateState === "available"}
                <div class="px-3 py-2 bg-indigo-900/30 border border-indigo-800/50 rounded-lg">
                  <p class="text-sm text-indigo-200 mb-2">{locale.t('settings.about.version_available').replace('{version}', updateVersion)}</p>
                  {#if isBrowserMode && !updateObj}
                    {#if browserUpdateMode === "local"}
                      <p class="text-xs text-indigo-200/80 mb-2">{locale.t('settings.about.switch_to_app_mode_hint')}</p>
                      <button
                        onclick={downloadAndInstallUpdate}
                        class="px-4 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors cursor-pointer"
                      >
                        {locale.t('settings.about.switch_to_app_mode')}
                      </button>
                    {:else}
                      <p class="text-xs text-indigo-200/80">{locale.t('settings.about.redeploy_to_update')}</p>
                    {/if}
                  {:else}
                    <button
                      onclick={downloadAndInstallUpdate}
                      class="px-4 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors cursor-pointer"
                    >
                      {locale.t('settings.about.download_install')}
                    </button>
                  {/if}
                </div>

              {:else if updateState === "downloading"}
                <div class="px-3 py-2 bg-indigo-900/30 border border-indigo-800/50 rounded-lg space-y-2">
                  <div class="flex items-center justify-between text-xs text-neutral-400">
                    <span>{locale.t('settings.about.downloading_version').replace('{version}', updateVersion)}</span>
                    {#if updateTotal > 0}
                      <span class="tabular-nums">{updatePercent}%</span>
                    {/if}
                  </div>
                  <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
                    <div
                      class="bg-indigo-500 h-full rounded-full transition-[width] duration-300"
                      style="width: {updateTotal > 0 ? updatePercent : 33}%"
                      class:animate-pulse={updateTotal === 0}
                    ></div>
                  </div>
                </div>

              {:else if updateState === "ready"}
                <div class="px-3 py-2 bg-emerald-900/30 border border-emerald-800/50 rounded-lg">
                  <p class="text-sm text-emerald-200 mb-2">{locale.t('settings.about.update_ready').replace('{version}', updateVersion)}</p>
                  <button
                    onclick={async () => { try { await stopComfyui(); } catch {} if (isTauri) { const { relaunch } = await import("@tauri-apps/plugin-process"); await relaunch(); } else { window.location.reload(); } }}
                    class="px-4 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg text-sm transition-colors cursor-pointer"
                  >
                    {locale.t('updater.restart_now')}
                  </button>
                </div>

              {:else if updateState === "error"}
                <div class="px-3 py-2 bg-red-900/30 border border-red-800/50 rounded-lg">
                  <p class="text-xs text-red-200">{updateError}</p>
                </div>
                <button
                  onclick={checkForUpdates}
                  class="text-xs text-neutral-500 hover:text-neutral-300 transition-colors cursor-pointer"
                >
                  {locale.t('settings.about.try_again')}
                </button>
              {/if}
            </div>

            <!-- Troubleshooting -->
            <div class="space-y-2">
              <p class="text-xs text-neutral-400">{locale.t('settings.about.troubleshooting')}</p>
              <div class="flex items-center gap-3">
                <button
                  onclick={handleExportLogs}
                  disabled={exportingLogs}
                  class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-100 rounded-lg text-sm transition-colors cursor-pointer disabled:opacity-50"
                >
                  {#if exportingLogs}
                    {locale.t('settings.about.saving_logs')}
                  {:else}
                    {locale.t('settings.about.export_logs')}
                  {/if}
                </button>
                {#if logExportDone}
                  <span class="text-xs text-emerald-400">{locale.t('settings.about.saved')}</span>
                {/if}
              </div>
              {#if logExportError}
                <p class="text-xs text-red-400">{logExportError}</p>
              {/if}
              <p class="text-[11px] text-neutral-500">{locale.t('settings.about.export_logs_desc')}</p>
            </div>

            <div class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2">
              <p class="text-[11px] text-neutral-500">{locale.t('settings.about.data_dir_hint')}</p>
            </div>
          </div>
          {/if}
        </section>
        {/if}

        {#if generation.devModeUnlocked}
        <section class="bg-neutral-900 rounded-xl border border-amber-800/50 overflow-hidden break-inside-avoid mb-4">
          <button
            class="w-full flex items-center justify-between px-4 py-3 border-b border-amber-800/30 text-left"
            onclick={() => (collapsed.developer = !collapsed.developer)}
          >
            <span class="text-[10px] font-semibold tracking-widest text-amber-400 uppercase">{locale.t('settings.developer.title')}</span>
            <svg class="w-4 h-4 text-amber-600 transition-transform {collapsed.developer ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>
          {#if !collapsed.developer}
          <div class="p-4 space-y-3">
            <label class="flex items-center gap-3 cursor-pointer select-none">
              <input
                type="checkbox"
                class="w-4 h-4 rounded accent-amber-400"
                bind:checked={generation.devMode}
              />
              <div>
                <p class="text-xs font-medium text-neutral-200">{locale.t('settings.developer.force_checkpoints')}</p>
                <p class="text-[11px] text-neutral-500 mt-0.5">{locale.t('settings.developer.force_checkpoints_desc')}</p>
              </div>
            </label>
            <label class="flex items-center gap-3 cursor-pointer select-none">
              <input
                type="checkbox"
                class="w-4 h-4 rounded accent-amber-400"
                bind:checked={generation.showTerminalLog}
              />
              <div>
                <p class="text-xs font-medium text-neutral-200">{locale.t('settings.developer.terminal_log')}</p>
                <p class="text-[11px] text-neutral-500 mt-0.5">{locale.t('settings.developer.terminal_log_desc')}</p>
              </div>
            </label>
          </div>
          {/if}
        </section>
        {/if}

        <p class="text-[10px] text-neutral-500 break-inside-avoid"><span class="text-amber-400">*</span> {locale.t('settings.restart_required')}</p>

        {#if error}
          <div class="px-3 py-2 bg-red-900/30 border border-red-800/50 rounded-lg text-red-200 text-xs break-inside-avoid">
            {error}
          </div>
        {/if}
      {/if}
    </div>
  </div>
</div>

{#if showThemeCreatorModal}
<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4"
  onclick={(e) => { if (e.target === e.currentTarget) showThemeCreatorModal = false; }}
  onkeydown={(e) => { if (e.key === "Escape") showThemeCreatorModal = false; }}
  role="dialog"
  aria-modal="true"
  aria-labelledby="theme-creator-title"
  tabindex="-1"
>
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-full max-w-3xl max-h-[90vh] overflow-auto p-5 space-y-4">
    <div class="flex items-start justify-between gap-3">
      <div>
        <h3 id="theme-creator-title" class="text-sm font-semibold text-neutral-100">
          {draftEditingProfileId
            ? locale.t("settings.appearance.theme_modal_edit_title")
            : locale.t("settings.appearance.theme_modal_title")}
        </h3>
        <p class="text-xs text-neutral-400 mt-1">{locale.t("settings.appearance.theme_modal_desc")}</p>
      </div>
      <button
        type="button"
        class="px-2 py-1 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs cursor-pointer"
        onclick={() => { showThemeCreatorModal = false; }}
      >{locale.t("common.close")}</button>
    </div>

    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t("settings.appearance.custom_theme_name")}</label>
      <input
        type="text"
        bind:value={draftThemeName}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100"
        placeholder={locale.t("settings.appearance.custom_theme_name_placeholder")}
      />
    </div>

    <div class="rounded-lg border border-neutral-800 bg-neutral-950/60 p-3 space-y-2">
      <p class="text-xs text-neutral-300 font-medium">{locale.t("settings.appearance.theme_colors")}</p>
      <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-2 text-[11px] text-neutral-500">
        <div>{locale.t("settings.appearance.dark_preview")}</div>
        <div>{locale.t("settings.appearance.light_preview")}</div>
        <div>{locale.t("settings.appearance.link_tones")}</div>
      </div>
      {#each THEME_TONE_FIELDS as field}
        <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-2 items-start">
          <div class="rounded border border-neutral-800 bg-neutral-900 p-2 space-y-1">
            <label class="block text-[11px] text-neutral-400">{locale.t(field.labelKey)}</label>
            <div class="flex gap-2">
              <input
                type="color"
                value={draftThemeDark[field.key]}
                oninput={(event) => updateDraftTone("dark", field.key, (event.target as HTMLInputElement).value)}
                class="h-9 w-12 rounded border border-neutral-700 bg-neutral-800"
              />
              <input
                type="text"
                value={draftThemeDark[field.key]}
                onblur={(event) => updateDraftTone("dark", field.key, (event.target as HTMLInputElement).value)}
                class="flex-1 bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-xs text-neutral-100"
                placeholder="#000000"
              />
            </div>
          </div>
          <div class="rounded border border-neutral-800 bg-neutral-900 p-2 space-y-1">
            <label class="block text-[11px] text-neutral-400">{locale.t(field.labelKey)}</label>
            <div class="flex gap-2">
              <input
                type="color"
                value={draftThemeLight[field.key]}
                oninput={(event) => updateDraftTone("light", field.key, (event.target as HTMLInputElement).value)}
                class="h-9 w-12 rounded border border-neutral-700 bg-neutral-800"
              />
              <input
                type="text"
                value={draftThemeLight[field.key]}
                onblur={(event) => updateDraftTone("light", field.key, (event.target as HTMLInputElement).value)}
                class="flex-1 bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-xs text-neutral-100"
                placeholder="#ffffff"
              />
            </div>
          </div>
          <label class="inline-flex items-center gap-1 text-[11px] text-neutral-400 pt-7">
            <input type="checkbox" bind:checked={draftToneLinked[field.key]} class="accent-indigo-500" />
            <span>{locale.t("settings.appearance.link")}</span>
          </label>
        </div>
      {/each}
    </div>

    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
      <div class="rounded-lg border border-neutral-800 bg-neutral-950/60 p-3 space-y-2">
        <label class="block text-xs text-neutral-400">{locale.t("settings.appearance.background_image")}</label>
        <input type="file" accept="image/*" onchange={(event) => setDraftThemeImage("background", event)} class="block w-full text-xs text-neutral-300" />
        <label class="flex items-center justify-between text-xs text-neutral-400">
          {locale.t("settings.appearance.background_fade")}
          <span>{Math.round(draftThemeBackgroundFade * 100)}%</span>
        </label>
        <input type="range" min="0.35" max="0.95" step="0.05" bind:value={draftThemeBackgroundFade} class="w-full accent-indigo-500" />
      </div>
      <div class="rounded-lg border border-neutral-800 bg-neutral-950/60 p-3 space-y-2">
        <label class="block text-xs text-neutral-400">{locale.t("settings.appearance.logo_image")}</label>
        <input type="file" accept="image/*" onchange={(event) => setDraftThemeImage("logo", event)} class="block w-full text-xs text-neutral-300" />
        <div class="h-14 w-14 rounded-lg border border-neutral-700 bg-neutral-900 flex items-center justify-center overflow-hidden">
          {#if draftThemeLogoImage}
            <img src={draftThemeLogoImage} alt={locale.t("settings.appearance.draft_logo_alt")} class="h-full w-full object-contain" />
          {:else}
            <span class="text-[10px] text-neutral-500">{locale.t("settings.appearance.status_off")}</span>
          {/if}
        </div>
        <p class="text-[10px] text-neutral-500">{locale.t("settings.appearance.logo_crop_hint")}</p>
      </div>
    </div>

    <label class="inline-flex items-center gap-2 text-xs text-neutral-300">
      <input type="checkbox" bind:checked={draftThemeHideBranding} class="accent-indigo-500" />
      {locale.t("settings.appearance.hide_branding")}
    </label>

    <div class="flex justify-end gap-2 pt-1">
      <button
        type="button"
        class="px-4 py-2 rounded-lg bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs cursor-pointer"
        onclick={() => { showThemeCreatorModal = false; }}
      >{locale.t("common.cancel")}</button>
      <button
        type="button"
        class="px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium cursor-pointer"
        onclick={createThemeProfileFromDraft}
      >{draftEditingProfileId ? locale.t("common.save") : locale.t("settings.appearance.create_theme")}</button>
    </div>
  </div>
</div>
{/if}

{#if showLogoCropModal && pendingLogoDataUrl}
<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-4"
  onclick={(e) => { if (e.target === e.currentTarget) showLogoCropModal = false; }}
  onkeydown={(e) => { if (e.key === "Escape") showLogoCropModal = false; }}
  role="dialog"
  aria-modal="true"
  aria-labelledby="logo-crop-title"
  tabindex="-1"
>
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-full max-w-md p-5 space-y-3">
    <h3 id="logo-crop-title" class="text-sm font-semibold text-neutral-100">{locale.t("settings.appearance.logo_crop_title")}</h3>
    <p class="text-xs text-neutral-400">{locale.t("settings.appearance.logo_crop_desc")}</p>

    <div class="w-56 h-56 mx-auto rounded-xl border border-neutral-700 overflow-hidden bg-neutral-950 relative">
      <img
        src={pendingLogoDataUrl}
        alt={locale.t("settings.appearance.crop_source_alt")}
        class="absolute inset-0 w-full h-full object-cover"
        style="transform: translate({logoCropPanX * 20}%, {logoCropPanY * 20}%) scale({logoCropZoom}); transform-origin: center;"
      />
    </div>

    <div class="space-y-2">
      <label class="block text-xs text-neutral-400">
        {locale.t("settings.appearance.crop_zoom")}
        <input type="range" min="1" max="3" step="0.05" bind:value={logoCropZoom} class="w-full mt-1 accent-indigo-500" />
      </label>
      <label class="block text-xs text-neutral-400">
        {locale.t("settings.appearance.crop_horizontal")}
        <input type="range" min="-1" max="1" step="0.01" bind:value={logoCropPanX} class="w-full mt-1 accent-indigo-500" />
      </label>
      <label class="block text-xs text-neutral-400">
        {locale.t("settings.appearance.crop_vertical")}
        <input type="range" min="-1" max="1" step="0.01" bind:value={logoCropPanY} class="w-full mt-1 accent-indigo-500" />
      </label>
    </div>

    <div class="flex justify-end gap-2">
      <button
        type="button"
        class="px-4 py-2 rounded-lg bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs cursor-pointer"
        onclick={() => { showLogoCropModal = false; pendingLogoDataUrl = null; }}
      >{locale.t("common.cancel")}</button>
      <button
        type="button"
        class="px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium cursor-pointer"
        onclick={confirmLogoCrop}
      >{locale.t("settings.appearance.apply_crop")}</button>
    </div>
  </div>
</div>
{/if}

<!-- About MooshieUI Modal -->
{#if showAboutModal}
<div
  class="fixed inset-0 bg-black/70 z-50 flex items-center justify-center p-4"
  onclick={(e) => { if (e.target === e.currentTarget) showAboutModal = false; }}
  onkeydown={(e) => { if (e.key === 'Escape') showAboutModal = false; }}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-full max-w-sm p-6 space-y-5">
    <!-- Header -->
    <div class="flex items-start justify-between">
      <div>
        <h3 class="text-base font-semibold text-neutral-100">{locale.t('settings.about.modal_title')}</h3>
        <p class="text-xs text-neutral-500 mt-0.5">{locale.t('settings.about.version')} {appVersion}</p>
      </div>
      <button
        onclick={() => (showAboutModal = false)}
        class="w-7 h-7 flex items-center justify-center rounded-lg hover:bg-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors cursor-pointer"
        aria-label={locale.t('common.close')}
      >
        <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
      </button>
    </div>

    <p class="text-xs text-neutral-400">{locale.t('settings.about.modal_tagline')}</p>

    <!-- Links -->
    <div class="space-y-2">
      <button
        onclick={() => openExternalUrl('https://github.com/Mooshieblob1/MooshieUI')}
        class="w-full flex items-center gap-3 px-4 py-2.5 rounded-lg bg-neutral-800 hover:bg-neutral-700 text-neutral-200 text-sm transition-colors cursor-pointer text-left"
      >
        <svg class="w-4 h-4 shrink-0 text-neutral-400" viewBox="0 0 24 24" fill="currentColor"><path d="M12 0C5.37 0 0 5.37 0 12c0 5.3 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 21.795 24 17.295 24 12c0-6.63-5.37-12-12-12"/></svg>
        {locale.t('settings.about.github_button')}
      </button>

      <button
        onclick={() => openExternalUrl('https://gpu.garden')}
        class="w-full flex items-center gap-3 px-4 py-2.5 rounded-lg bg-neutral-800 hover:bg-neutral-700 text-neutral-200 text-sm transition-colors cursor-pointer text-left"
      >
        <svg class="w-4 h-4 shrink-0 text-neutral-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>
        {locale.t('settings.about.gpu_garden_button')}
      </button>

      <button
        onclick={() => { showAboutModal = false; showReportModal = true; }}
        class="w-full flex items-center gap-3 px-4 py-2.5 rounded-lg bg-neutral-800 hover:bg-neutral-700 text-neutral-200 text-sm transition-colors cursor-pointer text-left"
      >
        <svg class="w-4 h-4 shrink-0 text-neutral-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z"/><polyline points="22,6 12,13 2,6"/></svg>
        {locale.t('settings.about.report_issue_button')}
      </button>
    </div>
  </div>
</div>
{/if}

<!-- Report an Issue Modal -->
{#if showReportModal}
<div
  class="fixed inset-0 bg-black/70 z-50 flex items-center justify-center p-4"
  onclick={(e) => { if (e.target === e.currentTarget) showReportModal = false; }}
  onkeydown={(e) => { if (e.key === 'Escape') showReportModal = false; }}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-full max-w-md p-6 space-y-4">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <h3 class="text-base font-semibold text-neutral-100">{locale.t('settings.about.report_title')}</h3>
      <button
        onclick={() => showReportModal = false}
        class="w-7 h-7 flex items-center justify-center rounded-lg hover:bg-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors cursor-pointer"
        aria-label={locale.t('common.close')}
      >
        <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
      </button>
    </div>

    <div class="space-y-3">
      <!-- Name -->
      <div>
        <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.about.report_name_label')}</label>
        <input
          type="text"
          bind:value={reportName}
          placeholder={locale.t('settings.about.report_name_placeholder')}
          class="w-full px-3 py-2 bg-neutral-800 border border-neutral-700 rounded-lg text-sm text-neutral-100 placeholder-neutral-600 focus:outline-none focus:border-indigo-500 transition-colors"
        />
      </div>

      <!-- Email -->
      <div>
        <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.about.report_email_label')}</label>
        <input
          type="email"
          bind:value={reportEmail}
          placeholder={locale.t('settings.about.report_email_placeholder')}
          class="w-full px-3 py-2 bg-neutral-800 border border-neutral-700 rounded-lg text-sm text-neutral-100 placeholder-neutral-600 focus:outline-none focus:border-indigo-500 transition-colors"
        />
      </div>

      <!-- Message -->
      <div>
        <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.about.report_message_label')}</label>
        <textarea
          bind:value={reportMessage}
          placeholder={locale.t('settings.about.report_message_placeholder')}
          rows="5"
          class="w-full px-3 py-2 bg-neutral-800 border border-neutral-700 rounded-lg text-sm text-neutral-100 placeholder-neutral-600 focus:outline-none focus:border-indigo-500 transition-colors resize-y"
        ></textarea>
      </div>
    </div>

    <div class="flex gap-3 justify-end pt-1">
      <button
        onclick={() => showReportModal = false}
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-300 rounded-lg text-sm transition-colors cursor-pointer"
      >
        {locale.t('common.cancel')}
      </button>
      <button
        onclick={openReportInMail}
        disabled={!reportMessage.trim()}
        class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {locale.t('settings.about.report_send')}
      </button>
    </div>
  </div>
</div>
{/if}

{#if showModelManager}
  <ModelManagerModal mobileFriendly={mobileFriendly} onclose={() => (showModelManager = false)} />
{/if}

{#if mobileFriendly && showScrollToTop}
  <button
    type="button"
    onclick={scrollSettingsToTop}
    class="fixed left-4 z-40 flex h-11 w-11 items-center justify-center rounded-full border border-neutral-700 bg-neutral-900/95 text-neutral-200 shadow-lg backdrop-blur transition-colors hover:border-indigo-500 hover:bg-neutral-800 hover:text-white active:scale-95"
    style="bottom: calc(env(safe-area-inset-bottom) + 4.5rem);"
    title={locale.t("common.go_to_top")}
    aria-label={locale.t("common.go_to_top")}
  >
    <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m18 15-6-6-6 6"/></svg>
  </button>
{/if}

{#if showQualityTagsWarning}
<div class="fixed inset-0 bg-black/70 z-50 flex items-center justify-center" role="dialog">
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl p-6 max-w-md mx-4 shadow-2xl">
    <h3 class="text-sm font-semibold text-neutral-100 mb-3">{locale.t('settings.quality_warning.title')}</h3>
    <p class="text-xs text-neutral-400 mb-2">{locale.t('settings.quality_warning.body', { tags: 'masterpiece, best quality, score_9' })}</p>
    <p class="text-xs text-neutral-400 mb-4">{locale.t('settings.quality_warning.body2')}</p>
    <div class="flex gap-3 justify-end">
      <button
        onclick={() => { showQualityTagsWarning = false; }}
        class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-xs font-medium transition-colors cursor-pointer"
      >
        {locale.t('settings.quality_warning.keep')}
      </button>
      <button
        onclick={() => {
          generation.autoQualityTags = false;
          generation.saveSettings();
          showQualityTagsWarning = false;
        }}
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-400 rounded-lg text-xs transition-colors cursor-pointer"
      >
        {locale.t('settings.quality_warning.disable')}
      </button>
    </div>
  </div>
</div>
{/if}

{#if showAdvancedModeWarning}
<div class="fixed inset-0 bg-black/70 z-50 flex items-center justify-center" role="dialog">
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl p-6 max-w-md mx-4 shadow-2xl">
    <h3 class="text-sm font-semibold text-neutral-100 mb-3">{locale.t('settings.advanced_mode.warning_title')}</h3>
    <p class="text-xs text-neutral-400 mb-4">{locale.t('settings.advanced_mode.warning_body')}</p>
    <div class="flex gap-3 justify-end">
      <button
        onclick={() => { showAdvancedModeWarning = false; }}
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-400 rounded-lg text-xs transition-colors cursor-pointer"
      >
        {locale.t('settings.advanced_mode.cancel')}
      </button>
      <button
        onclick={() => {
          generation.advancedMode = true;
          generation.saveSettings();
          showAdvancedModeWarning = false;
        }}
        class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-xs font-medium transition-colors cursor-pointer"
      >
        {locale.t('settings.advanced_mode.enable')}
      </button>
    </div>
  </div>
</div>
{/if}

<!-- Add Account Modal -->
{#if showAddAccountModal}
<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
  onclick={(e) => { if (e.target === e.currentTarget) showAddAccountModal = false; }}
  onkeydown={(e) => { if (e.key === 'Escape') showAddAccountModal = false; }}
  role="dialog"
  aria-modal="true"
  aria-labelledby="add-account-title"
  tabindex="-1"
>
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-full max-w-sm p-6 space-y-4">
    <h3 id="add-account-title" class="text-sm font-medium text-neutral-100">{locale.t('settings.lan.add_lan_account')}</h3>
    <div class="space-y-3">
      <div>
        <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.lan.username')}</label>
        <input
          type="text"
          bind:value={lanNewUser}
          placeholder={locale.t('settings.lan.enter_username')}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
        />
      </div>
      <div>
        <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.lan.password')}</label>
        <input
          type="password"
          bind:value={lanNewPass}
          placeholder={locale.t('settings.lan.password_min_placeholder')}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
        />
      </div>
      {#if lanAuthError}
        <p class="text-xs text-red-400">{lanAuthError}</p>
      {/if}
    </div>
    <div class="flex justify-end gap-2 pt-2">
      <button
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-400 rounded-lg text-xs transition-colors cursor-pointer"
        onclick={() => { showAddAccountModal = false; }}
      >{locale.t('common.cancel')}</button>
      <button
        class="px-4 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {lanAuthBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
        disabled={lanAuthBusy}
        onclick={async () => { await createLanAccount(); if (!lanAuthError) showAddAccountModal = false; }}
      >{locale.t('settings.lan.create_account')}</button>
    </div>
  </div>
</div>
{/if}

<!-- Reset Password Modal (admin) -->
{#if showResetPasswordModal}
<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
  onclick={(e) => { if (e.target === e.currentTarget) showResetPasswordModal = false; }}
  onkeydown={(e) => { if (e.key === 'Escape') showResetPasswordModal = false; }}
  role="dialog"
  aria-modal="true"
  aria-labelledby="reset-password-title"
  tabindex="-1"
>
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-full max-w-sm p-6 space-y-4">
    <h3 id="reset-password-title" class="text-sm font-medium text-neutral-100">{locale.t('settings.lan.reset_password')}</h3>
    <p class="text-xs text-neutral-400">{locale.t('settings.lan.reset_password_body', { user: resetTargetUser })}</p>
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.lan.temp_password')}</label>
      <input
        type="password"
        bind:value={resetTempPass}
        placeholder={locale.t('settings.lan.password_min_placeholder')}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
        onkeydown={(e) => { if (e.key === 'Enter') adminResetPassword(); }}
      />
    </div>
    {#if resetError}
      <p class="text-xs text-red-400">{resetError}</p>
    {/if}
    {#if resetSuccess}
      <p class="text-xs text-green-400">{locale.t('settings.lan.reset_success')}</p>
    {/if}
    <div class="flex justify-end gap-2 pt-2">
      <button
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-400 rounded-lg text-xs transition-colors cursor-pointer"
        onclick={() => { showResetPasswordModal = false; }}
      >{locale.t('common.close')}</button>
      {#if !resetSuccess}
      <button
        class="px-4 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {resetBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-amber-600 hover:bg-amber-500 text-white'}"
        disabled={resetBusy}
        onclick={adminResetPassword}
      >{locale.t('settings.lan.reset_password_btn')}</button>
      {/if}
    </div>
  </div>
</div>
{/if}

<!-- Delete Account Confirmation Modal -->
{#if showDeleteModal}
<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
  onclick={(e) => { if (e.target === e.currentTarget) showDeleteModal = false; }}
  onkeydown={(e) => { if (e.key === 'Escape') showDeleteModal = false; }}
  role="dialog"
  aria-modal="true"
  aria-labelledby="delete-account-title"
  tabindex="-1"
>
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-full max-w-sm p-6 space-y-4">
    <h3 id="delete-account-title" class="text-sm font-medium text-neutral-100">{locale.t('settings.lan.delete_account')}</h3>
    <p class="text-xs text-neutral-400">{locale.t('settings.lan.delete_confirm', { user: deleteTargetUser })}</p>

    <label class="flex items-start gap-2 cursor-pointer">
      <input
        type="checkbox"
        bind:checked={deleteKeepData}
        class="mt-0.5 accent-indigo-600"
      />
      <div>
        <span class="text-xs text-neutral-300">{locale.t('settings.lan.keep_user_data')}</span>
        <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.lan.keep_user_data_desc')}</p>
      </div>
    </label>

    {#if !deleteKeepData}
      <p class="text-[10px] text-red-400 bg-red-400/10 rounded-lg px-3 py-2">
        {locale.t('settings.lan.delete_images_warning', { user: deleteTargetUser })}
      </p>
    {/if}

    <div class="flex justify-end gap-2 pt-2">
      <button
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-400 rounded-lg text-xs transition-colors cursor-pointer"
        onclick={() => { showDeleteModal = false; }}
      >{locale.t('common.cancel')}</button>
      <button
        class="px-4 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {lanAuthBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-red-600 hover:bg-red-500 text-white'}"
        disabled={lanAuthBusy}
        onclick={async () => { await deleteLanAccount(deleteTargetUser, deleteKeepData); showDeleteModal = false; }}
      >{locale.t('settings.lan.delete_account_btn')}</button>
    </div>
  </div>
</div>
{/if}

{#if showStorageModal}
<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
  onclick={(e) => { if (e.target === e.currentTarget) showStorageModal = false; }}
  onkeydown={(e) => { if (e.key === 'Escape') showStorageModal = false; }}
  role="dialog"
  tabindex="-1"
>
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl p-5 w-80 space-y-3">
    <h3 class="text-sm font-medium text-neutral-200">{locale.t('settings.lan.storage_limit_title', { user: storageTargetUser })}</h3>
    <p class="text-xs text-neutral-400">{locale.t('settings.lan.storage_limit_desc')}</p>
    <input
      type="text"
      inputmode="decimal"
      min="0.1"
      max="100"
      step="0.1"
      bind:value={storageInputGB}
      class="w-full px-3 py-2 rounded-lg bg-neutral-800 border border-neutral-700 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500"
    />
    {#if storageError}
      <p class="text-xs text-red-400">{storageError}</p>
    {/if}
    <div class="flex justify-end gap-2 pt-1">
      <button
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-400 rounded-lg text-xs transition-colors cursor-pointer"
        onclick={() => { showStorageModal = false; }}
      >{locale.t('common.cancel')}</button>
      <button
        class="px-4 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {storageBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
        disabled={storageBusy}
        onclick={applyStorageLimit}
      >{locale.t('common.save')}</button>
    </div>
  </div>
</div>
{/if}

<!-- Account Actions Modal -->
{#if showAccountActionsModal && actionsTargetAccount}
<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
  onclick={(e) => { if (e.target === e.currentTarget) showAccountActionsModal = false; }}
  onkeydown={(e) => { if (e.key === 'Escape') showAccountActionsModal = false; }}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <div class="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl w-full max-w-xs p-5 space-y-3">
    <div class="flex items-center gap-2">
      <span class="inline-block w-2 h-2 rounded-full shrink-0 {actionsTargetAccount.online ? 'bg-green-500' : 'bg-neutral-600'}"></span>
      <h3 class="text-sm font-medium text-neutral-100 truncate">{actionsTargetAccount.username}</h3>
      {#if actionsTargetAccount.role === "moderator"}
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-indigo-600/30 text-indigo-300 font-medium shrink-0">{locale.t('common.role_mod')}</span>
      {/if}
    </div>
    <div class="flex flex-col gap-2">
      {#if isAdmin}
        <button
          class="w-full px-3 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer text-left {actionsTargetAccount.role === 'moderator' ? 'bg-indigo-600/20 text-indigo-300 hover:bg-indigo-600/30' : 'bg-neutral-800 text-neutral-300 hover:bg-neutral-700'}"
          disabled={lanAuthBusy}
          onclick={() => { toggleAccountRole(actionsTargetAccount!.username, actionsTargetAccount!.role); showAccountActionsModal = false; }}
        >{actionsTargetAccount.role === "moderator" ? locale.t('settings.lan.revoke_moderator') : locale.t('settings.lan.make_moderator')}</button>
      {/if}
      <button
        class="w-full px-3 py-2 rounded-lg text-xs font-medium bg-neutral-800 text-cyan-400 hover:bg-neutral-700 transition-colors cursor-pointer text-left"
        disabled={lanAuthBusy}
        onclick={() => { storageTargetUser = actionsTargetAccount!.username; storageInputGB = locale.formatDecimalForInput(actionsTargetAccount!.storage_limit_bytes / (1024 * 1024 * 1024), 1); storageError = null; showAccountActionsModal = false; showStorageModal = true; }}
      >{locale.t('settings.lan.storage_limit_btn', { size: locale.formatBytes(actionsTargetAccount.storage_limit_bytes) })}</button>
      {#if actionsTargetAccount.role === 'user'}
      <button
        class="w-full px-3 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer text-left {actionsTargetAccount.can_use_modelhub ? 'bg-emerald-600/20 text-emerald-300 hover:bg-emerald-600/30' : 'bg-neutral-800 text-neutral-300 hover:bg-neutral-700'}"
        disabled={lanAuthBusy}
        onclick={() => { toggleModelhubAccess(actionsTargetAccount!.username, actionsTargetAccount!.can_use_modelhub); showAccountActionsModal = false; }}
      >{actionsTargetAccount.can_use_modelhub ? locale.t('settings.lan.revoke_modelhub') : locale.t('settings.lan.grant_modelhub')}</button>
      {/if}
      <button
        class="w-full px-3 py-2 rounded-lg text-xs font-medium bg-neutral-800 text-amber-400 hover:bg-neutral-700 transition-colors cursor-pointer text-left"
        disabled={lanAuthBusy}
        onclick={() => { resetTargetUser = actionsTargetAccount!.username; resetTempPass = ''; resetError = null; resetSuccess = false; showAccountActionsModal = false; showResetPasswordModal = true; }}
      >{locale.t('settings.lan.reset_password')}</button>
      <button
        class="w-full px-3 py-2 rounded-lg text-xs font-medium bg-neutral-800 text-red-400 hover:bg-neutral-700 transition-colors cursor-pointer text-left"
        disabled={lanAuthBusy}
        onclick={() => { deleteTargetUser = actionsTargetAccount!.username; deleteKeepData = true; showAccountActionsModal = false; showDeleteModal = true; }}
      >{locale.t('settings.lan.remove_account')}</button>
    </div>
    <div class="flex justify-end pt-1">
      <button
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-400 rounded-lg text-xs transition-colors cursor-pointer"
        onclick={() => { showAccountActionsModal = false; }}
      >{locale.t('common.close')}</button>
    </div>
  </div>
</div>
{/if}

{#if showPromptAssistantSetup}
  <PromptAssistantSetupModal onClose={() => (showPromptAssistantSetup = false)} />
{/if}
