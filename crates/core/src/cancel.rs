// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Cooperative cancellation: [`CancelToken`] and [`Deadline`].
//!
//! New in Tidepool; not ported.
//!
//! A worker that runs a search, an index or a long parse checks its token
//! (and, if it has one, its deadline) between steps and stops early when
//! asked. Neither type blocks or waits.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A shared flag that asks work to stop.
///
/// Clones share the flag: cancel any clone and every clone sees it. Once
/// cancelled, a token stays cancelled.
///
/// ```
/// use tidepool_core::cancel::CancelToken;
///
/// let token = CancelToken::new();
/// let worker = token.clone();
/// assert!(!worker.is_cancelled());
/// token.cancel();
/// assert!(worker.is_cancelled());
/// ```
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// Creates a token that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the work that holds this token, or any clone of it, to stop.
    pub fn cancel(&self) {
        // Release: whatever the canceller wrote before cancelling is visible
        // to a worker that sees the flag.
        self.0.store(true, Ordering::Release);
    }

    /// Returns `true` once [`CancelToken::cancel`] has been called on this
    /// token or any clone of it.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// A point in time after which work should give up, or never.
///
/// Creating one never panics, however far away it is: a deadline beyond
/// what [`Instant`] can represent is treated as never.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Deadline {
    /// `None` means never.
    at: Option<Instant>,
}

impl Deadline {
    /// A deadline that never expires.
    pub const NEVER: Self = Self { at: None };

    /// A deadline `timeout` from now.
    pub fn after(timeout: Duration) -> Self {
        Self::after_from(Instant::now(), timeout)
    }

    /// A deadline at `instant`.
    pub const fn at(instant: Instant) -> Self {
        Self { at: Some(instant) }
    }

    /// The instant of the deadline, or `None` if it never expires.
    pub const fn instant(self) -> Option<Instant> {
        self.at
    }

    /// The time left until the deadline: zero once it has passed, and
    /// [`Duration::MAX`] for [`Deadline::NEVER`].
    pub fn remaining(self) -> Duration {
        self.remaining_from(Instant::now())
    }

    /// Returns `true` once the deadline has passed.
    pub fn expired(self) -> bool {
        self.expired_from(Instant::now())
    }

    fn after_from(now: Instant, timeout: Duration) -> Self {
        Self {
            at: now.checked_add(timeout),
        }
    }

    fn remaining_from(self, now: Instant) -> Duration {
        match self.at {
            Some(at) => at.saturating_duration_since(now),
            None => Duration::MAX,
        }
    }

    fn expired_from(self, now: Instant) -> bool {
        self.at.is_some_and(|at| now >= at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_cancellation() {
        let token = CancelToken::new();
        let clones = [token.clone(), token.clone()];
        assert!(!token.is_cancelled());
        clones[1].cancel();
        assert!(token.is_cancelled());
        assert!(clones[0].is_cancelled());
        // Cancelling again is harmless and it stays cancelled.
        token.cancel();
        assert!(token.is_cancelled());
        // Independent tokens are independent.
        assert!(!CancelToken::default().is_cancelled());
    }

    #[test]
    fn cancellation_is_seen_across_threads() {
        let token = CancelToken::new();
        let worker = token.clone();
        let handle = std::thread::spawn(move || {
            let mut spins: u64 = 0;
            while !worker.is_cancelled() {
                spins += 1;
                std::thread::yield_now();
            }
            spins
        });
        token.cancel();
        handle.join().expect("the worker stops when cancelled");
    }

    #[test]
    fn deadlines_count_down_and_saturate() {
        let now = Instant::now();
        let deadline = Deadline::after_from(now, Duration::from_secs(10));
        assert_eq!(deadline.instant(), now.checked_add(Duration::from_secs(10)));
        assert!(!deadline.expired_from(now));
        assert_eq!(deadline.remaining_from(now), Duration::from_secs(10));

        let later = now + Duration::from_secs(4);
        assert_eq!(deadline.remaining_from(later), Duration::from_secs(6));
        assert!(!deadline.expired_from(later));

        let at = now + Duration::from_secs(10);
        assert!(deadline.expired_from(at));
        assert_eq!(deadline.remaining_from(at), Duration::ZERO);

        let past = now + Duration::from_secs(60);
        assert!(deadline.expired_from(past));
        assert_eq!(deadline.remaining_from(past), Duration::ZERO);
    }

    #[test]
    fn a_zero_timeout_has_already_expired() {
        assert!(Deadline::after(Duration::ZERO).expired());
        assert_eq!(Deadline::after(Duration::ZERO).remaining(), Duration::ZERO);
        assert!(Deadline::at(Instant::now()).expired());
    }

    #[test]
    fn a_long_timeout_has_not_expired() {
        let deadline = Deadline::after(Duration::from_secs(3600));
        assert!(!deadline.expired());
        let left = deadline.remaining();
        assert!(left <= Duration::from_secs(3600) && left > Duration::from_secs(3500));
    }

    #[test]
    fn an_unrepresentable_deadline_is_never() {
        let deadline = Deadline::after(Duration::MAX);
        assert_eq!(deadline, Deadline::NEVER);
        assert_eq!(deadline.instant(), None);
        assert!(!deadline.expired());
        assert_eq!(deadline.remaining(), Duration::MAX);
    }
}
