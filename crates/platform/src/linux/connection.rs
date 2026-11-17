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

//! The connection to the X server: one safe method for each Xlib call.
//!
//! It also keeps every pointer Xlib hands out (the display, the graphics
//! context, the visual, the input method and its context) so that none
//! leaves this file, and frees each in the order Xlib needs.
//!
//! Ported in part from nadir-platform `linux/x11/connection.rs`: the
//! methods for the calls both make, the input method and its context, the
//! lookup of typed text and the taking of the environment's character
//! locale ([`use_environment_ctype`]), with Tidepool's names and reworded
//! comments. Adapted for Tidepool: the connection holds the [`Claim`] to
//! call Xlib, which it releases once it is closed; it opens the display
//! that [`display`](super::display) names, never `DISPLAY` itself; it
//! opens only Xlib's own input method (`@im=none`, ADR-0039), so Nadir's
//! watch for an input method server that stops, and its claim of the input
//! method for one connection, are left out; the window's visual is found
//! with `XMatchVisualInfo` and checked against BGRA8's byte order; an event
//! is a [`Received`], which only [`Connection::next_event`] makes, so that
//! the methods that take one are safe, and only the fields Xlib wrote are
//! read from it; and the graphics context and
//! [`Connection::put_pixels`], which shows a [`Pixels`] view, are new.
//!
//! # Safety
//!
//! The `SAFETY` comments below rely on these invariants:
//!
//! - `display` came from `XOpenDisplay`, and is closed only when the
//!   connection is dropped.
//! - `xlib` was loaded from Xlib itself, which is never unloaded.
//! - The connection holds the [`Claim`] of its thread until it is closed,
//!   and holds a raw pointer, so it is neither `Send` nor `Sync`: every
//!   Xlib call it makes comes from the claiming thread, while no other
//!   thread makes any.
//!
//! A window, cursor, colormap or atom that does not belong to the display
//! is no memory error: the server refuses the request, and the error
//! handler ([`claim`](super::claim)) records it. The methods that take one
//! are therefore safe.

use core::ffi::{CStr, c_char, c_int, c_long, c_uchar, c_uint, c_ulong};
use core::marker::PhantomData;
use core::ptr::{self, NonNull};
use std::os::fd::BorrowedFd;

use tidepool_core::geom::{DeviceRect, DeviceSize};
use tidepool_core::log::{debug, warn};
// The lookup statuses are used as `xlib::XLookupChars`, not imported by name:
// in a pattern, a bare name that is not upper case reads as a new variable.
use tidepool_sys::linux::libc::{LC_CTYPE, setlocale};
use tidepool_sys::linux::x11::{
    self as xlib, _XGC, AllocNone, AnyPropertyType, Atom, CWBackPixel, CWBorderPixel, CWColormap,
    Colormap, Cursor, Display, False, InputOutput, KeyPress, LSBFirst, NoEventMask,
    PropModeReplace, SelectionNotify, Status, Success, Time, True, TrueColor, Window, XEvent,
    XICRec, XIMPreeditNothing, XIMRec, XIMStatusNothing, XIMStyle, XImage, XImageFuncs,
    XNClientWindow, XNFilterEvents, XNFocusWindow, XNInputStyle, XPointer, XSelectionEvent,
    XSetWindowAttributes, XVisualInfo, Xlib, ZPixmap,
};

use super::claim::Claim;
use super::input::Incoming;
use crate::Error;
use crate::surface::{self, BYTES_PER_PIXEL, Pixels};

/// The depth of the window: 24 bits, eight for each of red, green and blue.
const DEPTH: c_int = 24;

/// Where red, green and blue are in a pixel value of the window's visual:
/// in a BGRA8 pixel read as a little-endian 32-bit value, red is the third
/// byte, green the second and blue the first.
const RGB_MASKS: [c_ulong; 3] = [0x00ff_0000, 0x0000_ff00, 0x0000_00ff];

/// The furthest from the window's origin that `XPutImage` can draw: the X
/// protocol carries its destination in 16 signed bits.
const MAX_COORDINATE: i32 = i16::MAX as i32;

/// The most bytes one key press may type. Xlib's own input method types a
/// few at a time; more is no typing, and is dropped.
const MAX_TYPED: usize = 64 * 1024;

/// The input method and the window's input context in it.
struct TextInput {
    im: NonNull<XIMRec>,
    ic: NonNull<XICRec>,
}

/// An event that [`Connection::next_event`] read, which only it makes.
///
/// Xlib made it from what the server sent, so its type, the fields every
/// event starts with, and the fields that [`Incoming::decode`] reads for
/// that type are initialized (`decode`'s `# Safety` says which those are
/// and why). The rest of its 192 bytes may be uninitialized memory of
/// Xlib's queue, which nothing here reads. It borrows the connection, so
/// the display it names is open for as long as it lives.
pub(super) struct Received<'c> {
    event: XEvent,
    /// Whether an input method took it ([`Connection::filter_event`]).
    taken: bool,
    _connection: PhantomData<&'c Connection>,
}

impl Received<'_> {
    /// What the event says.
    pub(super) fn decode(&self) -> Incoming {
        // SAFETY: Xlib made the event from what the server sent, so it
        // initialized the type and every field `decode` reads for it, as
        // `decode` requires: `_XWireToEvent` writes them (see `decode`).
        unsafe { Incoming::decode(&self.event) }
    }
}

/// An open connection to the X server, and Xlib's right to be called.
///
/// See the [module documentation](self) for what it promises.
pub(super) struct Connection {
    xlib: &'static Xlib,
    display: NonNull<Display>,
    /// The graphics context the window's pixels are put with, once made.
    gc: Option<NonNull<_XGC>>,
    /// The input method and the window's context, once opened.
    text: Option<TextInput>,
    /// Kept until the display is closed, which `Drop` does before the
    /// fields are dropped.
    _claim: Claim,
}

impl Connection {
    /// Connects to the X server that `name` names, with the claim of this
    /// thread, which the connection keeps until it closes. Returns `None`
    /// if Xlib could not connect.
    pub(super) fn open(claim: Claim, xlib: &'static Xlib, name: &CStr) -> Option<Self> {
        // SAFETY: the claim puts this call on the Xlib thread, and `name`
        // is a C string, so Xlib does not read `DISPLAY` itself.
        let display = unsafe { (xlib.XOpenDisplay)(name.as_ptr()) };
        Some(Self {
            xlib,
            display: NonNull::new(display)?,
            gc: None,
            text: None,
            _claim: claim,
        })
    }

    /// The descriptor of the connection, to poll. It belongs to Xlib: it is
    /// never read, written or closed here. `None` if Xlib has none, which
    /// an open display always has.
    pub(super) fn fd(&self) -> Option<BorrowedFd<'_>> {
        // SAFETY: the display is open.
        let fd = unsafe { (self.xlib.XConnectionNumber)(self.display.as_ptr()) };
        if fd < 0 {
            return None;
        }
        // SAFETY: the descriptor is open while the display is, and the
        // borrow cannot outlive `self`, which closes the display only when
        // it is dropped.
        Some(unsafe { BorrowedFd::borrow_raw(fd) })
    }

    /// Sends every queued request. It waits for no reply, but can block
    /// while the socket is full.
    pub(super) fn flush(&self) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XFlush)(self.display.as_ptr()) };
    }

    /// Sends every queued request and waits until the server has handled
    /// them, so that the error handler has seen any it refused.
    pub(super) fn sync(&self) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XSync)(self.display.as_ptr(), False) };
    }

    /// Returns how many events are waiting, after sending every queued
    /// request and reading what arrived if none are. Waits for no event.
    pub(super) fn pending(&self) -> c_int {
        // SAFETY: the display is open.
        unsafe { (self.xlib.XPending)(self.display.as_ptr()) }
    }

    /// Removes the next event from the queue, waiting for one if the queue
    /// is empty: call it only when [`Connection::pending`] says one waits.
    pub(super) fn next_event(&self) -> Received<'_> {
        let mut event = XEvent { pad: [0; 24] };
        // SAFETY: the display is open, and `event` is a live `XEvent`, which
        // has room for any event. Xlib copies all 192 bytes of its queue's
        // element over it, some of which it may never have written; a
        // union may hold such bytes, and only the fields Xlib wrote are
        // read (see `Received`). Xlib gives the result no meaning.
        unsafe { (self.xlib.XNextEvent)(self.display.as_ptr(), &raw mut event) };
        Received {
            event,
            taken: false,
            _connection: PhantomData,
        }
    }

    /// Gives `event` to the input methods that asked for it, and returns
    /// whether one took it: a dead key or a step of a Compose sequence. A
    /// key press it took types nothing now; the key still went down. Decode
    /// the event before: Xlib's own input method changes the press that
    /// ends a Compose sequence.
    pub(super) fn filter_event(&self, event: &mut Received<'_>) -> bool {
        // SAFETY: Xlib made the event, for a display that is open while
        // `event` borrows its connection. `XFilterEvent` reads its type and
        // display, which Xlib writes for every event, and its window, and
        // may rewrite a key press. A window of 0 stands for the event's own.
        let taken = unsafe { (self.xlib.XFilterEvent)(&raw mut event.event, 0) } != False;
        event.taken = taken;
        taken
    }

    /// The root window of the default screen, which the server owns.
    fn root(&self) -> (c_int, Window) {
        // SAFETY: the display is open.
        let screen = unsafe { (self.xlib.XDefaultScreen)(self.display.as_ptr()) };
        // SAFETY: the display is open, and Xlib just gave this screen number.
        let root = unsafe { (self.xlib.XRootWindow)(self.display.as_ptr(), screen) };
        (screen, root)
    }

    /// Creates a top-level window of `size` device pixels in a 24-bit
    /// TrueColor visual whose pixel values are BGRA8's, with a black
    /// background, and a colormap of that visual for it. The window is not
    /// shown until [`Connection::map_window`]. The caller destroys the
    /// window, then frees the colormap, before the connection closes.
    ///
    /// # Errors
    ///
    /// [`Error::NoVisual`] if the default screen has no such visual.
    pub(super) fn create_window(&self, size: DeviceSize) -> Result<(Window, Colormap), Error> {
        let (screen, root) = self.root();
        let mut info = XVisualInfo {
            visual: ptr::null_mut(),
            visualid: 0,
            screen: 0,
            depth: 0,
            class: 0,
            red_mask: 0,
            green_mask: 0,
            blue_mask: 0,
            colormap_size: 0,
            bits_per_rgb: 0,
        };
        // SAFETY: the display is open, the screen is its default, and
        // `info` is a live `XVisualInfo` for Xlib to write. Nothing is
        // allocated; the visual belongs to the display.
        let found = unsafe {
            (self.xlib.XMatchVisualInfo)(
                self.display.as_ptr(),
                screen,
                DEPTH,
                TrueColor,
                &raw mut info,
            )
        };
        if found == 0 || info.visual.is_null() {
            return Err(Error::NoVisual {
                reason: format!("screen {screen} has no 24-bit TrueColor visual"),
            });
        }
        let masks = [info.red_mask, info.green_mask, info.blue_mask];
        if masks != RGB_MASKS {
            return Err(Error::NoVisual {
                reason: format!(
                    "its 24-bit TrueColor visual has the red, green and blue masks \
                     {:#x}, {:#x} and {:#x}, not BGRA8's",
                    masks[0], masks[1], masks[2]
                ),
            });
        }
        // SAFETY: the display is open, and the visual came from Xlib for it
        // and lives until it is closed.
        let colormap = unsafe {
            (self.xlib.XCreateColormap)(self.display.as_ptr(), root, info.visual, AllocNone)
        };
        let mut attributes = XSetWindowAttributes {
            background_pixmap: 0,
            // Black on a TrueColor visual.
            background_pixel: 0,
            border_pixmap: 0,
            border_pixel: 0,
            bit_gravity: 0,
            win_gravity: 0,
            backing_store: 0,
            backing_planes: 0,
            backing_pixel: 0,
            save_under: False,
            event_mask: 0,
            do_not_propagate_mask: 0,
            override_redirect: False,
            colormap,
            cursor: 0,
        };
        // A window of a visual other than the root's needs a colormap of its
        // visual, and one of a depth other than the root's (24 on a root of
        // 30) a border pixel. The server refuses the window otherwise, so
        // both are always set.
        let mask = (CWBackPixel | CWBorderPixel | CWColormap) as c_ulong;
        let side = |value: i32| c_uint::try_from(value.max(1)).unwrap_or(1);
        // SAFETY: the display is open, `attributes` is a live struct, of
        // which the mask names three fields, and the visual came from Xlib
        // for the display. The depth is the visual's.
        let window = unsafe {
            (self.xlib.XCreateWindow)(
                self.display.as_ptr(),
                root,
                0,
                0,
                side(size.width),
                side(size.height),
                0,
                info.depth,
                InputOutput,
                info.visual,
                mask,
                &raw mut attributes,
            )
        };
        Ok((window, colormap))
    }

    /// Destroys a window.
    pub(super) fn destroy_window(&self, window: Window) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XDestroyWindow)(self.display.as_ptr(), window) };
    }

    /// Frees a colormap.
    pub(super) fn free_colormap(&self, colormap: Colormap) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XFreeColormap)(self.display.as_ptr(), colormap) };
    }

    /// Shows a window.
    pub(super) fn map_window(&self, window: Window) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XMapWindow)(self.display.as_ptr(), window) };
    }

    /// Chooses which events the server reports for a window.
    pub(super) fn select_input(&self, window: Window, event_mask: c_long) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XSelectInput)(self.display.as_ptr(), window, event_mask) };
    }

    /// Returns the atom for `name`, making it if the server has none.
    pub(super) fn intern(&self, name: &CStr) -> Atom {
        // SAFETY: the display is open, and `name` is a C string.
        unsafe { (self.xlib.XInternAtom)(self.display.as_ptr(), name.as_ptr(), False) }
    }

    /// Tells the window manager which messages it may send the window.
    /// Returns whether that worked.
    pub(super) fn set_protocols(&self, window: Window, protocols: &mut [Atom]) -> bool {
        let Ok(count) = c_int::try_from(protocols.len()) else {
            return false;
        };
        // SAFETY: the display is open, and `protocols` points to `count`
        // live atoms, which Xlib copies.
        let status = unsafe {
            (self.xlib.XSetWMProtocols)(
                self.display.as_ptr(),
                window,
                protocols.as_mut_ptr(),
                count,
            )
        };
        status != 0
    }

    /// Sets a property of a window to some bytes. Returns `false`, having
    /// done nothing, if there are more bytes than a request can count.
    pub(super) fn set_bytes(
        &self,
        window: Window,
        property: Atom,
        type_: Atom,
        data: &[u8],
    ) -> bool {
        let Ok(len) = c_int::try_from(data.len()) else {
            return false;
        };
        // SAFETY: the display is open, and `data` points to `len` live
        // bytes, which is what format 8 reads. Xlib copies them. It gives
        // the result no meaning.
        unsafe {
            (self.xlib.XChangeProperty)(
                self.display.as_ptr(),
                window,
                property,
                type_,
                8,
                PropModeReplace,
                data.as_ptr(),
                len,
            )
        };
        true
    }

    /// Sets a property of a window to 32-bit numbers of type `type_`, such
    /// as `CARDINAL`s or atoms, each given as a C `long`, of which Xlib
    /// sends the low 32 bits. Returns `false`, having done nothing, if
    /// there are more numbers than a request can count.
    pub(super) fn set_longs(
        &self,
        window: Window,
        property: Atom,
        type_: Atom,
        values: &[c_long],
    ) -> bool {
        self.change_longs(window, property, type_, PropModeReplace, values)
    }

    /// Sets a property of a window to no 32-bit numbers of type `type_`,
    /// making it if it does not exist, so that the server reports a
    /// [`PropertyNotify`](xlib::PropertyNotify) with its time: ICCCM's way to
    /// get a timestamp, with a write of nothing. It replaces rather than
    /// appends: an append to a property of another type or format is
    /// refused (`BadMatch`) and reported by no event, and any client can
    /// give the property another type.
    pub(super) fn touch_property(&self, window: Window, property: Atom, type_: Atom) {
        self.change_longs(window, property, type_, PropModeReplace, &[]);
    }

    /// `XChangeProperty` of 32-bit numbers in `mode`.
    fn change_longs(
        &self,
        window: Window,
        property: Atom,
        type_: Atom,
        mode: c_int,
        values: &[c_long],
    ) -> bool {
        let Ok(len) = c_int::try_from(values.len()) else {
            return false;
        };
        // SAFETY: the display is open. With format 32, Xlib reads `len`
        // items of the size of a C `long` from `data`, which is what
        // `values` holds (none for an empty slice, whose pointer is
        // dangling but never read), and copies them. It gives the result
        // no meaning.
        unsafe {
            (self.xlib.XChangeProperty)(
                self.display.as_ptr(),
                window,
                property,
                type_,
                32,
                mode,
                values.as_ptr().cast::<c_uchar>(),
                len,
            )
        };
        true
    }

    /// Asks for detectable auto-repeat, and returns whether we have it. With
    /// it, a held key repeats as presses only, with no releases between.
    pub(super) fn set_detectable_auto_repeat(&self) -> bool {
        // Left as it is when the server does not answer.
        let mut supported = False;
        // SAFETY: the display is open, and `supported` is a live `Bool`.
        let on = unsafe {
            (self.xlib.XkbSetDetectableAutoRepeat)(self.display.as_ptr(), True, &raw mut supported)
        };
        on != False && supported != False
    }

    /// Returns the user's X resources, as they were when the display was
    /// opened, as bytes, which any client may have set to anything.
    pub(super) fn resources(&self) -> Option<Vec<u8>> {
        // SAFETY: the display is open.
        let text = unsafe { (self.xlib.XResourceManagerString)(self.display.as_ptr()) };
        if text.is_null() {
            return None;
        }
        // SAFETY: Xlib returned a NUL-terminated string that it owns, and
        // that lives until the display is closed. It is copied here.
        let text = unsafe { CStr::from_ptr(text) };
        Some(text.to_bytes().to_vec())
    }

    /// Creates a pointer shape from the standard cursor font (`shape` is
    /// one of `X11/cursorfont.h`'s). The caller frees it with
    /// [`Connection::free_cursor`].
    ///
    /// Where libXcursor is installed, this opens and parses files at paths
    /// any client of the display can choose, and can block on one, so it is
    /// called only while a window opens (`docs/03-platforms.md` → Input →
    /// Cursors).
    pub(super) fn create_font_cursor(&self, shape: c_uint) -> Cursor {
        // SAFETY: the display is open. A shape the font lacks is an X
        // error, not a memory error.
        unsafe { (self.xlib.XCreateFontCursor)(self.display.as_ptr(), shape) }
    }

    /// Shows `cursor` while the pointer is over a window.
    pub(super) fn define_cursor(&self, window: Window, cursor: Cursor) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XDefineCursor)(self.display.as_ptr(), window, cursor) };
    }

    /// Frees a cursor. The server keeps the shape while a window uses it.
    pub(super) fn free_cursor(&self, cursor: Cursor) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XFreeCursor)(self.display.as_ptr(), cursor) };
    }
}

// -- selections and properties ------------------------------------------------

/// A property of a window, as [`Connection::get_property`] read it.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Property {
    /// Its type, or 0 (`None`) if the window has no such property.
    pub(super) type_: Atom,
    /// The size of its items in bits: 8, 16 or 32, or 0 if it does not
    /// exist.
    pub(super) format: c_int,
    /// The items read, for format 8.
    pub(super) bytes: Vec<u8>,
    /// The items read, for format 32, each in a C `long`.
    pub(super) longs: Vec<c_long>,
    /// How many bytes of the property are left after what was read.
    pub(super) bytes_after: c_ulong,
}

impl Connection {
    /// The most bytes of data one request can carry after a header of
    /// `header` bytes: the server's limit without BIG-REQUESTS, which the
    /// protocol makes at least 16,384 bytes.
    pub(super) fn max_request_data(&self, header: usize) -> usize {
        // SAFETY: the display is open.
        let units = unsafe { (self.xlib.XMaxRequestSize)(self.display.as_ptr()) };
        usize::try_from(units)
            .unwrap_or(0)
            .saturating_mul(4)
            .saturating_sub(header)
    }

    /// Deletes a property of a window, if it has it.
    pub(super) fn delete_property(&self, window: Window, property: Atom) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XDeleteProperty)(self.display.as_ptr(), window, property) };
    }

    /// Reads at most `max_bytes` bytes of a property of a window, rounded
    /// up to whole 4-byte units, and deletes the property if `delete` and
    /// nothing is left of it. Waits for the server, not for any other
    /// client, unless one holds a server grab. `None` if Xlib could not read it, or answered with more
    /// than was asked for or with a format it does not define, or the
    /// property holds more than [`MAX_LONGS`] items of format 32.
    ///
    /// Any client can set any property of any window, so what this returns
    /// is to be checked, not trusted.
    pub(super) fn get_property(
        &self,
        window: Window,
        property: Atom,
        delete: bool,
        max_bytes: usize,
    ) -> Option<Property> {
        let units = max_bytes.div_ceil(4);
        let long_length = c_long::try_from(units).ok()?;
        let mut type_: Atom = 0;
        let mut format: c_int = 0;
        let mut items: c_ulong = 0;
        let mut bytes_after: c_ulong = 0;
        let mut data: *mut c_uchar = ptr::null_mut();
        // SAFETY: the display is open, and every return is a live local
        // that Xlib may write; each was set first, so each is initialized
        // whatever Xlib writes. On success the data is Xlib's to free, as
        // `Data` does below; on failure it is null or freed the same way.
        let status = unsafe {
            (self.xlib.XGetWindowProperty)(
                self.display.as_ptr(),
                window,
                property,
                0,
                long_length,
                if delete { True } else { False },
                AnyPropertyType,
                &raw mut type_,
                &raw mut format,
                &raw mut items,
                &raw mut bytes_after,
                &raw mut data,
            )
        };
        // Built before anything can return, so that the data is freed on
        // every path; with the items only of a property that was read.
        let read = status == Success && type_ != 0;
        let data = Data {
            xlib: self.xlib,
            data,
            format,
            items: if read {
                usize::try_from(items).unwrap_or(0)
            } else {
                0
            },
        };
        if status != Success {
            return None;
        }
        let mut found = Property {
            type_,
            format,
            bytes: Vec::new(),
            longs: Vec::new(),
            bytes_after,
        };
        if type_ == 0 {
            // No such property: there are no items, and nothing to read.
            return Some(found);
        }
        let items = usize::try_from(items).ok()?;
        match format {
            8 => {
                // At most the 4-byte units asked for.
                if items > units.checked_mul(4)? {
                    return None;
                }
                found.bytes = data.bytes()?;
            }
            16 => {
                if items > units.checked_mul(2)? {
                    return None;
                }
            }
            32 => {
                // Xlib holds each item in a C `long`, twice the bytes asked
                // for: only the short lists Tidepool reads are copied.
                if items > units || items > MAX_LONGS {
                    return None;
                }
                found.longs = data.longs()?;
            }
            _ => return None,
        }
        Some(found)
    }

    /// Makes `window` the owner of `selection` from `time` on. It has no
    /// effect if another client took the selection after `time`: ask
    /// [`Connection::selection_owner`].
    pub(super) fn set_selection_owner(&self, selection: Atom, window: Window, time: Time) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe { (self.xlib.XSetSelectionOwner)(self.display.as_ptr(), selection, window, time) };
    }

    /// The number the next request on the connection will get, which the
    /// serial of an event it causes is at least.
    pub(super) fn next_request(&self) -> c_ulong {
        // SAFETY: the display is open. Xlib only reads its count.
        unsafe { (self.xlib.XNextRequest)(self.display.as_ptr()) }
    }

    /// The window that owns `selection`, or 0 if none does. Waits for the
    /// server, not for the owner.
    pub(super) fn selection_owner(&self, selection: Atom) -> Window {
        // SAFETY: the display is open.
        unsafe { (self.xlib.XGetSelectionOwner)(self.display.as_ptr(), selection) }
    }

    /// Asks the owner of `selection` to write it as `target` to `property`
    /// of `requestor`, and to say so with a
    /// [`SelectionNotify`]. Waits for nothing.
    pub(super) fn convert_selection(
        &self,
        selection: Atom,
        target: Atom,
        property: Atom,
        requestor: Window,
        time: Time,
    ) {
        // SAFETY: the display is open. Xlib gives the result no meaning.
        unsafe {
            (self.xlib.XConvertSelection)(
                self.display.as_ptr(),
                selection,
                target,
                property,
                requestor,
                time,
            )
        };
    }

    /// Tells `requestor` that its request for `selection` as `target` is
    /// answered in `property`, or refused with a `property` of 0. Returns
    /// whether Xlib queued it.
    pub(super) fn send_selection_notify(
        &self,
        requestor: Window,
        selection: Atom,
        target: Atom,
        property: Atom,
        time: Time,
    ) -> bool {
        // Every byte set, then the member: Xlib reads only its fields.
        let mut event = XEvent { pad: [0; 24] };
        event.xselection = XSelectionEvent {
            type_: SelectionNotify,
            serial: 0,
            send_event: True,
            display: self.display.as_ptr(),
            requestor,
            selection,
            target,
            property,
            time,
        };
        // SAFETY: the display is open, and `event` is a live, initialized
        // `XEvent` that Xlib only reads, for the type it names. The mask
        // sends it to the client that made the window.
        let status = unsafe {
            (self.xlib.XSendEvent)(
                self.display.as_ptr(),
                requestor,
                False,
                NoEventMask,
                &raw mut event,
            )
        };
        status != 0
    }
}

/// The most items of format 32 that [`Connection::get_property`] copies.
/// Tidepool reads only short lists of them (`INCR`'s size, and in the
/// tests `TARGETS` and `TIMESTAMP`).
pub(super) const MAX_LONGS: usize = 64;

/// What one `XGetWindowProperty` call returned: the data Xlib allocated
/// for us, which is freed with `XFree` when this is dropped, and its format
/// and number of items, as Xlib reported them.
///
/// Built only in [`Connection::get_property`], from that call's returns,
/// with `items` 0 unless the call read a property.
struct Data {
    xlib: &'static Xlib,
    /// The data, or null.
    data: *mut c_uchar,
    /// Its format: 8, 16 or 32.
    format: c_int,
    /// How many items of that format it holds.
    items: usize,
}

impl Data {
    /// Copies the items of format 8. `None` for another format, or for
    /// items with no data.
    fn bytes(&self) -> Option<Vec<u8>> {
        if self.format != 8 {
            return None;
        }
        // SAFETY: for format 8, Xlib allocated the items as `char`s.
        unsafe { self.copy::<u8>() }
    }

    /// Copies the items of format 32. `None` for another format, or for
    /// items with no data.
    fn longs(&self) -> Option<Vec<c_long>> {
        if self.format != 32 {
            return None;
        }
        // SAFETY: for format 32, Xlib allocated the items as `long`s.
        unsafe { self.copy::<c_long>() }
    }

    /// Copies every item, as `T`s.
    ///
    /// # Safety
    ///
    /// `T` must be the type Xlib gives an item of `self.format`: `u8`
    /// (`char`) for 8, `c_long` for 32.
    unsafe fn copy<T: Copy>(&self) -> Option<Vec<T>> {
        if self.items == 0 {
            return Some(Vec::new());
        }
        if self.data.is_null() {
            return None;
        }
        // SAFETY: the data and the count are what one successful
        // `XGetWindowProperty` returned (see `Data`): Xlib allocated the
        // items it reported, an array of the type its format gives, which
        // the caller promises is `T`, plus one zero byte, with `malloc`,
        // which aligns it for any type. It is not freed until `self` is
        // dropped, after the copy.
        let items = unsafe { core::slice::from_raw_parts(self.data.cast::<T>(), self.items) };
        Some(items.to_vec())
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        if !self.data.is_null() {
            // SAFETY: Xlib allocated the data for us and nothing has freed
            // it; it is not used after this. Xlib gives the result no
            // meaning.
            unsafe { (self.xlib.XFree)(self.data.cast()) };
        }
    }
}

// -- pixels ----------------------------------------------------------------

impl Connection {
    /// Makes the graphics context that [`Connection::put_pixels`] draws
    /// into `window` with, with every value its default. Returns whether
    /// that worked.
    pub(super) fn create_gc(&mut self, window: Window) -> bool {
        self.free_gc();
        // SAFETY: the display is open, and with a `valuemask` of 0 Xlib
        // reads no values, so they may be null.
        let gc =
            unsafe { (self.xlib.XCreateGC)(self.display.as_ptr(), window, 0, ptr::null_mut()) };
        self.gc = NonNull::new(gc);
        self.gc.is_some()
    }

    /// Frees the graphics context, if there is one.
    pub(super) fn free_gc(&mut self) {
        if let Some(gc) = self.gc.take() {
            // SAFETY: the display is open, and the context is its own,
            // made by `create_gc` and not freed since: `take` forgets it.
            // Xlib gives the result no meaning.
            unsafe { (self.xlib.XFreeGC)(self.display.as_ptr(), gc.as_ptr()) };
        }
    }

    /// Queues the drawing of `rect` of `pixels` into `window`, at the same
    /// place, with the graphics context of [`Connection::create_gc`]. The
    /// part of `rect` outside the frame, or further than `XPutImage` can
    /// reach, is left out. It waits for nothing; [`Connection::flush`]
    /// sends it.
    ///
    /// # Errors
    ///
    /// [`Error::CallFailed`] if there is no graphics context, or
    /// `XInitImage` refuses the image, which a [`Pixels`] should never
    /// allow.
    pub(super) fn put_pixels(
        &self,
        window: Window,
        pixels: &Pixels<'_>,
        rect: DeviceRect,
    ) -> Result<(), Error> {
        let gc = self.gc.ok_or(Error::CallFailed { call: "XCreateGC" })?;
        let reach = DeviceSize::new(MAX_COORDINATE, MAX_COORDINATE);
        let Some(rect) = surface::clip(rect, pixels.size(), reach) else {
            return Ok(());
        };
        let stride = pixels.stride();
        for band in bands(rect, stride) {
            let Some(rows) = Rows::of(pixels, band) else {
                // `clip` keeps every band inside the frame.
                continue;
            };
            // The image is the band's rows, whole from the left edge of the
            // frame to the band's right edge; the band is put from it.
            let mut image = XImage {
                width: rows.image_width,
                height: band.height,
                xoffset: 0,
                format: ZPixmap,
                // Xlib only reads the pixels of an image it puts: it copies
                // or converts them into its own buffer before it sends them.
                data: rows.start.as_ptr().cast_mut().cast::<c_char>(),
                byte_order: LSBFirst,
                bitmap_unit: 32,
                bitmap_bit_order: LSBFirst,
                bitmap_pad: 32,
                depth: DEPTH,
                bytes_per_line: rows.stride,
                bits_per_pixel: 32,
                red_mask: RGB_MASKS[0],
                green_mask: RGB_MASKS[1],
                blue_mask: RGB_MASKS[2],
                obdata: ptr::null_mut(),
                f: XImageFuncs {
                    create_image: None,
                    destroy_image: None,
                    get_pixel: None,
                    put_pixel: None,
                    sub_image: None,
                    add_pixel: None,
                },
            };
            // SAFETY: `image` is a live `XImage` that this function owns,
            // with every field set. `XInitImage` writes only its `f` (and
            // its `bytes_per_line`, which is not 0), and allocates nothing;
            // the image, and the frame under it, are never freed by Xlib.
            if unsafe { (self.xlib.XInitImage)(&raw mut image) } == 0 {
                return Err(Error::CallFailed { call: "XInitImage" });
            }
            // SAFETY: the display is open and the graphics context is its
            // own, made by `create_gc`. `XInitImage` accepted the image
            // after its fields were last set. Its `data` is the start of
            // the band's first row, and `Rows::of` checked that `height`
            // whole rows of `bytes_per_line` (the stride) bytes follow it
            // in `pixels`, and that `width` pixels of 4 bytes fit in a row.
            // Xlib reads no further: row by row it reads a request's pixels
            // only, from `src_x`, and where it sends rows straight from
            // `data` it sends whole rows of `bytes_per_line`, which are
            // there whatever scanline pad the server has. It only reads
            // them, before this returns. Its `int` arithmetic cannot
            // overflow: the band has at most `i32::MAX / stride` rows, so
            // `row * bytes_per_line` stays under `i32::MAX`, and its right
            // edge, the image's width, is at most `i16::MAX`, so `width *
            // bits_per_pixel` and `src_x * bits_per_pixel` are at most
            // 32 × 32,767. Xlib gives the result no meaning.
            unsafe {
                (self.xlib.XPutImage)(
                    self.display.as_ptr(),
                    window,
                    gc.as_ptr(),
                    &raw mut image,
                    rows.src_x,
                    0,
                    band.x,
                    band.y,
                    rows.width,
                    rows.height,
                )
            };
        }
        Ok(())
    }
}

/// The rows of a rectangle of a frame, as an `XImage` over them needs
/// them: whole rows, from the frame's left edge, so that the image never
/// starts in the middle of a row.
struct Rows<'a> {
    /// The frame from the start of the rectangle's first row on, of which
    /// at least `height` × `stride` bytes are the rectangle's rows.
    start: &'a [u8],
    /// The frame's stride.
    stride: c_int,
    /// The image's width: from the frame's left edge to the rectangle's
    /// right edge.
    image_width: c_int,
    /// Where the rectangle starts in the image's rows: its left edge.
    src_x: c_int,
    /// The rectangle's width.
    width: c_uint,
    /// The rectangle's height.
    height: c_uint,
}

impl<'a> Rows<'a> {
    /// The rows of `rect` in `pixels`, or `None` unless `rect` is inside
    /// the frame and not empty, and all of its rows, each a whole stride
    /// long, are in the bytes, as [`Pixels`] promises.
    fn of(pixels: &Pixels<'a>, rect: DeviceRect) -> Option<Self> {
        let size = pixels.size();
        if rect.is_empty()
            || rect.x < 0
            || rect.y < 0
            || rect.right() > size.width
            || rect.bottom() > size.height
        {
            return None;
        }
        let stride = pixels.stride();
        let y = usize::try_from(rect.y).ok()?;
        let height = usize::try_from(rect.height).ok()?;
        let right = usize::try_from(rect.right()).ok()?;
        // Each row's pixels, up to the rectangle's right edge, fit in the
        // stride.
        if right.checked_mul(BYTES_PER_PIXEL)? > stride {
            return None;
        }
        // The rows from the rectangle's first, whole, and no more than the
        // bytes hold: the stride times the frame's height covers them.
        let offset = y.checked_mul(stride)?;
        let end = y.checked_add(height)?.checked_mul(stride)?;
        if end > pixels.bytes().len() {
            return None;
        }
        Some(Self {
            start: pixels.bytes().get(offset..)?,
            stride: c_int::try_from(stride).ok()?,
            image_width: rect.right(),
            src_x: rect.x,
            width: c_uint::try_from(rect.width).ok()?,
            height: c_uint::try_from(height).ok()?,
        })
    }
}

/// Splits `rect` into bands of whole rows, each of at most
/// `i32::MAX / stride` rows, top to bottom.
///
/// Xlib finds a row of an image at `row * bytes_per_line` in C `int`s,
/// which must not overflow; with the image's data at the band's first
/// row, no row is further than that. With a stride of up to 64 KiB, which
/// is 16,384 pixels, a band is at least 32,767 rows, so any rectangle
/// `XPutImage` can draw, which is at most that tall, is one band.
fn bands(rect: DeviceRect, stride: usize) -> impl Iterator<Item = DeviceRect> {
    let rows = i32::MAX
        .checked_div(i32::try_from(stride).unwrap_or(i32::MAX))
        .unwrap_or(i32::MAX)
        .max(1);
    let mut top = rect.y;
    core::iter::from_fn(move || {
        let left = rect.bottom().saturating_sub(top);
        if rect.is_empty() || left <= 0 {
            return None;
        }
        let height = left.min(rows);
        let band = DeviceRect::new(rect.x, top, rect.width, height);
        top = top.saturating_add(height);
        Some(band)
    })
}

// -- typed text ------------------------------------------------------------

impl Connection {
    /// Opens Xlib's own input method (`@im=none`), which composes dead keys
    /// and Compose sequences from the locale's Compose file and connects
    /// to nothing, and an input context in it for `window`, so that key
    /// presses type text ([`Connection::lookup_text`]). Returns the events
    /// the context needs the window to select besides the keys. If none
    /// opens, it says so in the log and returns 0: the keys still work,
    /// and nothing is typed.
    ///
    /// It first takes the C library's character locale from the
    /// environment if the program left it at the default
    /// ([`use_environment_ctype`]): in the "C" locale, Xlib composes
    /// Latin-1 characters only.
    pub(super) fn open_text_input(&mut self, window: Window) -> c_long {
        self.close_text_input();
        use_environment_ctype(self.xlib);
        // SAFETY: the claim puts this call on the Xlib thread, and the
        // modifiers are a C string, which Xlib copies. A null result, a
        // failure, leaves the modifiers as they were.
        let set = unsafe { (self.xlib.XSetLocaleModifiers)(c"@im=none".as_ptr()) };
        if set.is_null() {
            warn!(
                "Xlib refused the locale modifiers @im=none: the keys work, and nothing is typed"
            );
            return 0;
        }
        let Some(text) = self.open_input_method(window) else {
            warn!("Xlib's own input method did not open: the keys work, and nothing is typed");
            return 0;
        };
        let mask = self.filter_events(&text);
        self.text = Some(text);
        mask
    }

    /// Opens the input method that the locale modifiers choose, and makes a
    /// context for `window` in it. Closes it again if the context fails.
    fn open_input_method(&self, window: Window) -> Option<TextInput> {
        // SAFETY: the display is open, and the database and the names may
        // be null.
        let im = unsafe {
            (self.xlib.XOpenIM)(
                self.display.as_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        let im = NonNull::new(im)?;
        // The two style bits are positive `long`s: the cast is exact.
        let style = (XIMPreeditNothing | XIMStatusNothing) as XIMStyle;
        // SAFETY: the input method is open. Every argument after it is an
        // `XPointer`, as libX11 reads each with `va_arg` (`ICWrap.c`): a
        // name, then its value, the style and then the window twice, each
        // a number made a pointer, which the input method turns back into
        // the number and never dereferences. The names are C strings that
        // Xlib only reads, and the list ends with a null pointer.
        let ic = unsafe {
            (self.xlib.XCreateIC)(
                im.as_ptr(),
                name_pointer(XNInputStyle),
                number_pointer(style),
                name_pointer(XNClientWindow),
                number_pointer(window),
                name_pointer(XNFocusWindow),
                number_pointer(window),
                ptr::null_mut::<c_char>(),
            )
        };
        let Some(ic) = NonNull::new(ic) else {
            // SAFETY: the input method is open and has no context, and is
            // not used again. Xlib gives the result no meaning here.
            unsafe { (self.xlib.XCloseIM)(im.as_ptr()) };
            return None;
        };
        Some(TextInput { im, ic })
    }

    /// The events that the input context needs its window to select.
    fn filter_events(&self, text: &TextInput) -> c_long {
        let mut mask: c_ulong = 0;
        // SAFETY: the context is live. Every argument after it is an
        // `XPointer`, as libX11 reads each with `va_arg` (`ICWrap.c`): the
        // name, a C string that Xlib only reads, then a pointer to the
        // `unsigned long` where Xlib writes the mask, which lives through
        // the call. The list ends with a null pointer.
        let failed = unsafe {
            (self.xlib.XGetICValues)(
                text.ic.as_ptr(),
                name_pointer(XNFilterEvents),
                (&raw mut mask).cast::<c_char>(),
                ptr::null_mut::<c_char>(),
            )
        };
        if failed.is_null() {
            // An event mask is a `long` that the header reads as `unsigned`.
            mask as c_long
        } else {
            0
        }
    }

    /// Returns what a key press typed, which may be nothing, in `buffer`,
    /// which grows to fit. `None` for an event that is no key press, one
    /// that an input method took, or with no input context.
    pub(super) fn lookup_text<'b>(
        &self,
        event: &mut Received<'_>,
        buffer: &'b mut Vec<u8>,
    ) -> Option<&'b str> {
        let text = self.text.as_ref()?;
        // SAFETY: Xlib writes the type of every event it makes.
        if event.taken || unsafe { event.event.type_ } != KeyPress {
            return None;
        }
        if buffer.len() < 64 {
            buffer.resize(64, 0);
        }
        // Once for the text, and once more if it did not fit.
        for _ in 0..2 {
            let room = c_int::try_from(buffer.len()).unwrap_or(c_int::MAX);
            let mut status: Status = 0;
            // SAFETY: the context is live. The event is a key press that
            // Xlib made for this display and that no input method took:
            // `_XWireToEvent` writes every field of a key press's `xkey`,
            // and an input method's copy of one is a copy. The buffer has
            // `room` bytes, the key symbol may be null, and the status is a
            // live `Status`.
            let length = unsafe {
                (self.xlib.Xutf8LookupString)(
                    text.ic.as_ptr(),
                    &raw mut event.event.xkey,
                    buffer.as_mut_ptr().cast::<c_char>(),
                    room,
                    ptr::null_mut(),
                    &raw mut status,
                )
            };
            let length = usize::try_from(length).unwrap_or(0);
            match status {
                xlib::XLookupChars | xlib::XLookupBoth => {
                    let typed = buffer.get(..length.min(buffer.len()))?;
                    // Xlib writes UTF-8 here.
                    return core::str::from_utf8(typed).ok();
                }
                xlib::XBufferOverflow if length > buffer.len() && length <= MAX_TYPED => {
                    buffer.resize(length, 0);
                }
                _ => return None,
            }
        }
        None
    }

    /// Whether an input method and a context for the window are open.
    #[cfg(test)]
    pub(super) fn has_text_input(&self) -> bool {
        self.text.is_some()
    }

    /// Tells the input method whether the window has the keyboard focus.
    pub(super) fn set_text_focus(&self, focused: bool) {
        let Some(text) = &self.text else {
            return;
        };
        if focused {
            // SAFETY: the context is live.
            unsafe { (self.xlib.XSetICFocus)(text.ic.as_ptr()) };
        } else {
            // SAFETY: the context is live.
            unsafe { (self.xlib.XUnsetICFocus)(text.ic.as_ptr()) };
        }
    }

    /// Destroys the input context, then closes its input method. Call it
    /// before the context's window is destroyed. Nothing is typed after it.
    pub(super) fn close_text_input(&mut self) {
        let Some(text) = self.text.take() else {
            return;
        };
        // SAFETY: the context is live, and goes before its input method;
        // `take` forgets it.
        unsafe { (self.xlib.XDestroyIC)(text.ic.as_ptr()) };
        // SAFETY: the input method is open, and has no context left. Xlib
        // gives the result no meaning here.
        unsafe { (self.xlib.XCloseIM)(text.im.as_ptr()) };
    }
}

/// A name of an input method's list, as the `XPointer` (`char *`) that
/// libX11 reads it as. Xlib only reads it.
fn name_pointer(name: &CStr) -> XPointer {
    name.as_ptr().cast_mut()
}

/// A number in an input method's list, such as a style or a window, as the
/// `XPointer` that libX11 reads every value as: a pointer with no
/// provenance, which the input method turns back into the number and never
/// dereferences.
fn number_pointer(value: c_ulong) -> XPointer {
    // A `c_ulong` and a pointer are both 64 bits on the targets
    // `tidepool-sys` declares Linux for, so the cast is exact.
    ptr::without_provenance_mut(value as usize)
}

/// Takes the C library's character locale (`LC_CTYPE`) from the environment
/// if it is the default, "C" (or "POSIX"), which a Rust program never
/// changes: Xlib's input methods read and compose text in it, and in "C"
/// they type Latin-1 characters only. A locale that is not installed, or
/// that Xlib cannot work in, leaves "C".
///
/// The locale is the whole process's, and so is Xlib's state of it. Only a
/// connection, on the Xlib thread, calls this. Another thread's C code that
/// reads the locale would race with it; Tidepool's other threads are Rust
/// code, which does not, and the window opens before the workers start
/// (`docs/01-architecture.md`, "The main loop").
fn use_environment_ctype(xlib: &Xlib) {
    // SAFETY: a null locale only returns the current name.
    let current = unsafe { setlocale(LC_CTYPE, ptr::null()) };
    if !current.is_null() {
        // SAFETY: the C library returned a NUL-terminated name, valid until
        // the next `setlocale`, which comes after this use.
        let name = unsafe { CStr::from_ptr(current) };
        if name != c"C" && name != c"POSIX" {
            return;
        }
    }
    // SAFETY: the empty name is a C string, and reads the environment. No
    // other thread sets or reads the locale meanwhile (see above).
    let set = unsafe { setlocale(LC_CTYPE, c"".as_ptr()) };
    if set.is_null() {
        debug!("the environment names no installed character locale: typed text stays Latin-1");
        return;
    }
    // SAFETY: the Xlib thread calls it, and it reads only the locale.
    if unsafe { (xlib.XSupportsLocale)() } == False {
        // SAFETY: as above; "C" is always installed.
        unsafe { setlocale(LC_CTYPE, c"C".as_ptr()) };
        debug!("Xlib cannot work in the environment's character locale: typed text stays Latin-1");
        return;
    }
    // Asked again: `XSupportsLocale` itself calls `setlocale`, after which
    // the name it returned before may be gone.
    // SAFETY: a null locale only returns the current name.
    let name = unsafe { setlocale(LC_CTYPE, ptr::null()) };
    if !name.is_null() {
        // SAFETY: the C library returned a NUL-terminated name, valid until
        // the next `setlocale`, which comes after this use.
        let name = unsafe { CStr::from_ptr(name) };
        debug!(
            "the character locale is the environment's from now on, {:?}, for typed text",
            name.to_string_lossy()
        );
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close_text_input();
        self.free_gc();
        // SAFETY: the display is open, and nothing uses it after this: the
        // pointer goes away with `self`, and every `Received` that borrowed
        // it is gone. Closing sends every queued request first. Xlib gives
        // the result no meaning. The claim is released after this, when
        // the fields are dropped.
        unsafe { (self.xlib.XCloseDisplay)(self.display.as_ptr()) };
    }
}

/// An image read back from the server, for the window tests.
#[cfg(test)]
pub(super) struct Image {
    image: NonNull<XImage>,
}

#[cfg(test)]
impl Image {
    /// The pixel value at (`x`, `y`) as red, green and blue, or `None` if
    /// that is outside the image or the image's masks are not BGRA8's.
    pub(super) fn rgb(&self, x: i32, y: i32) -> Option<[u8; 3]> {
        // SAFETY: the image came from `XGetImage` and is freed only when
        // this is dropped.
        let image = unsafe { self.image.as_ref() };
        if x < 0 || y < 0 || x >= image.width || y >= image.height {
            return None;
        }
        if [image.red_mask, image.green_mask, image.blue_mask] != RGB_MASKS {
            return None;
        }
        let get_pixel = image.f.get_pixel?;
        // SAFETY: `XGetImage` set the functions for this image, and (x, y)
        // is inside it.
        let pixel = unsafe { get_pixel(self.image.as_ptr(), x, y) };
        let [blue, green, red, ..] = pixel.to_le_bytes();
        Some([red, green, blue])
    }
}

#[cfg(test)]
impl Drop for Image {
    fn drop(&mut self) {
        // SAFETY: the image came from `XGetImage`, which set the function
        // that frees it with its data; nothing uses it after this.
        let destroy = unsafe { self.image.as_ref() }.f.destroy_image;
        if let Some(destroy) = destroy {
            // SAFETY: as above. The result means nothing.
            unsafe { destroy(self.image.as_ptr()) };
        }
    }
}

#[cfg(test)]
impl Connection {
    /// Reads `size` pixels of `window` from its top-left corner back from
    /// the server, which waits for it. `None` if the server refused, as it
    /// does for a window that is not viewable.
    pub(super) fn get_image(&self, window: Window, size: DeviceSize) -> Option<Image> {
        let side = |value: i32| c_uint::try_from(value).ok();
        // SAFETY: the display is open. The image Xlib returns is freed by
        // `Image`'s `Drop`.
        let image = unsafe {
            (self.xlib.XGetImage)(
                self.display.as_ptr(),
                window,
                0,
                0,
                side(size.width)?,
                side(size.height)?,
                xlib::AllPlanes,
                ZPixmap,
            )
        };
        Some(Image {
            image: NonNull::new(image)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> DeviceRect {
        DeviceRect::new(x, y, width, height)
    }

    #[test]
    fn a_rect_is_one_band_at_any_usual_stride() {
        for stride in [4, 4 * 640, 4 * 3840, 64 * 1024] {
            let all: Vec<_> = bands(rect(3, 5, 100, 32_767), stride).collect();
            assert_eq!(all, [rect(3, 5, 100, 32_767)], "stride {stride}");
        }
    }

    #[test]
    fn a_large_stride_splits_a_rect_into_bands_of_whole_rows() {
        // 2^31 - 1 over 2^30 is one row and a bit: one row a band.
        let all: Vec<_> = bands(rect(0, 10, 4, 3), 1 << 30).collect();
        assert_eq!(
            all,
            [rect(0, 10, 4, 1), rect(0, 11, 4, 1), rect(0, 12, 4, 1)]
        );
        // 2^31 - 1 over 2^20 is 2047 rows.
        let all: Vec<_> = bands(rect(1, 0, 4, 5000), 1 << 20).collect();
        assert_eq!(
            all,
            [
                rect(1, 0, 4, 2047),
                rect(1, 2047, 4, 2047),
                rect(1, 4094, 4, 906)
            ]
        );
        for band in &all {
            assert!((band.height - 1).checked_mul(1 << 20).is_some(), "{band:?}");
        }
    }

    #[test]
    fn an_empty_rect_has_no_bands() {
        assert_eq!(bands(rect(0, 0, 0, 10), 64).count(), 0);
        assert_eq!(bands(rect(0, 0, 10, 0), 64).count(), 0);
        // A stride of 0 belongs to a frame with no width, and is one band.
        assert_eq!(bands(rect(0, 0, 1, 2), 0).count(), 1);
    }

    #[test]
    fn rows_are_found_only_inside_the_frame() {
        let bytes = [0_u8; 10 * 20];
        let pixels = Pixels::new(&bytes, DeviceSize::new(4, 10), 20).expect("a padded view");
        let rows = Rows::of(&pixels, rect(1, 2, 3, 8)).expect("inside");
        // From the start of the first row, whole rows to the frame's end.
        assert_eq!(rows.start.len(), bytes.len() - 2 * 20);
        assert_eq!(
            (
                rows.stride,
                rows.image_width,
                rows.src_x,
                rows.width,
                rows.height
            ),
            (20, 4, 1, 3, 8)
        );
        // Every row the image covers, each a whole stride, is there.
        assert!(rows.start.len() >= 8 * 20);
        let rows = Rows::of(&pixels, rect(0, 9, 2, 1)).expect("the last row");
        assert_eq!((rows.start.len(), rows.image_width, rows.src_x), (20, 2, 0));
        for outside in [
            rect(2, 0, 3, 1),
            rect(0, 9, 1, 2),
            rect(-1, 0, 1, 1),
            rect(0, -1, 1, 1),
            rect(0, 0, 0, 1),
            rect(0, 0, 1, 0),
        ] {
            assert!(Rows::of(&pixels, outside).is_none(), "{outside:?}");
        }
    }
}
