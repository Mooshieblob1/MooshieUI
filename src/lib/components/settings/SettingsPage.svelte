<script lang="ts">
  import type { AppConfig } from "../../types/index.js";
  import { getConfig, updateConfig, stopComfyui, startComfyui, fetchReleaseNotes, importImageDirectory, exportLogs, getGalleryPath, setGalleryPath, setStorageLimit, installAttentionBackend, checkAttentionBackend, clearAllQueues } from "../../utils/api.js";
  import type { ReleaseNote, ImportResult, AttentionBackendStatus } from "../../utils/api.js";
  import { smoothScroll } from "../../utils/smoothScroll.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { autocomplete } from "../../stores/autocomplete.svelte.js";
  import { generation, DEFAULT_ANIMA_POSITIVE_QUALITY, DEFAULT_ANIMA_NEGATIVE_QUALITY, DEFAULT_ILLUSTRIOUS_POSITIVE_QUALITY, DEFAULT_ILLUSTRIOUS_NEGATIVE_QUALITY, DEFAULT_NANOSAUR_POSITIVE_QUALITY, DEFAULT_NANOSAUR_NEGATIVE_QUALITY } from "../../stores/generation.svelte.js";
  import { accessibility } from "../../stores/accessibility.svelte.js";
  import { locale, LOCALE_OPTIONS } from "../../stores/locale.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import OpenModelFolders from "./OpenModelFolders.svelte";
  import GpuStatusPanel from "./GpuStatusPanel.svelte";
  import { ipcInvoke, ipcListen, isTauri, isBrowserMode, authHeaders, clearAuthToken } from "../../utils/ipc.js";
  import { onMount } from "svelte";
  import { marked } from "marked";
  import { clearArtistImageCache, getArtistImageCacheCount } from "../../artist-gallery/imageCache.js";

  let { userRole = "admin" }: { userRole?: string } = $props();
  const isAdmin = $derived(userRole === "admin");
  const canManageServer = $derived(userRole === "admin" || userRole === "moderator");

  /** Open a directory picker. Returns path string or null. */
  async function openDirectoryDialog(title: string): Promise<string | null> {
    if (!isTauri) return null;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({ directory: true, multiple: false, title });
    return typeof selected === "string" ? selected : null;
  }

  // Configure marked for safe rendering (no raw HTML passthrough)
  marked.setOptions({ breaks: true, gfm: true });

  declare const __APP_VERSION__: string;
  const appVersion = __APP_VERSION__ ?? "dev";

  let config = $state<AppConfig | null>(null);
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
  let showCustomQualityTags = $state(false);

  // Attention backend state
  let attentionInstalling = $state(false);
  let attentionError = $state<string | null>(null);

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

  // LAN auth state
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

  function formatBytes(bytes: number): string {
    if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(0)} MB`;
    return `${(bytes / 1024).toFixed(0)} KB`;
  }

  async function applyStorageLimit() {
    storageBusy = true;
    storageError = null;
    try {
      const gb = parseFloat(storageInputGB);
      if (isNaN(gb) || gb < 0.1 || gb > 100) {
        storageError = "Enter a value between 0.1 and 100 GB.";
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
    if (!iso) return "Never";
    const diff = Date.now() - new Date(iso).getTime();
    if (diff < 0 || isNaN(diff)) return "Unknown";
    const sec = Math.floor(diff / 1000);
    if (sec < 60) return "Just now";
    const min = Math.floor(sec / 60);
    if (min < 60) return `${min}m ago`;
    const hrs = Math.floor(min / 60);
    if (hrs < 24) return `${hrs}h ago`;
    const days = Math.floor(hrs / 24);
    if (days < 7) return `${days}d ago`;
    const weeks = Math.floor(days / 7);
    if (weeks < 5) return `${weeks}w ago`;
    const months = Math.floor(days / 30);
    if (months < 12) return `${months}mo ago`;
    return `${Math.floor(days / 365)}y ago`;
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

  async function changeOwnPassword() {
    if (cpNewPass1.length < 4) { cpError = "New password must be at least 4 characters."; return; }
    if (cpNewPass1 !== cpNewPass2) { cpError = "Passwords do not match."; return; }
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
      if (!resp.ok) { cpError = data.error ?? "Failed to change password."; return; }
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
    if (resetTempPass.length < 4) { resetError = "Temporary password must be at least 4 characters."; return; }
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
      if (!resp.ok) { resetError = data.error ?? "Failed to reset password."; return; }
      resetSuccess = true;
      resetTempPass = "";
    } catch (e) {
      resetError = String(e);
    } finally {
      resetBusy = false;
    }
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
      lanAuthError = "Username required, password must be at least 4 characters.";
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
        lanAuthError = data.error ?? "Failed to create account.";
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
        lanAuthError = data.error ?? "Failed to delete account.";
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
        lanAuthError = data.error ?? "Failed to update role.";
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
        lanAuthError = data.error ?? "Failed to update Model Hub access.";
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
    const newMode = !config.browser_mode;
    console.log("[switchUiMode] isTauri:", isTauri, "isBrowserMode:", isBrowserMode, "config.browser_mode:", config.browser_mode, "newMode:", newMode);
    try {
      if (isTauri && newMode) {
        // App → Browser: call backend to start web server, open browser, hide window
        console.log("[switchUiMode] calling switch_to_browser_mode via Tauri invoke...");
        await ipcInvoke("switch_to_browser_mode");
        console.log("[switchUiMode] switch_to_browser_mode succeeded");
        config.browser_mode = true;
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
        switchingMode = true; // keep the message visible
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
    }
  }

  // Gallery path state
  let galleryPathDisplay = $state("");
  let galleryPathSaving = $state(false);
  let galleryPathMessage = $state<string | null>(null);

  async function handleExportLogs() {
    let destination: string | null = null;
    if (isTauri) {
      const { save: saveDialog } = await import("@tauri-apps/plugin-dialog");
      destination = await saveDialog({
        title: locale.t('settings.about.save_dialog_title'),
        defaultPath: "mooshieui-diagnostics.log",
        filters: [{ name: "Log Files", extensions: ["log", "txt"] }],
      });
    }
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
    return marked.parse(cleaned, { async: false }) as string;
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
    moveProgress = "Starting move...";

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
      moveError = typeof e === "string" ? e : e.message || "Unknown error";
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
  let updateState = $state<UpdateCheckState>("idle");
  let updateVersion = $state("");
  let updateError = $state("");
  let updateDownloaded = $state(0);
  let updateTotal = $state(0);
  let updateObj: Awaited<ReturnType<typeof check>> | null = null;

  const updatePercent = $derived(updateTotal > 0 ? Math.round((updateDownloaded / updateTotal) * 100) : 0);

  async function checkForUpdates() {
    updateState = "checking";
    updateError = "";
    try {
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
    if (!updateObj) return;
    updateState = "downloading";
    try {
      await updateObj.downloadAndInstall((event) => {
        if (event.event === "Started") {
          updateTotal = event.data.contentLength ?? 0;
          updateDownloaded = 0;
        } else if (event.event === "Progress") {
          updateDownloaded += event.data.chunkLength;
        } else if (event.event === "Finished") {
          updateState = "ready";
        }
      });
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
      paths: false,
      autocomplete: false,
      interrogator: false,
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
    { key: "connection", label: "Connection", keywords: "server mode url port remote autolaunch" },
    { key: "appearance", label: "Appearance", keywords: "theme dark light font scale size style presets fooocus" },
    { key: "performance", label: "Performance", keywords: "vram mode high low normal keep alive close attention backend sage flash" },
    { key: "quality", label: "Quality Tags", keywords: "quality tags auto masterpiece best quality anima illustrious noobai pony nanosaur positive negative prompt" },
    { key: "gpu", label: "GPU Workers", keywords: "gpu vram worker backend multi status utilization temperature power nvidia" },
    { key: "paths", label: "Paths", keywords: "comfyui install venv python cli arguments extra args shared model directory models" },
    { key: "gallery", label: "Gallery", keywords: "import images output directory swarmui comfyui external folder manual save mode save directory artist cache clear anima preview" },
    { key: "autocomplete", label: "Autocomplete", keywords: "tags taglist suggestions results url upload csv json danbooru" },
    { key: "interrogator", label: "Interrogator", keywords: "interrogate tags tagger threshold confidence onnx model" },
    { key: "civitai", label: "CivitAI", keywords: "civitai api key metadata model hub image fetch download authentication" },
    { key: "about", label: "About", keywords: "version update check updates about troubleshooting logs export diagnostic" },
  ];

  function sectionVisible(key: string): boolean {
    if (!search.trim()) return true;
    const s = sections.find((sec) => sec.key === key);
    if (!s) return false;
    const q = search.toLowerCase();
    return s.label.toLowerCase().includes(q) || s.keywords.includes(q);
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
  }

  onMount(async () => {
    try {
      await loadConfig();
    } catch (e) {
      error = `Failed to load config: ${e}`;
    } finally {
      loading = false;
    }
    loadInstallPath();
    getGalleryPath().then(p => { galleryPathDisplay = p; }).catch(() => {});
    void loadCacheCount();
    if (isBrowserMode) {
      loadLanAccounts();
      loadLanInfo();
    }
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
    try {
      await updateConfig(config);
    } catch (e) {
      error = `Failed to save: ${e}`;
    }
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
      attentionError = typeof e === "string" ? e : e.message || "Installation failed";
      config.attention_backend = previousBackend;
    } finally {
      attentionInstalling = false;
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
      setTimeout(() => (saved = false), 2000);
    } catch (e) {
      error = `Failed to save: ${e}`;
    } finally {
      saving = false;
    }
  }

  function applyTheme(theme: string) {
    document.documentElement.classList.toggle("light", theme === "light");
  }

  function applyFontScale(scale: number) {
    document.documentElement.style.setProperty("--font-scale", String(scale));
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
  <div class="flex-1 overflow-y-auto p-6" use:smoothScroll>
    <div class="columns-1 lg:columns-2 xl:columns-3 gap-4">
      {#if loading}
        <div class="flex items-center justify-center py-12 text-neutral-500">
          <div class="w-6 h-6 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin"></div>
        </div>
      {:else if config}
        <!-- Browser / App Mode Switch (admin only) -->
        {#if isAdmin}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <div class="p-5 space-y-3">
            <div class="flex items-center justify-between">
              <div>
                <h3 class="text-sm font-medium text-neutral-200">
                  {config.browser_mode ? "Web Browser Mode" : "App Mode"}
                </h3>
                <p class="text-xs text-neutral-500 mt-0.5">
                  {config.browser_mode
                    ? "UI runs in your web browser. Switch to use the native app window."
                    : "UI runs in the native app window. Switch to use your web browser."}
                </p>
              </div>
              <button
                class="px-4 py-2 text-sm font-medium rounded-lg transition-colors {config.browser_mode
                  ? 'bg-indigo-600 hover:bg-indigo-500 text-white'
                  : 'bg-neutral-700 hover:bg-neutral-600 text-neutral-200'}"
                onclick={switchUiMode}
              >
                {config.browser_mode ? "Switch to App Mode" : "Switch to Web Browser Mode"}
              </button>
            </div>
            {#if switchingMode}
              <p class="text-xs text-amber-400">
                {config.browser_mode
                  ? "Switched to app mode. The app window should now be visible — you can close this browser tab."
                  : "Switching to browser mode..."}
              </p>
            {/if}
            {#if config.browser_mode}
              <div class="flex items-center justify-between pt-2 border-t border-neutral-800">
                <div>
                  <label class="text-xs text-neutral-300 font-medium">Enable LAN Access</label>
                  <p class="text-xs text-neutral-500 mt-0.5">
                    Allow other devices on your network to access the UI. Requires authentication when enabled.
                  </p>
                </div>
                <label class="relative inline-flex items-center cursor-pointer">
                  <input
                    type="checkbox"
                    bind:checked={config.lan_enabled}
                    onchange={() => { checkRestartNeeded(); autoSave(); }}
                    class="sr-only peer"
                  />
                  <div class="w-9 h-5 bg-neutral-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-neutral-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-indigo-600"></div>
                </label>
              </div>
              {#if config.lan_enabled}
                <div class="space-y-3 pt-2 border-t border-neutral-800">
                  <p class="text-xs text-amber-400">
                    Warning: Enabling LAN access exposes the UI to your local network. Add at least one account to require authentication.
                  </p>

                  <!-- LAN address -->
                  {#if lanAddresses.length > 0}
                    <div class="bg-neutral-800 rounded-lg px-3 py-2">
                      <p class="text-xs text-neutral-400 mb-1">Access from other devices at:</p>
                      {#each lanAddresses as addr}
                        <p class="text-sm text-indigo-400 font-mono select-all">{addr}</p>
                      {/each}
                    </div>
                  {/if}

                  <!-- Existing accounts -->
                  {#if lanAccounts.length > 0}
                    <div class="space-y-2">
                      <div class="flex items-center justify-between">
                        <p class="text-xs text-neutral-400 font-medium">Accounts</p>
                        <p class="text-[10px] text-neutral-500">{sortedAccounts.length} of {lanAccounts.length}</p>
                      </div>

                      <!-- Search -->
                      <input
                        type="text"
                        placeholder="Search accounts…"
                        bind:value={accountSearch}
                        class="w-full px-3 py-1.5 rounded-lg bg-neutral-800 border border-neutral-700 text-xs text-neutral-200 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
                      />

                      <!-- Sort buttons -->
                      <div class="flex gap-1">
                        {#each [["name", "Name"], ["joined", "Joined"], ["last_online", "Last Online"]] as [key, label]}
                          <button
                            class="text-[10px] px-2 py-1 rounded cursor-pointer transition-colors {accountSort === key ? 'bg-indigo-600/30 text-indigo-300' : 'bg-neutral-800 text-neutral-400 hover:text-neutral-300'}"
                            onclick={() => { if (accountSort === key) { accountSortAsc = !accountSortAsc; } else { accountSort = key as typeof accountSort; accountSortAsc = true; } }}
                          >{label} {accountSort === key ? (accountSortAsc ? '↑' : '↓') : ''}</button>
                        {/each}
                      </div>

                      <!-- Scrollable account list (max 6 visible) -->
                      <div class="max-h-[288px] overflow-y-auto space-y-1 pr-1">
                        {#each sortedAccounts as account}
                          <div class="flex items-center justify-between bg-neutral-800 rounded-lg px-3 py-2">
                            <div class="flex items-center gap-2 min-w-0">
                              <span class="inline-block w-2 h-2 rounded-full shrink-0 {account.online ? 'bg-green-500' : 'bg-neutral-600'}"></span>
                              <span class="text-sm text-neutral-200 truncate" title={account.username}>{account.username}</span>
                              {#if account.role === "moderator"}
                                <span class="text-[10px] px-1.5 py-0.5 rounded bg-indigo-600/30 text-indigo-300 font-medium shrink-0">Mod</span>
                              {/if}
                              <span class="text-[10px] text-neutral-500 shrink-0">{formatBytes(account.storage_limit_bytes)}</span>
                              <span class="text-[10px] text-neutral-500 shrink-0" title={account.created_at ? `Joined: ${new Date(account.created_at).toLocaleDateString()}` : ''}>
                                {account.created_at ? relativeTime(account.created_at) : ''}
                              </span>
                              {#if !account.online && account.last_online}
                                <span class="text-[10px] text-neutral-600 shrink-0" title={`Last online: ${new Date(account.last_online).toLocaleString()}`}>
                                  · {relativeTime(account.last_online)}
                                </span>
                              {/if}
                            </div>
                            <button
                              class="shrink-0 ml-2 p-1 rounded hover:bg-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors cursor-pointer"
                              title="Manage {account.username}"
                              onclick={() => { actionsTargetAccount = account; showAccountActionsModal = true; }}
                            >
                              <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M11.49 3.17c-.38-1.56-2.6-1.56-2.98 0a1.532 1.532 0 01-2.286.948c-1.372-.836-2.942.734-2.106 2.106.54.886.061 2.042-.947 2.287-1.561.379-1.561 2.6 0 2.978a1.532 1.532 0 01.947 2.287c-.836 1.372.734 2.942 2.106 2.106a1.532 1.532 0 012.287.947c.379 1.561 2.6 1.561 2.978 0a1.533 1.533 0 012.287-.947c1.372.836 2.942-.734 2.106-2.106a1.533 1.533 0 01.947-2.287c1.561-.379 1.561-2.6 0-2.978a1.532 1.532 0 01-.947-2.287c.836-1.372-.734-2.942-2.106-2.106a1.532 1.532 0 01-2.287-.947zM10 13a3 3 0 100-6 3 3 0 000 6z" clip-rule="evenodd"/></svg>
                            </button>
                          </div>
                        {/each}
                      </div>
                    </div>
                  {:else}
                    <p class="text-xs text-neutral-500">No accounts yet — anyone on your network can access the UI without authentication.</p>
                  {/if}

                  <!-- Add account button -->
                  <button
                    class="w-full px-3 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {lanAuthBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
                    disabled={lanAuthBusy}
                    onclick={() => { lanNewUser = ''; lanNewPass = ''; lanAuthError = null; showAddAccountModal = true; }}
                  >+ Add Account</button>
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
            <h3 class="text-sm font-medium text-neutral-200">Account Management</h3>
            <p class="text-xs text-neutral-500">Manage user accounts. You can reset passwords and remove accounts (except admin and moderator accounts).</p>

            {#if lanAccounts.length > 0}
              <div class="space-y-2">
                <div class="flex items-center justify-between">
                  <p class="text-xs text-neutral-400 font-medium">Accounts</p>
                  <p class="text-[10px] text-neutral-500">{sortedAccounts.length} of {lanAccounts.length}</p>
                </div>

                <input
                  type="text"
                  placeholder="Search accounts…"
                  bind:value={accountSearch}
                  class="w-full px-3 py-1.5 rounded-lg bg-neutral-800 border border-neutral-700 text-xs text-neutral-200 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
                />

                <div class="flex gap-1">
                  {#each [["name", "Name"], ["joined", "Joined"], ["last_online", "Last Online"]] as [key, label]}
                    <button
                      class="text-[10px] px-2 py-1 rounded cursor-pointer transition-colors {accountSort === key ? 'bg-indigo-600/30 text-indigo-300' : 'bg-neutral-800 text-neutral-400 hover:text-neutral-300'}"
                      onclick={() => { if (accountSort === key) { accountSortAsc = !accountSortAsc; } else { accountSort = key as typeof accountSort; accountSortAsc = true; } }}
                    >{label} {accountSort === key ? (accountSortAsc ? '↑' : '↓') : ''}</button>
                  {/each}
                </div>

                <div class="max-h-[288px] overflow-y-auto space-y-1 pr-1">
                  {#each sortedAccounts as account}
                    <div class="flex items-center justify-between bg-neutral-800 rounded-lg px-3 py-2">
                      <div class="flex items-center gap-2 min-w-0">
                        <span class="inline-block w-2 h-2 rounded-full shrink-0 {account.online ? 'bg-green-500' : 'bg-neutral-600'}"></span>
                        <span class="text-sm text-neutral-200 truncate" title={account.username}>{account.username}</span>
                        {#if account.role === "moderator"}
                          <span class="text-[10px] px-1.5 py-0.5 rounded bg-indigo-600/30 text-indigo-300 font-medium shrink-0">Mod</span>
                        {/if}
                        {#if account.role === "admin"}
                          <span class="text-[10px] px-1.5 py-0.5 rounded bg-amber-600/30 text-amber-300 font-medium shrink-0">Admin</span>
                        {/if}
                        {#if account.role === "user"}
                          <span class="text-[10px] text-neutral-500 shrink-0">{formatBytes(account.storage_limit_bytes)}</span>
                        {/if}
                        <span class="text-[10px] text-neutral-500 shrink-0" title={account.created_at ? `Joined: ${new Date(account.created_at).toLocaleDateString()}` : ''}>
                          {account.created_at ? relativeTime(account.created_at) : ''}
                        </span>
                        {#if !account.online && account.last_online}
                          <span class="text-[10px] text-neutral-600 shrink-0" title={`Last online: ${new Date(account.last_online).toLocaleString()}`}>
                            · {relativeTime(account.last_online)}
                          </span>
                        {/if}
                      </div>
                      {#if account.role === "user"}
                        <button
                          class="shrink-0 ml-2 p-1 rounded hover:bg-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors cursor-pointer"
                          title="Manage {account.username}"
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
              <p class="text-xs text-neutral-500">No accounts found.</p>
            {/if}

            <!-- Add account button (moderators can create accounts too) -->
            <button
              class="w-full px-3 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {lanAuthBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
              disabled={lanAuthBusy}
              onclick={() => { lanNewUser = ''; lanNewPass = ''; lanAuthError = null; showAddAccountModal = true; }}
            >+ Add Account</button>

            {#if lanAuthError}
              <p class="text-xs text-red-400 mt-1">{lanAuthError}</p>
            {/if}
          </div>
        </section>
        {/if}

        <!-- Queue Management (admin / moderator in browser mode) -->
        {#if canManageServer && isBrowserMode}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <div class="p-5 space-y-3">
            <h3 class="text-sm font-medium text-neutral-200">Queue Management</h3>
            <p class="text-xs text-neutral-500">Clear all pending and active generations. This interrupts everyone's in-progress generation.</p>
            {#if clearQueueError}
              <p class="text-xs text-red-400">{clearQueueError}</p>
            {/if}
            {#if clearQueueDone}
              <p class="text-xs text-green-400">Queue cleared.</p>
            {/if}
            {#if showClearQueueConfirm}
              <p class="text-xs text-amber-300">This will interrupt all active and queued generations. Are you sure?</p>
              <div class="flex gap-2">
                <button
                  class="flex-1 py-2 rounded-lg text-xs font-medium bg-neutral-700 hover:bg-neutral-600 text-neutral-300 transition-colors cursor-pointer"
                  onclick={() => (showClearQueueConfirm = false)}
                >Cancel</button>
                <button
                  class="flex-1 py-2 rounded-lg text-xs font-medium bg-red-600 hover:bg-red-500 text-white transition-colors cursor-pointer disabled:opacity-50"
                  disabled={clearQueueBusy}
                  onclick={handleClearQueue}
                >{clearQueueBusy ? "Clearing…" : "Yes, Clear Queue"}</button>
              </div>
            {:else}
              <button
                class="w-full py-2 rounded-lg text-xs font-medium bg-red-600/20 hover:bg-red-600/40 text-red-300 border border-red-800/50 transition-colors cursor-pointer"
                onclick={() => { clearQueueError = null; showClearQueueConfirm = true; }}
              >Clear Queue</button>
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

          {#if config.server_mode === "autolaunch"}
          <div class="flex items-center justify-between">
            <div>
              <p class="text-sm text-neutral-200">{locale.t('settings.connection.auto_start')}</p>
              <p class="text-xs text-neutral-500">{locale.t('settings.connection.auto_start_desc')}</p>
            </div>
            <button
              class="w-10 h-5 rounded-full transition-colors cursor-pointer {config.auto_start !== false ? 'bg-indigo-600' : 'bg-neutral-700'}"
              onclick={() => { config.auto_start = config.auto_start === false; autoSave(); }}
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
                placeholder="http://127.0.0.1:8188"
              />
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
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.appearance.theme')}</label>
              <select
                bind:value={config.theme}
                onchange={() => { if (config) { applyTheme(config.theme); autoSave(); } }}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
              >
                <option value="dark">{locale.t('settings.appearance.theme_dark')}</option>
                <option value="light">{locale.t('settings.appearance.theme_light')}</option>
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
              <option value="sage_v1">{locale.t('settings.performance.attention_sage_v1')}</option>
              <option value="sage_v2">{locale.t('settings.performance.attention_sage_v2')}</option>
              <option value="flash_v1">{locale.t('settings.performance.attention_flash_v1')}</option>
              <option value="flash_v2">{locale.t('settings.performance.attention_flash_v2')}</option>
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
          </div>

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

          {#if generation.autoQualityTags}
          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="custom-quality-tags"
              bind:checked={showCustomQualityTags}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="custom-quality-tags" class="text-sm text-neutral-200">{locale.t('settings.performance.custom_quality_tags')}</label>
              <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t('settings.performance.custom_quality_tags_desc')}</p>
            </div>
          </div>

          {#if showCustomQualityTags}
          <div class="rounded-lg border border-amber-500/20 bg-neutral-950/50 p-3 space-y-3">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2">
                <svg class="w-3.5 h-3.5 text-amber-400/80 shrink-0" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
                <p class="text-[10px] text-amber-400/80">{locale.t('settings.performance.quality_tags_warning')}</p>
              </div>
              <button
                onclick={() => {
                  generation.customAnimaPositiveQuality = DEFAULT_ANIMA_POSITIVE_QUALITY;
                  generation.customAnimaNegativeQuality = DEFAULT_ANIMA_NEGATIVE_QUALITY;
                  generation.customIllustriousPositiveQuality = DEFAULT_ILLUSTRIOUS_POSITIVE_QUALITY;
                  generation.customIllustriousNegativeQuality = DEFAULT_ILLUSTRIOUS_NEGATIVE_QUALITY;
                  generation.customNanosaurPositiveQuality = DEFAULT_NANOSAUR_POSITIVE_QUALITY;
                  generation.customNanosaurNegativeQuality = DEFAULT_NANOSAUR_NEGATIVE_QUALITY;
                  generation.saveSettings();
                }}
                class="text-[10px] text-indigo-400 hover:text-indigo-300 transition-colors cursor-pointer whitespace-nowrap ml-3"
              >
                {locale.t('settings.performance.reset_defaults')}
              </button>
            </div>

            <!-- Anima Tags -->
            <div class="space-y-1.5">
              <p class="text-[10px] text-neutral-500 font-medium uppercase tracking-wide">{locale.t('settings.performance.anima')}</p>
              <div>
                <label for="anima-pos-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.positive')}</label>
                <textarea
                  id="anima-pos-quality"
                  bind:value={generation.customAnimaPositiveQuality}
                  onblur={() => generation.saveSettings()}
                  rows="2"
                  class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
                  placeholder="masterpiece, best quality, ..."
                ></textarea>
              </div>
              <div>
                <label for="anima-neg-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.negative')}</label>
                <textarea
                  id="anima-neg-quality"
                  bind:value={generation.customAnimaNegativeQuality}
                  onblur={() => generation.saveSettings()}
                  rows="2"
                  class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
                  placeholder="worst quality, low quality, ..."
                ></textarea>
              </div>
            </div>

            <!-- Illustrious/NoobAI Tags -->
            <div class="space-y-1.5">
              <p class="text-[10px] text-neutral-500 font-medium uppercase tracking-wide">{locale.t('settings.performance.illustrious')}</p>
              <div>
                <label for="illustrious-pos-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.positive')}</label>
                <textarea
                  id="illustrious-pos-quality"
                  bind:value={generation.customIllustriousPositiveQuality}
                  onblur={() => generation.saveSettings()}
                  rows="2"
                  class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
                  placeholder="best quality, masterpiece, ..."
                ></textarea>
              </div>
              <div>
                <label for="illustrious-neg-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.negative')}</label>
                <textarea
                  id="illustrious-neg-quality"
                  bind:value={generation.customIllustriousNegativeQuality}
                  onblur={() => generation.saveSettings()}
                  rows="2"
                  class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
                  placeholder="worst quality, bad quality, ..."
                ></textarea>
              </div>
            </div>

            <!-- Nanosaur Tags -->
            <div class="space-y-1.5">
              <p class="text-[10px] text-neutral-500 font-medium uppercase tracking-wide">{locale.t('settings.performance.nanosaur')}</p>
              <div>
                <label for="nanosaur-pos-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.positive')}</label>
                <textarea
                  id="nanosaur-pos-quality"
                  bind:value={generation.customNanosaurPositiveQuality}
                  onblur={() => generation.saveSettings()}
                  rows="2"
                  class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
                  placeholder="best quality, masterpiece, ..."
                ></textarea>
              </div>
              <div>
                <label for="nanosaur-neg-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.negative')}</label>
                <textarea
                  id="nanosaur-neg-quality"
                  bind:value={generation.customNanosaurNegativeQuality}
                  onblur={() => generation.saveSettings()}
                  rows="2"
                  class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
                  placeholder="worst quality, low quality, ..."
                ></textarea>
              </div>
            </div>
          </div>
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
            GPU Workers
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.gpu ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.gpu}
          <div class="px-5 pb-5">
            <GpuStatusPanel />
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

          <div>
            <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.paths.comfyui_install')}</label>
            <input
              type="text"
              bind:value={config.comfyui_path}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              placeholder="/path/to/ComfyUI"
            />
          </div>

          <div>
            <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.paths.venv')}</label>
            <input
              type="text"
              bind:value={config.venv_path}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              placeholder="/path/to/venv"
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
              placeholder="--fp16 --force-channels-last"
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
            {#if isAdmin}
            <!-- Current source -->
            <div>
              <label class="block text-xs text-neutral-400 mb-1">{locale.t('settings.autocomplete.tag_source')}</label>
              <div class="flex items-center gap-2 text-sm text-neutral-300">
                {#if autocomplete.sourceMode === "builtin"}
                  <span class="inline-block w-2 h-2 rounded-full bg-indigo-400"></span>
                  {locale.t('settings.autocomplete.source_builtin')} ({autocomplete.tags.length.toLocaleString()} {locale.t('settings.autocomplete.tags_count')})
                {:else if autocomplete.sourceMode === "url"}
                  <span class="inline-block w-2 h-2 rounded-full bg-green-400"></span>
                  URL: <span class="text-neutral-400 truncate max-w-xs">{autocomplete.sourceUrl}</span>
                  ({autocomplete.tags.length.toLocaleString()} tags)
                {:else if autocomplete.sourceMode === "file"}
                  <span class="inline-block w-2 h-2 rounded-full bg-green-400"></span>
                  File: {autocomplete.sourceFileName}
                  ({autocomplete.tags.length.toLocaleString()} tags)
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
                <span class="text-neutral-300">{config.interrogator_general_threshold.toFixed(2)}</span>
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
                <span class="text-neutral-300">{config.interrogator_character_threshold.toFixed(2)}</span>
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

        <!-- Account / Change Password (browser mode non-admin users) -->
        {#if isBrowserMode && !isAdmin}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden break-inside-avoid mb-4">
          <div class="p-5 space-y-3">
            <h3 class="text-sm font-medium text-neutral-200">Account</h3>
            <button
              class="w-full py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer bg-neutral-800 hover:bg-neutral-700 text-neutral-300 border border-neutral-700"
              onclick={() => {
                showChangePasswordForm = !showChangePasswordForm;
                cpError = null;
                cpSuccess = false;
              }}
            >
              {showChangePasswordForm ? "Cancel" : "Change Password"}
            </button>
            {#if showChangePasswordForm}
            <div class="space-y-2">
              <input
                type="password"
                bind:value={cpCurrentPass}
                placeholder="Current password"
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              />
              <input
                type="password"
                bind:value={cpNewPass1}
                placeholder="New password (4+ characters)"
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              />
              <input
                type="password"
                bind:value={cpNewPass2}
                placeholder="Confirm new password"
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                onkeydown={(e) => { if (e.key === "Enter") changeOwnPassword(); }}
              />
              {#if cpError}
                <p class="text-xs text-red-400">{cpError}</p>
              {/if}
              {#if cpSuccess}
                <p class="text-xs text-green-400">Password changed successfully.</p>
              {/if}
              <button
                class="w-full py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {cpBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
                disabled={cpBusy}
                onclick={changeOwnPassword}
              >
                {cpBusy ? "Saving..." : "Confirm Change"}
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
                <p class="text-sm text-neutral-200">MooshieUI</p>
                <p class="text-xs text-neutral-500">{locale.t('settings.about.version')} {appVersion}</p>
              </div>
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
                  <button
                    onclick={downloadAndInstallUpdate}
                    class="px-4 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors cursor-pointer"
                  >
                    {locale.t('settings.about.download_install')}
                  </button>
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
            <span class="text-[10px] font-semibold tracking-widest text-amber-400 uppercase">Developer</span>
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
                <p class="text-xs font-medium text-neutral-200">Force-show checkpoints tab</p>
                <p class="text-[11px] text-neutral-500 mt-0.5">Shows the Checkpoints tab in the bottom panel even when fewer than 10 checkpoints are installed.</p>
              </div>
            </label>
          </div>
          {/if}
        </section>
        {/if}

        <p class="text-[10px] text-neutral-500 break-inside-avoid"><span class="text-amber-400">*</span> Requires a restart of ComfyUI to take effect.</p>

        {#if error}
          <div class="px-3 py-2 bg-red-900/30 border border-red-800/50 rounded-lg text-red-200 text-xs break-inside-avoid">
            {error}
          </div>
        {/if}
      {/if}
    </div>
  </div>
</div>

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
    <h3 id="add-account-title" class="text-sm font-medium text-neutral-100">Add LAN Account</h3>
    <div class="space-y-3">
      <div>
        <label class="block text-xs text-neutral-400 mb-1">Username</label>
        <input
          type="text"
          bind:value={lanNewUser}
          placeholder="Enter username"
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
        />
      </div>
      <div>
        <label class="block text-xs text-neutral-400 mb-1">Password</label>
        <input
          type="password"
          bind:value={lanNewPass}
          placeholder="At least 4 characters"
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
      >Cancel</button>
      <button
        class="px-4 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {lanAuthBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
        disabled={lanAuthBusy}
        onclick={async () => { await createLanAccount(); if (!lanAuthError) showAddAccountModal = false; }}
      >Create Account</button>
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
    <h3 id="reset-password-title" class="text-sm font-medium text-neutral-100">Reset Password</h3>
    <p class="text-xs text-neutral-400">Set a temporary password for <span class="text-neutral-200 font-medium">{resetTargetUser}</span>. They will be asked to choose a new password on their next login.</p>
    <div>
      <label class="block text-xs text-neutral-400 mb-1">Temporary Password</label>
      <input
        type="password"
        bind:value={resetTempPass}
        placeholder="At least 4 characters"
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
        onkeydown={(e) => { if (e.key === 'Enter') adminResetPassword(); }}
      />
    </div>
    {#if resetError}
      <p class="text-xs text-red-400">{resetError}</p>
    {/if}
    {#if resetSuccess}
      <p class="text-xs text-green-400">Password reset. The user will be prompted to set a new password on their next login.</p>
    {/if}
    <div class="flex justify-end gap-2 pt-2">
      <button
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-400 rounded-lg text-xs transition-colors cursor-pointer"
        onclick={() => { showResetPasswordModal = false; }}
      >Close</button>
      {#if !resetSuccess}
      <button
        class="px-4 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {resetBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-amber-600 hover:bg-amber-500 text-white'}"
        disabled={resetBusy}
        onclick={adminResetPassword}
      >Reset Password</button>
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
    <h3 id="delete-account-title" class="text-sm font-medium text-neutral-100">Delete Account</h3>
    <p class="text-xs text-neutral-400">Are you sure you want to delete <span class="text-neutral-200 font-medium">{deleteTargetUser}</span>?</p>

    <label class="flex items-start gap-2 cursor-pointer">
      <input
        type="checkbox"
        bind:checked={deleteKeepData}
        class="mt-0.5 accent-indigo-600"
      />
      <div>
        <span class="text-xs text-neutral-300">Keep user data (gallery images)</span>
        <p class="text-[10px] text-neutral-500 mt-0.5">Re-creating an account with this name will restore access to their images.</p>
      </div>
    </label>

    {#if !deleteKeepData}
      <p class="text-[10px] text-red-400 bg-red-400/10 rounded-lg px-3 py-2">
        This will permanently delete all of {deleteTargetUser}'s generated images. This cannot be undone.
      </p>
    {/if}

    <div class="flex justify-end gap-2 pt-2">
      <button
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-400 rounded-lg text-xs transition-colors cursor-pointer"
        onclick={() => { showDeleteModal = false; }}
      >Cancel</button>
      <button
        class="px-4 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {lanAuthBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-red-600 hover:bg-red-500 text-white'}"
        disabled={lanAuthBusy}
        onclick={async () => { await deleteLanAccount(deleteTargetUser, deleteKeepData); showDeleteModal = false; }}
      >Delete Account</button>
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
    <h3 class="text-sm font-medium text-neutral-200">Storage Limit — {storageTargetUser}</h3>
    <p class="text-xs text-neutral-400">Set the maximum gallery storage for this user (in GB).</p>
    <input
      type="number"
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
      >Cancel</button>
      <button
        class="px-4 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer {storageBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
        disabled={storageBusy}
        onclick={applyStorageLimit}
      >Save</button>
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
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-indigo-600/30 text-indigo-300 font-medium shrink-0">Mod</span>
      {/if}
    </div>
    <div class="flex flex-col gap-2">
      {#if isAdmin}
        <button
          class="w-full px-3 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer text-left {actionsTargetAccount.role === 'moderator' ? 'bg-indigo-600/20 text-indigo-300 hover:bg-indigo-600/30' : 'bg-neutral-800 text-neutral-300 hover:bg-neutral-700'}"
          disabled={lanAuthBusy}
          onclick={() => { toggleAccountRole(actionsTargetAccount!.username, actionsTargetAccount!.role); showAccountActionsModal = false; }}
        >{actionsTargetAccount.role === "moderator" ? "Revoke Moderator" : "Make Moderator"}</button>
      {/if}
      <button
        class="w-full px-3 py-2 rounded-lg text-xs font-medium bg-neutral-800 text-cyan-400 hover:bg-neutral-700 transition-colors cursor-pointer text-left"
        disabled={lanAuthBusy}
        onclick={() => { storageTargetUser = actionsTargetAccount!.username; storageInputGB = (actionsTargetAccount!.storage_limit_bytes / (1024 * 1024 * 1024)).toFixed(1); storageError = null; showAccountActionsModal = false; showStorageModal = true; }}
      >Storage Limit — {formatBytes(actionsTargetAccount.storage_limit_bytes)}</button>
      {#if actionsTargetAccount.role === 'user'}
      <button
        class="w-full px-3 py-2 rounded-lg text-xs font-medium transition-colors cursor-pointer text-left {actionsTargetAccount.can_use_modelhub ? 'bg-emerald-600/20 text-emerald-300 hover:bg-emerald-600/30' : 'bg-neutral-800 text-neutral-300 hover:bg-neutral-700'}"
        disabled={lanAuthBusy}
        onclick={() => { toggleModelhubAccess(actionsTargetAccount!.username, actionsTargetAccount!.can_use_modelhub); showAccountActionsModal = false; }}
      >{actionsTargetAccount.can_use_modelhub ? "Revoke Model Hub Access" : "Grant Model Hub Access"}</button>
      {/if}
      <button
        class="w-full px-3 py-2 rounded-lg text-xs font-medium bg-neutral-800 text-amber-400 hover:bg-neutral-700 transition-colors cursor-pointer text-left"
        disabled={lanAuthBusy}
        onclick={() => { resetTargetUser = actionsTargetAccount!.username; resetTempPass = ''; resetError = null; resetSuccess = false; showAccountActionsModal = false; showResetPasswordModal = true; }}
      >Reset Password</button>
      <button
        class="w-full px-3 py-2 rounded-lg text-xs font-medium bg-neutral-800 text-red-400 hover:bg-neutral-700 transition-colors cursor-pointer text-left"
        disabled={lanAuthBusy}
        onclick={() => { deleteTargetUser = actionsTargetAccount!.username; deleteKeepData = true; showAccountActionsModal = false; showDeleteModal = true; }}
      >Remove Account</button>
    </div>
    <div class="flex justify-end pt-1">
      <button
        class="px-4 py-2 bg-neutral-800 hover:bg-neutral-700 text-neutral-400 rounded-lg text-xs transition-colors cursor-pointer"
        onclick={() => { showAccountActionsModal = false; }}
      >Close</button>
    </div>
  </div>
</div>
{/if}
