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

//! Declarations for Linux: the C library, and Xlib, whose functions are a
//! table to be loaded from `libX11.so.6` while the program runs (M2.2).
//!
//! - [`libc`]: the dynamic loader, `poll` and `pipe2`.
//! - [`x11`]: the Xlib types, constants and events that a window, its
//!   events and its presentation are to use (M2.2), and [`x11::Xlib`], the
//!   table of its functions in `libX11.so.6`.
//!
//! The layouts these modules assert were measured on x86_64 Linux by a C
//! program, and hold on x86_64 and aarch64 Linux, where `long` and
//! pointers are 8 bytes. Any other Linux target fails the build at the
//! assertions below, which say why, before the assertions of each struct
//! would: another architecture, even an LP64 one such as riscv64 or
//! powerpc64 (sparc also gives `O_NONBLOCK` and `O_CLOEXEC` other values,
//! and mips `O_NONBLOCK`), and the 32-bit ABIs of these two, x32 and arm64's ILP32.

use core::ffi::c_long;

const _: () = assert!(
    cfg!(any(target_arch = "x86_64", target_arch = "aarch64")),
    "tidepool-sys declares Linux for x86_64 and aarch64 only: its layouts \
     and constants were checked there, and other architectures differ \
     (sparc gives O_NONBLOCK and O_CLOEXEC other values, and mips O_NONBLOCK)"
);

const _: () = assert!(
    size_of::<c_long>() == 8 && size_of::<*const u8>() == 8,
    "tidepool-sys's Linux layouts are for the LP64 ABIs of x86_64 and \
     aarch64, where `long` and pointers are 8 bytes, not for x32 or ILP32"
);

pub mod libc;
pub mod x11;
