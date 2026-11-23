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

//! The backdrop's builds (M3.8, ADR-0059): [`Builds`] decides when to start
//! building jabbott.dev's backdrop for the window's size and scale, and
//! which finished builds to keep; [`Threads`] runs each build on a thread
//! of its own.
//!
//! New in Tidepool.
//!
//! # Why the backdrop is built off the main thread
//!
//! Building the backdrop takes 99 to 147 ms at 1280 × 800, and 189 ms at
//! 1920 × 1080, in a release build on the reference machine (ADR-0045).
//! The main loop's rule is that anything taking more than about 1 ms runs
//! on another thread, so that typing never waits (ADR-0012). `gfx` starts
//! no threads, and [`Backdrop::ensure`] cannot be stopped part-way, so each
//! build runs to its end on a thread of its own, and `gui` keeps or drops
//! what it gives.
//!
//! # Scheduling
//!
//! - [`Builds::request`] asks for the backdrop of a size and scale. They
//!   become the *current* ones: a result for any other is *stale*.
//! - At most [`MAX_BUILDS`] builds run at once. A request made while that
//!   many run waits, and replaces any request already waiting, so during a
//!   drag only the latest size is built next. Two, not one, because a
//!   window manager may resize a new window at once, as COSMIC does about
//!   1 ms after the first expose: the build for the window's real size
//!   starts then, instead of after the first, wrong-sized one ends.
//! - A request for the current size and scale does nothing, and one for a
//!   size and scale already being built starts no second build of them.
//! - [`Builds::poll`] takes the builds that finished, and never waits. Each
//!   lets a waiting request start. A stale result is dropped. A build that
//!   failed, a thread that could not start, or one that ended without a
//!   result (as when it panics) logs a warning, and the window keeps its
//!   flat background until a resize asks again.
//!
//! [`Builds`] is generic over the [`Builder`] that starts builds and hands
//! their results back, so its tests run without threads.
//!
//! # Threads
//!
//! [`Threads`] starts each build on a thread named
//! [`THREAD_NAME`]. The thread builds a [`Backdrop`] of its own, sends it
//! over a channel and then wakes the window, through the function it was
//! given (the window's [`Waker`](tidepool_platform::events::Waker)), so
//! that the main loop's wait ends and [`Builds::poll`] takes it. A
//! thread still running when its `Gui` is dropped is left to finish: its
//! send then fails, and its wake does nothing anyone sees. One still
//! running when the process exits ends with it.

use std::fmt;
use std::io;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use tidepool_core::geom::{DeviceSize, Scale};
use tidepool_core::log::warn;
use tidepool_gfx::GfxError;
use tidepool_gfx::backdrop::{Backdrop, BackdropStyle};

/// The most builds that run at once.
pub(crate) const MAX_BUILDS: usize = 2;

/// The name of each build's thread, as debuggers and `top -H` show it.
pub(crate) const THREAD_NAME: &str = "tidepool-backdrop";

/// What a backdrop is built for: a frame's size and scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Want {
    /// The frame's size, in device pixels.
    pub(crate) size: DeviceSize,
    /// The window's scale.
    pub(crate) scale: Scale,
}

/// Why a build gave no backdrop.
#[derive(Debug)]
pub(crate) enum Failure {
    /// `gfx` could not build it.
    Gfx(GfxError),
    /// Its thread could not be started.
    Spawn(io::Error),
    /// Its thread ended without a result, as when the build panics.
    Lost,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gfx(error) => write!(f, "{error}"),
            Self::Spawn(error) => write!(f, "its thread did not start: {error}"),
            Self::Lost => f.write_str("its thread ended without a result"),
        }
    }
}

/// Starts builds, and hands back the ones that finished.
pub(crate) trait Builder {
    /// What a build makes: a [`Backdrop`], or a stand-in in tests.
    type Built;

    /// Starts building for `want`, without waiting for it.
    ///
    /// # Errors
    ///
    /// The error of starting the build's thread.
    fn start(&mut self, want: Want) -> io::Result<()>;

    /// A build that finished, and what it was for; `None` if none has
    /// finished since the last call. It never waits.
    fn take(&mut self) -> Option<(Want, Result<Self::Built, Failure>)>;
}

/// Which builds run, which waits, and which results to keep (the [module
/// documentation](self#scheduling)).
#[derive(Debug)]
pub(crate) struct Builds<B> {
    builder: B,
    /// What the latest request asked for; `None` before the first.
    current: Option<Want>,
    /// What the builds running now are for: at most [`MAX_BUILDS`], each
    /// once.
    running: Vec<Want>,
    /// The request waiting for a build to finish. When there is one, it is
    /// [`current`](Self::current).
    waiting: Option<Want>,
}

impl<B: Builder> Builds<B> {
    /// No builds yet, started by `builder`.
    pub(crate) const fn new(builder: B) -> Self {
        Self {
            builder,
            current: None,
            running: Vec::new(),
            waiting: None,
        }
    }

    /// Asks for the backdrop of `want`, which becomes the current size and
    /// scale: its build starts now if fewer than [`MAX_BUILDS`] run, or
    /// waits, in place of any request waiting already.
    pub(crate) fn request(&mut self, want: Want) {
        if self.current == Some(want) {
            return;
        }
        self.current = Some(want);
        if self.running.contains(&want) {
            // Being built already: what waits is for an older size.
            self.waiting = None;
        } else if self.running.len() < MAX_BUILDS {
            self.waiting = None;
            self.start(want);
        } else {
            self.waiting = Some(want);
        }
    }

    /// Takes the builds that finished, without waiting, and starts the
    /// waiting request when one of them frees its place. Returns the
    /// backdrop for the current size and scale if one came; drops stale
    /// ones, and logs a warning for a current one that failed.
    pub(crate) fn poll(&mut self) -> Option<B::Built> {
        let mut ready = None;
        while let Some((want, result)) = self.builder.take() {
            if let Some(at) = self.running.iter().position(|running| *running == want) {
                self.running.swap_remove(at);
            }
            if self.running.len() < MAX_BUILDS
                && let Some(next) = self.waiting.take()
            {
                self.start(next);
            }
            if self.current != Some(want) {
                continue;
            }
            match result {
                Ok(built) => ready = Some(built),
                Err(failure) => warn_failed(want, &failure),
            }
        }
        ready
    }

    /// Starts the build for `want`, or logs why it could not.
    fn start(&mut self, want: Want) {
        match self.builder.start(want) {
            Ok(()) => self.running.push(want),
            Err(error) => warn_failed(want, &Failure::Spawn(error)),
        }
    }
}

/// Logs that the backdrop for `want` was not built, and why.
fn warn_failed(want: Want, failure: &Failure) {
    warn!(
        "the backdrop of {} by {} px at scale {} was not built: {failure}; the window shows \
         its flat background until it is resized",
        want.size.width,
        want.size.height,
        want.scale.factor()
    );
}

/// Wakes the window's wait, from a build's thread.
pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

/// Builds a backdrop: [`build`], or a stand-in in tests.
type Build = fn(Want, &BackdropStyle) -> Result<Backdrop, GfxError>;

/// What a build's thread sends: what it built for, and its result, `None`
/// if it ended without one.
type Done = (Want, Option<Result<Backdrop, GfxError>>);

/// Runs each build on a thread of its own (the [module
/// documentation](self#threads)).
pub(crate) struct Threads {
    style: Arc<BackdropStyle>,
    wake: Wake,
    build: Build,
    sender: Sender<Done>,
    receiver: Receiver<Done>,
}

impl Threads {
    /// Builds in `style`, and calls `wake` after each build sent its
    /// result.
    pub(crate) fn new(style: BackdropStyle, wake: Wake) -> Self {
        Self::with_build(style, wake, build)
    }

    /// The same, building with `build`.
    fn with_build(style: BackdropStyle, wake: Wake, build: Build) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            style: Arc::new(style),
            wake,
            build,
            sender,
            receiver,
        }
    }
}

/// Shows the style, not the channel or the functions.
impl fmt::Debug for Threads {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Threads")
            .field("style", &self.style)
            .finish_non_exhaustive()
    }
}

impl Builder for Threads {
    type Built = Backdrop;

    fn start(&mut self, want: Want) -> io::Result<()> {
        let (style, build) = (Arc::clone(&self.style), self.build);
        let (sender, wake) = (self.sender.clone(), Arc::clone(&self.wake));
        thread::Builder::new()
            .name(THREAD_NAME.to_string())
            .spawn(move || {
                // Made on the thread, so that a thread that never starts
                // reports nothing: `start`'s error says it.
                let mut report = Report {
                    want,
                    result: None,
                    sender,
                    wake,
                };
                report.result = Some(build(want, &style));
            })?;
        Ok(())
    }

    fn take(&mut self) -> Option<(Want, Result<Backdrop, Failure>)> {
        // The channel never closes: this holds a sender.
        let (want, result) = self.receiver.try_recv().ok()?;
        let result = result.map_or(Err(Failure::Lost), |built| built.map_err(Failure::Gfx));
        Some((want, result))
    }
}

/// Builds the backdrop of `want` in `style`, in a cache of its own.
fn build(want: Want, style: &BackdropStyle) -> Result<Backdrop, GfxError> {
    let mut backdrop = Backdrop::new();
    backdrop.ensure(want.size, want.scale, style)?;
    Ok(backdrop)
}

/// A build's report: sent, then the window woken, when it is dropped at
/// the end of the build's thread, even as a panic unwinds it.
struct Report {
    want: Want,
    result: Option<Result<Backdrop, GfxError>>,
    sender: Sender<Done>,
    wake: Wake,
}

impl Drop for Report {
    fn drop(&mut self) {
        // The send fails only once the `Gui` is gone, and then nobody needs
        // the result.
        let _ = self.sender.send((self.want, self.result.take()));
        (self.wake)();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use tidepool_core::log;

    use super::*;

    /// Starts nothing: records what it was asked to start, and hands back
    /// what a test says finished. What it "builds" is the want itself.
    #[derive(Debug, Default)]
    struct Fake {
        started: Vec<Want>,
        /// Whether starting fails, as when no thread can be had.
        refuse: bool,
        done: VecDeque<(Want, Result<Want, Failure>)>,
    }

    impl Builder for Fake {
        type Built = Want;

        fn start(&mut self, want: Want) -> io::Result<()> {
            if self.refuse {
                return Err(io::Error::other("no thread to be had"));
            }
            self.started.push(want);
            Ok(())
        }

        fn take(&mut self) -> Option<(Want, Result<Want, Failure>)> {
            self.done.pop_front()
        }
    }

    fn want(width: i32, height: i32) -> Want {
        Want {
            size: DeviceSize::new(width, height),
            scale: Scale::ONE,
        }
    }

    fn builds() -> Builds<Fake> {
        Builds::new(Fake::default())
    }

    /// Says that the build for `want` finished with `result`.
    fn finish(builds: &mut Builds<Fake>, want: Want, result: Result<Want, Failure>) {
        builds.builder.done.push_back((want, result));
    }

    /// Whether the log's last lines hold one with all of `parts`.
    fn logged(parts: &[&str]) -> bool {
        log::tail(log::RING_CAPACITY)
            .iter()
            .any(|line| line.contains(" WARN ") && parts.iter().all(|part| line.contains(part)))
    }

    #[test]
    fn at_most_two_run_and_a_newer_request_replaces_the_waiting_one() {
        let mut builds = builds();
        let (a, b, c, d) = (
            want(800, 600),
            want(810, 600),
            want(820, 600),
            want(830, 600),
        );
        builds.request(a);
        builds.request(b);
        assert_eq!(builds.builder.started, [a, b]);
        builds.request(c);
        builds.request(d);
        assert_eq!(builds.builder.started, [a, b], "two run, and the rest wait");
        assert_eq!(
            builds.waiting,
            Some(d),
            "the newer request replaced the older"
        );

        // A stale result frees its place for the waiting request, the
        // newest; the one it replaced never runs.
        finish(&mut builds, a, Ok(a));
        assert_eq!(builds.poll(), None, "a stale backdrop is dropped");
        assert_eq!(builds.builder.started, [a, b, d]);
        assert_eq!(builds.waiting, None);
        finish(&mut builds, b, Ok(b));
        assert_eq!(builds.poll(), None);
        assert_eq!(builds.builder.started, [a, b, d], "nothing waits");
        finish(&mut builds, d, Ok(d));
        assert_eq!(builds.poll(), Some(d), "the current size's backdrop");
        assert!(builds.running.is_empty());
    }

    #[test]
    fn a_stale_result_is_dropped_and_the_waiting_request_then_starts() {
        let mut builds = builds();
        let (first, second, third) = (want(1280, 800), want(1232, 662), want(1000, 700));
        builds.request(first);
        builds.request(second);
        builds.request(third);
        // `second` is stale now, and `first` too.
        finish(&mut builds, second, Ok(second));
        assert_eq!(builds.poll(), None);
        assert_eq!(builds.builder.started, [first, second, third]);
        finish(&mut builds, third, Ok(third));
        finish(&mut builds, first, Ok(first));
        assert_eq!(
            builds.poll(),
            Some(third),
            "of two in one poll, the current"
        );
        assert!(builds.running.is_empty());
    }

    #[test]
    fn a_request_for_the_current_size_or_one_being_built_starts_nothing() {
        let mut builds = builds();
        let (a, b, c) = (want(640, 480), want(641, 480), want(642, 480));
        builds.request(a);
        builds.request(a);
        assert_eq!(builds.builder.started, [a]);
        builds.request(b);
        builds.request(c);
        assert_eq!(builds.waiting, Some(c));
        // Back to `a`, which is still being built: it is current again, and
        // `c` no longer waits.
        builds.request(a);
        assert_eq!(builds.builder.started, [a, b]);
        assert_eq!(builds.waiting, None);
        finish(&mut builds, b, Ok(b));
        finish(&mut builds, a, Ok(a));
        assert_eq!(builds.poll(), Some(a));
        assert_eq!(builds.builder.started, [a, b]);

        // Another scale is another backdrop.
        let double = Want {
            scale: Scale::new(2.0).expect("a scale"),
            ..a
        };
        builds.request(double);
        assert_eq!(builds.builder.started, [a, b, double]);
    }

    #[test]
    fn a_failed_build_leaves_the_flat_base_and_logs_a_warning() {
        let mut builds = builds();
        let failed = want(1237, 661);
        builds.request(failed);
        finish(
            &mut builds,
            failed,
            Err(Failure::Gfx(GfxError::OutOfMemory { bytes: 64 })),
        );
        assert_eq!(builds.poll(), None);
        assert!(builds.running.is_empty());
        assert!(
            logged(&[
                "the backdrop of 1237 by 661 px at scale 1 was not built: ",
                "until"
            ]),
            "{:#?}",
            log::tail(8)
        );
        // The next resize asks again.
        let next = want(1238, 661);
        builds.request(next);
        assert_eq!(builds.builder.started, [failed, next]);

        // A stale failure is dropped without a word.
        let stale = want(1239, 661);
        builds.request(stale);
        finish(&mut builds, next, Err(Failure::Lost));
        assert_eq!(builds.poll(), None);
        assert!(!logged(&["the backdrop of 1238 by 661 px"]));
    }

    #[test]
    fn a_thread_that_cannot_start_is_a_failure() {
        let mut builds = builds();
        builds.builder.refuse = true;
        let refused = want(1241, 663);
        builds.request(refused);
        assert!(builds.running.is_empty(), "nothing runs");
        assert!(
            logged(&[
                "the backdrop of 1241 by 663 px at scale 1 was not built: its thread did not \
                 start: no thread to be had"
            ]),
            "{:#?}",
            log::tail(8)
        );
        // The next request starts at once, nothing having taken a place.
        builds.builder.refuse = false;
        let next = want(1242, 663);
        builds.request(next);
        assert_eq!(builds.builder.started, [next]);
    }

    #[test]
    fn a_waiting_request_that_cannot_start_is_a_failure_too() {
        let mut builds = builds();
        let (a, b, c) = (want(1251, 700), want(1252, 700), want(1253, 700));
        builds.request(a);
        builds.request(b);
        builds.request(c);
        builds.builder.refuse = true;
        finish(&mut builds, a, Ok(a));
        assert_eq!(builds.poll(), None);
        assert_eq!(builds.running, [b]);
        assert_eq!(builds.waiting, None);
        assert!(logged(&[
            "the backdrop of 1253 by 700 px at scale 1 was not built"
        ]));
    }

    /// A wake that counts its calls.
    fn counting() -> (Wake, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&count);
        let wake: Wake = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        (wake, count)
    }

    /// Polls `threads` until a build finishes, for at most 5 s.
    fn next(threads: &mut Threads) -> (Want, Result<Backdrop, Failure>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(done) = threads.take() {
                return done;
            }
            assert!(Instant::now() < deadline, "no build finished in 5 s");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_thread_builds_the_backdrop_sends_it_and_then_wakes_the_window() {
        let (wake, count) = counting();
        let style = BackdropStyle::jabbott_dev();
        let mut threads = Threads::new(style.clone(), wake);
        assert!(threads.take().is_none(), "nothing yet");
        let small = Want {
            size: DeviceSize::new(24, 16),
            scale: Scale::new(1.5).expect("a scale"),
        };
        threads.start(small).expect("a thread");
        let (built_for, result) = next(&mut threads);
        assert_eq!(built_for, small);
        let backdrop = result.expect("the backdrop");
        assert_eq!(backdrop.base().size(), small.size);
        let mut alone = Backdrop::new();
        alone
            .ensure(small.size, small.scale, &style)
            .expect("the backdrop");
        assert_eq!(backdrop.base().bytes(), alone.base().bytes());
        // The wake comes after the send.
        let deadline = Instant::now() + Duration::from_secs(5);
        while count.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "no wake in 5 s");
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_build_gfx_refuses_is_reported_as_its_error() {
        let (wake, _) = counting();
        let mut threads = Threads::new(BackdropStyle::jabbott_dev(), wake);
        let negative = want(-1, 10);
        threads.start(negative).expect("a thread");
        let (built_for, result) = next(&mut threads);
        assert_eq!(built_for, negative);
        assert!(
            matches!(result, Err(Failure::Gfx(GfxError::NegativeSize { .. }))),
            "{result:?}"
        );
    }

    #[test]
    fn a_build_that_panics_is_reported_lost() {
        /// Panics, as a bug in `gfx` would.
        fn panics(_: Want, _: &BackdropStyle) -> Result<Backdrop, GfxError> {
            panic!("a build that panics, on purpose: the test expects this message");
        }
        let (wake, count) = counting();
        let mut threads = Threads::with_build(BackdropStyle::jabbott_dev(), wake, panics);
        threads.start(want(8, 8)).expect("a thread");
        let (built_for, result) = next(&mut threads);
        assert_eq!(built_for, want(8, 8));
        assert!(matches!(result, Err(Failure::Lost)), "{result:?}");
        let deadline = Instant::now() + Duration::from_secs(5);
        while count.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "no wake in 5 s");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn each_failure_says_what_went_wrong() {
        assert_eq!(
            Failure::Gfx(GfxError::OutOfMemory { bytes: 64 }).to_string(),
            GfxError::OutOfMemory { bytes: 64 }.to_string()
        );
        assert_eq!(
            Failure::Spawn(io::Error::other("no")).to_string(),
            "its thread did not start: no"
        );
        assert_eq!(
            Failure::Lost.to_string(),
            "its thread ended without a result"
        );
    }
}
