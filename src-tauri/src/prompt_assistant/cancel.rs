//! Cancelling an in-flight prompt assistant request.
//!
//! Every backend already stops cleanly when its future is dropped: reqwest
//! closes the connection (so a hosted provider stops generating and the
//! bundled llama-server frees its slot), and a companion request kills its
//! child process and releases the per-provider lock on the way out. What was
//! missing was a way for the client to make that drop happen. Without one,
//! closing the Enhance window only hid the request: it kept running to the end,
//! and the next Enhance queued behind it (on the companion lock, or the local
//! model's slot) until it did.
//!
//! The client tags a request with an id it generates and, on cancel, sends
//! that id back. [`run`] races the work against that signal and drops the work
//! when the signal wins.
//!
//! A cancel can overtake the request it names: both are separate IPC calls,
//! and nothing orders them once they reach the backend. A cancel for an id
//! that is not running yet is remembered for a short while, so the request is
//! dropped as soon as it registers instead of running to the end unnoticed.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

/// How long a cancel that arrived first waits for its request.
const EARLY_CANCEL_TTL: Duration = Duration::from_secs(120);
/// Bound on remembered early cancels, so a client sending ids that never
/// arrive cannot grow the map without limit.
const EARLY_CANCEL_CAP: usize = 256;

struct Running {
    owner: Option<String>,
    stop: oneshot::Sender<()>,
}

#[derive(Default)]
struct Registry {
    running: HashMap<String, Running>,
    early: HashMap<String, (Option<String>, Instant)>,
}

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

fn with_registry<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    let mut guard = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Registry::default))
}

/// Removes a finished request's entry, whichever way it finished.
struct Registration(String);

impl Drop for Registration {
    fn drop(&mut self) {
        with_registry(|r| r.running.remove(&self.0));
    }
}

/// Run `work`, dropping it if [`cancel`] names `id` first.
///
/// Returns `None` when the request was cancelled. Without an id the work runs
/// to the end as before, which keeps every caller that never cancels unchanged.
///
/// `owner` is the browser-mode account that sent the request; a cancel only
/// counts when it comes from the same one, so an id seen by one user cannot
/// stop another user's request. The desktop app passes `None` on both sides.
pub async fn run<T>(
    id: Option<&str>,
    owner: Option<&str>,
    work: impl Future<Output = T>,
) -> Option<T> {
    let Some(id) = id.filter(|id| !id.is_empty()) else {
        return Some(work.await);
    };
    let (stop, stopped) = oneshot::channel();
    let early = with_registry(|r| {
        let now = Instant::now();
        r.early
            .retain(|_, (_, at)| now.duration_since(*at) < EARLY_CANCEL_TTL);
        if let Some((by, _)) = r.early.remove(id) {
            if by.as_deref() == owner {
                return true;
            }
        }
        r.running.insert(
            id.to_string(),
            Running {
                owner: owner.map(str::to_string),
                stop,
            },
        );
        false
    });
    if early {
        return None;
    }
    let _registration = Registration(id.to_string());
    tokio::select! {
        out = work => Some(out),
        // Only ever sent by `cancel`; the sender is otherwise dropped by
        // `Registration` after this select has finished.
        _ = stopped => None,
    }
}

/// Cancel the request `id` that `owner` started, or remember the cancel until
/// it starts. Returns whether a running request was stopped.
pub fn cancel(id: &str, owner: Option<&str>) -> bool {
    if id.is_empty() {
        return false;
    }
    with_registry(|r| {
        match r.running.get(id) {
            Some(running) if running.owner.as_deref() == owner => {
                let running = r.running.remove(id).expect("checked above");
                let _ = running.stop.send(());
                true
            }
            // Someone else's request: leave it alone, and don't remember the
            // cancel either, or it would sit there until it expired.
            Some(_) => false,
            None => {
                if r.early.len() >= EARLY_CANCEL_CAP {
                    if let Some(oldest) = r
                        .early
                        .iter()
                        .min_by_key(|(_, (_, at))| *at)
                        .map(|(k, _)| k.clone())
                    {
                        r.early.remove(&oldest);
                    }
                }
                r.early
                    .insert(id.to_string(), (owner.map(str::to_string), Instant::now()));
                false
            }
        }
    })
}

/// The error text a cancelled request reports. The client has already moved on
/// by the time it arrives, so it is never shown; it only has to be recognisable.
pub const CANCELLED: &str = "prompt_assistant.cancelled";

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn unique(name: &str) -> String {
        format!("{name}-{}", uuid::Uuid::new_v4())
    }

    #[tokio::test]
    async fn without_an_id_the_work_runs_to_the_end() {
        assert_eq!(run(None, None, async { 7 }).await, Some(7));
        assert_eq!(run(Some(""), None, async { 8 }).await, Some(8));
    }

    #[tokio::test]
    async fn a_cancel_drops_the_running_work() {
        let id = unique("running");
        let dropped = Arc::new(AtomicBool::new(false));
        struct Flag(Arc<AtomicBool>);
        impl Drop for Flag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let flag = Flag(dropped.clone());
        let task = {
            let id = id.clone();
            tokio::spawn(async move {
                run(Some(&id), None, async move {
                    let _flag = flag;
                    std::future::pending::<()>().await
                })
                .await
            })
        };
        // Let the task register before cancelling it.
        for _ in 0..100 {
            if with_registry(|r| r.running.contains_key(&id)) {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(cancel(&id, None));
        assert_eq!(task.await.unwrap(), None);
        assert!(
            dropped.load(Ordering::SeqCst),
            "the work future was dropped"
        );
        assert!(!with_registry(|r| r.running.contains_key(&id)));
    }

    #[tokio::test]
    async fn a_cancel_that_arrives_first_still_stops_the_request() {
        let id = unique("early");
        assert!(!cancel(&id, Some("ann")));
        assert_eq!(run(Some(&id), Some("ann"), async { 1 }).await, None);
        // Used up: the same id runs normally afterwards.
        assert_eq!(run(Some(&id), Some("ann"), async { 2 }).await, Some(2));
    }

    #[tokio::test]
    async fn another_owner_cannot_cancel() {
        let id = unique("owner");
        assert!(!cancel(&id, Some("bob")));
        assert_eq!(run(Some(&id), Some("ann"), async { 3 }).await, Some(3));
    }

    #[tokio::test]
    async fn a_finished_request_leaves_nothing_behind() {
        let id = unique("finished");
        assert_eq!(run(Some(&id), None, async { 4 }).await, Some(4));
        assert!(!with_registry(|r| r.running.contains_key(&id)));
    }
}
