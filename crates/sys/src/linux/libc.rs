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

//! The C library on Linux (glibc): the dynamic loader, and `poll` and
//! `pipe2`, which the event loop is to wait in and its waker to write to
//! (M2.2).
//!
//! Ported in part from nadir-sys `linux/libc.rs`: the loader, its flags and
//! its tests, with Tidepool's names and reworded comments (the header
//! quotes gain `extern`), and one more assertion in the test of a missing
//! library: reading `dlerror`'s message clears it.
//! Nadir's error numbers for ALSA are left out; `poll`, `pipe2` and their
//! tests are new.
//!
//! Sources: `<dlfcn.h>`, and `<bits/dlfcn.h>` for the flag values;
//! `<sys/poll.h>` (which `<poll.h>` includes), and `<bits/poll.h>` for the
//! event bits; `<unistd.h>` for `pipe2`, and `<bits/fcntl-linux.h>` (which
//! `<fcntl.h>` includes) for its flags, whose values are the kernel's
//! `<asm-generic/fcntl.h>`. Each item gives its declaration there, without
//! glibc's attribute macros (`__THROW`, `__wur` and the like). x86_64 and
//! aarch64 both use glibc's generic `bits/` headers for these, and the
//! kernel's generic flags.
//!
//! Only what `tidepool-platform` is to call (M2.2) is declared, and nothing
//! `std` already gives: an `errno` is read with
//! `std::io::Error::last_os_error`, the two descriptors from `pipe2` are
//! each wrapped in `std::os::fd::OwnedFd`, which closes it, and `read` and
//! `write` on them go through `std::fs::File`, except in code that must be
//! async-signal-safe, for which `std` promises nothing: the tasks that
//! write `Waker::wake` and the terminal's signal handler (M2.2, M7.1) are
//! to declare `write` here. The
//! waker's self-pipe needs `pipe2`: `std::io::pipe` makes close-on-exec
//! ends, but cannot make them non-blocking without `fcntl`, which is
//! variadic, and the waker's write end must never block. A socket pair
//! would do the same, but it would be a socket of Tidepool's own, against
//! `AGENTS.md` rule 8. The declarations for
//! processes, the PTY, the terminal, signals and file watching are added by
//! the tasks that call them (M5.7, M5.9, M7.1, M4.5).
//!
//! A library that Tidepool loads is to stay loaded until the process
//! exits, so `dlclose` is not declared: the function pointers of a table
//! loaded from one can never dangle.

// `pollfd` and `nfds_t` keep the names their header gives them.
#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int, c_short, c_ulong, c_void};
use core::mem::offset_of;

// ---------------------------------------------------------------------------
// The dynamic loader
// ---------------------------------------------------------------------------

/// A [`dlopen`] flag: resolve every undefined symbol of the library before
/// `dlopen` returns, and fail if one cannot be resolved.
/// `bits/dlfcn.h`: `#define RTLD_NOW 0x00002`.
pub const RTLD_NOW: c_int = 0x00002;

/// A [`dlopen`] flag: do not make the library's symbols available to
/// libraries loaded after it. This is the default.
/// `bits/dlfcn.h`: `#define RTLD_LOCAL 0`.
pub const RTLD_LOCAL: c_int = 0;

// Before glibc 2.34 these functions are in libdl. From 2.34 on they are in
// libc itself, and libdl is an empty library kept so that this still links.
// Our floor is glibc 2.31 (docs/03-platforms.md), so libdl is named.
#[link(name = "dl")]
unsafe extern "C" {
    /// Loads the shared library `filename` and returns a handle to it. A null
    /// `filename` gives a handle to the program itself. Returns null on
    /// failure; [`dlerror`] then says why.
    /// `dlfcn.h`: `extern void *dlopen (const char *__file, int __mode)`.
    ///
    /// The handle is ours. We never close it.
    ///
    /// # Safety
    ///
    /// - `filename` is null or points to a NUL-terminated string.
    /// - Loading a library runs its initializers, which can do anything. The
    ///   library must be one that is sound to load into this process.
    pub fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;

    /// Returns the address of `symbol` in the library `handle`, or null if
    /// the library and its dependencies do not define it.
    /// `dlfcn.h`: `extern void *dlsym (void *__restrict __handle, const
    /// char *__restrict __name)`.
    ///
    /// The address stays valid while the library is loaded.
    ///
    /// # Safety
    ///
    /// - `handle` came from [`dlopen`] and was not closed.
    /// - `symbol` points to a NUL-terminated string.
    pub fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;

    /// Describes the last failure of [`dlopen`] or [`dlsym`] on this thread,
    /// and clears it. Returns null if there was none since the last call.
    /// `dlfcn.h`: `extern char *dlerror (void)`.
    ///
    /// The string belongs to the C library: do not free it. It is valid
    /// until the next call of a `dl*` function on this thread.
    pub safe fn dlerror() -> *mut c_char;
}

// ---------------------------------------------------------------------------
// Waiting on file descriptors
// ---------------------------------------------------------------------------

/// The number of entries in the array that [`poll`] takes.
/// `sys/poll.h`: `typedef unsigned long int nfds_t;`
pub type nfds_t = c_ulong;

/// One file descriptor for [`poll`] to watch, and what it found.
/// `sys/poll.h`: `struct pollfd { int fd; short int events; short int
/// revents; };`
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct pollfd {
    /// The file descriptor. `poll` skips a negative one, and sets its
    /// `revents` to 0.
    pub fd: c_int,
    /// What to wait for: [`POLLIN`] and others. [`POLLERR`], [`POLLHUP`]
    /// and [`POLLNVAL`] are reported whether or not they are asked for.
    pub events: c_short,
    /// What happened, written by `poll`.
    pub revents: c_short,
}

const _: () = assert!(size_of::<pollfd>() == 8);
const _: () = assert!(align_of::<pollfd>() == 4);
const _: () = assert!(offset_of!(pollfd, fd) == 0);
const _: () = assert!(offset_of!(pollfd, events) == 4);
const _: () = assert!(offset_of!(pollfd, revents) == 6);

/// A [`pollfd`] event: there is data to read, or, on a socket, the other
/// end closed, and a read returns 0 (or fails, if the connection was
/// reset). A pipe reports a closed writer as [`POLLHUP`] instead.
/// `bits/poll.h`: `#define POLLIN 0x001`
pub const POLLIN: c_short = 0x001;

/// A [`pollfd`] event, in `revents` only: an error condition, such as a
/// socket with a pending error.
/// `bits/poll.h`: `#define POLLERR 0x008`
pub const POLLERR: c_short = 0x008;

/// A [`pollfd`] event, in `revents` only: the other end hung up, as when a
/// pipe's last writer closes or the peer of a Unix socket, such as a local
/// X server, closes it. A TCP peer's orderly close shows as [`POLLIN`]
/// alone; a reset adds [`POLLERR`] and `POLLHUP`. Data may still be left
/// to read; an empty pipe whose writer closed reports `POLLHUP` alone.
/// `bits/poll.h`: `#define POLLHUP 0x010`
pub const POLLHUP: c_short = 0x010;

/// A [`pollfd`] event, in `revents` only: `fd` is not an open file
/// descriptor. `bits/poll.h`: `#define POLLNVAL 0x020`
pub const POLLNVAL: c_short = 0x020;

unsafe extern "C" {
    /// Waits until one of the `nfds` file descriptors at `fds` has an event
    /// it asks for (or an error, a hang-up or an invalid descriptor), or
    /// until `timeout` milliseconds pass. A `timeout` of -1 waits for ever,
    /// and 0 returns at once.
    ///
    /// Returns how many entries have a nonzero `revents`, 0 if the time ran
    /// out first, or -1 on failure, with `errno` set: `EINTR` if a signal
    /// arrived first, which is no error, and the call can be made again.
    /// Read `errno` with `std::io::Error::last_os_error`.
    /// `sys/poll.h`: `extern int poll (struct pollfd *__fds, nfds_t __nfds,
    /// int __timeout)`.
    ///
    /// # Safety
    ///
    /// `fds` points to `nfds` initialized [`pollfd`]s that nothing else
    /// reads or writes during the call: `poll` writes their `revents`. It
    /// may be dangling if `nfds` is 0.
    pub fn poll(fds: *mut pollfd, nfds: nfds_t, timeout: c_int) -> c_int;
}

// ---------------------------------------------------------------------------
// Pipes
// ---------------------------------------------------------------------------

/// A [`pipe2`] flag: close the descriptor in any program this process
/// starts with `exec`, so that it leaks into no child.
/// `bits/fcntl-linux.h`: `# define __O_CLOEXEC 02000000` and `# define
/// O_CLOEXEC __O_CLOEXEC`; the kernel's `asm-generic/fcntl.h`: `#define
/// O_CLOEXEC (1 << 19)`.
pub const O_CLOEXEC: c_int = 0o2000000;

/// A [`pipe2`] flag: a read or write that would wait fails at once with
/// `EAGAIN` (`std::io::ErrorKind::WouldBlock`), so a write to a full pipe
/// never blocks. `bits/fcntl-linux.h`: `# define O_NONBLOCK 04000`; the
/// kernel's `asm-generic/fcntl.h`: `#define O_NONBLOCK (1 << 11)`.
pub const O_NONBLOCK: c_int = 0o4000;

unsafe extern "C" {
    /// Makes a pipe, and writes its read end to `pipefd[0]` and its write
    /// end to `pipefd[1]`, both with `flags` ([`O_CLOEXEC`], [`O_NONBLOCK`])
    /// set before any other thread can see them. Returns 0, or -1 with
    /// `errno` set, such as `EMFILE` when the process has too many
    /// descriptors open; read it with `std::io::Error::last_os_error`.
    /// `unistd.h`: `extern int pipe2 (int __pipedes[2], int __flags)`.
    ///
    /// Both descriptors are ours. Wrap each in `OwnedFd` as soon as the call
    /// succeeds, which closes it on drop.
    ///
    /// # Safety
    ///
    /// `pipefd` points to two `c_int`s that this thread may write and that
    /// nothing else reads or writes during the call. `pipe2` writes them
    /// only when it succeeds.
    pub fn pipe2(pipefd: *mut c_int, flags: c_int) -> c_int;
}

#[cfg(test)]
mod tests {
    use core::ffi::CStr;
    use std::fs::{self, File};
    use std::io::{self, ErrorKind, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::path::Path;

    use super::*;

    #[test]
    fn the_program_itself_can_be_opened_and_searched() {
        // SAFETY: a null name asks for the program itself, which loads
        // nothing new.
        let program = unsafe { dlopen(core::ptr::null(), RTLD_NOW | RTLD_LOCAL) };
        assert!(!program.is_null());

        // SAFETY: the handle is the one just opened, and the name is a C
        // string.
        let found = unsafe { dlsym(program, c"malloc".as_ptr()) };
        assert!(!found.is_null());

        // SAFETY: as above.
        let absent = unsafe { dlsym(program, c"tidepool_sys_has_no_such_symbol".as_ptr()) };
        assert!(absent.is_null());
    }

    #[test]
    fn a_library_that_does_not_exist_fails_with_a_message() {
        let name = c"libtidepool-sys-has-no-such-library.so.0";
        // SAFETY: the name is a C string. No such library exists, so
        // nothing is loaded.
        let handle = unsafe { dlopen(name.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        assert!(handle.is_null());

        let message = dlerror();
        assert!(!message.is_null());
        // SAFETY: `dlerror` returned a NUL-terminated string that stays
        // valid until this thread calls a `dl*` function again, which it
        // does not do while `message` is in use.
        let message = unsafe { CStr::from_ptr(message) }.to_string_lossy();
        assert!(
            message.contains("libtidepool-sys-has-no-such-library.so.0"),
            "{message}"
        );

        // Reading the message cleared it.
        assert!(dlerror().is_null());
    }

    /// A pipe made as the waker's self-pipe is to be made (M2.2): its read
    /// end and its write end, both close-on-exec and non-blocking.
    fn pipe() -> (OwnedFd, OwnedFd) {
        let mut fds: [c_int; 2] = [-1; 2];
        // SAFETY: `fds` is two `c_int`s that only this function uses, and
        // `pipe2` writes nothing else.
        let result = unsafe { pipe2(fds.as_mut_ptr(), O_CLOEXEC | O_NONBLOCK) };
        assert_eq!(result, 0, "pipe2: {}", io::Error::last_os_error());
        // SAFETY: `pipe2` succeeded, so `fds[0]` is an open descriptor, and
        // nothing else owns it or closes it.
        let read_end = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        // SAFETY: as above, for `fds[1]`.
        let write_end = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        (read_end, write_end)
    }

    /// Polls `fds` once, without waiting, and returns what `poll` returned.
    fn poll_now(fds: &mut [pollfd]) -> c_int {
        let count = nfds_t::try_from(fds.len()).expect("a few entries fit in nfds_t");
        // SAFETY: `fds` is `count` initialized entries that this function
        // borrows mutably for the call, and a timeout of 0 does not wait.
        unsafe { poll(fds.as_mut_ptr(), count, 0) }
    }

    fn watching(fd: &OwnedFd) -> pollfd {
        pollfd {
            fd: fd.as_raw_fd(),
            events: POLLIN,
            revents: -1,
        }
    }

    /// Says on standard error, as the gate counts it, that `test` skipped.
    fn skipped(test: &str, reason: &str) {
        let _ = writeln!(io::stderr(), "SKIPPED: {test}: {reason}");
    }

    /// The file status flags of `fd`, with `O_CLOEXEC` for the descriptor's
    /// close-on-exec flag, as `/proc/self/fdinfo` shows them (in octal).
    fn flags(fd: &OwnedFd) -> c_int {
        let path = format!("/proc/self/fdinfo/{}", fd.as_raw_fd());
        let info = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
        let octal = info
            .lines()
            .find_map(|line| line.strip_prefix("flags:"))
            .unwrap_or_else(|| panic!("{path} has no flags: line: {info:?}"));
        c_int::from_str_radix(octal.trim(), 8)
            .unwrap_or_else(|error| panic!("{path}: flags {octal:?}: {error}"))
    }

    #[test]
    fn pipe2_makes_both_ends_close_on_exec_and_non_blocking() {
        let test = "pipe2_makes_both_ends_close_on_exec_and_non_blocking";
        if !Path::new("/proc/self/fdinfo").is_dir() {
            skipped(test, "/proc is not mounted, so the flags cannot be read");
            return;
        }
        let (read_end, write_end) = pipe();
        for (end, fd) in [("read", &read_end), ("write", &write_end)] {
            let flags = flags(fd);
            assert_eq!(flags & O_CLOEXEC, O_CLOEXEC, "{end} end: {flags:#o}");
            assert_eq!(flags & O_NONBLOCK, O_NONBLOCK, "{end} end: {flags:#o}");
        }
    }

    #[test]
    fn an_empty_pipe_refuses_a_read_and_a_full_one_a_write_rather_than_wait() {
        let test = "an_empty_pipe_refuses_a_read_and_a_full_one_a_write_rather_than_wait";
        if !Path::new("/proc/self/fdinfo").is_dir() {
            skipped(
                test,
                "/proc is not mounted, so blocking ends cannot be ruled out first",
            );
            return;
        }
        let (read_end, write_end) = pipe();
        // An end that blocks would hang this test rather than fail it.
        for (end, fd) in [("read", &read_end), ("write", &write_end)] {
            assert_ne!(flags(fd) & O_NONBLOCK, 0, "the {end} end would block");
        }
        let mut reader = File::from(read_end);
        let mut writer = File::from(write_end);

        let read = reader.read(&mut [0; 16]);
        assert_eq!(
            read.map_err(|error| error.kind()),
            Err(ErrorKind::WouldBlock)
        );

        // A pipe holds 16 pages (64 KiB with 4 KiB pages) unless it is
        // resized; 16 MiB is well past any size it could have here.
        let chunk = [0_u8; 4096];
        let mut written = 0_usize;
        let refused = loop {
            match writer.write(&chunk) {
                Ok(count) => written += count,
                Err(error) => break error.kind(),
            }
            assert!(
                written <= 16 << 20,
                "the pipe took {written} bytes and never filled"
            );
        };
        assert_eq!(refused, ErrorKind::WouldBlock, "after {written} bytes");
        assert!(written > 0);
    }

    #[test]
    fn poll_reports_nothing_until_a_byte_arrives_and_then_pollin() {
        let (read_end, write_end) = pipe();

        let mut fds = [watching(&read_end)];
        assert_eq!(poll_now(&mut fds), 0);
        assert_eq!(fds[0].revents, 0);

        // The writer stays open: closing it would add a hang-up.
        let mut writer = File::from(write_end);
        writer
            .write_all(&[1])
            .expect("one byte fits in an empty pipe");
        let mut fds = [watching(&read_end)];
        assert_eq!(poll_now(&mut fds), 1);
        assert_eq!(fds[0].revents, POLLIN);
        drop(writer);
    }

    #[test]
    fn poll_reports_a_hang_up_unasked_and_counts_only_ready_entries() {
        let (read_end, write_end) = pipe();
        let (quiet, _quiet_writer) = pipe();
        // Closing the only write end hangs up the read end.
        drop(write_end);

        let mut fds = [
            pollfd {
                fd: read_end.as_raw_fd(),
                events: 0,
                revents: -1,
            },
            watching(&quiet),
            // A negative descriptor is skipped.
            pollfd {
                fd: -1,
                events: POLLIN,
                revents: -1,
            },
        ];
        assert_eq!(poll_now(&mut fds), 1);
        assert_eq!(fds[0].revents, POLLHUP, "{:#x}", fds[0].revents);
        assert_eq!(fds[1].revents, 0);
        assert_eq!(fds[2].revents, 0);
    }

    #[test]
    fn poll_with_no_entries_times_out() {
        assert_eq!(poll_now(&mut []), 0);
    }
}
