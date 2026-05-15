use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::process::Child;
use tokio::sync::{broadcast, Mutex, Notify, RwLock};
use tokio::task::JoinHandle;

use crate::comfyui::gpu_manager::GpuManager;
use crate::config::AppConfig;
#[cfg(any(feature = "desktop", feature = "server"))]
use crate::interrogator::InterrogatorState;

/// Stored Tauri AppHandle so the headless web server can control the window.
#[cfg(feature = "desktop")]
pub type TauriAppHandle = tauri::AppHandle;

/// An event that can be sent to both Tauri and browser SSE clients.
#[derive(Clone, Debug)]
pub struct BroadcastEvent {
    pub event: String,
    pub payload: serde_json::Value,
}

/// Result slot for a held prompt: (prompt_id, worker_id) or error message.
pub type HeldPromptResult = Arc<Mutex<Option<Result<(String, u32), String>>>>;

/// A prompt held locally, waiting to be submitted to ComfyUI.
/// Used for fair queuing — only one prompt per user is submitted at a time.
pub struct HeldPrompt {
    pub workflow: serde_json::Value,
    pub username: Option<String>,
    /// The placeholder prompt_id returned to the client (for alias binding).
    pub placeholder_id: String,
    /// Signalled when this prompt is submitted to ComfyUI.
    pub submitted: Arc<Notify>,
    /// Filled in by the submission reactor after `queue_prompt`.
    pub result: HeldPromptResult,
}

/// Tracks which user submitted which prompt for per-user event isolation.
/// Uses std::sync::RwLock for fast, non-async reads in SSE stream filters.
pub struct PromptQueue {
    /// prompt_id → username (None = admin/local)
    owners: std::sync::RwLock<HashMap<String, Option<String>>>,
    /// prompt_id → worker_id that is executing this prompt.
    worker_map: std::sync::RwLock<HashMap<String, u32>>,
    /// Ordered list of (prompt_id, username) for queue position tracking
    pub(crate) queue: std::sync::RwLock<Vec<(String, Option<String>)>>,
    /// Prompts waiting to be submitted to ComfyUI (fair queue held prompts).
    pub(crate) held: std::sync::Mutex<Vec<HeldPrompt>>,
    /// Notified when a held prompt should be submitted (after a completion).
    pub(crate) drain_notify: Notify,
    /// Maps ComfyUI's real prompt_id → our placeholder prompt_id.
    /// Used to translate WebSocket events so the frontend sees consistent IDs.
    aliases: std::sync::RwLock<HashMap<String, String>>,
    /// Real ComfyUI prompt_ids whose completion/error arrived before `bind_alias`
    /// was called (race condition in server mode). `bind_alias` checks this set
    /// and immediately finishes the placeholder if the real_id is found here.
    deferred_finishes: std::sync::RwLock<std::collections::HashSet<String>>,
    /// Placeholder/real prompt ids explicitly canceled while submission may
    /// still be racing. Submission tasks check this before binding aliases.
    cancelled: std::sync::RwLock<std::collections::HashSet<String>>,
}

impl Default for PromptQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptQueue {
    pub fn new() -> Self {
        Self {
            owners: std::sync::RwLock::new(HashMap::new()),
            worker_map: std::sync::RwLock::new(HashMap::new()),
            queue: std::sync::RwLock::new(Vec::new()),
            held: std::sync::Mutex::new(Vec::new()),
            drain_notify: Notify::new(),
            aliases: std::sync::RwLock::new(HashMap::new()),
            deferred_finishes: std::sync::RwLock::new(std::collections::HashSet::new()),
            cancelled: std::sync::RwLock::new(std::collections::HashSet::new()),
        }
    }

    /// Record a new prompt submission.
    /// Uses round-robin fairness: if user X already has N queued prompts,
    /// the new prompt is placed after each other user has had at least N turns.
    /// This ensures no single user can monopolise the queue.
    ///
    /// Note: This tracks the *display* order for queue position UI.
    /// Actual ComfyUI submission order is handled by the prompt queue reactor.
    pub fn insert(&self, prompt_id: &str, username: Option<String>) {
        self.owners
            .write()
            .unwrap()
            .insert(prompt_id.to_string(), username.clone());

        let mut queue = self.queue.write().unwrap();

        // Count how many prompts this user already has in the queue
        let user_count = queue.iter().filter(|(_, owner)| *owner == username).count();

        if user_count == 0 || queue.is_empty() {
            // First prompt from this user (or empty queue): find earliest fair slot.
            // Place after one prompt from each user who already has prompts queued.
            let mut seen_users = std::collections::HashSet::new();
            let mut insert_pos = queue.len();

            for (i, (_, owner)) in queue.iter().enumerate() {
                let key = owner.clone();
                if seen_users.insert(key) {
                    continue;
                }
                // We've seen a user twice → round 1 ended before this position
                insert_pos = i;
                break;
            }

            queue.insert(insert_pos, (prompt_id.to_string(), username));
        } else {
            // User already has prompts queued. Place after the last occurrence
            // of this user's prompt, then after one prompt from each other user.
            let unique_users: std::collections::HashSet<_> =
                queue.iter().map(|(_, owner)| owner.clone()).collect();
            let num_users = unique_users.len();

            let mut last_own_idx = 0;
            let mut own_seen = 0;
            for (i, (_, owner)) in queue.iter().enumerate() {
                if *owner == username {
                    own_seen += 1;
                    last_own_idx = i;
                    if own_seen == user_count {
                        break;
                    }
                }
            }

            // Skip past one prompt from each other user after our last prompt
            let mut insert_pos = last_own_idx + 1;
            let mut other_seen = std::collections::HashSet::new();
            let mut scan_pos = insert_pos;
            while scan_pos < queue.len() {
                let (_, ref owner) = queue[scan_pos];
                if *owner != username {
                    other_seen.insert(owner.clone());
                    insert_pos = scan_pos + 1;
                    if other_seen.len() >= num_users - 1 {
                        break;
                    }
                } else {
                    break;
                }
                scan_pos += 1;
            }

            queue.insert(insert_pos, (prompt_id.to_string(), username));
        }
    }

    /// Remove a completed/errored prompt from the position queue.
    /// Keeps the ownership mapping so in-flight SSE events can still be filtered.
    /// Returns the worker_id that was handling this prompt (if tracked).
    pub fn finish(&self, prompt_id: &str) -> Option<u32> {
        let worker_id = self.worker_map.write().unwrap().remove(prompt_id);
        self.queue
            .write()
            .unwrap()
            .retain(|(id, _)| id != prompt_id);
        worker_id
    }

    /// Remove a prompt entirely (position + ownership).
    /// Only used for explicit cancellation, not completion.
    pub fn remove(&self, prompt_id: &str) {
        self.cancel_and_remove(prompt_id);
    }

    /// Mark a prompt as canceled and remove its placeholder/aliases from
    /// internal queue tracking. Returns ids that should be deleted from ComfyUI.
    pub fn cancel_and_remove(&self, prompt_id: &str) -> Vec<String> {
        let ids = self.related_ids(prompt_id);
        {
            let mut cancelled = self.cancelled.write().unwrap();
            for id in &ids {
                cancelled.insert(id.clone());
            }
        }
        {
            let mut owners = self.owners.write().unwrap();
            for id in &ids {
                owners.remove(id);
            }
        }
        {
            let mut worker_map = self.worker_map.write().unwrap();
            for id in &ids {
                worker_map.remove(id);
            }
        }
        self.queue
            .write()
            .unwrap()
            .retain(|(id, _)| !ids.iter().any(|removed| removed == id));
        self.aliases
            .write()
            .unwrap()
            .retain(|real, placeholder| !ids.iter().any(|id| id == real || id == placeholder));
        ids
    }

    /// Remove held prompts matching any placeholder/real id and return them
    /// so callers can wake their waiting submission tasks with a cancellation.
    pub fn take_held_related_to(&self, prompt_ids: &[String]) -> Vec<HeldPrompt> {
        if prompt_ids.is_empty() {
            return Vec::new();
        }

        let id_set: HashSet<String> = prompt_ids.iter().cloned().collect();
        let mut held = self.held.lock().unwrap();
        let mut kept: Vec<HeldPrompt> = Vec::with_capacity(held.len());
        let mut taken: Vec<HeldPrompt> = Vec::new();

        for hp in held.drain(..) {
            let is_match = id_set.contains(&hp.placeholder_id)
                || self
                    .related_ids(&hp.placeholder_id)
                    .iter()
                    .any(|id| id_set.contains(id));

            if is_match {
                taken.push(hp);
            } else {
                kept.push(hp);
            }
        }

        *held = kept;
        taken
    }

    /// Return a prompt id plus its placeholder/real-id aliases.
    pub fn related_ids(&self, prompt_id: &str) -> Vec<String> {
        let aliases = self.aliases.read().unwrap();
        let placeholder = aliases
            .get(prompt_id)
            .cloned()
            .unwrap_or_else(|| prompt_id.to_string());
        let mut ids = vec![placeholder.clone()];
        if prompt_id != placeholder {
            ids.push(prompt_id.to_string());
        }
        for (real_id, alias_placeholder) in aliases.iter() {
            if alias_placeholder == &placeholder && !ids.iter().any(|id| id == real_id) {
                ids.push(real_id.clone());
            }
        }
        ids
    }

    /// Whether a placeholder/real prompt id was explicitly canceled.
    pub fn is_cancelled(&self, prompt_id: &str) -> bool {
        let cancelled = self.cancelled.read().unwrap();
        self.related_ids(prompt_id)
            .iter()
            .any(|id| cancelled.contains(id))
    }

    /// Record which worker is handling a prompt.
    pub fn set_worker(&self, prompt_id: &str, worker_id: u32) {
        self.worker_map
            .write()
            .unwrap()
            .insert(prompt_id.to_string(), worker_id);
    }

    /// Get the worker handling a prompt.
    pub fn worker_of(&self, prompt_id: &str) -> Option<u32> {
        self.worker_map.read().unwrap().get(prompt_id).copied()
    }

    /// Snapshot of the worker_map (prompt_id → worker_id) for read-only inspection.
    pub fn worker_map_snapshot(&self) -> HashMap<String, u32> {
        self.worker_map.read().unwrap().clone()
    }

    /// Check if a prompt_id belongs to a specific user.
    /// Returns true if:
    /// - The prompt is untracked (submitted before queue tracking started)
    /// - The prompt belongs to the given user
    /// - The user is None (admin) — admin sees everything
    pub fn is_owned_by(&self, prompt_id: &str, username: &Option<String>) -> bool {
        // Admin sees everything
        if username.is_none() {
            return true;
        }
        let owners = self.owners.read().unwrap();
        match owners.get(prompt_id) {
            None => false, // Unknown prompt — don't leak to other users
            Some(owner) => owner == username,
        }
    }

    /// Get the owner username for a prompt_id, if tracked.
    pub fn owner_of(&self, prompt_id: &str) -> Option<String> {
        let owners = self.owners.read().unwrap();
        owners.get(prompt_id).and_then(|o| o.clone())
    }

    /// Get queue position for a specific user's prompts.
    /// Returns vec of (prompt_id, position) where position is 0-indexed globally.
    pub fn user_positions(&self, username: &Option<String>) -> Vec<(String, usize)> {
        let queue = self.queue.read().unwrap();
        queue
            .iter()
            .enumerate()
            .filter(|(_, (_, owner))| owner == username)
            .map(|(pos, (id, _))| (id.clone(), pos))
            .collect()
    }

    /// Get total queue length.
    pub fn len(&self) -> usize {
        self.queue.read().unwrap().len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.read().unwrap().is_empty()
    }

    /// Count how many prompts from a given user are currently active (submitted to ComfyUI).
    /// These are prompts that are in the queue (tracking) but NOT in the held list.
    pub fn active_count_for_user(&self, username: &Option<String>) -> usize {
        let queue = self.queue.read().unwrap();
        let held = self.held.lock().unwrap();
        let held_usernames: Vec<_> = held.iter().map(|h| &h.username).collect();

        // Count queued prompts for this user that are NOT held
        // (i.e. they have been submitted to ComfyUI)
        queue
            .iter()
            .filter(|(_, owner)| owner == username)
            .count()
            .saturating_sub(held_usernames.iter().filter(|u| ***u == *username).count())
    }

    /// Take the next held prompt that should be submitted (round-robin fair).
    /// Prefers users who have the fewest active (submitted) prompts.
    pub fn take_next_held(&self) -> Option<HeldPrompt> {
        let mut held = self.held.lock().unwrap();
        if held.is_empty() {
            return None;
        }
        // Just take the first held prompt — the insert order already ensures fairness.
        Some(held.remove(0))
    }

    /// Bind an alias: map ComfyUI's real prompt_id → our placeholder_id.
    /// Also registers ownership for the real_id so SSE filtering works if
    /// an event arrives before alias resolution.
    ///
    /// Returns `true` if a completion/error event arrived before this alias was
    /// bound (race condition). In that case the placeholder has already been
    /// removed from the queue and the caller **must** release the GPU worker.
    pub fn bind_alias(&self, placeholder_id: &str, comfyui_id: &str) -> bool {
        self.aliases
            .write()
            .unwrap()
            .insert(comfyui_id.to_string(), placeholder_id.to_string());
        // Copy ownership so is_owned_by works for the real id too
        let username = self
            .owners
            .read()
            .unwrap()
            .get(placeholder_id)
            .cloned()
            .flatten();
        self.owners
            .write()
            .unwrap()
            .insert(comfyui_id.to_string(), username);
        // Check if completion/error arrived before this alias was bound.
        let was_deferred = self.deferred_finishes.write().unwrap().remove(comfyui_id);
        if was_deferred {
            self.worker_map.write().unwrap().remove(placeholder_id);
            self.queue
                .write()
                .unwrap()
                .retain(|(id, _)| id != placeholder_id);
            self.owners.write().unwrap().remove(placeholder_id);
        }
        was_deferred
    }

    /// Park a ComfyUI real prompt_id whose completion/error arrived before
    /// `bind_alias` was called. The next `bind_alias` call for this id will
    /// immediately finish the corresponding placeholder.
    pub fn park_deferred_finish(&self, comfyui_id: &str) {
        self.deferred_finishes
            .write()
            .unwrap()
            .insert(comfyui_id.to_string());
    }

    /// Resolve a prompt_id: if it's a ComfyUI real_id with a bound alias,
    /// return the placeholder_id. Otherwise return the original id.
    pub fn resolve_alias(&self, prompt_id: &str) -> String {
        self.aliases
            .read()
            .unwrap()
            .get(prompt_id)
            .cloned()
            .unwrap_or_else(|| prompt_id.to_string())
    }

    /// Clean up alias entries for a finished prompt.
    pub fn cleanup_alias(&self, placeholder_id: &str) {
        let mut aliases = self.aliases.write().unwrap();
        aliases.retain(|_, v| v != placeholder_id);
        self.cancelled.write().unwrap().remove(placeholder_id);
    }

    /// Clear all queue tracking state (owners, worker_map, queue).
    /// Does NOT clear held or aliases — those are managed separately.
    pub fn clear_all(&self) {
        self.queue.write().unwrap().clear();
        self.owners.write().unwrap().clear();
        self.worker_map.write().unwrap().clear();
        self.cancelled.write().unwrap().clear();
    }
}

pub struct AppState {
    pub config: RwLock<AppConfig>,
    pub comfyui_process: Mutex<Option<Child>>,
    pub ws_handle: Mutex<Option<JoinHandle<()>>>,
    pub client_id: String,
    pub http_client: reqwest::Client,
    #[cfg(any(feature = "desktop", feature = "server"))]
    pub interrogator: Arc<RwLock<InterrogatorState>>,
    /// Broadcast channel for SSE events in browser mode.
    pub event_tx: broadcast::Sender<BroadcastEvent>,
    /// Timestamp of last heartbeat from browser client.
    pub last_heartbeat: Mutex<std::time::Instant>,
    /// Tauri AppHandle — set after app setup so the web server can show/hide the window.
    #[cfg(feature = "desktop")]
    pub app_handle: Mutex<Option<TauriAppHandle>>,
    /// Set to true when switching from browser mode to app mode.
    /// Prevents the heartbeat watchdog from killing the process.
    pub app_mode_active: std::sync::atomic::AtomicBool,
    /// True once the embedded web server has been started (prevents double-bind).
    pub web_server_running: std::sync::atomic::AtomicBool,
    /// True once the shared prompt cleanup/watchdog reactors have been spawned
    /// (prevents duplicates when both desktop and browser mode start them).
    pub cleanup_reactors_started: std::sync::atomic::AtomicBool,
    /// Multi-user generation queue — tracks prompt ownership and position.
    pub prompt_queue: PromptQueue,
    /// Multi-GPU worker manager — distributes prompts across N GPU backends.
    pub gpu_manager: GpuManager,
    /// Tracks output temp filenames by placeholder prompt_id for recovery.
    /// Populated by the WebSocket bridge when `comfyui:output_image` fires;
    /// consumed (and cleared) by `recover_prompt_outputs`.
    pub output_image_cache: std::sync::RwLock<HashMap<String, Vec<String>>>,
    /// Tracks the last live-preview temp_filename per placeholder prompt_id.
    /// Updated by the WebSocket bridge on every `comfyui:preview` event.
    /// Sent to clients that reconnect mid-generation via the SSE initial burst.
    pub last_preview_by_prompt: std::sync::RwLock<HashMap<String, String>>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        let http_client = reqwest::Client::new();
        let gpu_manager = GpuManager::new(&config, http_client.clone());
        Self {
            config: RwLock::new(config),
            comfyui_process: Mutex::new(None),
            ws_handle: Mutex::new(None),
            client_id: uuid::Uuid::new_v4().to_string(),
            http_client,
            #[cfg(any(feature = "desktop", feature = "server"))]
            interrogator: Arc::new(RwLock::new(InterrogatorState::new())),
            event_tx,
            last_heartbeat: Mutex::new(std::time::Instant::now()),
            #[cfg(feature = "desktop")]
            app_handle: Mutex::new(None),
            app_mode_active: std::sync::atomic::AtomicBool::new(false),
            web_server_running: std::sync::atomic::AtomicBool::new(false),
            cleanup_reactors_started: std::sync::atomic::AtomicBool::new(false),
            prompt_queue: PromptQueue::new(),
            gpu_manager,
            output_image_cache: std::sync::RwLock::new(HashMap::new()),
            last_preview_by_prompt: std::sync::RwLock::new(HashMap::new()),
        }
    }

    pub async fn base_url(&self) -> String {
        let config = self.config.read().await;
        config.server_url.clone()
    }

    /// Cancel a generation in a worker-aware fashion.
    ///
    /// If `prompt_id` is `Some(id)`:
    /// - Cancels the held prompt (if it never reached ComfyUI), OR
    /// - Looks up which worker owns the prompt, interrupts that worker,
    ///   and deletes the prompt from that worker's pending queue.
    /// - Falls back to all-worker fanout if the prompt isn't tracked yet.
    ///
    /// If `prompt_id` is `None`, every running worker is interrupted.
    /// In all cases, /free is issued on each affected worker to flush VRAM —
    /// rapid interrupts on Blackwell GPUs with cudaMallocAsync can leave
    /// VRAM in a corrupted state, producing all-black images on the next gen.
    pub async fn interrupt_prompt(
        &self,
        prompt_id: Option<&str>,
    ) -> Result<(), crate::error::AppError> {
        // 1. If a specific prompt is being cancelled and it's still held
        //    locally (never submitted to ComfyUI), cancel it there and stop.
        if let Some(pid) = prompt_id {
            let held_prompt = {
                let mut held = self.prompt_queue.held.lock().unwrap();
                held.iter()
                    .position(|hp| hp.placeholder_id == pid)
                    .map(|idx| held.remove(idx))
            };
            if let Some(hp) = held_prompt {
                {
                    let mut result = hp.result.lock().await;
                    *result = Some(Err("generation.error_cancelled".to_string()));
                }
                hp.submitted.notify_one();
                self.prompt_queue.remove(pid);
                self.prompt_queue.drain_notify.notify_one();
                self.broadcast_queue_positions();
                return Ok(());
            }
        }

        // 2. Decide which workers to hit.
        //    - Specific prompt with a known worker → just that one.
        //    - Otherwise → all running workers.
        let target_worker_ids: Vec<u32> =
            match prompt_id.and_then(|id| self.prompt_queue.worker_of(id)) {
                Some(wid) => vec![wid],
                None => self.gpu_manager.workers.iter().map(|w| w.id).collect(),
            };

        for wid in &target_worker_ids {
            let _ = self.gpu_manager.interrupt(Some(*wid)).await;
        }

        // 3. Delete the specific prompt (and any aliases) from each targeted
        //    worker's pending queue so it doesn't resume on requeue.
        if let Some(pid) = prompt_id {
            let ids_to_delete = self.prompt_queue.cancel_and_remove(pid);

            for wid in &target_worker_ids {
                if let Some(worker) = self.gpu_manager.workers.get(*wid as usize) {
                    let _ = self
                        .http_client
                        .post(format!("{}/queue", worker.base_url))
                        .json(&serde_json::json!({ "delete": ids_to_delete }))
                        .send()
                        .await;
                }
            }
            self.broadcast_queue_positions();
        }

        // 4. Flush VRAM on every targeted worker.
        for wid in &target_worker_ids {
            if let Some(worker) = self.gpu_manager.workers.get(*wid as usize) {
                let _ = self
                    .http_client
                    .post(format!("{}/free", worker.base_url))
                    .json(&serde_json::json!({
                        "unload_models": true,
                        "free_memory": true,
                    }))
                    .send()
                    .await;
            }
        }

        if prompt_id.is_some() {
            for wid in &target_worker_ids {
                self.gpu_manager.mark_worker_error_then_idle(*wid).await;
            }
            self.prompt_queue.drain_notify.notify_one();
        }

        Ok(())
    }

    /// Cancel every prompt owned by `username` — held, queued and currently
    /// running. This mirrors `clear_all_queues` but scoped to a single user so
    /// clicking "Cancel" wipes out *their* in-flight and pending prompts
    /// instead of just sending a global /interrupt that leaves stale entries
    /// in the ComfyUI worker queues (the source of "ghost cancel" runs where
    /// an old gen keeps producing while a new one starts).
    ///
    /// `username == None` matches admin-owned prompts (single-user desktop
    /// mode, where every prompt has `owner = None`, results in all prompts
    /// being cancelled — the desired desktop behavior).
    pub async fn interrupt_user_prompts(
        &self,
        username: Option<&str>,
    ) -> Result<(), crate::error::AppError> {
        let owner_filter: Option<String> = username.map(|s| s.to_string());

        // 1. Drain this user's held (not-yet-submitted) prompts so their
        //    background submission tasks exit cleanly.
        let owned_held: Vec<HeldPrompt> = {
            let mut held = self.prompt_queue.held.lock().unwrap();
            let mut kept: Vec<HeldPrompt> = Vec::with_capacity(held.len());
            let mut taken: Vec<HeldPrompt> = Vec::new();
            for hp in held.drain(..) {
                if hp.username == owner_filter {
                    taken.push(hp);
                } else {
                    kept.push(hp);
                }
            }
            *held = kept;
            taken
        };
        for hp in owned_held {
            {
                let mut result = hp.result.lock().await;
                *result = Some(Err("generation.error_cancelled".to_string()));
            }
            hp.submitted.notify_one();
            self.prompt_queue.remove(&hp.placeholder_id);
        }

        // 2. Collect this user's queued prompt_ids (both running and pending).
        let owned_ids: Vec<String> = {
            let queue = self.prompt_queue.queue.read().unwrap();
            queue
                .iter()
                .filter(|(_, owner)| owner == &owner_filter)
                .map(|(id, _)| id.clone())
                .collect()
        };

        // 3. Resolve which workers are currently running owned prompts, and
        //    drop the prompts (plus aliases) from internal tracking.
        let mut workers_to_interrupt: std::collections::HashSet<u32> =
            std::collections::HashSet::new();
        let mut ids_to_delete: Vec<String> = Vec::new();
        for pid in &owned_ids {
            if let Some(wid) = self.prompt_queue.worker_of(pid) {
                workers_to_interrupt.insert(wid);
            }
            for related in self.prompt_queue.cancel_and_remove(pid) {
                if !ids_to_delete.contains(&related) {
                    ids_to_delete.push(related);
                }
            }
        }

        // 4. Interrupt every worker that was executing one of this user's
        //    prompts. Other users' workers are left alone.
        for wid in &workers_to_interrupt {
            let _ = self.gpu_manager.interrupt(Some(*wid)).await;
        }

        // 5. Delete the owned ids (placeholder + real) from every worker's
        //    pending queue. We fan out to all workers because a pending prompt
        //    may not yet have a worker assignment — ComfyUI ignores unknown
        //    ids, so this is safe.
        if !ids_to_delete.is_empty() {
            for worker in &self.gpu_manager.workers {
                let _ = self
                    .http_client
                    .post(format!("{}/queue", worker.base_url))
                    .json(&serde_json::json!({ "delete": ids_to_delete }))
                    .send()
                    .await;
            }
        }

        // 6. Flush VRAM on each interrupted worker (avoids the black-image
        //    bug on Blackwell GPUs after a rapid interrupt).
        for wid in &workers_to_interrupt {
            if let Some(worker) = self.gpu_manager.workers.get(*wid as usize) {
                let _ = self
                    .http_client
                    .post(format!("{}/free", worker.base_url))
                    .json(&serde_json::json!({
                        "unload_models": true,
                        "free_memory": true,
                    }))
                    .send()
                    .await;
            }
        }

        // 7. Mark interrupted workers idle so the next prompt can dispatch
        //    immediately instead of waiting behind the cancelled one.
        for wid in &workers_to_interrupt {
            self.gpu_manager.mark_worker_error_then_idle(*wid).await;
        }

        self.prompt_queue.drain_notify.notify_one();
        self.broadcast_queue_positions();
        Ok(())
    }

    /// Broadcast an event to SSE clients.
    pub fn broadcast(&self, event: &str, payload: serde_json::Value) {
        let _ = self.event_tx.send(BroadcastEvent {
            event: event.to_string(),
            payload,
        });
    }

    /// Broadcast queue position updates to all SSE clients.
    pub fn broadcast_queue_positions(&self) {
        let queue = self.prompt_queue.queue.read().unwrap();
        let total = queue.len();
        if total == 0 {
            self.broadcast(
                "mooshie:queue_update",
                serde_json::json!({ "total": 0_u32 }),
            );
            return;
        }
        for (pos, (prompt_id, _owner)) in queue.iter().enumerate() {
            self.broadcast(
                "mooshie:queue_update",
                serde_json::json!({
                    "prompt_id": prompt_id,
                    "position": pos,
                    "total": total,
                }),
            );
        }
    }
}
