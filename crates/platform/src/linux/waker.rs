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

//! The self-pipe that wakes a window's wait: [`Waker`] writes a byte to it,
//! and the wait polls its read end with the X connection.
//!
//! New in Tidepool; not ported (Nadir's loop does not wait).
//!
//! The pipe comes from `pipe2` with `O_CLOEXEC | O_NONBLOCK`
//! (`tidepool_sys::linux::libc` says why not `std::io::pipe` or a socket
//! pair). Both ends live in one [`SelfPipe`], which the window and every
//! waker share, so the read end is open as long as anything can write: a
//! write never meets `EPIPE`, and never raises `SIGPIPE`. A wake is one
//! `write(2)` of one byte, which is async-signal-safe; the wait reads the
//! pipe empty through `File`.

use core::ffi::c_int;
use std::fs::File;
use std::io::{self, ErrorKind, Read as _};
use std::os::fd::{AsFd as _, AsRawFd as _, BorrowedFd, FromRawFd as _, OwnedFd};
use std::sync::Arc;

use tidepool_sys::linux::libc::{O_CLOEXEC, O_NONBLOCK, pipe2, write};

use crate::Error;

/// The two ends of a window's self-pipe, both close-on-exec and
/// non-blocking.
pub(super) struct SelfPipe {
    /// The end the wait polls and reads.
    read: File,
    /// The end a [`Waker`] writes to.
    write: OwnedFd,
}

impl SelfPipe {
    /// Makes a pipe.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if `pipe2` fails, as when the process has too many
    /// descriptors open.
    pub(super) fn new() -> Result<Arc<Self>, Error> {
        let mut fds: [c_int; 2] = [-1; 2];
        // SAFETY: `fds` is two `c_int`s that only this function uses, and
        // `pipe2` writes nothing else.
        let result = unsafe { pipe2(fds.as_mut_ptr(), O_CLOEXEC | O_NONBLOCK) };
        if result != 0 {
            return Err(Error::Io {
                call: "pipe2",
                source: io::Error::last_os_error(),
            });
        }
        // SAFETY: `pipe2` succeeded, so `fds[0]` is an open descriptor that
        // nothing else owns or closes.
        let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        // SAFETY: as above, for `fds[1]`.
        let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        Ok(Arc::new(Self {
            read: File::from(read),
            write,
        }))
    }

    /// The end to poll for a wake.
    pub(super) fn read_end(&self) -> BorrowedFd<'_> {
        self.read.as_fd()
    }

    /// Reads every byte waiting in the pipe, and returns whether there was
    /// any: whether something woke the window since the last call.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if reading fails for a reason other than an empty
    /// pipe or a signal.
    pub(super) fn drain(&self) -> Result<bool, Error> {
        let mut woken = false;
        let mut buffer = [0_u8; 64];
        loop {
            match (&self.read).read(&mut buffer) {
                // The write end is open while `self` is, so the pipe never
                // ends; if it did, there would be nothing more to read.
                Ok(0) => return Ok(woken),
                Ok(_) => woken = true,
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(woken),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(source) => {
                    return Err(Error::Io {
                        call: "read",
                        source,
                    });
                }
            }
        }
    }
}

/// Wakes a window's wait from any thread, by writing a byte to its
/// self-pipe.
#[derive(Clone)]
pub(crate) struct Waker {
    pipe: Arc<SelfPipe>,
}

impl Waker {
    /// A waker for the window whose self-pipe this is.
    pub(super) fn new(pipe: Arc<SelfPipe>) -> Self {
        Self { pipe }
    }

    /// Writes one byte to the pipe, with one `write(2)`, which is
    /// async-signal-safe, and nothing else that is not.
    pub(crate) fn wake(&self) {
        let byte = [1_u8];
        // The result needs no handling. If the byte was not written, the
        // pipe is full (`EAGAIN`): bytes are already waiting, so the wait
        // wakes anyway. Nothing else can fail: the write end does not
        // block, so no signal interrupts it (`EINTR`); the read end is
        // open while this pipe is (`EPIPE`); and the descriptor is open
        // (`EBADF`).
        //
        // SAFETY: `byte` is one readable byte, and the write end is open:
        // `self.pipe` owns it, and closes it only when the last `Arc` goes,
        // which is not before this call returns.
        let _ = unsafe { write(self.pipe.write.as_raw_fd(), byte.as_ptr().cast(), 1) };
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::thread;

    use super::*;

    /// The file status flags of `fd`, with `O_CLOEXEC` for its
    /// close-on-exec flag, from `/proc/self/fdinfo`, or `None` if that is
    /// not mounted.
    fn flags(fd: BorrowedFd<'_>) -> Option<c_int> {
        let info = fs::read_to_string(format!("/proc/self/fdinfo/{}", fd.as_raw_fd())).ok()?;
        let octal = info.lines().find_map(|line| line.strip_prefix("flags:"))?;
        c_int::from_str_radix(octal.trim(), 8).ok()
    }

    #[test]
    fn both_ends_are_close_on_exec_and_non_blocking() {
        let pipe = SelfPipe::new().expect("a pipe");
        for fd in [pipe.read_end(), pipe.write.as_fd()] {
            if let Some(flags) = flags(fd) {
                assert_eq!(flags & O_CLOEXEC, O_CLOEXEC, "{flags:#o}");
                assert_eq!(flags & O_NONBLOCK, O_NONBLOCK, "{flags:#o}");
            }
        }
    }

    #[test]
    fn a_wake_is_read_once_however_many_came() {
        let pipe = SelfPipe::new().expect("a pipe");
        assert_eq!(pipe.drain().ok(), Some(false));
        let waker = Waker::new(Arc::clone(&pipe));
        waker.wake();
        waker.clone().wake();
        assert_eq!(pipe.drain().ok(), Some(true));
        assert_eq!(pipe.drain().ok(), Some(false));
    }

    #[test]
    fn a_full_pipe_takes_more_wakes_without_blocking() {
        let pipe = SelfPipe::new().expect("a pipe");
        if flags(pipe.write.as_fd()).is_some_and(|flags| flags & O_NONBLOCK == 0) {
            panic!("the write end would block");
        }
        let waker = Waker::new(Arc::clone(&pipe));
        // A pipe holds 64 KiB unless it is resized; 1 MiB of wakes fills
        // it many times over, and each wake past that returns at once.
        for _ in 0..(1 << 20) {
            waker.wake();
        }
        assert_eq!(pipe.drain().ok(), Some(true));
        assert_eq!(pipe.drain().ok(), Some(false));
    }

    #[test]
    fn a_waker_on_another_thread_wakes_and_outlives_the_window() {
        let pipe = SelfPipe::new().expect("a pipe");
        let waker = Waker::new(Arc::clone(&pipe));
        thread::spawn(move || waker.wake()).join().expect("joined");
        assert_eq!(pipe.drain().ok(), Some(true));

        // The window goes first; its waker still writes, to a pipe that
        // stays open, and no one reads.
        let waker = Waker::new(Arc::clone(&pipe));
        drop(pipe);
        waker.wake();
    }
}
