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

//! Shared libraries that are loaded while the program runs, and Xlib's
//! table, which is loaded once for the process.
//!
//! Ported from nadir-platform `linux/library.rs`: [`Library`], its error
//! and its tests, with Tidepool's names. [`xlib`] is new: Nadir loads Xlib
//! for each connection.

use core::ffi::{CStr, c_void};
use core::ptr::NonNull;
use std::sync::OnceLock;

use tidepool_sys::dynlib;
use tidepool_sys::linux::libc::{RTLD_LOCAL, RTLD_NOW, dlerror, dlopen, dlsym};
use tidepool_sys::linux::x11::Xlib;

use crate::Error;

/// The file name of Xlib, for the loader.
const XLIB: &CStr = c"libX11.so.6";

/// The file name of Xlib, for messages.
const XLIB_NAME: &str = "libX11.so.6";

/// A shared library, loaded with `dlopen`.
///
/// It stays loaded until the process exits: dropping this does not unload
/// it (`dlclose` is not even declared). Function pointers taken from it
/// therefore never dangle.
pub(super) struct Library {
    handle: NonNull<c_void>,
}

impl Library {
    /// Loads the library called `name`. The loader looks for it in its
    /// usual places.
    ///
    /// # Errors
    ///
    /// If the library cannot be loaded, returns what the loader said.
    ///
    /// # Safety
    ///
    /// Loading a library runs its initializers. The library must be one
    /// that is sound to load into this process.
    pub(super) unsafe fn open(name: &CStr) -> Result<Self, String> {
        // SAFETY: `name` is a C string, and the caller vouches for the
        // library.
        let handle = unsafe { dlopen(name.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        match NonNull::new(handle) {
            Some(handle) => Ok(Self { handle }),
            None => Err(last_error()),
        }
    }
}

/// What the loader says about its last failure on this thread.
fn last_error() -> String {
    let message = dlerror();
    if message.is_null() {
        return "the loader gave no reason".to_string();
    }
    // SAFETY: `dlerror` returned a NUL-terminated string. It stays valid
    // until this thread calls a `dl*` function again, and it is copied
    // before that.
    let message = unsafe { CStr::from_ptr(message) };
    message.to_string_lossy().into_owned()
}

impl dynlib::Library for Library {
    fn symbol(&self, name: &CStr) -> *const c_void {
        // SAFETY: the handle came from `dlopen` and is never closed, and
        // `name` is a C string.
        unsafe { dlsym(self.handle.as_ptr(), name.as_ptr()) }
    }
}

/// Why Xlib could not be loaded, kept for every later call.
#[derive(Clone, Debug)]
enum LoadFailure {
    /// The loader could not load the library, and said this.
    NotFound(String),
    /// The library lacks these functions.
    Missing(Vec<&'static str>),
}

/// Xlib's table, or why it could not be loaded.
static XLIB_TABLE: OnceLock<Result<Xlib, LoadFailure>> = OnceLock::new();

/// Returns Xlib's table, loading `libX11.so.6` the first time. A library
/// that fails to load fails the same way every time after.
///
/// # Errors
///
/// [`Error::LibraryNotFound`] if the loader cannot load it, and
/// [`Error::MissingSymbols`] if it lacks a function of the table.
pub(super) fn xlib() -> Result<&'static Xlib, Error> {
    let loaded = XLIB_TABLE.get_or_init(|| {
        // SAFETY: Xlib is a system library that is sound to load into any
        // process. Loading it opens no display; from libX11 1.8 on, its
        // initializer calls `XInitThreads`.
        let library = unsafe { Library::open(XLIB) }.map_err(LoadFailure::NotFound)?;
        // SAFETY: the library is Xlib, which defines these functions as its
        // headers declare them, and a `Library` is never unloaded.
        unsafe { Xlib::load(&library) }.map_err(|missing| LoadFailure::Missing(missing.names))
    });
    match loaded {
        Ok(xlib) => Ok(xlib),
        Err(LoadFailure::NotFound(reason)) => Err(Error::LibraryNotFound {
            library: XLIB_NAME,
            reason: reason.clone(),
        }),
        Err(LoadFailure::Missing(symbols)) => Err(Error::MissingSymbols {
            library: XLIB_NAME,
            symbols: symbols.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use tidepool_sys::dynlib::Library as _;

    use super::*;

    #[test]
    fn a_loaded_library_finds_its_symbols_and_no_others() {
        // SAFETY: the C library is already part of this process.
        let libc =
            unsafe { Library::open(c"libc.so.6") }.unwrap_or_else(|reason| panic!("{reason}"));
        assert!(!libc.symbol(c"malloc").is_null());
        assert!(
            libc.symbol(c"tidepool_platform_has_no_such_symbol")
                .is_null()
        );
    }

    #[test]
    fn a_missing_library_gives_the_loaders_reason() {
        // SAFETY: no such library exists, so nothing is loaded.
        let result = unsafe { Library::open(c"libtidepool-platform-has-no-such-library.so.0") };
        let reason = result.err().unwrap_or_default();
        assert!(
            reason.contains("libtidepool-platform-has-no-such-library.so.0"),
            "{reason}"
        );
    }

    #[test]
    fn xlib_loads_once_and_is_the_same_table_every_time() {
        match (xlib(), xlib()) {
            (Ok(first), Ok(second)) => assert!(core::ptr::eq(first, second)),
            (Err(Error::LibraryNotFound { reason, .. }), Err(Error::LibraryNotFound { .. })) => {
                // This machine has no Xlib. Say so as the gate counts it
                // (docs/04-quality.md).
                let _ = writeln!(
                    io::stderr(),
                    "SKIPPED: xlib_loads_once_and_is_the_same_table_every_time: \
                     libX11.so.6 cannot be loaded: {reason}"
                );
            }
            (first, second) => panic!("{:?} then {:?}", first.err(), second.err()),
        }
    }
}
