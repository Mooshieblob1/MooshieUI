use std::collections::HashMap;
use std::sync::Arc;

use tokio::process::Child;
use tokio::sync::{broadcast, Mutex, Notify, RwLock};
use tokio::task::JoinHandle;

use crate::config::AppConfig;
use crate::interrogator::InterrogatorState;

/// Stored Tauri AppHandle so the headless web server can control the window.
pub type TauriAppHandle = tauri::AppHandle;

/// An event that can be sent to both Tauri and browser SSE clients.
#[derive(Clone, Debug)]
pub struct BroadcastEvent {
    pub event: String,
    pub payload: serde_json::Value,
}

/// Result slot for a held prompt: (prompt_id, seed) or error message.
pub type HeldPromptResult = Arc<Mutex<Option<Result<(String, i64), String>>>>;

/// A prompt held locally, waiting to be submitted to ComfyUI.
/// Used for fair queuing — only one prompt per user is submitted at a time.
pub struct HeldPrompt {
    pub workflow: serde_json::Value,
    pub username: Option<String>,
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
    /// Ordered list of (prompt_id, username) for queue position tracking
    pub(crate) queue: std::sync::RwLock<Vec<(String, Option<String>)>>,
    /// Prompts waiting to be submitted to ComfyUI (fair queue held prompts).
    pub(crate) held: std::sync::Mutex<Vec<HeldPrompt>>,
    /// Notified when a held prompt should be submitted (after a completion).
    pub(crate) drain_notify: Notify,
}

impl PromptQueue {
    pub fn new() -> Self {
        Self {
            owners: std::sync::RwLock::new(HashMap::new()),
            queue: std::sync::RwLock::new(Vec::new()),
            held: std::sync::Mutex::new(Vec::new()),
            drain_notify: Notify::new(),
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
            for i in insert_pos..queue.len() {
                let (_, ref owner) = queue[i];
                if *owner != username {
                    other_seen.insert(owner.clone());
                    insert_pos = i + 1;
                    if other_seen.len() >= num_users - 1 {
                        break;
                    }
                } else {
                    break;
                }
            }

            queue.insert(insert_pos, (prompt_id.to_string(), username));
        }
    }

    /// Remove a completed/errored prompt from the position queue.
    /// Keeps the ownership mapping so in-flight SSE events can still be filtered.
    pub fn finish(&self, prompt_id: &str) {
        self.queue
            .write()
            .unwrap()
            .retain(|(id, _)| id != prompt_id);
    }

    /// Remove a prompt entirely (position + ownership).
    /// Only used for explicit cancellation, not completion.
    pub fn remove(&self, prompt_id: &str) {
        self.owners.write().unwrap().remove(prompt_id);
        self.queue
            .write()
            .unwrap()
            .retain(|(id, _)| id != prompt_id);
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
}

pub struct AppState {
    pub config: RwLock<AppConfig>,
    pub comfyui_process: Mutex<Option<Child>>,
    pub ws_handle: Mutex<Option<JoinHandle<()>>>,
    pub client_id: String,
    pub http_client: reqwest::Client,
    pub interrogator: Arc<RwLock<InterrogatorState>>,
    /// Broadcast channel for SSE events in browser mode.
    pub event_tx: broadcast::Sender<BroadcastEvent>,
    /// Timestamp of last heartbeat from browser client.
    pub last_heartbeat: Mutex<std::time::Instant>,
    /// Tauri AppHandle — set after app setup so the web server can show/hide the window.
    pub app_handle: Mutex<Option<TauriAppHandle>>,
    /// Set to true when switching from browser mode to app mode.
    /// Prevents the heartbeat watchdog from killing the process.
    pub app_mode_active: std::sync::atomic::AtomicBool,
    /// True once the embedded web server has been started (prevents double-bind).
    pub web_server_running: std::sync::atomic::AtomicBool,
    /// Multi-user generation queue — tracks prompt ownership and position.
    pub prompt_queue: PromptQueue,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        Self {
            config: RwLock::new(config),
            comfyui_process: Mutex::new(None),
            ws_handle: Mutex::new(None),
            client_id: uuid::Uuid::new_v4().to_string(),
            http_client: reqwest::Client::new(),
            interrogator: Arc::new(RwLock::new(InterrogatorState::new())),
            event_tx,
            last_heartbeat: Mutex::new(std::time::Instant::now()),
            app_handle: Mutex::new(None),
            app_mode_active: std::sync::atomic::AtomicBool::new(false),
            web_server_running: std::sync::atomic::AtomicBool::new(false),
            prompt_queue: PromptQueue::new(),
        }
    }

    pub async fn base_url(&self) -> String {
        let config = self.config.read().await;
        config.server_url.clone()
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
