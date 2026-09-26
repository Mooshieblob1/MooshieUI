use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Mutex;

/// Upper bound on distinct keys tracked in one window. Past this, requests from
/// new keys are refused until the window rolls over, so a flood of spoofed or
/// rotating addresses cannot grow the map without bound.
const MAX_TRACKED_KEYS: usize = 50_000;

/// Counts for the current 60-second window only. Every key shares the same
/// window boundaries, so older windows are dropped wholesale on rollover.
struct Windows {
    window: u64,
    counts: HashMap<String, u32>,
}

/// Fixed-window per-key limiter. Window is 60 seconds.
pub struct RateLimiter {
    limit: u32,
    max_keys: usize,
    windows: Mutex<Windows>,
}

impl RateLimiter {
    pub fn new(limit: u32) -> Self {
        Self::with_capacity(limit, MAX_TRACKED_KEYS)
    }

    fn with_capacity(limit: u32, max_keys: usize) -> Self {
        Self {
            limit,
            max_keys,
            windows: Mutex::new(Windows {
                window: 0,
                counts: HashMap::new(),
            }),
        }
    }

    /// Returns true if the request is allowed, false if the key is over its limit.
    pub fn check(&self, key: &str, now_secs: u64) -> bool {
        let window = now_secs / 60;
        let mut w = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        if w.window != window {
            // Every tracked entry belongs to the expired window: drop them all
            // (and release the memory) instead of keeping stale keys forever.
            w.window = window;
            w.counts = HashMap::new();
        }
        if let Some(count) = w.counts.get_mut(key) {
            if *count >= self.limit {
                return false;
            }
            *count += 1;
            return true;
        }
        if self.limit == 0 || w.counts.len() >= self.max_keys {
            return false;
        }
        w.counts.insert(key.to_string(), 1);
        true
    }

    #[cfg(test)]
    fn tracked_keys(&self) -> usize {
        self.windows.lock().unwrap().counts.len()
    }
}

/// Global (all clients) per-minute budget for GitHub writes: issue creations and
/// comments. The per-IP limit alone does not bound what many addresses can make
/// the maintainer's token do, so every write must first take from this budget.
pub struct WriteBudget {
    per_min: u32,
    state: Mutex<(u64, u32)>, // (window, writes taken)
}

impl WriteBudget {
    pub fn new(per_min: u32) -> Self {
        Self {
            per_min,
            state: Mutex::new((0, 0)),
        }
    }

    /// Take one write from the current window's budget. False when exhausted.
    pub fn try_take(&self, now_secs: u64) -> bool {
        let window = now_secs / 60;
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.0 != window {
            *s = (window, 0);
        }
        if s.1 >= self.per_min {
            return false;
        }
        s.1 += 1;
        true
    }
}

/// Rate-limit key for a client address taken from the `cf-connecting-ip` header.
///
/// IPv4 addresses are keyed as-is. IPv6 addresses are keyed by their /64 prefix,
/// since a single subscriber usually controls a whole /64 and could otherwise
/// rotate through addresses for a fresh limit each time. A missing or
/// unparsable value shares one "unknown" bucket rather than minting a new key.
pub fn rate_limit_key(raw: Option<&str>) -> String {
    match raw.map(str::trim).and_then(|s| s.parse::<IpAddr>().ok()) {
        Some(IpAddr::V4(v4)) => v4.to_string(),
        Some(IpAddr::V6(v6)) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return v4.to_string();
            }
            let s = v6.segments();
            let net = Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0);
            format!("{net}/64")
        }
        None => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_limit_then_blocks() {
        let rl = RateLimiter::new(3);
        assert!(rl.check("1.2.3.4", 100));
        assert!(rl.check("1.2.3.4", 101));
        assert!(rl.check("1.2.3.4", 102));
        assert!(!rl.check("1.2.3.4", 103), "4th in same window is blocked");
    }

    #[test]
    fn resets_in_next_window() {
        let rl = RateLimiter::new(1);
        assert!(rl.check("ip", 0));
        assert!(!rl.check("ip", 30));
        assert!(rl.check("ip", 60), "new 60s window resets the count");
    }

    #[test]
    fn keys_are_independent() {
        let rl = RateLimiter::new(1);
        assert!(rl.check("a", 0));
        assert!(rl.check("b", 0));
    }

    #[test]
    fn expired_windows_are_pruned() {
        let rl = RateLimiter::new(5);
        for i in 0..100 {
            assert!(rl.check(&format!("10.0.0.{i}"), 10));
        }
        assert_eq!(rl.tracked_keys(), 100);
        assert!(rl.check("10.0.1.1", 70));
        assert_eq!(rl.tracked_keys(), 1, "keys from the old window are dropped");
    }

    #[test]
    fn new_keys_are_refused_when_the_map_is_full() {
        let rl = RateLimiter::with_capacity(5, 2);
        assert!(rl.check("a", 0));
        assert!(rl.check("b", 0));
        assert!(!rl.check("c", 0), "third distinct key exceeds capacity");
        assert!(rl.check("a", 0), "already tracked keys keep working");
        assert_eq!(rl.tracked_keys(), 2);
        assert!(rl.check("c", 60), "capacity frees up on window rollover");
    }

    #[test]
    fn global_budget_is_shared_and_resets_per_window() {
        let b = WriteBudget::new(2);
        assert!(b.try_take(0));
        assert!(b.try_take(1));
        assert!(!b.try_take(59), "budget exhausted for this minute");
        assert!(b.try_take(60), "next window has a fresh budget");
    }

    #[test]
    fn zero_budget_refuses_everything() {
        let b = WriteBudget::new(0);
        assert!(!b.try_take(0));
    }

    #[test]
    fn ipv4_keys_are_exact() {
        assert_eq!(rate_limit_key(Some("203.0.113.7")), "203.0.113.7");
        assert_eq!(rate_limit_key(Some(" 203.0.113.7 ")), "203.0.113.7");
    }

    #[test]
    fn ipv6_keys_collapse_to_the_64() {
        let a = rate_limit_key(Some("2001:db8:1:2:aaaa:bbbb:cccc:dddd"));
        let b = rate_limit_key(Some("2001:db8:1:2::1"));
        assert_eq!(a, "2001:db8:1:2::/64");
        assert_eq!(a, b);
        assert_ne!(a, rate_limit_key(Some("2001:db8:1:3::1")));
    }

    #[test]
    fn ipv4_mapped_ipv6_is_keyed_as_ipv4() {
        assert_eq!(rate_limit_key(Some("::ffff:198.51.100.9")), "198.51.100.9");
    }

    #[test]
    fn missing_or_garbage_addresses_share_one_bucket() {
        assert_eq!(rate_limit_key(None), "unknown");
        assert_eq!(rate_limit_key(Some("not-an-ip")), "unknown");
        assert_eq!(rate_limit_key(Some("")), "unknown");
    }
}
