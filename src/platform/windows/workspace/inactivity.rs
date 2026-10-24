use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Policy {
    pub keep_running: bool,
    inactive_since: Option<Instant>,
    observed: Option<Instant>,
    expired: bool,
}

impl Policy {
    pub fn reset(&mut self) {
        self.inactive_since = None;
        self.observed = None;
        self.expired = false;
    }
    pub fn keep(&mut self, value: bool) {
        self.keep_running = value;
        self.reset();
    }
    pub fn remaining(&self, now: Instant, timeout: Option<u32>) -> Option<u64> {
        let duration = Duration::from_secs(timeout? as u64 * 60);
        self.inactive_since.map(|since| {
            duration
                .saturating_sub(now.saturating_duration_since(since))
                .as_secs()
        })
    }
    pub fn observe(
        &mut self,
        now: Instant,
        active: bool,
        available: bool,
        timeout: Option<u32>,
    ) -> bool {
        let interrupted = self
            .observed
            .is_some_and(|last| now.saturating_duration_since(last) > Duration::from_secs(2));
        self.observed = Some(now);
        if active || !available || self.keep_running || timeout.is_none() || interrupted {
            self.inactive_since = None;
            if active || !available || interrupted {
                self.expired = false;
            }
            return false;
        }
        if self.expired {
            return false;
        }
        let since = *self.inactive_since.get_or_insert(now);
        if now.saturating_duration_since(since) >= Duration::from_secs(timeout.unwrap() as u64 * 60)
        {
            self.expired = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_locked_sleep_and_keep_running_reset_countdowns() {
        let start = Instant::now();
        let mut policy = Policy::default();
        for tick in 0..61 {
            assert!(!policy.observe(start + Duration::from_secs(tick), true, true, Some(1)));
        }
        for tick in 61..121 {
            assert!(!policy.observe(start + Duration::from_secs(tick), false, true, Some(1)));
        }
        assert!(policy.observe(start + Duration::from_secs(121), false, true, Some(1)));
        assert!(!policy.observe(start + Duration::from_secs(122), false, true, Some(1)));
        policy.keep(true);
        assert!(!policy.observe(start + Duration::from_secs(123), false, true, Some(1)));
        policy.keep(false);
        assert!(!policy.observe(start + Duration::from_secs(124), false, false, Some(1)));
        assert_eq!(
            policy.remaining(start + Duration::from_secs(124), Some(1)),
            None
        );
        assert!(!policy.observe(start + Duration::from_secs(125), false, true, Some(1)));
        assert!(!policy.observe(start + Duration::from_secs(300), false, true, Some(1)));
        assert_eq!(
            policy.remaining(start + Duration::from_secs(300), Some(1)),
            None
        );
    }
}
