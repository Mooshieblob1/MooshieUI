use std::collections::HashMap;
use std::sync::Mutex;

/// Fixed-window per-key limiter. Window is 60 seconds.
pub struct RateLimiter {
    limit: u32,
    windows: Mutex<HashMap<String, (u64, u32)>>, // key -> (window_start_secs, count)
}

impl RateLimiter {
    pub fn new(limit: u32) -> Self {
        Self {
            limit,
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Returns true if the request is allowed, false if the key is over its limit.
    pub fn check(&self, key: &str, now_secs: u64) -> bool {
        let window = now_secs / 60;
        let mut map = self.windows.lock().unwrap();
        let entry = map.entry(key.to_string()).or_insert((window, 0));
        if entry.0 != window {
            *entry = (window, 0);
        }
        if entry.1 >= self.limit {
            return false;
        }
        entry.1 += 1;
        true
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
}
