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

//! The one thread that may call Xlib, and the handler of X errors.
//!
//! New in Tidepool. Nadir lets one connection at a time keep Xlib's input
//! method state; Tidepool keeps all of Xlib to one thread.
//!
//! # The Xlib thread
//!
//! Xlib's calls in the process must never overlap (`tidepool_sys`'s
//! `linux::x11`, "Calling Xlib"): Xlib keeps state for the whole process,
//! such as the error handlers and the locale, and Tidepool never calls
//! `XInitThreads`. A [`Claim`] makes sure of it. The first window to open
//! takes the claim for its thread; each later window on that thread takes
//! it too; and a window on any other thread fails to open with
//! [`Error::WrongThread`] while any of them is open. When the last one is
//! dropped, the claim is free for any thread. Every Xlib call in this
//! backend is made by a window, or while one is being opened, on the
//! claiming thread: each holds a `Claim`, which is neither `Send` nor
//! `Sync`, until its connection is closed. Loading `libX11.so.6` happens
//! under the claim too.
//!
//! # X errors
//!
//! Xlib's default handler of an X error, a request the server refused,
//! prints it and ends the process. [`install_error_handler`] replaces it,
//! for the whole process, with [`record_error`], which counts the error and
//! keeps the last one for [`take_errors`]: a window logs them as it waits
//! for events, and fails to open if the server refused a request of its
//! opening. The handler of a lost connection stays Xlib's own, which
//! prints a message and ends the process: a lost X server is not
//! recovered from yet (crash handling is M4.9's).

use core::ffi::c_int;
use core::marker::PhantomData;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};

use tidepool_sys::linux::x11::{Display, XErrorEvent, Xlib};

use crate::Error;

/// Which thread has the claim, and for how many windows.
struct State {
    /// The thread, while `windows` is not 0.
    thread: Option<ThreadId>,
    /// How many [`Claim`]s exist.
    windows: usize,
}

/// The claim, for the whole process.
static STATE: Mutex<State> = Mutex::new(State {
    thread: None,
    windows: 0,
});

/// Locks [`STATE`]. Nothing panics while holding it, but if something did,
/// the state would still be whole, so a poisoned lock is taken as it is.
fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// This thread's right to call Xlib, held by a window (or a window being
/// opened) until its connection is closed. Neither `Send` nor `Sync`, so it
/// is dropped on the thread that took it.
pub(super) struct Claim {
    _not_send: PhantomData<*const ()>,
}

impl Claim {
    /// Takes the claim for this thread.
    ///
    /// # Errors
    ///
    /// [`Error::WrongThread`] if another thread has it.
    pub(super) fn take() -> Result<Self, Error> {
        let this = thread::current().id();
        let mut state = state();
        if state.thread.is_some_and(|owner| owner != this) {
            return Err(Error::WrongThread);
        }
        state.thread = Some(this);
        // Each claim belongs to a window with a connection of its own, so
        // this never comes near `usize::MAX`.
        state.windows = state.windows.saturating_add(1);
        Ok(Self {
            _not_send: PhantomData,
        })
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let mut state = state();
        state.windows = state.windows.saturating_sub(1);
        if state.windows == 0 {
            state.thread = None;
        }
    }
}

/// How many X errors [`record_error`] has seen since [`take_errors`] last
/// took them.
static ERROR_COUNT: AtomicU64 = AtomicU64::new(0);

/// The last X error's codes: its error code, then the major and minor
/// opcodes of its request, one byte each from the lowest.
static LAST_ERROR_CODES: AtomicU32 = AtomicU32::new(0);

/// The serial number of the last X error's request.
static LAST_ERROR_SERIAL: AtomicU64 = AtomicU64::new(0);

/// Requests the X server refused since they were last taken, and the last
/// of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct XErrors {
    /// How many.
    pub(super) count: u64,
    /// What went wrong with the last: `BadWindow` and so on (`X11/X.h`).
    pub(super) error_code: u8,
    /// The major opcode of the last one's request (`X11/Xproto.h`).
    pub(super) request_code: u8,
    /// The minor opcode of the last one's request.
    pub(super) minor_code: u8,
    /// The serial number of the last one's request.
    pub(super) serial: u64,
}

/// Returns the X errors since the last call, if any, and forgets them.
pub(super) fn take_errors() -> Option<XErrors> {
    // Errors are recorded and taken on the Xlib thread, so the order of
    // these loads among themselves does not matter.
    let count = ERROR_COUNT.swap(0, Ordering::Relaxed);
    if count == 0 {
        return None;
    }
    let [error_code, request_code, minor_code, _] =
        LAST_ERROR_CODES.load(Ordering::Relaxed).to_le_bytes();
    Some(XErrors {
        count,
        error_code,
        request_code,
        minor_code,
        serial: LAST_ERROR_SERIAL.load(Ordering::Relaxed),
    })
}

/// Makes [`record_error`] Xlib's handler of X errors, for every display of
/// the process, in place of the one that ends the process. It stays
/// installed until the process exits; installing it again changes nothing.
pub(super) fn install_error_handler(xlib: &Xlib, _claim: &Claim) {
    // SAFETY: the claim puts this call on the Xlib thread, and the handler
    // does what an X error handler may (see `record_error`). The handler it
    // replaces is Xlib's default, or this one from an earlier window:
    // nothing else in the process installs one, so it is not kept.
    unsafe { (xlib.XSetErrorHandler)(Some(record_error)) };
}

/// What Xlib calls for each X error: counts it and keeps its codes, for
/// [`take_errors`].
///
/// It makes no Xlib call, allocates nothing and cannot panic, as Xlib's
/// manual asks of a handler that is called from inside its calls. The
/// result is ignored.
///
/// # Safety
///
/// Only Xlib calls it, with null or an `XErrorEvent` that is valid for the
/// call.
unsafe extern "C" fn record_error(_display: *mut Display, event: *mut XErrorEvent) -> c_int {
    // SAFETY: Xlib passes an error event that is valid for the call, or
    // null, which `as_ref` gives as `None`.
    if let Some(event) = unsafe { event.as_ref() } {
        let codes = u32::from_le_bytes([event.error_code, event.request_code, event.minor_code, 0]);
        LAST_ERROR_CODES.store(codes, Ordering::Relaxed);
        // A serial is an `unsigned long`, a `u64` on these targets.
        LAST_ERROR_SERIAL.store(event.serial, Ordering::Relaxed);
    }
    ERROR_COUNT.fetch_add(1, Ordering::Relaxed);
    0
}

/// Makes the tests that take the claim, record errors or open windows run
/// one at a time: libtest runs tests on threads of its own, and the claim
/// and the errors are the whole process's. Each such test holds this for
/// as long as it runs.
#[cfg(test)]
pub(super) fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use core::ptr;

    use super::*;

    #[test]
    fn a_thread_takes_the_claim_as_often_as_it_likes_and_others_wait_for_the_last() {
        let _serial = serial();
        let first = Claim::take().expect("a free claim");
        let second = Claim::take().expect("this thread's own claim");
        let elsewhere = || {
            thread::spawn(|| Claim::take().map(drop))
                .join()
                .expect("joined")
        };

        assert!(matches!(elsewhere(), Err(Error::WrongThread)));
        drop(first);
        assert!(matches!(elsewhere(), Err(Error::WrongThread)));
        drop(second);
        // Free again, for any thread.
        assert!(elsewhere().is_ok());
        assert!(Claim::take().is_ok());
    }

    #[test]
    fn an_error_is_counted_and_the_last_one_kept() {
        let _serial = serial();
        let _ = take_errors();
        assert_eq!(take_errors(), None);

        let event = |error_code: u8, request_code: u8, serial: u64| XErrorEvent {
            type_: 0,
            display: ptr::null_mut(),
            resourceid: 0,
            serial,
            error_code,
            request_code,
            minor_code: 7,
        };
        for (code, request, serial) in [(3, 8, 100), (8, 72, 101)] {
            let mut error = event(code, request, serial);
            // SAFETY: the event is valid for the call, and the handler
            // keeps nothing of it.
            let result = unsafe { record_error(ptr::null_mut(), &raw mut error) };
            assert_eq!(result, 0);
        }
        assert_eq!(
            take_errors(),
            Some(XErrors {
                count: 2,
                error_code: 8,
                request_code: 72,
                minor_code: 7,
                serial: 101,
            })
        );
        assert_eq!(take_errors(), None);

        // A null event is counted, and keeps the codes of the last.
        // SAFETY: the handler takes null for the event.
        unsafe { record_error(ptr::null_mut(), ptr::null_mut()) };
        assert_eq!(
            take_errors().map(|errors| (errors.count, errors.serial)),
            Some((1, 101))
        );
    }
}
