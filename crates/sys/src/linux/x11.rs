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

//! Xlib, the client library of the X Window System (`libX11.so.6`), which
//! Tidepool is to load with `dlopen` (ADR-0010, M2.2).
//!
//! Ported in part from nadir-sys `linux/x11.rs`.
//!
//! - Kept with Nadir's fields and signatures: the basic types (`XID`
//!   through `Time`, `Bool`, `Status`, `True`, `False`), `Display` and
//!   `Visual`; `XSetWindowAttributes` and `XVisualInfo`; the key, button,
//!   motion, focus, configure and client-message events; `InputOutput`,
//!   `CWBackPixel`, `CWBorderPixel`, `CWColormap`, `TrueColor`,
//!   `AllocNone`, `PropModeReplace`, `XA_STRING`, `XA_WM_NAME`, Nadir's
//!   event masks and event types but two masks (below), the buttons and
//!   the focus modes; and 19 of the table's functions: `XOpenDisplay`,
//!   `XCloseDisplay`, `XDefaultScreen`, `XRootWindow`, `XCreateColormap`,
//!   `XFreeColormap`, `XCreateWindow`, `XDestroyWindow`, `XMapWindow`,
//!   `XSelectInput`, `XInternAtom`, `XSetWMProtocols`, `XChangeProperty`,
//!   `XPending`, `XNextEvent`, `XkbSetDetectableAutoRepeat`,
//!   `XDefineCursor`, `XFreeCursor` and `XResourceManagerString`.
//! - Changed: the comments; the table is written `unsafe extern "C"`
//!   (`dynlib` says why) and its functions are grouped by use; the offset
//!   assertions now cover every field;
//!   `XEvent` gains `xany` and `xexpose`; and the table's test prints
//!   `SKIPPED:` with `dlerror`'s reason when `libX11.so.6` cannot be loaded.
//! - Left out, since Tidepool does not call them yet: `XSizeHints`,
//!   `XAspectRatio`, `PMinSize`, `PMaxSize`, `XColor`, `XA_ATOM`,
//!   `VisualIDMask`, `SubstructureNotifyMask`, `SubstructureRedirectMask`,
//!   `XGetVisualInfo`, `XFree`, `XSetWMNormalHints`, `XSendEvent`,
//!   `XCreateBitmapFromData`, `XCreatePixmapCursor`, `XFreePixmap` and
//!   `XUndefineCursor`.
//! - New here: `XPointer`; the graphics context (`_XGC`, `GC`,
//!   `XGCValues`, `XCreateGC`, `XFreeGC`); the images (`ZPixmap`,
//!   `LSBFirst`, `AllPlanes`, `XImageFuncs`, `XImage`, `XInitImage`,
//!   `XPutImage`, `XGetImage`); `XAnyEvent`, `XExposeEvent`,
//!   `ExposureMask` and `Expose`; the modifier masks; `XErrorEvent`,
//!   `XErrorHandler`, `XIOErrorHandler`, `XSetErrorHandler` and
//!   `XSetIOErrorHandler`; `XA_CARDINAL`, `XA_WM_CLIENT_MACHINE` and
//!   `XA_WM_CLASS`; the core font cursors (the `XC_` shapes and
//!   `XCreateFontCursor`); and `XConnectionNumber`, `XFlush`, `XSync` and
//!   `XMatchVisualInfo`.
//!
//! Sources: `<X11/X.h>`, `<X11/Xlib.h>`, `<X11/Xutil.h>`, `<X11/Xatom.h>`,
//! `<X11/cursorfont.h>` and `<X11/XKBlib.h>`. Each item names the header it
//! comes from and gives the declaration there. The headers write most
//! parameter names only as comments (`Display* /* display */`); the
//! declarations here write them as names.
//!
//! Only what `tidepool-platform`'s window, events and surface are to call
//! (M2.2) is declared. Not declared yet: the key lookup
//! (`docs/03-platforms.md` names `XLookupKeysym`, while
//! `tidepool_core::input::Key` is a physical key, which M2.2 reconciles)
//! and typed text, which needs an input context (`XOpenIM`, and
//! `XCreateIC`, which is variadic) for `Xutf8LookupString` (M2.2); and the
//! selections and window properties that the clipboard reads (M2.7).
//!
//! # Types
//!
//! A struct has all the fields of its header, in the header's order, with
//! the header's names; `type` is `type_`, since `type` is a keyword in Rust.
//! The [`XEvent`] union has only the members we use, and the padding that
//! fixes its size. A type that only Xlib looks inside ([`Display`],
//! [`_XGC`]), or that we only pass a pointer to ([`Visual`],
//! [`XGCValues`]), is opaque: it has no fields, cannot be made, and has no
//! layout to assert.
//!
//! # Constants
//!
//! A constant keeps its header's name and value. One that the header gives a
//! type (`1L<<0`, `((Atom) 4)`) has that type. One that the header leaves as
//! a bare integer (`#define KeyPress 2`, `(1<<0)`) has the type of the field
//! or parameter it is used with.
//!
//! # Layouts
//!
//! The sizes, alignments and offsets asserted below are those a C compiler
//! gives for the headers on x86_64 Linux. A C program printed them with
//! `sizeof`, `_Alignof` and `offsetof`; they were not read off the Rust
//! types. They hold on x86_64 and aarch64 Linux, where `long` and pointers
//! are 8 bytes; the parent module fails the build on any other target.
//! Every field of every struct has its offset asserted.
//!
//! # Calling Xlib
//!
//! What every function in [`Xlib`] requires of its caller:
//!
//! - A `display` argument came from `XOpenDisplay` and is not closed yet.
//! - Xlib calls in the process never overlap: each one, for any display or
//!   for none, happens after the one before it has returned. One thread
//!   making them all does this; in Tidepool that is the main thread
//!   (`docs/03-platforms.md`). Xlib keeps state for the whole process, such
//!   as the error handlers, which `XOpenDisplay`, `XSetErrorHandler` and
//!   `XSetIOErrorHandler` use, and the last two take no display.
//! - Tidepool never calls `XInitThreads`. Whether Xlib locks anyway
//!   depends on how libX11 was built: from 1.8 on, by default, it calls
//!   `XInitThreads` itself when the library is loaded, `dlopen` included,
//!   and a build without that option, like any earlier version, locks
//!   nothing. The rule above is sound either way.
//! - A window, pixmap, cursor, colormap, graphics context or atom argument
//!   belongs to that display.
//! - A pointer argument is valid for what the function reads or writes
//!   through it.
//!
//! Many Xlib functions are declared to return `int`, and the Xlib manual
//! gives that result no meaning. They queue a request and wait for no reply
//! of their own. They can still wait: writing the request blocks while the
//! connection's socket is full, and now and then Xlib makes a round trip
//! of its own to keep its request numbers in step. If the server rejects
//! it, an X error arrives later, and the error
//! handler is called with it; Xlib's default handler prints it and ends the
//! process, so `tidepool-platform` is to install its own with
//! `XSetErrorHandler` (M2.2). Where a result does mean something, the
//! function's documentation here says what.

// The constants keep the names their headers give them (`KeyPress`, `True`),
// so that each one can be compared with its header.
#![allow(non_upper_case_globals)]

use core::ffi::{c_char, c_int, c_long, c_short, c_uchar, c_uint, c_ulong};
use core::marker::{PhantomData, PhantomPinned};
use core::mem::offset_of;

use crate::dynlib::dynlib;

// ---------------------------------------------------------------------------
// Basic types
// ---------------------------------------------------------------------------

/// The id of a resource that the server holds for us.
/// `X11/X.h`: `typedef unsigned long XID;`
pub type XID = c_ulong;

/// A window. `X11/X.h`: `typedef XID Window;`
pub type Window = XID;

/// A window or a pixmap. `X11/X.h`: `typedef XID Drawable;`
pub type Drawable = XID;

/// A pointer shape. `X11/X.h`: `typedef XID Cursor;`
pub type Cursor = XID;

/// A color table. `X11/X.h`: `typedef XID Colormap;`
pub type Colormap = XID;

/// An image held by the server. `X11/X.h`: `typedef XID Pixmap;`
pub type Pixmap = XID;

/// The id of a visual: a way of turning pixel values into colors.
/// `X11/X.h`: `typedef unsigned long VisualID;`
pub type VisualID = c_ulong;

/// A number the server assigns to a name.
/// `X11/X.h`: `typedef unsigned long Atom;`
pub type Atom = c_ulong;

/// A server timestamp, in milliseconds.
/// `X11/X.h`: `typedef unsigned long Time;`
pub type Time = c_ulong;

/// A truth value: [`True`] or [`False`]. `X11/Xlib.h`: `#define Bool int`
pub type Bool = c_int;

/// The result of some Xlib functions: zero for failure.
/// `X11/Xlib.h`: `#define Status int`
pub type Status = c_int;

/// Data that Xlib hangs on an object. `X11/Xlib.h`: `typedef char
/// *XPointer;`
pub type XPointer = *mut c_char;

/// `X11/Xlib.h`: `#define True 1`
pub const True: Bool = 1;

/// `X11/Xlib.h`: `#define False 0`
pub const False: Bool = 0;

/// A connection to an X server. Only Xlib looks inside it.
/// `X11/Xlib.h`: `typedef struct _XDisplay Display;`
#[repr(C)]
pub struct Display {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// A way of turning pixel values into colors. The header defines its fields,
/// but we only pass pointers to it, so it is opaque here.
/// `X11/Xlib.h`: `typedef struct { ... } Visual;`
#[repr(C)]
pub struct Visual {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// The state that drawing requests use, such as the plane mask and clip.
/// Only Xlib looks inside it. The header gives two of its fields only to
/// code that defines `XLIB_ILLEGAL_ACCESS` (Xlib's own definition is in
/// `Xlibint.h`), and says that "a GC should be treated as opaque by
/// application code". `X11/Xlib.h`: `typedef struct _XGC { ... } *GC;`
#[repr(C)]
pub struct _XGC {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// A graphics context. `X11/Xlib.h`: `typedef struct _XGC *GC;`
pub type GC = *mut _XGC;

/// The values of a new graphics context, for `XCreateGC`. The header
/// defines its fields, but we pass null with a `valuemask` of 0, which
/// gives every value its default, so it is opaque here.
/// `X11/Xlib.h`: `typedef struct { ... } XGCValues;`
#[repr(C)]
pub struct XGCValues {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

// ---------------------------------------------------------------------------
// Creating a window
// ---------------------------------------------------------------------------

/// The `class` of a window that can be drawn to, for `XCreateWindow`.
/// `X11/X.h`: `#define InputOutput 1`
pub const InputOutput: c_uint = 1;

/// A bit of the `valuemask` of `XCreateWindow`: `background_pixel` is set,
/// which the server fills newly exposed parts of the window with.
/// `X11/X.h`: `#define CWBackPixel (1L<<1)`
pub const CWBackPixel: c_long = 1 << 1;

/// A bit of the `valuemask` of `XCreateWindow`: `border_pixel` is set. A
/// window of a depth other than its parent's needs it (or a border pixmap),
/// because the default copies the parent's border, which needs the same
/// depth. Without one the server refuses the window.
/// `X11/X.h`: `#define CWBorderPixel (1L<<3)`
pub const CWBorderPixel: c_long = 1 << 3;

/// A bit of the `valuemask` of `XCreateWindow`: `colormap` is set. A window
/// of a visual other than its parent's needs a colormap of that visual, or
/// the server refuses the window.
/// `X11/X.h`: `#define CWColormap (1L<<13)`
pub const CWColormap: c_long = 1 << 13;

/// The attributes of a new window, for `XCreateWindow`. The function reads
/// only the fields that its `valuemask` names.
/// `X11/Xlib.h`: `typedef struct { ... } XSetWindowAttributes;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XSetWindowAttributes {
    /// The background as a pixmap.
    pub background_pixmap: Pixmap,
    /// The background as one pixel value.
    pub background_pixel: c_ulong,
    /// The border as a pixmap.
    pub border_pixmap: Pixmap,
    /// The border as one pixel value.
    pub border_pixel: c_ulong,
    /// Where the contents stay when the window is resized.
    pub bit_gravity: c_int,
    /// Where the window stays when its parent is resized.
    pub win_gravity: c_int,
    /// Whether the server should keep the contents of hidden parts.
    pub backing_store: c_int,
    /// The planes to keep, if the server keeps contents.
    pub backing_planes: c_ulong,
    /// The value for the planes that are not kept.
    pub backing_pixel: c_ulong,
    /// Whether the server should save what the window covers.
    pub save_under: Bool,
    /// The events the window wants.
    pub event_mask: c_long,
    /// The events that must not pass on to the parent.
    pub do_not_propagate_mask: c_long,
    /// Whether the window manager is to leave the window alone.
    pub override_redirect: Bool,
    /// The color table of the window.
    pub colormap: Colormap,
    /// The pointer shape over the window.
    pub cursor: Cursor,
}

const _: () = assert!(size_of::<XSetWindowAttributes>() == 112);
const _: () = assert!(align_of::<XSetWindowAttributes>() == 8);
const _: () = assert!(offset_of!(XSetWindowAttributes, background_pixmap) == 0);
const _: () = assert!(offset_of!(XSetWindowAttributes, background_pixel) == 8);
const _: () = assert!(offset_of!(XSetWindowAttributes, border_pixmap) == 16);
const _: () = assert!(offset_of!(XSetWindowAttributes, border_pixel) == 24);
const _: () = assert!(offset_of!(XSetWindowAttributes, bit_gravity) == 32);
const _: () = assert!(offset_of!(XSetWindowAttributes, win_gravity) == 36);
const _: () = assert!(offset_of!(XSetWindowAttributes, backing_store) == 40);
const _: () = assert!(offset_of!(XSetWindowAttributes, backing_planes) == 48);
const _: () = assert!(offset_of!(XSetWindowAttributes, backing_pixel) == 56);
const _: () = assert!(offset_of!(XSetWindowAttributes, save_under) == 64);
const _: () = assert!(offset_of!(XSetWindowAttributes, event_mask) == 72);
const _: () = assert!(offset_of!(XSetWindowAttributes, do_not_propagate_mask) == 80);
const _: () = assert!(offset_of!(XSetWindowAttributes, override_redirect) == 88);
const _: () = assert!(offset_of!(XSetWindowAttributes, colormap) == 96);
const _: () = assert!(offset_of!(XSetWindowAttributes, cursor) == 104);

// ---------------------------------------------------------------------------
// Visuals and colormaps
// ---------------------------------------------------------------------------

/// A visual and what it is like, as `XMatchVisualInfo` describes it.
/// `X11/Xutil.h`: `typedef struct { ... } XVisualInfo;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XVisualInfo {
    /// The visual. It belongs to the display, and lives until the display
    /// is closed.
    pub visual: *mut Visual,
    /// The id of the visual.
    pub visualid: VisualID,
    /// The number of the screen the visual is on.
    pub screen: c_int,
    /// The bits in a pixel of a window of this visual.
    pub depth: c_int,
    /// How pixel values become colors: [`TrueColor`] and others. `class` in
    /// C, and `c_class` in C++.
    pub class: c_int,
    /// The bits of a pixel value that hold red.
    pub red_mask: c_ulong,
    /// The bits that hold green.
    pub green_mask: c_ulong,
    /// The bits that hold blue.
    pub blue_mask: c_ulong,
    /// The number of entries in a colormap of the visual.
    pub colormap_size: c_int,
    /// The bits of each of red, green and blue.
    pub bits_per_rgb: c_int,
}

const _: () = assert!(size_of::<XVisualInfo>() == 64);
const _: () = assert!(align_of::<XVisualInfo>() == 8);
const _: () = assert!(offset_of!(XVisualInfo, visual) == 0);
const _: () = assert!(offset_of!(XVisualInfo, visualid) == 8);
const _: () = assert!(offset_of!(XVisualInfo, screen) == 16);
const _: () = assert!(offset_of!(XVisualInfo, depth) == 20);
const _: () = assert!(offset_of!(XVisualInfo, class) == 24);
const _: () = assert!(offset_of!(XVisualInfo, red_mask) == 32);
const _: () = assert!(offset_of!(XVisualInfo, green_mask) == 40);
const _: () = assert!(offset_of!(XVisualInfo, blue_mask) == 48);
const _: () = assert!(offset_of!(XVisualInfo, colormap_size) == 56);
const _: () = assert!(offset_of!(XVisualInfo, bits_per_rgb) == 60);

/// A [`XVisualInfo::class`], and the `class` of `XMatchVisualInfo`: a pixel
/// value is red, green and blue bits, which mean fixed intensities.
/// Desktops draw in visuals of this class.
/// `X11/X.h`: `#define TrueColor 4`
pub const TrueColor: c_int = 4;

/// The `alloc` of `XCreateColormap`: allocate no entries, which is what a
/// colormap of a [`TrueColor`] visual needs.
/// `X11/X.h`: `#define AllocNone 0`
pub const AllocNone: c_int = 0;

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// The `mode` of `XChangeProperty` that discards the old value.
/// `X11/X.h`: `#define PropModeReplace 0`
pub const PropModeReplace: c_int = 0;

/// The property type "unsigned 32-bit numbers", as `_NET_WM_PID` and
/// `_NET_WM_ICON` hold. `X11/Xatom.h`: `#define XA_CARDINAL ((Atom) 6)`
pub const XA_CARDINAL: Atom = 6;

/// The property type "Latin-1 text", as `WM_CLASS` and
/// `WM_CLIENT_MACHINE` hold. `X11/Xatom.h`: `#define XA_STRING ((Atom) 31)`
pub const XA_STRING: Atom = 31;

/// The property that names the machine a window's client runs on, which
/// a window that sets `_NET_WM_PID` must also set (EWMH).
/// `X11/Xatom.h`: `#define XA_WM_CLIENT_MACHINE ((Atom) 36)`
pub const XA_WM_CLIENT_MACHINE: Atom = 36;

/// The property that holds a window's title, for window managers that do
/// not read `_NET_WM_NAME`.
/// `X11/Xatom.h`: `#define XA_WM_NAME ((Atom) 39)`
pub const XA_WM_NAME: Atom = 39;

/// The property that holds a window's instance and class names, each
/// ending with a NUL. `X11/Xatom.h`: `#define XA_WM_CLASS ((Atom) 67)`
pub const XA_WM_CLASS: Atom = 67;

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Event selection: keys going down. `X11/X.h`: `#define KeyPressMask (1L<<0)`
pub const KeyPressMask: c_long = 1 << 0;

/// Event selection: keys going up.
/// `X11/X.h`: `#define KeyReleaseMask (1L<<1)`
pub const KeyReleaseMask: c_long = 1 << 1;

/// Event selection: pointer buttons going down.
/// `X11/X.h`: `#define ButtonPressMask (1L<<2)`
pub const ButtonPressMask: c_long = 1 << 2;

/// Event selection: pointer buttons going up.
/// `X11/X.h`: `#define ButtonReleaseMask (1L<<3)`
pub const ButtonReleaseMask: c_long = 1 << 3;

/// Event selection: the pointer moving.
/// `X11/X.h`: `#define PointerMotionMask (1L<<6)`
pub const PointerMotionMask: c_long = 1 << 6;

/// Event selection: parts of the window that must be drawn again.
/// `X11/X.h`: `#define ExposureMask (1L<<15)`
pub const ExposureMask: c_long = 1 << 15;

/// Event selection: changes to the window itself, such as its size.
/// `X11/X.h`: `#define StructureNotifyMask (1L<<17)`
pub const StructureNotifyMask: c_long = 1 << 17;

/// Event selection: the window gaining or losing the keyboard focus.
/// `X11/X.h`: `#define FocusChangeMask (1L<<21)`
pub const FocusChangeMask: c_long = 1 << 21;

/// Event type: a key went down; see [`XKeyEvent`].
/// `X11/X.h`: `#define KeyPress 2`
pub const KeyPress: c_int = 2;

/// Event type: a key went up; see [`XKeyEvent`].
/// `X11/X.h`: `#define KeyRelease 3`
pub const KeyRelease: c_int = 3;

/// Event type: a pointer button went down; see [`XButtonEvent`].
/// `X11/X.h`: `#define ButtonPress 4`
pub const ButtonPress: c_int = 4;

/// Event type: a pointer button went up; see [`XButtonEvent`].
/// `X11/X.h`: `#define ButtonRelease 5`
pub const ButtonRelease: c_int = 5;

/// Event type: the pointer moved; see [`XMotionEvent`].
/// `X11/X.h`: `#define MotionNotify 6`
pub const MotionNotify: c_int = 6;

/// Event type: the window gained the keyboard focus; see
/// [`XFocusChangeEvent`]. `X11/X.h`: `#define FocusIn 9`
pub const FocusIn: c_int = 9;

/// Event type: the window lost the keyboard focus; see
/// [`XFocusChangeEvent`]. `X11/X.h`: `#define FocusOut 10`
pub const FocusOut: c_int = 10;

/// Event type: a part of the window must be drawn again; see
/// [`XExposeEvent`]. `X11/X.h`: `#define Expose 12`
pub const Expose: c_int = 12;

/// Event type: the window's size, position or stacking changed; see
/// [`XConfigureEvent`]. `X11/X.h`: `#define ConfigureNotify 22`
pub const ConfigureNotify: c_int = 22;

/// Event type: a message from another client; see [`XClientMessageEvent`].
/// `X11/X.h`: `#define ClientMessage 33`
pub const ClientMessage: c_int = 33;

/// A bit of the `state` of a key, button or motion event: Shift is held.
/// `X11/X.h`: `#define ShiftMask (1<<0)`
pub const ShiftMask: c_uint = 1 << 0;

/// A bit of `state`: Control is held. `X11/X.h`: `#define ControlMask
/// (1<<2)`
pub const ControlMask: c_uint = 1 << 2;

/// A bit of `state`: the first extra modifier is held, which the usual
/// keyboard map gives to Alt. `X11/X.h`: `#define Mod1Mask (1<<3)`
pub const Mod1Mask: c_uint = 1 << 3;

/// A bit of `state`: the fourth extra modifier is held, which the usual
/// keyboard map gives to Super (the Windows key).
/// `X11/X.h`: `#define Mod4Mask (1<<6)`
pub const Mod4Mask: c_uint = 1 << 6;

/// [`XButtonEvent::button`]: the left button. `X11/X.h`: `#define Button1 1`
pub const Button1: c_uint = 1;

/// [`XButtonEvent::button`]: the middle button.
/// `X11/X.h`: `#define Button2 2`
pub const Button2: c_uint = 2;

/// [`XButtonEvent::button`]: the right button.
/// `X11/X.h`: `#define Button3 3`
pub const Button3: c_uint = 3;

/// [`XButtonEvent::button`]: one step of the wheel away from the user.
/// `X11/X.h`: `#define Button4 4`
pub const Button4: c_uint = 4;

/// [`XButtonEvent::button`]: one step of the wheel toward the user. Servers
/// report a step of a sideways wheel as buttons 6 (left) and 7 (right),
/// which `X.h` gives no names.
/// `X11/X.h`: `#define Button5 5`
pub const Button5: c_uint = 5;

/// [`XFocusChangeEvent::mode`]: a keyboard grab started, which moves the
/// focus for as long as the grab lasts. `X11/X.h`: `#define NotifyGrab 1`
pub const NotifyGrab: c_int = 1;

/// [`XFocusChangeEvent::mode`]: a keyboard grab ended.
/// `X11/X.h`: `#define NotifyUngrab 2`
pub const NotifyUngrab: c_int = 2;

/// The part that every event starts with, whatever its type.
/// `X11/Xlib.h`: `typedef struct { ... } XAnyEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XAnyEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window the event is about, by which one display's events are
    /// told apart for each of its windows.
    pub window: Window,
}

const _: () = assert!(size_of::<XAnyEvent>() == 40);
const _: () = assert!(align_of::<XAnyEvent>() == 8);
const _: () = assert!(offset_of!(XAnyEvent, type_) == 0);
const _: () = assert!(offset_of!(XAnyEvent, serial) == 8);
const _: () = assert!(offset_of!(XAnyEvent, send_event) == 16);
const _: () = assert!(offset_of!(XAnyEvent, display) == 24);
const _: () = assert!(offset_of!(XAnyEvent, window) == 32);

/// A key going down ([`KeyPress`]) or up ([`KeyRelease`]).
/// `X11/Xlib.h`: `typedef struct { ... } XKeyEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XKeyEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window the event is reported to.
    pub window: Window,
    /// The root window of the screen.
    pub root: Window,
    /// The child window the pointer is in, if any.
    pub subwindow: Window,
    /// When it happened.
    pub time: Time,
    /// The pointer's x in the window.
    pub x: c_int,
    /// The pointer's y in the window.
    pub y: c_int,
    /// The pointer's x on the screen.
    pub x_root: c_int,
    /// The pointer's y on the screen.
    pub y_root: c_int,
    /// The modifier keys and buttons held just before the event:
    /// [`ShiftMask`] and others.
    pub state: c_uint,
    /// Which physical key. With xkeyboard-config's `evdev` keycodes, which
    /// the evdev and libinput drivers and XWayland use, it is the Linux
    /// evdev code plus 8. That comes from those keycodes, not from XKB: the
    /// legacy `kbd` driver's `xfree86` keycodes differ (Up is 98 there).
    pub keycode: c_uint,
    /// Whether the pointer is on the same screen as the window.
    pub same_screen: Bool,
}

const _: () = assert!(size_of::<XKeyEvent>() == 96);
const _: () = assert!(align_of::<XKeyEvent>() == 8);
const _: () = assert!(offset_of!(XKeyEvent, type_) == 0);
const _: () = assert!(offset_of!(XKeyEvent, serial) == 8);
const _: () = assert!(offset_of!(XKeyEvent, send_event) == 16);
const _: () = assert!(offset_of!(XKeyEvent, display) == 24);
const _: () = assert!(offset_of!(XKeyEvent, window) == 32);
const _: () = assert!(offset_of!(XKeyEvent, root) == 40);
const _: () = assert!(offset_of!(XKeyEvent, subwindow) == 48);
const _: () = assert!(offset_of!(XKeyEvent, time) == 56);
const _: () = assert!(offset_of!(XKeyEvent, x) == 64);
const _: () = assert!(offset_of!(XKeyEvent, y) == 68);
const _: () = assert!(offset_of!(XKeyEvent, x_root) == 72);
const _: () = assert!(offset_of!(XKeyEvent, y_root) == 76);
const _: () = assert!(offset_of!(XKeyEvent, state) == 80);
const _: () = assert!(offset_of!(XKeyEvent, keycode) == 84);
const _: () = assert!(offset_of!(XKeyEvent, same_screen) == 88);

/// A pointer button going down ([`ButtonPress`]) or up ([`ButtonRelease`]).
/// `X11/Xlib.h`: `typedef struct { ... } XButtonEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XButtonEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window the event is reported to.
    pub window: Window,
    /// The root window of the screen.
    pub root: Window,
    /// The child window the pointer is in, if any.
    pub subwindow: Window,
    /// When it happened.
    pub time: Time,
    /// The pointer's x in the window.
    pub x: c_int,
    /// The pointer's y in the window.
    pub y: c_int,
    /// The pointer's x on the screen.
    pub x_root: c_int,
    /// The pointer's y on the screen.
    pub y_root: c_int,
    /// The modifier keys and buttons held just before the event:
    /// [`ShiftMask`] and others.
    pub state: c_uint,
    /// Which button: [`Button1`] and so on.
    pub button: c_uint,
    /// Whether the pointer is on the same screen as the window.
    pub same_screen: Bool,
}

const _: () = assert!(size_of::<XButtonEvent>() == 96);
const _: () = assert!(align_of::<XButtonEvent>() == 8);
const _: () = assert!(offset_of!(XButtonEvent, type_) == 0);
const _: () = assert!(offset_of!(XButtonEvent, serial) == 8);
const _: () = assert!(offset_of!(XButtonEvent, send_event) == 16);
const _: () = assert!(offset_of!(XButtonEvent, display) == 24);
const _: () = assert!(offset_of!(XButtonEvent, window) == 32);
const _: () = assert!(offset_of!(XButtonEvent, root) == 40);
const _: () = assert!(offset_of!(XButtonEvent, subwindow) == 48);
const _: () = assert!(offset_of!(XButtonEvent, time) == 56);
const _: () = assert!(offset_of!(XButtonEvent, x) == 64);
const _: () = assert!(offset_of!(XButtonEvent, y) == 68);
const _: () = assert!(offset_of!(XButtonEvent, x_root) == 72);
const _: () = assert!(offset_of!(XButtonEvent, y_root) == 76);
const _: () = assert!(offset_of!(XButtonEvent, state) == 80);
const _: () = assert!(offset_of!(XButtonEvent, button) == 84);
const _: () = assert!(offset_of!(XButtonEvent, same_screen) == 88);

/// The pointer moving ([`MotionNotify`]).
/// `X11/Xlib.h`: `typedef struct { ... } XMotionEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XMotionEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window the event is reported to.
    pub window: Window,
    /// The root window of the screen.
    pub root: Window,
    /// The child window the pointer is in, if any.
    pub subwindow: Window,
    /// When it happened.
    pub time: Time,
    /// The pointer's x in the window.
    pub x: c_int,
    /// The pointer's y in the window.
    pub y: c_int,
    /// The pointer's x on the screen.
    pub x_root: c_int,
    /// The pointer's y on the screen.
    pub y_root: c_int,
    /// The modifier keys and buttons held: [`ShiftMask`] and others.
    pub state: c_uint,
    /// Whether this is a hint that stands for several motions.
    pub is_hint: c_char,
    /// Whether the pointer is on the same screen as the window.
    pub same_screen: Bool,
}

const _: () = assert!(size_of::<XMotionEvent>() == 96);
const _: () = assert!(align_of::<XMotionEvent>() == 8);
const _: () = assert!(offset_of!(XMotionEvent, type_) == 0);
const _: () = assert!(offset_of!(XMotionEvent, serial) == 8);
const _: () = assert!(offset_of!(XMotionEvent, send_event) == 16);
const _: () = assert!(offset_of!(XMotionEvent, display) == 24);
const _: () = assert!(offset_of!(XMotionEvent, window) == 32);
const _: () = assert!(offset_of!(XMotionEvent, root) == 40);
const _: () = assert!(offset_of!(XMotionEvent, subwindow) == 48);
const _: () = assert!(offset_of!(XMotionEvent, time) == 56);
const _: () = assert!(offset_of!(XMotionEvent, x) == 64);
const _: () = assert!(offset_of!(XMotionEvent, y) == 68);
const _: () = assert!(offset_of!(XMotionEvent, x_root) == 72);
const _: () = assert!(offset_of!(XMotionEvent, y_root) == 76);
const _: () = assert!(offset_of!(XMotionEvent, state) == 80);
const _: () = assert!(offset_of!(XMotionEvent, is_hint) == 84);
const _: () = assert!(offset_of!(XMotionEvent, same_screen) == 88);

/// The window gaining ([`FocusIn`]) or losing ([`FocusOut`]) the keyboard
/// focus. `X11/Xlib.h`: `typedef struct { ... } XFocusChangeEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XFocusChangeEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window.
    pub window: Window,
    /// Why the focus moved: [`NotifyGrab`], [`NotifyUngrab`], or a value
    /// for an ordinary change.
    pub mode: c_int,
    /// How the two windows the focus moved between are related.
    pub detail: c_int,
}

const _: () = assert!(size_of::<XFocusChangeEvent>() == 48);
const _: () = assert!(align_of::<XFocusChangeEvent>() == 8);
const _: () = assert!(offset_of!(XFocusChangeEvent, type_) == 0);
const _: () = assert!(offset_of!(XFocusChangeEvent, serial) == 8);
const _: () = assert!(offset_of!(XFocusChangeEvent, send_event) == 16);
const _: () = assert!(offset_of!(XFocusChangeEvent, display) == 24);
const _: () = assert!(offset_of!(XFocusChangeEvent, window) == 32);
const _: () = assert!(offset_of!(XFocusChangeEvent, mode) == 40);
const _: () = assert!(offset_of!(XFocusChangeEvent, detail) == 44);

/// A rectangle of the window that must be drawn again ([`Expose`]), as when
/// it is first shown or another window stops covering it. The server
/// sends one event for each rectangle of one exposure; `count` says how
/// many more follow.
/// `X11/Xlib.h`: `typedef struct { ... } XExposeEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XExposeEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window.
    pub window: Window,
    /// The left of the rectangle, in the window.
    pub x: c_int,
    /// The top of the rectangle, in the window.
    pub y: c_int,
    /// The width of the rectangle, in pixels.
    pub width: c_int,
    /// The height of the rectangle, in pixels.
    pub height: c_int,
    /// If not zero, at least this many more `Expose` events of the same
    /// exposure follow.
    pub count: c_int,
}

const _: () = assert!(size_of::<XExposeEvent>() == 64);
const _: () = assert!(align_of::<XExposeEvent>() == 8);
const _: () = assert!(offset_of!(XExposeEvent, type_) == 0);
const _: () = assert!(offset_of!(XExposeEvent, serial) == 8);
const _: () = assert!(offset_of!(XExposeEvent, send_event) == 16);
const _: () = assert!(offset_of!(XExposeEvent, display) == 24);
const _: () = assert!(offset_of!(XExposeEvent, window) == 32);
const _: () = assert!(offset_of!(XExposeEvent, x) == 40);
const _: () = assert!(offset_of!(XExposeEvent, y) == 44);
const _: () = assert!(offset_of!(XExposeEvent, width) == 48);
const _: () = assert!(offset_of!(XExposeEvent, height) == 52);
const _: () = assert!(offset_of!(XExposeEvent, count) == 56);

/// The window's size, position, border or stacking changing
/// ([`ConfigureNotify`]). `X11/Xlib.h`: `typedef struct { ... }
/// XConfigureEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XConfigureEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window the event is reported to.
    pub event: Window,
    /// The window that changed.
    pub window: Window,
    /// The new x of the window in its parent.
    pub x: c_int,
    /// The new y of the window in its parent.
    pub y: c_int,
    /// The new width of the inside of the window, in pixels.
    pub width: c_int,
    /// The new height of the inside of the window, in pixels.
    pub height: c_int,
    /// The new border width.
    pub border_width: c_int,
    /// The sibling the window is now stacked above, if any.
    pub above: Window,
    /// Whether the window manager leaves the window alone.
    pub override_redirect: Bool,
}

const _: () = assert!(size_of::<XConfigureEvent>() == 88);
const _: () = assert!(align_of::<XConfigureEvent>() == 8);
const _: () = assert!(offset_of!(XConfigureEvent, type_) == 0);
const _: () = assert!(offset_of!(XConfigureEvent, serial) == 8);
const _: () = assert!(offset_of!(XConfigureEvent, send_event) == 16);
const _: () = assert!(offset_of!(XConfigureEvent, display) == 24);
const _: () = assert!(offset_of!(XConfigureEvent, event) == 32);
const _: () = assert!(offset_of!(XConfigureEvent, window) == 40);
const _: () = assert!(offset_of!(XConfigureEvent, x) == 48);
const _: () = assert!(offset_of!(XConfigureEvent, y) == 52);
const _: () = assert!(offset_of!(XConfigureEvent, width) == 56);
const _: () = assert!(offset_of!(XConfigureEvent, height) == 60);
const _: () = assert!(offset_of!(XConfigureEvent, border_width) == 64);
const _: () = assert!(offset_of!(XConfigureEvent, above) == 72);
const _: () = assert!(offset_of!(XConfigureEvent, override_redirect) == 80);

/// The payload of an [`XClientMessageEvent`]. Which member is in use depends
/// on the event's `format`. The header gives this union no name.
/// `X11/Xlib.h`: `union { char b[20]; short s[10]; long l[5]; } data;`
#[repr(C)]
#[derive(Clone, Copy)]
pub union XClientMessageData {
    /// For `format` 8.
    pub b: [c_char; 20],
    /// For `format` 16.
    pub s: [c_short; 10],
    /// For `format` 32. Each item is a C `long`, which is 8 bytes on 64-bit
    /// Linux, but only its low 32 bits travel to and from the server.
    pub l: [c_long; 5],
}

const _: () = assert!(size_of::<XClientMessageData>() == 40);
const _: () = assert!(align_of::<XClientMessageData>() == 8);
const _: () = assert!(offset_of!(XClientMessageData, b) == 0);
const _: () = assert!(offset_of!(XClientMessageData, s) == 0);
const _: () = assert!(offset_of!(XClientMessageData, l) == 0);

/// A message from another client ([`ClientMessage`]). The window manager
/// sends one when the user asks to close the window.
/// `X11/Xlib.h`: `typedef struct { ... } XClientMessageEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XClientMessageEvent {
    /// The event type. `type` in C.
    pub type_: c_int,
    /// The number of the last request the server processed.
    pub serial: c_ulong,
    /// Whether another client sent this with `XSendEvent`.
    pub send_event: Bool,
    /// The connection the event was read from.
    pub display: *mut Display,
    /// The window the message is about.
    pub window: Window,
    /// What kind of message this is, such as `WM_PROTOCOLS`.
    pub message_type: Atom,
    /// The size of the items in `data`, in bits: 8, 16 or 32.
    pub format: c_int,
    /// The payload. For `WM_PROTOCOLS`, `l[0]` is the protocol, such as
    /// `WM_DELETE_WINDOW`.
    pub data: XClientMessageData,
}

const _: () = assert!(size_of::<XClientMessageEvent>() == 96);
const _: () = assert!(align_of::<XClientMessageEvent>() == 8);
const _: () = assert!(offset_of!(XClientMessageEvent, type_) == 0);
const _: () = assert!(offset_of!(XClientMessageEvent, serial) == 8);
const _: () = assert!(offset_of!(XClientMessageEvent, send_event) == 16);
const _: () = assert!(offset_of!(XClientMessageEvent, display) == 24);
const _: () = assert!(offset_of!(XClientMessageEvent, window) == 32);
const _: () = assert!(offset_of!(XClientMessageEvent, message_type) == 40);
const _: () = assert!(offset_of!(XClientMessageEvent, format) == 48);
const _: () = assert!(offset_of!(XClientMessageEvent, data) == 56);

/// Any event. `type_` says which member holds it.
///
/// The header's union has a member for every kind of event. This one has
/// only those we read, and `pad`, which gives it the header's size.
/// `XNextEvent` can therefore write any event into it.
/// `X11/Xlib.h`: `typedef union _XEvent { ... } XEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub union XEvent {
    /// The event type: [`KeyPress`] and so on. `type` in C. Every member
    /// starts with it.
    pub type_: c_int,
    /// The part every event has, such as its window.
    pub xany: XAnyEvent,
    /// For [`KeyPress`] and [`KeyRelease`].
    pub xkey: XKeyEvent,
    /// For [`ButtonPress`] and [`ButtonRelease`].
    pub xbutton: XButtonEvent,
    /// For [`MotionNotify`].
    pub xmotion: XMotionEvent,
    /// For [`FocusIn`] and [`FocusOut`].
    pub xfocus: XFocusChangeEvent,
    /// For [`Expose`].
    pub xexpose: XExposeEvent,
    /// For [`ConfigureNotify`].
    pub xconfigure: XConfigureEvent,
    /// For [`ClientMessage`].
    pub xclient: XClientMessageEvent,
    /// Fixes the size. `X11/Xlib.h`: `long pad[24];`
    pub pad: [c_long; 24],
}

const _: () = assert!(size_of::<XEvent>() == 192);
const _: () = assert!(align_of::<XEvent>() == 8);
const _: () = assert!(offset_of!(XEvent, type_) == 0);
const _: () = assert!(offset_of!(XEvent, xany) == 0);
const _: () = assert!(offset_of!(XEvent, xkey) == 0);
const _: () = assert!(offset_of!(XEvent, xbutton) == 0);
const _: () = assert!(offset_of!(XEvent, xmotion) == 0);
const _: () = assert!(offset_of!(XEvent, xfocus) == 0);
const _: () = assert!(offset_of!(XEvent, xexpose) == 0);
const _: () = assert!(offset_of!(XEvent, xconfigure) == 0);
const _: () = assert!(offset_of!(XEvent, xclient) == 0);
const _: () = assert!(offset_of!(XEvent, pad) == 0);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A request the server refused, as an error handler receives it.
/// `X11/Xlib.h`: `typedef struct { ... } XErrorEvent;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XErrorEvent {
    /// The event type, always 0 for an error. `type` in C.
    pub type_: c_int,
    /// The connection the error was read from.
    pub display: *mut Display,
    /// The resource the failed request named, such as a window.
    pub resourceid: XID,
    /// The number of the failed request, which `XNextRequest` would have
    /// given just before it was queued.
    pub serial: c_ulong,
    /// What went wrong: `BadWindow`, `BadMatch` and so on (`X11/X.h`).
    pub error_code: c_uchar,
    /// The major opcode of the failed request (`X11/Xproto.h`), such as 72
    /// for `PutImage`.
    pub request_code: c_uchar,
    /// The minor opcode of the failed request, for an extension's.
    pub minor_code: c_uchar,
}

const _: () = assert!(size_of::<XErrorEvent>() == 40);
const _: () = assert!(align_of::<XErrorEvent>() == 8);
const _: () = assert!(offset_of!(XErrorEvent, type_) == 0);
const _: () = assert!(offset_of!(XErrorEvent, display) == 8);
const _: () = assert!(offset_of!(XErrorEvent, resourceid) == 16);
const _: () = assert!(offset_of!(XErrorEvent, serial) == 24);
const _: () = assert!(offset_of!(XErrorEvent, error_code) == 32);
const _: () = assert!(offset_of!(XErrorEvent, request_code) == 33);
const _: () = assert!(offset_of!(XErrorEvent, minor_code) == 34);

/// A function that Xlib calls for each X error, for every display of the
/// process, in place of the default handler, which prints the error and
/// ends the process. Its result is ignored. `None` restores the default.
///
/// Xlib calls it from inside whichever Xlib call read the error, on that
/// thread. It must not call an Xlib function that makes a request or reads
/// events on that display, as the Xlib manual says ("Using the Default
/// Error Handlers"): one that waits for a reply can block for ever there.
/// A panic that reaches it ends the process, since it is `extern "C"`.
/// `X11/Xlib.h`: `typedef int (*XErrorHandler) (Display*, XErrorEvent*);`
pub type XErrorHandler =
    Option<unsafe extern "C" fn(display: *mut Display, error_event: *mut XErrorEvent) -> c_int>;

/// A function that Xlib calls when the connection to the server fails,
/// such as when the server exits, in place of the default handler, which
/// prints a message. Either way, the process exits once it returns: it can
/// record what happened, not recover. The connection is gone, so a call
/// on that display can do nothing useful. `None` restores the default.
/// `X11/Xlib.h`: `typedef int (*XIOErrorHandler) (Display*);`
pub type XIOErrorHandler = Option<unsafe extern "C" fn(display: *mut Display) -> c_int>;

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

/// The `format` of an image whose pixels are whole values, one after
/// another, each the depth of the drawable: the format of a frame.
/// `X11/X.h`: `#define ZPixmap 2`
pub const ZPixmap: c_int = 2;

/// An [`XImage::byte_order`]: the least significant byte of a pixel value
/// comes first, as in a frame of 32-bit values on x86_64 and aarch64.
/// `X11/X.h`: `#define LSBFirst 0`
pub const LSBFirst: c_int = 0;

/// The `plane_mask` of `XGetImage` that reads every plane.
/// `X11/Xlib.h`: `#define AllPlanes ((unsigned long)~0L)`
pub const AllPlanes: c_ulong = !0;

/// The functions that work on an [`XImage`]: what `XInitImage` (or
/// `XGetImage`) puts in its `f`, chosen for the image's other fields. Each
/// is null until then. Xlib calls them without checking for null: an
/// `XPutImage` that converts pixels calls `get_pixel`. The header names
/// this struct `funcs`, inside `XImage`.
/// `X11/Xlib.h`: `struct funcs { ... } f;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XImageFuncs {
    /// Makes another image like `XCreateImage`. `struct _XImage
    /// *(*create_image)(struct _XDisplay*, Visual*, unsigned int, int, int,
    /// char*, unsigned int, unsigned int, int, int);`
    pub create_image: Option<
        unsafe extern "C" fn(
            display: *mut Display,
            visual: *mut Visual,
            depth: c_uint,
            format: c_int,
            offset: c_int,
            data: *mut c_char,
            width: c_uint,
            height: c_uint,
            bitmap_pad: c_int,
            bytes_per_line: c_int,
        ) -> *mut XImage,
    >,
    /// Frees the image's `data`, its `obdata` and the image itself, with
    /// the C library's `free`. This is what the C macro `XDestroyImage`
    /// calls, and how an image from `XGetImage` is freed. Never call it on
    /// an image whose memory is not the C library's, such as an `XImage` of
    /// ours that `XInitImage` set up over a frame.
    /// `int (*destroy_image) (struct _XImage *);`
    pub destroy_image: Option<unsafe extern "C" fn(image: *mut XImage) -> c_int>,
    /// Returns the pixel value at (`x`, `y`): the C macro `XGetPixel`.
    /// `unsigned long (*get_pixel) (struct _XImage *, int, int);`
    pub get_pixel: Option<unsafe extern "C" fn(image: *mut XImage, x: c_int, y: c_int) -> c_ulong>,
    /// Sets the pixel value at (`x`, `y`): the C macro `XPutPixel`.
    /// `int (*put_pixel) (struct _XImage *, int, int, unsigned long);`
    pub put_pixel: Option<
        unsafe extern "C" fn(image: *mut XImage, x: c_int, y: c_int, pixel: c_ulong) -> c_int,
    >,
    /// Copies a rectangle into a new image: the C macro `XSubImage`.
    /// `struct _XImage *(*sub_image) (struct _XImage *, int, int, unsigned
    /// int, unsigned int);`
    pub sub_image: Option<
        unsafe extern "C" fn(
            image: *mut XImage,
            x: c_int,
            y: c_int,
            width: c_uint,
            height: c_uint,
        ) -> *mut XImage,
    >,
    /// Adds a constant to every pixel value: the C macro `XAddPixel`.
    /// `int (*add_pixel) (struct _XImage *, long);`
    pub add_pixel: Option<unsafe extern "C" fn(image: *mut XImage, value: c_long) -> c_int>,
}

const _: () = assert!(size_of::<XImageFuncs>() == 48);
const _: () = assert!(align_of::<XImageFuncs>() == 8);
const _: () = assert!(offset_of!(XImageFuncs, create_image) == 0);
const _: () = assert!(offset_of!(XImageFuncs, destroy_image) == 8);
const _: () = assert!(offset_of!(XImageFuncs, get_pixel) == 16);
const _: () = assert!(offset_of!(XImageFuncs, put_pixel) == 24);
const _: () = assert!(offset_of!(XImageFuncs, sub_image) == 32);
const _: () = assert!(offset_of!(XImageFuncs, add_pixel) == 40);

/// An image in the client's memory, which `XPutImage` sends to a drawable
/// and `XGetImage` reads one into.
///
/// Who owns it depends on where it came from. One that we fill in over a
/// frame and pass to `XInitImage` is ours, and so is its `data`: nothing
/// in Xlib frees either. One that `XGetImage` returns is Xlib's, with its
/// `data`, and is freed through its own [`XImageFuncs::destroy_image`].
/// `X11/Xlib.h`: `typedef struct _XImage { ... } XImage;`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XImage {
    /// The width, in pixels.
    pub width: c_int,
    /// The height, in pixels.
    pub height: c_int,
    /// The pixels to skip at the start of each row.
    pub xoffset: c_int,
    /// How the pixels are laid out: [`ZPixmap`] and others.
    pub format: c_int,
    /// The pixels, row after row from the top.
    pub data: *mut c_char,
    /// The order of the bytes in a pixel value: [`LSBFirst`] or
    /// `MSBFirst`. `XPutImage` converts to the server's order.
    pub byte_order: c_int,
    /// The unit a row is made of, in bits (8, 16 or 32); for bitmaps.
    pub bitmap_unit: c_int,
    /// The order of the bits in a unit, for bitmaps: [`LSBFirst`] or
    /// `MSBFirst`.
    pub bitmap_bit_order: c_int,
    /// What each row is padded to a multiple of, in bits: 8, 16 or 32.
    pub bitmap_pad: c_int,
    /// The depth of the image: 24 for a frame in a 24-bit visual.
    pub depth: c_int,
    /// The bytes from the start of one row to the start of the next.
    /// `XInitImage` works it out if it is 0.
    pub bytes_per_line: c_int,
    /// The bits each pixel takes, for [`ZPixmap`]: 32 for a frame.
    pub bits_per_pixel: c_int,
    /// The bits of a pixel value that hold red.
    pub red_mask: c_ulong,
    /// The bits that hold green.
    pub green_mask: c_ulong,
    /// The bits that hold blue.
    pub blue_mask: c_ulong,
    /// Data that Xlib's image functions hang on the image. Null in an image
    /// of ours.
    pub obdata: XPointer,
    /// The functions that work on the image, which `XInitImage` fills in
    /// for the other fields as they are then. Xlib calls them without
    /// checking for null, so an image given to it must come from
    /// `XGetImage`, or from an `XInitImage` that returned nonzero after its
    /// other fields were last set.
    pub f: XImageFuncs,
}

const _: () = assert!(size_of::<XImage>() == 136);
const _: () = assert!(align_of::<XImage>() == 8);
const _: () = assert!(offset_of!(XImage, width) == 0);
const _: () = assert!(offset_of!(XImage, height) == 4);
const _: () = assert!(offset_of!(XImage, xoffset) == 8);
const _: () = assert!(offset_of!(XImage, format) == 12);
const _: () = assert!(offset_of!(XImage, data) == 16);
const _: () = assert!(offset_of!(XImage, byte_order) == 24);
const _: () = assert!(offset_of!(XImage, bitmap_unit) == 28);
const _: () = assert!(offset_of!(XImage, bitmap_bit_order) == 32);
const _: () = assert!(offset_of!(XImage, bitmap_pad) == 36);
const _: () = assert!(offset_of!(XImage, depth) == 40);
const _: () = assert!(offset_of!(XImage, bytes_per_line) == 44);
const _: () = assert!(offset_of!(XImage, bits_per_pixel) == 48);
const _: () = assert!(offset_of!(XImage, red_mask) == 56);
const _: () = assert!(offset_of!(XImage, green_mask) == 64);
const _: () = assert!(offset_of!(XImage, blue_mask) == 72);
const _: () = assert!(offset_of!(XImage, obdata) == 80);
const _: () = assert!(offset_of!(XImage, f) == 88);

// ---------------------------------------------------------------------------
// Cursors
// ---------------------------------------------------------------------------

/// The `shape` of `XCreateFontCursor`: the arrow, for most of the window.
/// `X11/cursorfont.h`: `#define XC_left_ptr 68`
pub const XC_left_ptr: c_uint = 68;

/// The `shape` of `XCreateFontCursor`: the text beam, over editable text.
/// `X11/cursorfont.h`: `#define XC_xterm 152`
pub const XC_xterm: c_uint = 152;

/// The `shape` of `XCreateFontCursor`: the pointing hand, over a link or
/// a button. `X11/cursorfont.h`: `#define XC_hand2 60`
pub const XC_hand2: c_uint = 60;

/// The `shape` of `XCreateFontCursor`: the left-right arrow, over a
/// splitter between columns.
/// `X11/cursorfont.h`: `#define XC_sb_h_double_arrow 108`
pub const XC_sb_h_double_arrow: c_uint = 108;

/// The `shape` of `XCreateFontCursor`: the up-down arrow, over a splitter
/// between rows. `X11/cursorfont.h`: `#define XC_sb_v_double_arrow 116`
pub const XC_sb_v_double_arrow: c_uint = 116;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

dynlib! {
    /// The functions we call in `libX11.so.6`.
    ///
    /// The module documentation lists what every one of them requires.
    pub struct Xlib: unsafe extern "C" {
        // The connection.

        /// Connects to the X server that `display_name` names, or the one in
        /// the `DISPLAY` environment variable if it is null. Returns null on
        /// failure.
        ///
        /// It connects a socket to the server: a Unix socket for `:N`, or
        /// TCP when the name has a host, and also for `:N` when no Unix
        /// socket answers (libxcb's fallback; `unix:N` never uses TCP). It
        /// sends the Xauthority cookie for that display if the authority
        /// file has one.
        /// `X11/Xlib.h`: `extern Display *XOpenDisplay(_Xconst char*
        /// display_name)`
        ///
        /// The connection is ours. Close it with `XCloseDisplay`.
        fn XOpenDisplay(display_name: *const c_char) -> *mut Display;

        /// Closes the connection. The server then destroys every window,
        /// graphics context, cursor and colormap that was made through it.
        /// `display` must not be used again.
        /// `X11/Xlib.h`: `extern int XCloseDisplay(Display* display)`
        fn XCloseDisplay(display: *mut Display) -> c_int;

        /// Returns the file descriptor of the connection, which becomes
        /// readable when the server sends something. It belongs to Xlib:
        /// poll it, but never read, write or close it.
        /// `X11/Xlib.h`: `extern int XConnectionNumber(Display* display)`
        fn XConnectionNumber(display: *mut Display) -> c_int;

        /// Sends every queued request to the server. It waits for no reply
        /// or event, but it can block while the connection's socket is
        /// full.
        /// `X11/Xlib.h`: `extern int XFlush(Display* display)`
        fn XFlush(display: *mut Display) -> c_int;

        /// Sends every queued request and waits until the server has
        /// processed them all, so that the error handler has been called
        /// for any of them it refused. With `discard` [`True`] it also
        /// drops every event in the queue.
        /// `X11/Xlib.h`: `extern int XSync(Display* display, Bool discard)`
        fn XSync(display: *mut Display, discard: Bool) -> c_int;

        /// Returns the number of the screen that `XOpenDisplay` chose.
        /// `X11/Xlib.h`: `extern int XDefaultScreen(Display* display)`
        fn XDefaultScreen(display: *mut Display) -> c_int;

        /// Returns the root window of a screen. The window belongs to the
        /// server.
        /// `X11/Xlib.h`: `extern Window XRootWindow(Display* display, int
        /// screen_number)`
        fn XRootWindow(display: *mut Display, screen_number: c_int) -> Window;

        // Visuals and colormaps.

        /// Finds a visual of `depth` and `class` (such as 24 and
        /// [`TrueColor`]) on a screen, and writes it to `vinfo_return`.
        /// Returns zero if the screen has none.
        /// `X11/Xutil.h`: `extern Status XMatchVisualInfo(Display* display,
        /// int screen, int depth, int class, XVisualInfo* vinfo_return)`
        ///
        /// Nothing is allocated. The visual belongs to the display, and
        /// lives until it is closed.
        fn XMatchVisualInfo(
            display: *mut Display,
            screen: c_int,
            depth: c_int,
            class: c_int,
            vinfo_return: *mut XVisualInfo,
        ) -> Status;

        /// Creates a colormap of `visual` on the screen of the window `w`.
        /// `X11/Xlib.h`: `extern Colormap XCreateColormap(Display* display,
        /// Window w, Visual* visual, int alloc)`
        ///
        /// The colormap is ours. Free it with `XFreeColormap`.
        fn XCreateColormap(
            display: *mut Display,
            w: Window,
            visual: *mut Visual,
            alloc: c_int,
        ) -> Colormap;

        /// Frees a colormap. A window that still uses it has no colormap
        /// after this.
        /// `X11/Xlib.h`: `extern int XFreeColormap(Display* display,
        /// Colormap colormap)`
        fn XFreeColormap(display: *mut Display, colormap: Colormap) -> c_int;

        // Windows.

        /// Creates a window, which is not shown until it is mapped, and
        /// returns its id. `attributes` may be null if `valuemask` is 0.
        /// `X11/Xlib.h`: `extern Window XCreateWindow(Display* display,
        /// Window parent, int x, int y, unsigned int width, unsigned int
        /// height, unsigned int border_width, int depth, unsigned int
        /// class, Visual* visual, unsigned long valuemask,
        /// XSetWindowAttributes* attributes)`
        ///
        /// The window is ours. Destroy it with `XDestroyWindow`.
        fn XCreateWindow(
            display: *mut Display,
            parent: Window,
            x: c_int,
            y: c_int,
            width: c_uint,
            height: c_uint,
            border_width: c_uint,
            depth: c_int,
            class: c_uint,
            visual: *mut Visual,
            valuemask: c_ulong,
            attributes: *mut XSetWindowAttributes,
        ) -> Window;

        /// Destroys a window and all its children.
        /// `X11/Xlib.h`: `extern int XDestroyWindow(Display* display,
        /// Window w)`
        fn XDestroyWindow(display: *mut Display, w: Window) -> c_int;

        /// Shows a window.
        /// `X11/Xlib.h`: `extern int XMapWindow(Display* display, Window w)`
        fn XMapWindow(display: *mut Display, w: Window) -> c_int;

        /// Chooses which events the server reports for a window:
        /// [`KeyPressMask`] and so on.
        /// `X11/Xlib.h`: `extern int XSelectInput(Display* display, Window
        /// w, long event_mask)`
        fn XSelectInput(display: *mut Display, w: Window, event_mask: c_long) -> c_int;

        // Atoms and properties.

        /// Returns the atom for a name. If the server has none: makes one
        /// when `only_if_exists` is [`False`], and returns 0 when it is
        /// [`True`]. `atom_name` is a NUL-terminated string.
        /// `X11/Xlib.h`: `extern Atom XInternAtom(Display* display,
        /// _Xconst char* atom_name, Bool only_if_exists)`
        fn XInternAtom(
            display: *mut Display,
            atom_name: *const c_char,
            only_if_exists: Bool,
        ) -> Atom;

        /// Sets the `WM_PROTOCOLS` property of a window to `count` atoms at
        /// `protocols`: the messages the window manager may send it, such
        /// as `WM_DELETE_WINDOW`. Returns zero if it could not get the
        /// `WM_PROTOCOLS` atom.
        /// `X11/Xlib.h`: `extern Status XSetWMProtocols(Display* display,
        /// Window w, Atom* protocols, int count)`
        fn XSetWMProtocols(
            display: *mut Display,
            w: Window,
            protocols: *mut Atom,
            count: c_int,
        ) -> Status;

        /// Sets a property of a window to `nelements` items at `data`.
        /// `format` is the size of an item in bits: 8, 16 or 32.
        ///
        /// With `format` 32, `data` must point to an array of C `long`,
        /// which is 8 bytes an item on 64-bit Linux. Xlib sends the low 32
        /// bits of each.
        /// `X11/Xlib.h`: `extern int XChangeProperty(Display* display,
        /// Window w, Atom property, Atom type, int format, int mode,
        /// _Xconst unsigned char* data, int nelements)`
        fn XChangeProperty(
            display: *mut Display,
            w: Window,
            property: Atom,
            type_: Atom,
            format: c_int,
            mode: c_int,
            data: *const c_uchar,
            nelements: c_int,
        ) -> c_int;

        // Events.

        /// Returns how many events are waiting. If none are waiting, it
        /// first sends every queued request to the server and reads what
        /// has arrived. It waits for no event, but sending can block while
        /// the connection's socket is full.
        /// `X11/Xlib.h`: `extern int XPending(Display* display)`
        fn XPending(display: *mut Display) -> c_int;

        /// Removes the next event from the queue and writes it to
        /// `event_return`. If the queue is empty, it sends every queued
        /// request and blocks until an event arrives.
        /// `X11/Xlib.h`: `extern int XNextEvent(Display* display, XEvent*
        /// event_return)`
        fn XNextEvent(display: *mut Display, event_return: *mut XEvent) -> c_int;

        /// Turns detectable auto-repeat on or off for this client. When it
        /// is on, a key that is held down produces repeated [`KeyPress`]
        /// events with no [`KeyRelease`] between them, so a release event
        /// always means the key went up.
        ///
        /// Returns whether detectable auto-repeat is on after the call:
        /// [`False`] may mean that the server has no XKB, or that the
        /// request failed. If `supported` is not null, it receives whether
        /// the server can do this, but only when the server answers, that
        /// is, when it has XKB and the request succeeded; otherwise it is
        /// left as it was, so set it (to [`False`]) before the call.
        /// `X11/XKBlib.h`: `extern Bool XkbSetDetectableAutoRepeat(Display
        /// * dpy, Bool detectable, Bool * supported)`
        fn XkbSetDetectableAutoRepeat(
            display: *mut Display,
            detectable: Bool,
            supported: *mut Bool,
        ) -> Bool;

        // Errors.

        /// Sets the function that is called for every X error, for every
        /// display of the process, and returns the one it replaces. See
        /// [`XErrorHandler`] for what the function may do.
        /// `X11/Xlib.h`: `extern XErrorHandler XSetErrorHandler
        /// (XErrorHandler handler)`
        fn XSetErrorHandler(handler: XErrorHandler) -> XErrorHandler;

        /// Sets the function that is called when the connection to a server
        /// fails, after which the process exits, and returns the one it
        /// replaces. See [`XIOErrorHandler`].
        /// `X11/Xlib.h`: `extern XIOErrorHandler XSetIOErrorHandler
        /// (XIOErrorHandler handler)`
        fn XSetIOErrorHandler(handler: XIOErrorHandler) -> XIOErrorHandler;

        // Drawing.

        /// Creates a graphics context for drawing to drawables of the depth
        /// and screen of `d`. `values` may be null if `valuemask` is 0,
        /// which gives every value its default. Returns null on failure.
        /// `X11/Xlib.h`: `extern GC XCreateGC(Display* display, Drawable d,
        /// unsigned long valuemask, XGCValues* values)`
        ///
        /// The graphics context is ours. Free it with `XFreeGC`.
        fn XCreateGC(
            display: *mut Display,
            d: Drawable,
            valuemask: c_ulong,
            values: *mut XGCValues,
        ) -> GC;

        /// Frees a graphics context.
        /// `X11/Xlib.h`: `extern int XFreeGC(Display* display, GC gc)`
        fn XFreeGC(display: *mut Display, gc: GC) -> c_int;

        /// Fills in the `f` of an image that the caller has filled in
        /// otherwise, and works out `bytes_per_line` if it is 0. Returns
        /// zero if the fields describe no image Xlib can handle, such as a
        /// depth over 32 or a `bitmap_pad` other than 8, 16 or 32.
        /// `X11/Xlib.h`: `extern Status XInitImage(XImage* image)`
        ///
        /// Nothing is allocated: the image and its `data` stay the
        /// caller's.
        fn XInitImage(image: *mut XImage) -> Status;

        /// Queues a request that draws the `width` by `height` rectangle of
        /// `image` at (`src_x`, `src_y`) into the drawable `d` at
        /// (`dest_x`, `dest_y`). It waits for no reply of its own, but it
        /// can wait as the module documentation says. Before it returns,
        /// Xlib has copied the pixels into its buffer or, for a large
        /// image, written them to the socket, so the image may change
        /// after that.
        ///
        /// `image` must come from `XGetImage`, or from an `XInitImage` that
        /// returned nonzero after its other fields were last set. Its
        /// `format` is [`ZPixmap`], the only one declared here, and its
        /// `data` holds `bytes_per_line` × the image's own `height` bytes
        /// (an `XYPixmap` would hold `depth` times that). Where the
        /// server stores pixels of the image's depth at another
        /// `bits_per_pixel`, Xlib converts them one by one through the
        /// image's `f.get_pixel`, which it calls without checking for null.
        /// `X11/Xlib.h`: `extern int XPutImage(Display* display, Drawable d,
        /// GC gc, XImage* image, int src_x, int src_y, int dest_x, int
        /// dest_y, unsigned int width, unsigned int height)`
        fn XPutImage(
            display: *mut Display,
            d: Drawable,
            gc: GC,
            image: *mut XImage,
            src_x: c_int,
            src_y: c_int,
            dest_x: c_int,
            dest_y: c_int,
            width: c_uint,
            height: c_uint,
        ) -> c_int;

        /// Reads the `width` by `height` rectangle of the drawable `d` at
        /// (`x`, `y`) into a new image, in `format` ([`ZPixmap`]), keeping
        /// the planes of `plane_mask` ([`AllPlanes`]). It waits for the
        /// server. Returns null on failure. A window must be viewable (it
        /// and its ancestors mapped), and the rectangle inside it and on
        /// the screen, or the server refuses the request with `BadMatch`.
        /// `X11/Xlib.h`: `extern XImage *XGetImage(Display* display,
        /// Drawable d, int x, int y, unsigned int width, unsigned int
        /// height, unsigned long plane_mask, int format)`
        ///
        /// The image and its `data` are Xlib's to free: free them through
        /// the image's [`XImageFuncs::destroy_image`].
        fn XGetImage(
            display: *mut Display,
            d: Drawable,
            x: c_int,
            y: c_int,
            width: c_uint,
            height: c_uint,
            plane_mask: c_ulong,
            format: c_int,
        ) -> *mut XImage;

        // Cursors.

        /// Creates a pointer shape from the standard cursor font: `shape`
        /// is [`XC_left_ptr`] or another of `X11/cursorfont.h`.
        /// `X11/Xlib.h`: `extern Cursor XCreateFontCursor(Display* display,
        /// unsigned int shape)`
        ///
        /// The cursor is ours. Free it with `XFreeCursor`.
        fn XCreateFontCursor(display: *mut Display, shape: c_uint) -> Cursor;

        /// Sets the pointer shape that shows while the pointer is over a
        /// window.
        /// `X11/Xlib.h`: `extern int XDefineCursor(Display* display, Window
        /// w, Cursor cursor)`
        fn XDefineCursor(display: *mut Display, w: Window, cursor: Cursor) -> c_int;

        /// Frees a cursor. The server keeps the shape for as long as a
        /// window still uses it.
        /// `X11/Xlib.h`: `extern int XFreeCursor(Display* display, Cursor
        /// cursor)`
        fn XFreeCursor(display: *mut Display, cursor: Cursor) -> c_int;

        // Resources.

        /// Returns the `RESOURCE_MANAGER` property of screen 0's root
        /// window, whatever the default screen is, as it was when the
        /// display was opened: the user's X resources, such as
        /// `Xft.dpi`, as NUL-terminated text. Returns null if there is none.
        /// `X11/Xlib.h`: `extern char *XResourceManagerString(Display*
        /// display)`
        ///
        /// The string belongs to Xlib: do not free it. It is valid until
        /// the display is closed.
        fn XResourceManagerString(display: *mut Display) -> *mut c_char;
    }
}

#[cfg(test)]
mod tests {
    use core::ffi::{CStr, c_void};
    use std::io::{self, Write};

    use super::*;
    use crate::dynlib::Library;
    use crate::linux::libc::{RTLD_LOCAL, RTLD_NOW, dlerror, dlopen, dlsym};

    /// A library opened with `dlopen`.
    struct Opened(*mut c_void);

    impl Library for Opened {
        fn symbol(&self, name: &CStr) -> *const c_void {
            // SAFETY: the handle came from `dlopen` and is never closed, and
            // the name is a C string.
            unsafe { dlsym(self.0, name.as_ptr()) }
        }
    }

    #[test]
    fn the_installed_xlib_has_every_function_we_declare() {
        // SAFETY: the name is a C string, and Xlib is sound to load into any
        // process: loading it opens no display.
        let handle = unsafe { dlopen(c"libX11.so.6".as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        if handle.is_null() {
            let reason = dlerror();
            let reason = if reason.is_null() {
                "no reason given".into()
            } else {
                // SAFETY: `dlerror` returned a NUL-terminated string that
                // stays valid until this thread calls a `dl*` function
                // again, which it does not do while `reason` is in use.
                unsafe { CStr::from_ptr(reason) }.to_string_lossy()
            };
            // This machine has no Xlib, so there is nothing to compare
            // with. Say so as the gate counts it (docs/04-quality.md).
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: the_installed_xlib_has_every_function_we_declare: \
                 libX11.so.6 cannot be loaded: {reason}"
            );
            return;
        }
        // SAFETY: the library is Xlib, which defines these functions as its
        // headers declare them, and it is never unloaded.
        let table = unsafe { Xlib::load(&Opened(handle)) };
        if let Err(missing) = table {
            panic!("libX11.so.6: {missing}");
        }
    }
}
