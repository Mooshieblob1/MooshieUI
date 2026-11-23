//! One NovelAI generation at a time per account.
//!
//! NovelAI runs a single image request per account at a time and answers a
//! second one that arrives while the first is still running with 429
//! ("Concurrent generation is locked"). Each queued generation used to fire
//! its request the moment it was submitted, so queueing several meant every
//! request after the first was refused or left waiting until it timed out.
//!
//! Every request that runs a model now passes through the account's gate
//! first. Waiters are served in the order they arrived (tokio's mutex is
//! fair), so a queue of generations runs in the order it was queued, and
//! consecutive requests are spaced by a short gap so the next one does not
//! land while NovelAI is still releasing the previous one.
//!
//! Gates are keyed by a hash of the API key rather than by user, because the
//! limit belongs to the NovelAI account: two app users on two keys run side by
//! side, and the same key used from two places shares one gate.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

/// Quiet time between the end of one request on an account and the start of
/// the next. NovelAI's lock can outlive the response by a moment.
pub const REQUEST_GAP: Duration = Duration::from_millis(1000);

/// How often a waiter checks whether its generation was cancelled.
const CANCEL_POLL: Duration = Duration::from_millis(250);

/// The time the previous request on this account finished, if any.
type Slot = Arc<AsyncMutex<Option<Instant>>>;

static GATES: OnceLock<Mutex<HashMap<[u8; 32], Slot>>> = OnceLock::new();

/// The waiter's generation was cancelled before it reached NovelAI.
#[derive(Debug, PartialEq, Eq)]
pub struct Cancelled;

/// The right to send requests on one account, queued fairly.
#[derive(Clone)]
pub struct AccountGate {
    slot: Slot,
    gap: Duration,
}

/// Held while requests run. Dropping it hands the account to the next waiter.
pub struct GatePass {
    guard: OwnedMutexGuard<Option<Instant>>,
}

impl Drop for GatePass {
    fn drop(&mut self) {
        *self.guard = Some(Instant::now());
    }
}

impl AccountGate {
    /// The process-wide gate for this API key.
    pub fn for_key(api_key: &str) -> Self {
        let id: [u8; 32] = Sha256::digest(api_key.as_bytes()).into();
        let mut gates = GATES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let slot = Arc::clone(gates.entry(id).or_default());
        Self::with_slot(slot, REQUEST_GAP)
    }

    fn with_slot(slot: Slot, gap: Duration) -> Self {
        Self { slot, gap }
    }

    /// Wait for this account's turn, then for the gap after the previous
    /// request, giving up as soon as `cancelled` says so.
    ///
    /// The lock future is created once and polled across the cancel checks, so
    /// a waiter keeps its place in line rather than rejoining at the back.
    pub async fn enter(
        &self,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<GatePass, Cancelled> {
        let lock = Arc::clone(&self.slot).lock_owned();
        tokio::pin!(lock);
        let guard = loop {
            tokio::select! {
                guard = &mut lock => break guard,
                _ = tokio::time::sleep(CANCEL_POLL) => {
                    if cancelled() {
                        return Err(Cancelled);
                    }
                }
            }
        };
        let pass = GatePass { guard };

        if let Some(last) = *pass.guard {
            let ready = last + self.gap;
            sleep_until_unless(ready, cancelled).await?;
        }
        if cancelled() {
            return Err(Cancelled);
        }
        Ok(pass)
    }
}

/// Sleep until `deadline`, checking `cancelled` every poll interval.
async fn sleep_until_unless(
    deadline: Instant,
    cancelled: &(dyn Fn() -> bool + Send + Sync),
) -> Result<(), Cancelled> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Ok(());
        }
        if cancelled() {
            return Err(Cancelled);
        }
        tokio::time::sleep((deadline - now).min(CANCEL_POLL)).await;
    }
}

/// Sleep for `wait`, checking `cancelled` every poll interval.
pub async fn sleep_unless(
    wait: Duration,
    cancelled: &(dyn Fn() -> bool + Send + Sync),
) -> Result<(), Cancelled> {
    sleep_until_unless(Instant::now() + wait, cancelled).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn never() -> bool {
        false
    }

    fn gate(gap: Duration) -> AccountGate {
        AccountGate::with_slot(Arc::default(), gap)
    }

    #[test]
    fn one_key_shares_a_gate_and_two_keys_do_not() {
        let a = AccountGate::for_key("gate-test-key-a");
        let a_again = AccountGate::for_key("gate-test-key-a");
        let b = AccountGate::for_key("gate-test-key-b");
        assert!(Arc::ptr_eq(&a.slot, &a_again.slot));
        assert!(!Arc::ptr_eq(&a.slot, &b.slot));
    }

    #[tokio::test]
    async fn a_second_request_waits_for_the_first_and_the_gap() {
        let gate = gate(Duration::from_millis(150));
        let first = gate.enter(&never).await.unwrap();

        let started = Instant::now();
        let second = {
            let gate = gate.clone();
            tokio::spawn(async move {
                let _pass = gate.enter(&never).await.unwrap();
                Instant::now()
            })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !second.is_finished(),
            "the second request ran alongside the first"
        );
        let released = Instant::now();
        drop(first);

        let entered = second.await.unwrap();
        assert!(entered >= released + Duration::from_millis(150));
        assert!(entered > started);
    }

    #[tokio::test]
    async fn waiters_are_served_in_the_order_they_queued() {
        let gate = gate(Duration::ZERO);
        let first = gate.enter(&never).await.unwrap();
        let order = Arc::new(Mutex::new(Vec::new()));

        let mut handles = Vec::new();
        for n in 0..4 {
            let gate = gate.clone();
            let order = Arc::clone(&order);
            handles.push(tokio::spawn(async move {
                let _pass = gate.enter(&never).await.unwrap();
                order.lock().unwrap().push(n);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }));
            // Let each waiter reach the lock, and sit through a cancel poll,
            // before the next one queues.
            tokio::time::sleep(CANCEL_POLL + Duration::from_millis(50)).await;
        }
        drop(first);
        for handle in handles {
            handle.await.unwrap();
        }
        assert_eq!(*order.lock().unwrap(), vec![0, 1, 2, 3]);
    }

    #[tokio::test]
    async fn a_cancelled_waiter_leaves_the_line_without_sending() {
        let gate = gate(Duration::ZERO);
        let first = gate.enter(&never).await.unwrap();
        let cancel = Arc::new(AtomicBool::new(false));

        let waiter = {
            let gate = gate.clone();
            let cancel = Arc::clone(&cancel);
            tokio::spawn(async move {
                let check = move || cancel.load(Ordering::SeqCst);
                gate.enter(&check).await.map(|_| ())
            })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(waiter.await.unwrap(), Err(Cancelled));

        // The account is still held by the first request, and is free after.
        drop(first);
        assert!(gate.enter(&never).await.is_ok());
    }

    #[tokio::test]
    async fn a_cancel_during_the_gap_is_honoured() {
        let gate = gate(Duration::from_secs(30));
        drop(gate.enter(&never).await.unwrap());
        let cancelled = || true;
        let started = Instant::now();
        assert!(gate.enter(&cancelled).await.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
