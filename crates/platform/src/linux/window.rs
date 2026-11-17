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

//! The X11 window, its events and its presentation, which
//! [`crate::window::Window`] wraps.
//!
//! Ported in part from nadir-platform `linux/x11.rs`: the order of the
//! opening, the atoms, the title in `_NET_WM_NAME` and `WM_NAME` (and
//! [`legacy_title_type`]), the drain of events through the input method,
//! and the tests of the title's type and the event mask. Adapted for
//! Tidepool: one connection and one claim of the Xlib thread for each
//! window; the window draws by `XPutImage` from a [`Pixels`] view, not
//! OpenGL; it says its class, process and machine (`WM_CLASS`,
//! `_NET_WM_PID`, `WM_CLIENT_MACHINE`) and its pointer shape; its events
//! are waited for with `poll` on the X connection and a self-pipe
//! ([`Window::wait_events`]); and Nadir's size hints, fullscreen and
//! hidden pointer are left out.
//!
//! What this backend does not do yet, because nothing needs it:
//!
//! - It reads the scale once, when the window opens, from the `Xft.dpi`
//!   resource. A change of the desktop's scale while Tidepool runs goes
//!   unnoticed. Under XWayland the compositor decides what it tells X
//!   clients, so the value can differ from the compositor's own scale.
//! - It shows no icon (`_NET_WM_ICON`): Tidepool has none yet.
//! - It shows nothing of a text being composed (an input method's
//!   preedit); dead keys and Compose sequences type when they are done.

use core::ffi::{c_int, c_long, c_uint};
use std::ffi::CString;
use std::io;
use std::os::fd::AsRawFd as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tidepool_core::geom::{DeviceRect, DeviceSize, Scale, Size};
use tidepool_core::log::{debug, warn};
use tidepool_sys::linux::libc::{POLLERR, POLLHUP, POLLIN, POLLNVAL, nfds_t, poll, pollfd};
use tidepool_sys::linux::x11::{
    self as xlib, Atom, ButtonPressMask, ButtonReleaseMask, Colormap, Cursor, ExposureMask,
    FocusChangeMask, KeyPressMask, KeyReleaseMask, PointerMotionMask, PropertyChangeMask,
    StructureNotifyMask, XA_CARDINAL, XA_STRING, XA_WM_CLASS, XA_WM_CLIENT_MACHINE, XA_WM_NAME,
};

use super::claim::{self, Claim};
use super::connection::Connection;
use super::input::{self, CloseMessage, Incoming, Translator};
use super::selection::Clipboard;
use super::waker::{SelfPipe, Waker};
use super::{display, library, xresources};
use crate::Error;
use crate::clipboard::Selection;
use crate::events::Event;
use crate::surface::{self, Pixels};
use crate::window::{CursorShape, WindowConfig};

/// The events we ask the server for, besides those the input method needs.
/// The selection events come whatever the mask; the clipboard needs the
/// property changes.
const EVENT_MASK: c_long = KeyPressMask
    | KeyReleaseMask
    | ButtonPressMask
    | ButtonReleaseMask
    | PointerMotionMask
    | ExposureMask
    | StructureNotifyMask
    | FocusChangeMask
    | PropertyChangeMask;

/// The longest one turn of taking events lasts, so that a flood of events
/// that another client causes holds a wait past its deadline, or keeps a
/// wake unseen, for no longer than this.
const DRAIN_BUDGET: Duration = Duration::from_millis(10);

/// The widest and tallest window we ask for. The X protocol carries a size
/// in 16 bits, and servers stop at half of that.
const MAX_DIMENSION: i32 = 32767;

/// The window's `WM_CLASS`: its instance name and its class name, each
/// ending with a NUL, as ICCCM 4.1.2.5 has them. Window managers and
/// desktop files match windows by them.
const CLASS: &[u8] = b"tidepool\0Tidepool\0";

/// Where Linux keeps the machine's host name, which `WM_CLIENT_MACHINE`
/// holds: the name `gethostname` returns.
const HOST_NAME: &str = "/proc/sys/kernel/hostname";

/// The atoms we use, looked up once for each window.
struct Atoms {
    wm_protocols: Atom,
    wm_delete_window: Atom,
    net_wm_name: Atom,
    net_wm_pid: Atom,
    utf8_string: Atom,
}

impl Atoms {
    fn intern(connection: &Connection) -> Self {
        Self {
            wm_protocols: connection.intern(c"WM_PROTOCOLS"),
            wm_delete_window: connection.intern(c"WM_DELETE_WINDOW"),
            net_wm_name: connection.intern(c"_NET_WM_NAME"),
            net_wm_pid: connection.intern(c"_NET_WM_PID"),
            utf8_string: connection.intern(c"UTF8_STRING"),
        }
    }
}

/// An X11 window, with a connection of its own.
pub(crate) struct Window {
    // The connection is declared first, but closes last: `Drop` frees the
    // window's resources through it before the fields are dropped.
    connection: Connection,
    window: xlib::Window,
    /// The colormap of the window's visual.
    colormap: Colormap,
    atoms: Atoms,
    translator: Translator,
    clipboard: Clipboard,
    /// The cursor of each shape, by [`CursorShape::index`], all made in
    /// [`Window::open`] (0 until then), so that no later call creates one:
    /// creating one is where libXcursor reads files.
    cursors: [Cursor; CursorShape::ALL.len()],
    /// The shape shown now.
    cursor: Option<CursorShape>,
    /// The self-pipe that the window's wakers write to.
    pipe: Arc<SelfPipe>,
    /// Where typed text is looked up.
    lookup: Vec<u8>,
    /// Events read outside a wait, which the next wait reports first.
    backlog: Vec<Event>,
}

impl Window {
    /// Opens a window, as [`crate::window::Window::open`] says.
    pub(crate) fn open(config: &WindowConfig) -> Result<Self, Error> {
        // Every Xlib call from here on is made with the claim, loading
        // Xlib included.
        let claim = Claim::take()?;
        let xlib = library::xlib()?;
        claim::install_error_handler(xlib, &claim);
        // Those of other windows: this one's are told apart below.
        report_errors();

        let value = std::env::var_os("DISPLAY");
        let refused = |reason| Error::Display {
            value: value.clone(),
            reason,
        };
        let name = display::display_name(value.as_deref()).map_err(refused)?;
        // An accepted name is digits, a dot and `unix:`, so it has no NUL.
        let c_name = CString::new(name.as_str()).map_err(|_| refused("holds a NUL"))?;
        let pipe = SelfPipe::new()?;
        let connection = Connection::open(claim, xlib, &c_name)
            .ok_or(Error::ConnectionFailed { display: name })?;

        if !connection.set_detectable_auto_repeat() {
            warn!(
                "the X server has no detectable auto-repeat: a held key will report \
                 releases and presses in place of repeats"
            );
        }
        let scale = xresources::scale(connection.resources().as_deref());
        let size = device_size(config.size, scale);
        let (window, colormap) = connection.create_window(size)?;
        let atoms = Atoms::intern(&connection);
        let close = CloseMessage {
            wm_protocols: atoms.wm_protocols,
            wm_delete_window: atoms.wm_delete_window,
        };
        let clipboard = Clipboard::new(&connection, window);
        // From here on, dropping `backend` destroys the window.
        let mut backend = Self {
            connection,
            window,
            colormap,
            atoms,
            translator: Translator::new(close, size, scale),
            clipboard,
            cursors: [0; CursorShape::ALL.len()],
            cursor: None,
            pipe,
            lookup: Vec::new(),
            backlog: Vec::new(),
        };
        if !backend.connection.create_gc(window) {
            return Err(Error::CallFailed { call: "XCreateGC" });
        }
        let typing = backend.connection.open_text_input(window);
        backend.connection.select_input(window, EVENT_MASK | typing);
        // Every cursor now, while the window opens: where libXcursor is
        // installed, creating a cursor makes it look for the theme's files,
        // at paths any client of the display can choose, and that must not
        // happen once this window is running; opening another window
        // does it again (`docs/03-platforms.md` → Input → Cursors). Each
        // is kept in `backend`, whose `Drop` frees it, before anything
        // below can fail.
        for shape in CursorShape::ALL {
            let cursor = backend.connection.create_font_cursor(font_cursor(shape));
            if let Some(slot) = backend.cursors.get_mut(shape.index()) {
                *slot = cursor;
            }
        }
        let mut protocols = [backend.atoms.wm_delete_window];
        if !backend.connection.set_protocols(window, &mut protocols) {
            return Err(Error::CallFailed {
                call: "XSetWMProtocols",
            });
        }
        backend.set_title(&config.title);
        backend.set_identity();
        backend.set_cursor(CursorShape::Arrow);
        backend.connection.map_window(window);
        // Wait for the server, so that a request it refused fails the
        // opening here rather than showing in the log later.
        backend.connection.sync();
        if let Some(errors) = claim::take_errors() {
            return Err(Error::Refused {
                error_code: errors.error_code,
                request_code: errors.request_code,
                minor_code: errors.minor_code,
            });
        }
        debug!(
            "opened an X11 window of {}x{} device pixels at scale {} on {}",
            size.width,
            size.height,
            scale.factor(),
            value
                .as_deref()
                .map(|value| format!("{value:?}"))
                .unwrap_or_default()
        );
        Ok(backend)
    }

    /// Sets the title, in `_NET_WM_NAME`, which current window managers
    /// read and is always UTF-8, and in `WM_NAME`, which older ones read.
    pub(crate) fn set_title(&mut self, title: &str) {
        let bytes = title.as_bytes();
        let modern = self.connection.set_bytes(
            self.window,
            self.atoms.net_wm_name,
            self.atoms.utf8_string,
            bytes,
        );
        let legacy = self.connection.set_bytes(
            self.window,
            XA_WM_NAME,
            legacy_title_type(title, self.atoms.utf8_string),
            bytes,
        );
        if !(modern && legacy) {
            warn!("the window title is too long to set");
        }
    }

    /// Says what the window is, for the window manager: its class, and its
    /// process with the machine it runs on. EWMH asks for
    /// `WM_CLIENT_MACHINE` with `_NET_WM_PID`, so neither is set if the
    /// host name cannot be read.
    fn set_identity(&self) {
        self.connection
            .set_bytes(self.window, XA_WM_CLASS, XA_STRING, CLASS);
        let Some(host) = host_name() else {
            debug!("{HOST_NAME} could not be read: the window does not say its process");
            return;
        };
        self.connection
            .set_bytes(self.window, XA_WM_CLIENT_MACHINE, XA_STRING, &host);
        let pid = c_long::from(std::process::id());
        self.connection
            .set_longs(self.window, self.atoms.net_wm_pid, XA_CARDINAL, &[pid]);
    }

    pub(crate) fn size(&self) -> DeviceSize {
        self.translator.size()
    }

    pub(crate) fn scale(&self) -> Scale {
        self.translator.scale()
    }

    pub(crate) fn is_focused(&self) -> bool {
        self.translator.is_focused()
    }

    /// Shows the pointer shape over the window, with the cursor `open`
    /// made for it. It creates none, so it reads no file: one
    /// `XDefineCursor` request.
    pub(crate) fn set_cursor(&mut self, shape: CursorShape) {
        if self.cursor == Some(shape) {
            return;
        }
        let Some(&cursor) = self.cursors.get(shape.index()) else {
            return;
        };
        self.connection.define_cursor(self.window, cursor);
        self.cursor = Some(shape);
    }

    pub(crate) fn waker(&self) -> Waker {
        Waker::new(Arc::clone(&self.pipe))
    }

    /// Makes `text` the selection's, as
    /// [`crate::window::Window::set_selection`] says.
    ///
    /// It waits for the server, so that the `PropertyNotify` with the time
    /// to take the selection at is read, and handles the events read with
    /// it, which the next wait reports: in turns, until the time stamp is
    /// handled. `XSync` put it in Xlib's queue behind only what the server
    /// sent first, however much another client's flood put there, so the
    /// turns end; what came after it waits for the next wait.
    pub(crate) fn set_selection(&mut self, selection: Selection, text: &str) {
        self.clipboard.set(&self.connection, selection, text);
        self.connection.sync();
        let mut backlog = core::mem::take(&mut self.backlog);
        while self.drain(&mut backlog, 0) && self.clipboard.is_stamping() {}
        self.backlog = backlog;
        self.connection.flush();
    }

    /// Asks for the selection's text, as
    /// [`crate::window::Window::request_selection`] says.
    pub(crate) fn request_selection(&mut self, selection: Selection) {
        self.clipboard
            .request(&self.connection, selection, Instant::now());
        self.connection.flush();
    }

    /// Waits for events, as [`crate::window::Window::wait_events`] says.
    pub(crate) fn wait_events(
        &mut self,
        deadline: Option<Instant>,
        events: &mut Vec<Event>,
    ) -> Result<(), Error> {
        let start = events.len();
        // What was read outside a wait comes first.
        events.append(&mut self.backlog);
        loop {
            // The requests queued since the last wait, as by `present`, or
            // by the clipboard's answers in the last turn.
            self.connection.flush();
            // Xlib may hold events it read while waiting for a reply, which
            // the descriptor no longer shows: take them before polling.
            let more = self.drain(events, start);
            self.clipboard
                .expire(&self.connection, Instant::now(), events);
            // The errors among what was read, and any since the last wait.
            report_errors();
            if events.len() > start {
                // A wake that is waiting too, without waiting for one.
                if self.pipe.drain()? {
                    events.push(Event::Woken);
                }
                // What the clipboard answered while draining.
                self.connection.flush();
                return Ok(());
            }
            if more {
                // Events wait in Xlib's queue, which `poll` cannot see:
                // see to a wake and the deadline, then take more.
                if self.pipe.drain()? {
                    events.push(Event::Woken);
                }
                if events.len() > start
                    || deadline.is_some_and(|deadline| Instant::now() >= deadline)
                {
                    self.connection.flush();
                    return Ok(());
                }
                continue;
            }
            let Some(x_fd) = self.connection.fd().map(|fd| fd.as_raw_fd()) else {
                return Err(Error::ConnectionLost);
            };
            let mut fds = [
                pollfd {
                    fd: x_fd,
                    events: POLLIN,
                    revents: 0,
                },
                pollfd {
                    fd: self.pipe.read_end().as_raw_fd(),
                    events: POLLIN,
                    revents: 0,
                },
            ];
            // The clipboard's timeouts end a wait too, without an event
            // unless a paste ends.
            let wake = earliest(deadline, self.clipboard.deadline());
            let timeout = poll_timeout(wake, Instant::now());
            // SAFETY: `fds` is two initialized entries that this function
            // borrows mutably for the call. Both descriptors are open: the
            // connection's while its display is, which `self` keeps, and
            // the pipe's while `self.pipe` is.
            let ready = unsafe { poll(fds.as_mut_ptr(), fds.len() as nfds_t, timeout) };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    // A signal is no event: wait again for what is left.
                    continue;
                }
                return Err(Error::Io {
                    call: "poll",
                    source: error,
                });
            }
            // The pipe is read below whether or not `poll` saw it.
            let [x_ready, _] = fds.map(|fd| fd.revents);
            // Both sources, when both are ready, so that one call reports
            // everything that is waiting: the server's first.
            if x_ready != 0 {
                self.drain(events, start);
                report_errors();
            }
            // Whether or not `poll` saw it: a wake that came while the
            // server's events were taken is seen now, not a turn later.
            if self.pipe.drain()? {
                events.push(Event::Woken);
            }
            self.clipboard
                .expire(&self.connection, Instant::now(), events);
            if x_ready & (POLLERR | POLLHUP | POLLNVAL) != 0 {
                // The drain read what the server sent before it went; then
                // Xlib sees the loss, and its handler ends the process.
                return if events.len() > start {
                    Ok(())
                } else {
                    Err(Error::ConnectionLost)
                };
            }
            if events.len() > start {
                self.connection.flush();
                return Ok(());
            }
            // What came from the server was no event, such as a reply, an
            // error or a request the clipboard answered, or a timeout of
            // the clipboard's passed: wait again, unless the deadline did.
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                self.connection.flush();
                return Ok(());
            }
        }
    }

    /// Takes the events Xlib has or can read without waiting, for at most
    /// [`DRAIN_BUDGET`], appends what they amount to, and reads the parts
    /// of pastes they announced. Returns whether it stopped for the budget,
    /// with events perhaps left in Xlib's queue, which polling the
    /// connection does not show. `events` from `start` on are this wait's.
    fn drain(&mut self, events: &mut Vec<Event>, start: usize) -> bool {
        let until = Instant::now() + DRAIN_BUDGET;
        loop {
            if self.take_events(events, start, until) {
                // The parts announced so far, so that a flood of events
                // cannot put them off for ever.
                self.clipboard
                    .read_parts(&self.connection, Instant::now(), events);
                return true;
            }
            // Reading a part waits for the server, which may queue more
            // events meanwhile: take those too.
            if !self
                .clipboard
                .read_parts(&self.connection, Instant::now(), events)
            {
                return false;
            }
        }
    }

    /// Takes every event Xlib has or can read without waiting, until
    /// `until`, and appends what they amount to. Returns whether it stopped
    /// at `until`.
    fn take_events(&mut self, events: &mut Vec<Event>, start: usize, until: Instant) -> bool {
        // `pending` reads what has arrived once the queue is empty.
        while self.connection.pending() > 0 {
            if Instant::now() >= until {
                return true;
            }
            let mut event = self.connection.next_event();
            // Decoded before the input method sees it: Xlib's own sets the
            // keycode of the press that ends a Compose sequence to 0 when
            // it takes it, and sends its text as a copy of that press.
            let incoming = event.decode();
            let taken = self.connection.filter_event(&mut event);
            // Any other event an input method takes is its own. A key it
            // takes still moved: only what it types is the input method's.
            if taken && !matches!(incoming, Incoming::Key { .. }) {
                continue;
            }
            if let Incoming::Selection(selection) = incoming {
                self.clipboard
                    .handle(&self.connection, selection, Instant::now(), events);
                continue;
            }
            self.translator.handle(incoming, events, start);
            if let Incoming::Key {
                pressed: true,
                state,
                ..
            } = incoming
                && let Some(typed) = self.connection.lookup_text(&mut event, &mut self.lookup)
            {
                input::push_typed(typed, state, events);
            }
            if let Some(focused) = input::focus_change(incoming) {
                self.connection.set_text_focus(focused);
            }
        }
        false
    }

    /// Shows the damaged parts of `pixels`, as
    /// [`crate::window::Window::present`] says.
    pub(crate) fn present(
        &mut self,
        pixels: &Pixels<'_>,
        damage: &[DeviceRect],
    ) -> Result<(), Error> {
        let window = self.translator.size();
        for &rect in damage {
            if let Some(rect) = surface::clip(rect, pixels.size(), window) {
                self.connection.put_pixels(self.window, pixels, rect)?;
            }
        }
        self.connection.flush();
        Ok(())
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // The input context goes before its window.
        self.connection.close_text_input();
        for cursor in self.cursors {
            if cursor != 0 {
                self.connection.free_cursor(cursor);
            }
        }
        self.connection.free_gc();
        self.connection.destroy_window(self.window);
        self.connection.free_colormap(self.colormap);
        // The fields drop after this: the connection closes, and then
        // releases its claim of the Xlib thread.
    }
}

/// Logs the requests the X server refused since the last call.
fn report_errors() {
    if let Some(errors) = claim::take_errors() {
        warn!(
            "the X server refused {} request(s) since the last wait; the last: error {} for \
             request {}.{} (serial {})",
            errors.count, errors.error_code, errors.request_code, errors.minor_code, errors.serial
        );
    }
}

/// The earlier of two deadlines, where `None` is never.
fn earliest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// The milliseconds `poll` is to wait for `deadline`, from `now`: rounded
/// up, so that a wait never ends before the deadline; -1, for ever, with no
/// deadline; and at most `c_int::MAX`, after which the loop waits again.
fn poll_timeout(deadline: Option<Instant>, now: Instant) -> c_int {
    let Some(deadline) = deadline else {
        return -1;
    };
    let left = deadline.saturating_duration_since(now);
    let millis = left.as_nanos().div_ceil(1_000_000);
    c_int::try_from(millis).unwrap_or(c_int::MAX)
}

/// The window's size in device pixels for a logical size: rounded up, and
/// within what X can make a window of.
fn device_size(logical: Size, scale: Scale) -> DeviceSize {
    let size = scale.to_device(logical);
    DeviceSize::new(
        size.width.clamp(1, MAX_DIMENSION),
        size.height.clamp(1, MAX_DIMENSION),
    )
}

/// Returns the type to give the `WM_NAME` property for this title.
///
/// The property's own type, `STRING`, is Latin-1. A title in plain ASCII is
/// the same bytes in Latin-1 and in UTF-8, so it can have that type. Any
/// other title is marked as UTF-8, which window managers accept here too.
fn legacy_title_type(title: &str, utf8_string: Atom) -> Atom {
    if title.is_ascii() {
        XA_STRING
    } else {
        utf8_string
    }
}

/// The core font cursor of each shape (`X11/cursorfont.h`).
fn font_cursor(shape: CursorShape) -> c_uint {
    match shape {
        CursorShape::Arrow => xlib::XC_left_ptr,
        CursorShape::Text => xlib::XC_xterm,
        CursorShape::Hand => xlib::XC_hand2,
        CursorShape::ColumnResize => xlib::XC_sb_h_double_arrow,
        CursorShape::RowResize => xlib::XC_sb_v_double_arrow,
    }
}

/// The machine's host name, as `WM_CLIENT_MACHINE` is to hold it, or
/// `None` if it cannot be read.
fn host_name() -> Option<Vec<u8>> {
    let name = std::fs::read(HOST_NAME).ok()?;
    let name = name.trim_ascii();
    (!name.is_empty() && !name.contains(&0)).then(|| name.to_vec())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::sync::MutexGuard;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::super::connection::MAX_LONGS;
    use super::super::selection::SelectionEvent;
    use super::*;
    use crate::clipboard::MAX_PASTE_BYTES;

    // -- without a server -----------------------------------------------------

    #[test]
    fn the_timeout_rounds_up_and_waits_for_ever_without_a_deadline() {
        let now = Instant::now();
        assert_eq!(poll_timeout(None, now), -1);
        assert_eq!(poll_timeout(Some(now), now), 0);
        // A deadline that passed already does not wait.
        assert_eq!(
            poll_timeout(now.checked_sub(Duration::from_secs(1)), now),
            0
        );
        assert_eq!(poll_timeout(Some(now + Duration::from_nanos(1)), now), 1);
        assert_eq!(poll_timeout(Some(now + Duration::from_millis(5)), now), 5);
        assert_eq!(
            poll_timeout(Some(now + Duration::from_micros(5001)), now),
            6
        );
        assert_eq!(
            poll_timeout(Some(now + Duration::from_secs(60 * 60 * 24 * 365)), now),
            c_int::MAX
        );
    }

    #[test]
    fn a_logical_size_becomes_one_x_can_make() {
        let scale = |factor| Scale::new(factor).expect("a valid scale");
        assert_eq!(
            device_size(Size::new(640.0, 360.0), scale(1.0)),
            DeviceSize::new(640, 360)
        );
        assert_eq!(
            device_size(Size::new(640.0, 360.0), scale(1.5)),
            DeviceSize::new(960, 540)
        );
        // Rounded up, so the content fits.
        assert_eq!(
            device_size(Size::new(101.0, 3.0), scale(1.25)),
            DeviceSize::new(127, 4)
        );
        assert_eq!(device_size(Size::ZERO, scale(1.0)), DeviceSize::new(1, 1));
        assert_eq!(
            device_size(Size::new(-5.0, f32::NAN), scale(2.0)),
            DeviceSize::new(1, 1)
        );
        assert_eq!(
            device_size(Size::new(f32::INFINITY, 20_000.0), scale(2.0)),
            DeviceSize::new(32767, 32767)
        );
    }

    #[test]
    fn only_an_ascii_title_is_a_latin_1_string() {
        const UTF8_STRING: Atom = 400;
        assert_eq!(legacy_title_type("Tidepool", UTF8_STRING), XA_STRING);
        assert_eq!(legacy_title_type("", UTF8_STRING), XA_STRING);
        // Latin-1 could hold the first of these, but not as these bytes.
        assert_eq!(legacy_title_type("Café", UTF8_STRING), UTF8_STRING);
        assert_eq!(legacy_title_type("タイドプール", UTF8_STRING), UTF8_STRING);
    }

    #[test]
    fn the_event_mask_selects_what_the_translator_handles() {
        // The values are those of `X11/X.h`.
        let expected = (1 << 0)
            | (1 << 1)
            | (1 << 2)
            | (1 << 3)
            | (1 << 6)
            | (1 << 15)
            | (1 << 17)
            | (1 << 21)
            | (1 << 22);
        assert_eq!(EVENT_MASK, expected);
    }

    #[test]
    fn the_class_names_the_instance_and_the_class() {
        let names: Vec<&[u8]> = CLASS.split(|&byte| byte == 0).collect();
        assert_eq!(names, [&b"tidepool"[..], b"Tidepool", b""]);
    }

    #[test]
    fn each_shape_has_its_own_font_cursor() {
        // The values are those of `X11/cursorfont.h`.
        let shapes: Vec<c_uint> = CursorShape::ALL.into_iter().map(font_cursor).collect();
        assert_eq!(shapes, [68, 152, 60, 108, 116]);
    }

    // -- with a server --------------------------------------------------------

    /// Says whether the window tests may run, as `TIDEPOOL_IT_X11=1`
    /// allows, and if so holds the lock that runs them one at a time. If
    /// not, it says on standard error, as the gate counts it, that `test`
    /// skipped (`docs/04-quality.md`).
    fn x11(test: &str) -> Option<MutexGuard<'static, ()>> {
        let reason = match std::env::var_os("TIDEPOOL_IT_X11") {
            Some(value) if value == "1" => return Some(claim::serial()),
            Some(value) => format!("TIDEPOOL_IT_X11 is {value:?}, not 1"),
            None => "TIDEPOOL_IT_X11 is not set".to_string(),
        };
        // One write, so that the line reaches the stream whole.
        let _ = io::stderr().write_all(format!("SKIPPED: {test}: {reason}\n").as_bytes());
        None
    }

    fn config(title: &str) -> WindowConfig {
        WindowConfig {
            title: title.to_string(),
            size: Size::new(64.0, 48.0),
        }
    }

    /// Calls `open` until the server takes the connection, up to 20 times
    /// 50 ms apart, and returns what the last call returned.
    ///
    /// Stock `xvfb-run` starts Xvfb without `-noreset`, so the server
    /// resets each time its last client leaves, as when one of these
    /// tests, which run one at a time, drops its only window; while it
    /// resets it refuses connections, and the next test's `XOpenDisplay`
    /// fails. Only that failure is tried again: any other result, an
    /// [`Error::WrongThread`] included, is returned at once.
    fn retrying<T>(mut open: impl FnMut() -> Result<T, Error>) -> Result<T, Error> {
        for _ in 1..20 {
            match open() {
                Err(Error::ConnectionFailed { .. }) => thread::sleep(Duration::from_millis(50)),
                done => return done,
            }
        }
        open()
    }

    fn open(title: &str) -> Window {
        retrying(|| Window::open(&config(title))).unwrap_or_else(|error| panic!("{title}: {error}"))
    }

    /// Waits up to `limit` for events that `done` accepts, and returns every
    /// event that came.
    fn wait_for(
        window: &mut Window,
        limit: Duration,
        done: impl Fn(&[Event]) -> bool,
    ) -> Vec<Event> {
        let deadline = Instant::now() + limit;
        let mut events = Vec::new();
        while !done(&events) {
            assert!(Instant::now() < deadline, "not in {limit:?}: {events:?}");
            window
                .wait_events(Some(deadline), &mut events)
                .unwrap_or_else(|error| panic!("{error}"));
        }
        events
    }

    /// Opens a window and waits until the server has shown it, and until
    /// nothing more comes for a while.
    fn open_shown(title: &str) -> Window {
        let mut window = open(title);
        wait_for(&mut window, Duration::from_secs(5), |events| {
            events
                .iter()
                .any(|event| matches!(event, Event::Exposed(_)))
        });
        settle(&mut window);
        window
    }

    /// Waits until a wait of 100 ms reports nothing, for up to 2 s.
    fn settle(window: &mut Window) {
        for _ in 0..20 {
            let mut events = Vec::new();
            let deadline = Instant::now() + Duration::from_millis(100);
            window
                .wait_events(Some(deadline), &mut events)
                .unwrap_or_else(|error| panic!("{error}"));
            if events.is_empty() {
                return;
            }
        }
        panic!("events kept coming");
    }

    /// The color of the test pattern at (`x`, `y`), as BGRA bytes. No pixel
    /// of it is black, the window's background, and its alpha, which the
    /// window does not show, is not 255.
    fn pattern(x: i32, y: i32) -> [u8; 4] {
        let channel = |value: i32| 1 + (value.rem_euclid(250) as u8);
        [channel(x * 3 + y), channel(y * 7), channel(x * 5), 0x80]
    }

    /// A frame of `size` with the pattern, and four bytes of padding at the
    /// end of each row, and its stride.
    fn frame(size: DeviceSize) -> (Vec<u8>, usize) {
        let width = usize::try_from(size.width).expect("a width");
        let height = usize::try_from(size.height).expect("a height");
        let stride = width * 4 + 4;
        let mut bytes = vec![0xee; stride * height];
        for y in 0..size.height {
            for x in 0..size.width {
                let at =
                    usize::try_from(y).expect("y") * stride + usize::try_from(x).expect("x") * 4;
                bytes[at..at + 4].copy_from_slice(&pattern(x, y));
            }
        }
        (bytes, stride)
    }

    #[test]
    fn a_presented_frame_reads_back_exactly_where_it_was_damaged() {
        let test = "a_presented_frame_reads_back_exactly_where_it_was_damaged";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut window = open_shown(test);
        let size = window.size();
        let (width, height) = (size.width, size.height);
        let (bytes, stride) = frame(size);
        let pixels = Pixels::new(&bytes, size, stride).expect("a frame");
        let damage = [
            DeviceRect::new(2, 3, 10, 7),
            DeviceRect::new(20, 1, 5, 5),
            // Over the right and bottom edges.
            DeviceRect::new(width - 6, height - 4, 50, 50),
            // Over the left edge.
            DeviceRect::new(-3, height / 2, 6, 4),
            // Empty, and outside.
            DeviceRect::new(30, 30, 0, 9),
            DeviceRect::new(width + 10, 0, 5, 5),
        ];
        window.present(&pixels, &damage).expect("presented");
        let _ = claim::take_errors();
        window.connection.sync();
        assert_eq!(claim::take_errors(), None, "the server refused a request");

        let clipped: Vec<DeviceRect> = damage
            .iter()
            .filter_map(|&rect| surface::clip(rect, size, size))
            .collect();
        assert_eq!(clipped.len(), 4);
        let image = window
            .connection
            .get_image(window.window, size)
            .expect("the window reads back");
        let mut wrong = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let damaged = clipped
                    .iter()
                    .any(|rect| rect.contains(tidepool_core::geom::DevicePoint::new(x, y)));
                let expected = if damaged {
                    let [blue, green, red, _] = pattern(x, y);
                    [red, green, blue]
                } else {
                    [0, 0, 0]
                };
                let actual = image.rgb(x, y);
                if actual != Some(expected) {
                    wrong.push(((x, y), expected, actual));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of {} pixels differ, the first: {:?}",
            wrong.len(),
            width * height,
            &wrong[..wrong.len().min(5)]
        );
        drop(image);

        // A frame smaller than the window changes only what it covers,
        // whatever the damage says.
        let white = [0xff_u8; 3 * 3 * 4];
        let small = Pixels::new(&white, DeviceSize::new(3, 3), 12).expect("a small frame");
        window
            .present(&small, &[DeviceRect::new(0, 0, width, height)])
            .expect("presented");
        window.connection.sync();
        let image = window
            .connection
            .get_image(window.window, size)
            .expect("the window reads back");
        assert_eq!(image.rgb(0, 0), Some([0xff; 3]));
        assert_eq!(image.rgb(2, 2), Some([0xff; 3]));
        assert_eq!(
            image.rgb(3, 3),
            Some({
                let [blue, green, red, _] = pattern(3, 3);
                [red, green, blue]
            })
        );
        assert_eq!(image.rgb(40, 40), Some([0, 0, 0]));
    }

    #[test]
    fn a_waker_on_another_thread_ends_a_wait_within_5_ms() {
        let test = "a_waker_on_another_thread_ends_a_wait_within_5_ms";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut window = open_shown(test);
        let waker = window.waker();
        let (sender, receiver) = mpsc::channel();
        let waking = thread::spawn(move || {
            // Long enough for the window to be waiting.
            thread::sleep(Duration::from_millis(200));
            let before = Instant::now();
            waker.wake();
            let _ = sender.send(before);
        });
        let far = Instant::now() + Duration::from_secs(30);
        let mut events = Vec::new();
        let returned = loop {
            window
                .wait_events(Some(far), &mut events)
                .unwrap_or_else(|error| panic!("{error}"));
            let now = Instant::now();
            if events.contains(&Event::Woken) {
                break now;
            }
            assert!(now < far, "never woken: {events:?}");
            events.clear();
        };
        let before = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("the other thread woke the window");
        waking.join().expect("joined");
        assert!(returned >= before, "the wait ended before the wake");
        let took = returned.duration_since(before);
        assert!(
            took < Duration::from_millis(5),
            "the wait ended {took:?} after the wake, not within 5 ms"
        );
        // Woken once, and not again.
        let mut events = Vec::new();
        window
            .wait_events(Some(Instant::now()), &mut events)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!events.contains(&Event::Woken), "{events:?}");
    }

    #[test]
    fn one_wait_reports_both_the_servers_events_and_a_wake() {
        let test = "one_wait_reports_both_the_servers_events_and_a_wake";
        let Some(_serial) = x11(test) else {
            return;
        };
        // The opening waits for the server with `XSync`, so the window's
        // first exposure is already in Xlib's queue, or on its way.
        let mut window = open(test);
        window.waker().wake();
        let limit = Instant::now() + Duration::from_secs(5);
        while window.connection.pending() == 0 {
            assert!(Instant::now() < limit, "nothing came from the server");
            thread::sleep(Duration::from_millis(1));
        }
        let mut events = Vec::new();
        window
            .wait_events(None, &mut events)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Exposed(_))),
            "{events:?}"
        );
        assert!(events.contains(&Event::Woken), "{events:?}");
    }

    #[test]
    fn a_wait_with_nothing_to_report_ends_at_its_deadline() {
        let test = "a_wait_with_nothing_to_report_ends_at_its_deadline";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut window = open_shown(test);
        let wait = Duration::from_millis(150);
        let mut quiet = None;
        for _ in 0..5 {
            let deadline = Instant::now() + wait;
            let mut events = Vec::new();
            window
                .wait_events(Some(deadline), &mut events)
                .unwrap_or_else(|error| panic!("{error}"));
            let ended = Instant::now();
            if events.is_empty() {
                quiet = Some((deadline, ended));
                break;
            }
        }
        let (deadline, ended) = quiet.expect("a wait with no events");
        assert!(ended >= deadline, "ended {:?} early", deadline - ended);
        let late = ended - deadline;
        assert!(late < Duration::from_millis(50), "ended {late:?} late");

        // A deadline that passed already returns at once.
        let start = Instant::now();
        let mut events = Vec::new();
        window
            .wait_events(start.checked_sub(Duration::from_secs(1)), &mut events)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn a_window_on_another_thread_fails_to_open_while_one_is_open() {
        let test = "a_window_on_another_thread_fails_to_open_while_one_is_open";
        let Some(_serial) = x11(test) else {
            return;
        };
        let elsewhere = |title: &'static str| {
            thread::spawn(move || {
                retrying(|| crate::window::Window::open(&config(title))).map(drop)
            })
            .join()
            .expect("joined")
        };
        let first = open("first");
        let refused = elsewhere("on another thread");
        assert!(matches!(refused, Err(Error::WrongThread)), "{refused:?}");
        // A second window on this thread opens, with a connection of its
        // own.
        let second = open("second on this thread");
        drop(first);
        let refused = elsewhere("on another thread, while one is open");
        assert!(matches!(refused, Err(Error::WrongThread)), "{refused:?}");
        drop(second);
        // With none open, any thread may open one.
        let opened = elsewhere("on another thread, once none is open");
        assert!(opened.is_ok(), "{opened:?}");
    }

    #[test]
    fn every_request_of_the_window_is_accepted() {
        let test = "every_request_of_the_window_is_accepted";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut window = open_shown(test);
        assert!(window.connection.has_text_input(), "no input method opened");
        window.set_title("Tidepool — ünïcödé ✓");
        for shape in CursorShape::ALL {
            window.set_cursor(shape);
        }
        let black = [0_u8; 4];
        let pixel = Pixels::new(&black, DeviceSize::new(1, 1), 4).expect("a pixel");
        window
            .present(&pixel, &[DeviceRect::new(0, 0, 1, 1)])
            .expect("presented");
        let _ = claim::take_errors();
        window.connection.sync();
        assert_eq!(claim::take_errors(), None);
        assert!(!window.scale().factor().is_nan());
    }

    #[test]
    fn a_refused_request_is_recorded_and_the_process_lives_on() {
        let test = "a_refused_request_is_recorded_and_the_process_lives_on";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut window = open_shown(test);
        let _ = claim::take_errors();
        // No window has the id 0 (`None`): the server refuses with
        // `BadWindow` (3) the `MapWindow` request (8).
        window.connection.map_window(0);
        window.connection.sync();
        let errors = claim::take_errors().expect("the error was recorded");
        assert_eq!(
            (errors.count, errors.error_code, errors.request_code),
            (1, 3, 8)
        );
        // A wait logs the next one, and goes on.
        window.connection.map_window(0);
        let mut events = Vec::new();
        window
            .wait_events(
                Some(Instant::now() + Duration::from_millis(200)),
                &mut events,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            claim::take_errors(),
            None,
            "the wait did not take the error"
        );
        let logged = tidepool_core::log::tail(50);
        assert!(
            logged
                .iter()
                .any(|line| line.contains("the X server refused 1 request(s)")
                    && line.contains("error 3 for request 8.0")),
            "{logged:#?}"
        );
    }

    // -- the clipboard, with a server -----------------------------------------

    /// How long a clipboard test waits for what it expects.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// Waits on each window in turn, a few milliseconds each, until `done`
    /// accepts what the windows have reported, which it returns, or
    /// `PATIENCE` passes.
    fn pump(windows: &mut [&mut Window], done: impl Fn(&[Vec<Event>]) -> bool) -> Vec<Vec<Event>> {
        let limit = Instant::now() + PATIENCE;
        let mut seen: Vec<Vec<Event>> = windows.iter().map(|_| Vec::new()).collect();
        while !done(&seen) {
            assert!(Instant::now() < limit, "not in {PATIENCE:?}: {seen:?}");
            for (window, events) in windows.iter_mut().zip(&mut seen) {
                let deadline = Instant::now() + Duration::from_millis(5);
                window
                    .wait_events(Some(deadline), events)
                    .unwrap_or_else(|error| panic!("{error}"));
            }
        }
        seen
    }

    /// The pastes among `events`.
    fn pastes(events: &[Event]) -> Vec<(Selection, Option<String>)> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::Pasted { selection, text } => Some((*selection, text.clone())),
                _ => None,
            })
            .collect()
    }

    /// Waits on the windows until the one at `asker` reports a paste, and
    /// returns its pastes.
    fn paste_in(windows: &mut [&mut Window], asker: usize) -> Vec<(Selection, Option<String>)> {
        let seen = pump(windows, |seen| !pastes(&seen[asker]).is_empty());
        pastes(&seen[asker])
    }

    /// Waits on `window` until it owns `selection`, or no longer does.
    fn until_owned(window: &mut Window, selection: Selection, owned: bool) {
        let limit = Instant::now() + PATIENCE;
        while window.clipboard.owns(selection) != owned {
            assert!(Instant::now() < limit, "{selection:?} owned is not {owned}");
            let mut events = Vec::new();
            window
                .wait_events(Some(Instant::now() + Duration::from_millis(5)), &mut events)
                .unwrap_or_else(|error| panic!("{error}"));
        }
    }

    /// A text of at least `len` bytes, with characters of one to four
    /// bytes, so that parts split some of them.
    fn long_text(len: usize) -> String {
        let mut text = String::with_capacity(len + 16);
        let mut line = 0_u32;
        while text.len() < len {
            text.push_str(&format!("{line:06} tidepool é ✓ 🌊\n"));
            line += 1;
        }
        text
    }

    #[test]
    fn text_copied_in_one_window_pastes_in_another() {
        let test = "text_copied_in_one_window_pastes_in_another";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut copier = open_shown("copier");
        let mut paster = open_shown("paster");
        copier.set_selection(Selection::Clipboard, "Tidepool — ünïcödé ✓ 🌊\r\n\ttabs");
        copier.set_selection(Selection::Primary, "the primary selection");
        until_owned(&mut copier, Selection::Clipboard, true);
        assert!(copier.clipboard.owns(Selection::Primary));

        paster.request_selection(Selection::Clipboard);
        let pasted = paste_in(&mut [&mut copier, &mut paster], 1);
        assert_eq!(
            pasted,
            [(
                Selection::Clipboard,
                Some("Tidepool — ünïcödé ✓ 🌊\r\n\ttabs".to_string())
            )]
        );
        paster.request_selection(Selection::Primary);
        let pasted = paste_in(&mut [&mut copier, &mut paster], 1);
        assert_eq!(
            pasted,
            [(
                Selection::Primary,
                Some("the primary selection".to_string())
            )]
        );
        // And back the other way, with an empty text.
        paster.set_selection(Selection::Clipboard, "");
        until_owned(&mut paster, Selection::Clipboard, true);
        copier.request_selection(Selection::Clipboard);
        let pasted = paste_in(&mut [&mut copier, &mut paster], 0);
        assert_eq!(pasted, [(Selection::Clipboard, Some(String::new()))]);
        assert_eq!(claim::take_errors(), None, "the server refused a request");
    }

    #[test]
    fn a_1_mib_paste_goes_through_incr() {
        let test = "a_1_mib_paste_goes_through_incr";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut copier = open_shown("copier");
        let mut paster = open_shown("paster");
        let text = long_text(1024 * 1024);
        assert!(text.len() > copier.clipboard.part());
        copier.set_selection(Selection::Clipboard, &text);
        paster.request_selection(Selection::Clipboard);
        let pasted = paste_in(&mut [&mut copier, &mut paster], 1);
        assert_eq!(pasted.len(), 1);
        assert_eq!(pasted[0].0, Selection::Clipboard);
        let got = pasted[0].1.as_deref().expect("the text pasted");
        assert_eq!(got.len(), text.len());
        assert!(got == text, "the paste differs from the copy");
        // One transfer, of whole parts and an empty one that ends it.
        let parts = text.len().div_ceil(copier.clipboard.part()) + 1;
        assert_eq!(copier.clipboard.counts.sent_incr, 1);
        assert_eq!(copier.clipboard.counts.parts_sent, parts);
        assert_eq!(paster.clipboard.counts.received_incr, 1);
        // The empty part that the paster read ended the transfer, and the
        // paster's window is no longer watched.
        assert!(copier.clipboard.deadline().is_none());
        assert_eq!(claim::take_errors(), None, "the server refused a request");
    }

    #[test]
    fn a_window_pastes_what_it_just_set() {
        let test = "a_window_pastes_what_it_just_set";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut window = open_shown(test);
        window.set_selection(Selection::Clipboard, "first");
        window.set_selection(Selection::Clipboard, "second");
        window.request_selection(Selection::Clipboard);
        // A second request joins the first: one answer.
        window.request_selection(Selection::Clipboard);
        let pasted = paste_in(&mut [&mut window], 0);
        assert_eq!(pasted, [(Selection::Clipboard, Some("second".to_string()))]);
        settle(&mut window);
        // And a long one, by INCR to its own window.
        let text = long_text(600 * 1024);
        window.set_selection(Selection::Clipboard, &text);
        window.request_selection(Selection::Clipboard);
        let pasted = paste_in(&mut [&mut window], 0);
        assert!(
            pasted == [(Selection::Clipboard, Some(text))],
            "the paste differs"
        );
        assert_eq!(window.clipboard.counts.received_incr, 1);
        assert_eq!(claim::take_errors(), None, "the server refused a request");
    }

    #[test]
    fn a_selection_nobody_owns_pastes_nothing() {
        let test = "a_selection_nobody_owns_pastes_nothing";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut window = open_shown(test);
        if window
            .connection
            .selection_owner(window.connection.intern(c"CLIPBOARD"))
            != 0
        {
            // Another client of this server holds it: nothing to test here.
            let _ = io::stderr()
                .write_all(format!("SKIPPED: {test}: another client owns CLIPBOARD\n").as_bytes());
            return;
        }
        window.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut window], 0),
            [(Selection::Clipboard, None)]
        );
    }

    #[test]
    fn a_paste_that_gets_no_answer_ends_with_nothing() {
        let test = "a_paste_that_gets_no_answer_ends_with_nothing";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut owner = open_shown("owner");
        let mut paster = open_shown("paster");
        owner.set_selection(Selection::Primary, "never sent");
        until_owned(&mut owner, Selection::Primary, true);
        let timeout = Duration::from_millis(300);
        paster.clipboard.limit_for_test(MAX_PASTE_BYTES, timeout);
        let asked = Instant::now();
        paster.request_selection(Selection::Primary);
        // The owner is not waited on, so it never answers.
        let pasted = paste_in(&mut [&mut paster], 0);
        assert_eq!(pasted, [(Selection::Primary, None)]);
        assert!(asked.elapsed() >= timeout, "{:?}", asked.elapsed());
        assert!(asked.elapsed() < timeout * 5, "{:?}", asked.elapsed());
        // The owner's late answer is not taken for another paste.
        settle(&mut owner);
        let mut events = Vec::new();
        paster
            .wait_events(
                Some(Instant::now() + Duration::from_millis(100)),
                &mut events,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(pastes(&events).is_empty(), "{events:?}");
    }

    #[test]
    fn a_selection_another_window_takes_is_given_up() {
        let test = "a_selection_another_window_takes_is_given_up";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut first = open_shown("first");
        let mut second = open_shown("second");
        first.set_selection(Selection::Clipboard, "from the first");
        until_owned(&mut first, Selection::Clipboard, true);
        second.set_selection(Selection::Clipboard, "from the second");
        until_owned(&mut second, Selection::Clipboard, true);
        until_owned(&mut first, Selection::Clipboard, false);
        first.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut first, &mut second], 0),
            [(Selection::Clipboard, Some("from the second".to_string()))]
        );
    }

    #[test]
    fn a_paste_larger_than_the_limit_is_nothing() {
        let test = "a_paste_larger_than_the_limit_is_nothing";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut copier = open_shown("copier");
        let mut paster = open_shown("paster");
        let part = copier.clipboard.part();
        // Whole: one byte over the limit, then at it.
        paster.clipboard.limit_for_test(1000, TIMEOUT_FOR_TESTS);
        copier.set_selection(Selection::Clipboard, &"x".repeat(1001));
        paster.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut copier, &mut paster], 1),
            [(Selection::Clipboard, None)]
        );
        copier.set_selection(Selection::Clipboard, &"y".repeat(1000));
        paster.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut copier, &mut paster], 1),
            [(Selection::Clipboard, Some("y".repeat(1000)))]
        );
        // By INCR: over the limit after the first part.
        paster
            .clipboard
            .limit_for_test(part + 10, TIMEOUT_FOR_TESTS);
        copier.set_selection(Selection::Clipboard, &"z".repeat(part * 3));
        paster.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut copier, &mut paster], 1),
            [(Selection::Clipboard, None)]
        );
        assert_eq!(paster.clipboard.counts.received_incr, 1);
    }

    /// The timeout for tests that do not test it.
    const TIMEOUT_FOR_TESTS: Duration = Duration::from_secs(5);

    #[test]
    fn an_owner_answers_targets_and_timestamp_and_refuses_the_rest() {
        let test = "an_owner_answers_targets_and_timestamp_and_refuses_the_rest";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut owner = open_shown("owner");
        let asker = open_shown("asker");
        owner.set_selection(Selection::Clipboard, "text");
        until_owned(&mut owner, Selection::Clipboard, true);

        let atom = |name: &core::ffi::CStr| asker.connection.intern(name);
        let (clipboard, property) = (atom(c"CLIPBOARD"), atom(c"TIDEPOOL_TEST"));
        // Asks for `target` with the asker's connection itself, and returns
        // the property the owner answered in (0 if refused) and its value.
        let mut ask = |target: Atom, into: Atom, sent_property: Atom| {
            asker.connection.delete_property(asker.window, into);
            asker.connection.convert_selection(
                clipboard,
                target,
                sent_property,
                asker.window,
                xlib::CurrentTime,
            );
            asker.connection.flush();
            let limit = Instant::now() + PATIENCE;
            loop {
                assert!(Instant::now() < limit, "no answer for target {target}");
                let mut events = Vec::new();
                owner
                    .wait_events(Some(Instant::now() + Duration::from_millis(5)), &mut events)
                    .unwrap_or_else(|error| panic!("{error}"));
                while asker.connection.pending() > 0 {
                    let event = asker.connection.next_event();
                    if let Incoming::Selection(SelectionEvent::Notify {
                        property,
                        target: answered,
                        ..
                    }) = event.decode()
                    {
                        assert_eq!(answered, target);
                        let value = asker
                            .connection
                            .get_property(asker.window, into, true, 4096)
                            .expect("the property reads");
                        return (property, value);
                    }
                }
            }
        };
        let (targets, utf8_string) = (atom(c"TARGETS"), atom(c"UTF8_STRING"));
        let (answered, value) = ask(targets, property, property);
        assert_eq!(answered, property);
        assert_eq!((value.type_, value.format), (xlib::XA_ATOM, 32));
        let listed: Vec<Atom> = value.longs.iter().map(|&atom| atom as Atom).collect();
        assert_eq!(listed, [targets, atom(c"TIMESTAMP"), utf8_string]);

        let (answered, value) = ask(atom(c"TIMESTAMP"), property, property);
        assert_eq!(answered, property);
        assert_eq!((value.type_, value.format), (xlib::XA_INTEGER, 32));
        assert_eq!(value.longs.len(), 1);
        assert_ne!(value.longs[0], 0);

        // An obsolete client gives no property: the target names it.
        let (answered, value) = ask(utf8_string, utf8_string, 0);
        assert_eq!(answered, utf8_string);
        assert_eq!(
            (value.type_, value.bytes.as_slice()),
            (utf8_string, &b"text"[..])
        );

        for refused in [c"STRING", c"MULTIPLE", c"TEXT", c"COMPOUND_TEXT"] {
            let (answered, value) = ask(atom(refused), property, property);
            assert_eq!(answered, 0, "{refused:?}");
            assert_eq!(value.type_, 0, "{refused:?}: the property was written");
        }
        assert_eq!(claim::take_errors(), None, "the server refused a request");
    }

    /// Waits on `window` for up to `limit` and returns its pastes, if any.
    fn pastes_within(window: &mut Window, limit: Duration) -> Vec<(Selection, Option<String>)> {
        let mut events = Vec::new();
        window
            .wait_events(Some(Instant::now() + limit), &mut events)
            .unwrap_or_else(|error| panic!("{error}"));
        pastes(&events)
    }

    #[test]
    fn a_copy_survives_another_client_meddling_with_its_time_stamp() {
        let test = "a_copy_survives_another_client_meddling_with_its_time_stamp";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut owner = open_shown("owner");
        let mut meddler = open_shown("meddler");
        owner.set_selection(Selection::Clipboard, "first");
        assert!(owner.clipboard.owns(Selection::Clipboard));
        let atom = |name: &core::ffi::CStr| meddler.connection.intern(name);
        let (clipboard, utf8_string, stamp) = (
            atom(c"CLIPBOARD"),
            atom(c"UTF8_STRING"),
            atom(c"TIDEPOOL_TIMESTAMP"),
        );
        // Asked to answer into its own time stamp property, the owner
        // refuses, and the property keeps the type the owner gave it.
        meddler.connection.convert_selection(
            clipboard,
            utf8_string,
            stamp,
            owner.window,
            xlib::CurrentTime,
        );
        meddler.connection.flush();
        settle(&mut owner);
        let kept = owner
            .connection
            .get_property(owner.window, stamp, false, 64)
            .expect("the property reads");
        assert_eq!((kept.type_, kept.format), (xlib::XA_INTEGER, 32));
        // Given another type and format by another client, the property
        // still gives the next copy its time.
        assert!(
            meddler
                .connection
                .set_bytes(owner.window, stamp, xlib::XA_STRING, b"x")
        );
        meddler.connection.sync();
        // The change it makes is an event too, which must not stand in for
        // the time stamp of the next copy.
        settle(&mut owner);
        owner.set_selection(Selection::Clipboard, "second");
        assert!(owner.clipboard.owns(Selection::Clipboard));
        meddler.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut owner, &mut meddler], 1),
            [(Selection::Clipboard, Some("second".to_string()))]
        );
        assert_eq!(claim::take_errors(), None, "the server refused a request");
    }

    #[test]
    fn an_incr_paste_whose_parts_never_end_ends_with_nothing() {
        let test = "an_incr_paste_whose_parts_never_end_ends_with_nothing";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut copier = open_shown("copier");
        let mut paster = open_shown("paster");
        let meddler = open_shown("meddler");
        let timeout = Duration::from_millis(100);
        paster.clipboard.limit_for_test(MAX_PASTE_BYTES, timeout);
        let text = long_text(copier.clipboard.part() * 3);
        copier.set_selection(Selection::Clipboard, &text);
        paster.request_selection(Selection::Clipboard);
        // Until the owner answers by INCR, and no longer.
        let limit = Instant::now() + PATIENCE;
        while paster.clipboard.counts.received_incr == 0 {
            assert!(Instant::now() < limit, "no INCR answer");
            for window in [&mut copier, &mut paster] {
                let mut events = Vec::new();
                window
                    .wait_events(Some(Instant::now() + Duration::from_millis(5)), &mut events)
                    .unwrap_or_else(|error| panic!("{error}"));
            }
        }
        let started = Instant::now();
        // Another client writes a byte as the next part, more often than
        // the timeout, for ever.
        let property = paster
            .clipboard
            .pending_property(Selection::Clipboard)
            .expect("a paste is pending");
        let utf8_string = meddler.connection.intern(c"UTF8_STRING");
        let pasted = loop {
            assert!(started.elapsed() < PATIENCE, "the paste never ended");
            meddler
                .connection
                .set_bytes(paster.window, property, utf8_string, b"x");
            meddler.connection.flush();
            let pasted = pastes_within(&mut paster, Duration::from_millis(40));
            if !pasted.is_empty() {
                break pasted;
            }
        };
        let took = started.elapsed();
        assert_eq!(pasted, [(Selection::Clipboard, None)]);
        let overall = timeout * super::super::selection::RECEIVE_TIMEOUTS;
        assert!(took >= overall - Duration::from_millis(100), "{took:?}");
        assert!(took < overall + Duration::from_secs(1), "{took:?}");
        // The selection pastes again.
        copier.set_selection(Selection::Clipboard, "small");
        paster.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut copier, &mut paster], 1),
            [(Selection::Clipboard, Some("small".to_string()))]
        );
    }

    #[test]
    fn an_answer_in_format_32_pastes_nothing() {
        let test = "an_answer_in_format_32_pastes_nothing";
        let Some(_serial) = x11(test) else {
            return;
        };
        // The owner holds the selection but is never waited on, so only the
        // meddler's answers come.
        let mut owner = open_shown("owner");
        let mut paster = open_shown("paster");
        let meddler = open_shown("meddler");
        owner.set_selection(Selection::Clipboard, "real");
        let atom = |name: &core::ffi::CStr| meddler.connection.intern(name);
        let (clipboard, utf8_string) = (atom(c"CLIPBOARD"), atom(c"UTF8_STRING"));
        // More items than are ever copied, and a few: neither is text.
        for items in [MAX_LONGS * 16, 3] {
            paster.request_selection(Selection::Clipboard);
            let property = paster
                .clipboard
                .pending_property(Selection::Clipboard)
                .expect("a paste is pending");
            let longs = vec![0x41; items];
            assert!(
                meddler
                    .connection
                    .set_longs(paster.window, property, utf8_string, &longs)
            );
            assert!(meddler.connection.send_selection_notify(
                paster.window,
                clipboard,
                utf8_string,
                property,
                xlib::CurrentTime
            ));
            meddler.connection.flush();
            assert_eq!(
                paste_in(&mut [&mut paster], 0),
                [(Selection::Clipboard, None)],
                "{items} items"
            );
        }
        drop(owner);
    }

    #[test]
    fn an_abandoned_transfer_neither_spoils_the_next_paste_nor_outlives_a_new_request() {
        let test = "an_abandoned_transfer_neither_spoils_the_next_paste_nor_outlives_a_new_request";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut copier = open_shown("copier");
        let mut paster = open_shown("paster");
        let asker = open_shown("asker");
        let text = long_text(copier.clipboard.part() * 3);
        copier.set_selection(Selection::Clipboard, &text);
        // An INCR transfer that the paster gives up after its first part:
        // the next paste, in another answer property, gets the new text.
        paster
            .clipboard
            .limit_for_test(copier.clipboard.part() / 2, TIMEOUT_FOR_TESTS);
        paster.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut copier, &mut paster], 1),
            [(Selection::Clipboard, None)]
        );
        assert!(copier.clipboard.deadline().is_some(), "the transfer ended");
        copier.set_selection(Selection::Clipboard, "short");
        paster
            .clipboard
            .limit_for_test(MAX_PASTE_BYTES, TIMEOUT_FOR_TESTS);
        paster.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut copier, &mut paster], 1),
            [(Selection::Clipboard, Some("short".to_string()))]
        );

        // A client that asks into one property each time, as many do: its
        // new request ends the transfer it abandoned there.
        copier.set_selection(Selection::Clipboard, &text);
        let atom = |name: &core::ffi::CStr| asker.connection.intern(name);
        let (clipboard, utf8_string, property) = (
            atom(c"CLIPBOARD"),
            atom(c"UTF8_STRING"),
            atom(c"TIDEPOOL_TEST"),
        );
        let ask = || {
            asker.connection.convert_selection(
                clipboard,
                utf8_string,
                property,
                asker.window,
                xlib::CurrentTime,
            );
            asker.connection.flush();
        };
        ask();
        let limit = Instant::now() + PATIENCE;
        while copier.clipboard.transfers_to(asker.window) == 0 {
            assert!(Instant::now() < limit, "no INCR transfer began");
            let _ = pastes_within(&mut copier, Duration::from_millis(5));
        }
        copier.set_selection(Selection::Clipboard, "short again");
        // The asker never took a part: only the new request ends the
        // transfer, here or in 5 s.
        ask();
        settle(&mut copier);
        let answer = asker
            .connection
            .get_property(asker.window, property, false, 4096)
            .expect("the property reads");
        assert_eq!(
            (answer.type_, answer.bytes.as_slice()),
            (utf8_string, &b"short again"[..])
        );
        // The paster's transfer, in a resting property, may still run; the
        // asker's does not.
        assert_eq!(copier.clipboard.transfers_to(asker.window), 0);
        drop(paster);
    }

    /// Waits on `window` with a deadline 50 ms away, as many times as it
    /// takes for `done`, and checks that no wait took more than 250 ms.
    /// Returns the pastes it reported. `feed` runs before each wait.
    fn responsive_until(
        window: &mut Window,
        mut feed: impl FnMut(),
        done: impl Fn(&[(Selection, Option<String>)]) -> bool,
    ) -> Vec<(Selection, Option<String>)> {
        let started = Instant::now();
        let mut seen = Vec::new();
        while !done(&seen) {
            assert!(started.elapsed() < PATIENCE, "not done: {seen:?}");
            feed();
            let before = Instant::now();
            seen.extend(pastes_within(window, Duration::from_millis(50)));
            let took = before.elapsed();
            assert!(
                took < Duration::from_millis(250),
                "a wait of 50 ms took {took:?}"
            );
        }
        seen
    }

    #[test]
    fn a_flood_of_parts_neither_holds_a_wait_nor_outlasts_the_paste() {
        let test = "a_flood_of_parts_neither_holds_a_wait_nor_outlasts_the_paste";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut copier = open_shown("copier");
        let mut paster = open_shown("paster");
        let meddler = open_shown("meddler");
        let timeout = Duration::from_millis(100);
        paster.clipboard.limit_for_test(MAX_PASTE_BYTES, timeout);
        copier.set_selection(
            Selection::Clipboard,
            &long_text(copier.clipboard.part() * 3),
        );
        paster.request_selection(Selection::Clipboard);
        let limit = Instant::now() + PATIENCE;
        while paster.clipboard.counts.received_incr == 0 {
            assert!(Instant::now() < limit, "no INCR answer");
            for window in [&mut copier, &mut paster] {
                let mut events = Vec::new();
                window
                    .wait_events(Some(Instant::now() + Duration::from_millis(5)), &mut events)
                    .unwrap_or_else(|error| panic!("{error}"));
            }
        }
        let property = paster
            .clipboard
            .pending_property(Selection::Clipboard)
            .expect("a paste is pending");
        let utf8_string = meddler.connection.intern(c"UTF8_STRING");
        let target = paster.window;
        let started = Instant::now();
        // Thousands of one-byte parts before each wait, without waiting for
        // the paster to take any.
        let mut writes = 0;
        let pasted = responsive_until(
            &mut paster,
            || {
                for _ in 0..5_000 {
                    meddler
                        .connection
                        .set_bytes(target, property, utf8_string, b"x");
                }
                writes += 5_000;
                meddler.connection.flush();
            },
            |seen| !seen.is_empty(),
        );
        assert_eq!(pasted, [(Selection::Clipboard, None)]);
        // The property was read once a turn, not once a write.
        let reads = paster.clipboard.counts.parts_read;
        assert!(reads * 100 < writes, "{reads} reads for {writes} writes");
        let overall = timeout * super::super::selection::RECEIVE_TIMEOUTS;
        assert!(
            started.elapsed() < overall + Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_flood_of_requests_is_refused_past_the_budget_and_holds_no_wait() {
        let test = "a_flood_of_requests_is_refused_past_the_budget_and_holds_no_wait";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut owner = open_shown("owner");
        let asker = open_shown("asker");
        let mut paster = open_shown("paster");
        let text = "y".repeat(100 * 1024);
        assert!(text.len() <= owner.clipboard.part());
        owner.clipboard.serve_limit_for_test(300 * 1024);
        owner.set_selection(Selection::Clipboard, &text);
        let atom = |name: &core::ffi::CStr| asker.connection.intern(name);
        let (clipboard, utf8_string, property) = (
            atom(c"CLIPBOARD"),
            atom(c"UTF8_STRING"),
            atom(c"TIDEPOOL_TEST"),
        );
        const REQUESTS: usize = 2_000;
        for _ in 0..REQUESTS {
            asker.connection.convert_selection(
                clipboard,
                utf8_string,
                property,
                asker.window,
                xlib::CurrentTime,
            );
        }
        asker.connection.flush();
        let (mut answered, mut refused) = (0, 0);
        let limit = Instant::now() + PATIENCE;
        while answered + refused < REQUESTS {
            assert!(
                Instant::now() < limit,
                "{answered} answered, {refused} refused"
            );
            let before = Instant::now();
            let _ = pastes_within(&mut owner, Duration::from_millis(50));
            let took = before.elapsed();
            assert!(
                took < Duration::from_millis(250),
                "a wait of 50 ms took {took:?}"
            );
            while asker.connection.pending() > 0 {
                let event = asker.connection.next_event();
                if let Incoming::Selection(SelectionEvent::Notify { property, .. }) = event.decode()
                {
                    if property == 0 {
                        refused += 1;
                    } else {
                        answered += 1;
                    }
                }
            }
        }
        assert!(
            answered >= 1 && refused >= REQUESTS / 2,
            "{answered} answered, {refused} refused"
        );
        // A second later the budget is whole again.
        thread::sleep(Duration::from_millis(1100));
        paster.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut owner, &mut paster], 1),
            [(Selection::Clipboard, Some(text))]
        );
    }

    #[test]
    fn a_clear_at_the_time_of_the_windows_own_claim_does_not_give_the_selection_up() {
        let test = "a_clear_at_the_time_of_the_windows_own_claim_does_not_give_the_selection_up";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut owner = open_shown("owner");
        let meddler = open_shown("meddler");
        let mut paster = open_shown("paster");
        owner.set_selection(Selection::Clipboard, "first");
        let clipboard = meddler.connection.intern(c"CLIPBOARD");
        // A server time of the meddler's own.
        let stamp = meddler.connection.intern(c"TIDEPOOL_TEST_TIME");
        meddler
            .connection
            .touch_property(meddler.window, stamp, xlib::XA_INTEGER);
        meddler.connection.sync();
        let mut time = None;
        while meddler.connection.pending() > 0 {
            let event = meddler.connection.next_event();
            if let Incoming::Selection(SelectionEvent::Property {
                window,
                atom,
                time: at,
                ..
            }) = event.decode()
                && window == meddler.window
                && atom == stamp
            {
                time = Some(at);
            }
        }
        let time = time.expect("the meddler's time stamp");
        // The meddler claims at that time, and then the owner claims at the
        // very same time: the server takes both, the owner's last, and the
        // meddler's claim has sent the owner a clear of that time.
        meddler
            .connection
            .set_selection_owner(clipboard, meddler.window, time);
        meddler.connection.sync();
        owner
            .clipboard
            .set(&owner.connection, Selection::Clipboard, "second");
        let stamped = SelectionEvent::Property {
            serial: core::ffi::c_ulong::MAX,
            window: owner.window,
            atom: owner.clipboard.stamp_property(),
            time,
            deleted: false,
            sent: false,
        };
        owner
            .clipboard
            .handle(&owner.connection, stamped, Instant::now(), &mut Vec::new());
        assert!(owner.clipboard.owns(Selection::Clipboard));
        assert_eq!(owner.connection.selection_owner(clipboard), owner.window);
        settle(&mut owner);
        assert!(
            owner.clipboard.owns(Selection::Clipboard),
            "the window gave up a selection it holds"
        );
        paster.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut owner, &mut paster], 1),
            [(Selection::Clipboard, Some("second".to_string()))]
        );
    }

    #[test]
    fn a_late_answer_to_an_old_paste_is_not_taken_for_a_new_one() {
        let test = "a_late_answer_to_an_old_paste_is_not_taken_for_a_new_one";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut old_owner = open_shown("old owner");
        let mut new_owner = open_shown("new owner");
        let mut paster = open_shown("paster");
        old_owner.set_selection(Selection::Clipboard, "old");
        // The old owner is not waited on, so the first paste gets no answer.
        paster
            .clipboard
            .limit_for_test(MAX_PASTE_BYTES, Duration::from_millis(300));
        paster.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut paster], 0),
            [(Selection::Clipboard, None)]
        );
        new_owner.set_selection(Selection::Clipboard, "new");
        paster
            .clipboard
            .limit_for_test(MAX_PASTE_BYTES, TIMEOUT_FOR_TESTS);
        paster.request_selection(Selection::Clipboard);
        // The old owner answers the first request only now, while the
        // second is pending, and before the new owner answers it.
        settle(&mut old_owner);
        let early = pastes_within(&mut paster, Duration::from_millis(200));
        assert!(early.is_empty(), "the late answer was taken: {early:?}");
        assert_eq!(
            paste_in(&mut [&mut new_owner, &mut paster], 1),
            [(Selection::Clipboard, Some("new".to_string()))]
        );
    }

    #[test]
    fn a_flood_of_events_holds_neither_a_wait_nor_a_wake() {
        let test = "a_flood_of_events_holds_neither_a_wait_nor_a_wake";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut window = open_shown(test);
        let meddler = open_shown("meddler");
        // Hundreds of thousands of changes of a property of the window,
        // each an event it must take, though none is reported.
        let noise = meddler.connection.intern(c"TIDEPOOL_TEST_NOISE");
        for _ in 0..300_000 {
            meddler
                .connection
                .set_bytes(window.window, noise, xlib::XA_STRING, b"x");
        }
        meddler.connection.sync();
        // Every one of them in Xlib's queue before the wait: the server
        // sends them all before the reply to this round trip, however
        // slowly it writes them under load.
        window.connection.sync();
        window.waker().wake();
        let before = Instant::now();
        let mut events = Vec::new();
        window
            .wait_events(None, &mut events)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(events.contains(&Event::Woken), "{events:?}");
        // The wait saw the wake before it had taken every event: the rest
        // wait in Xlib's queue for the next turn. (Xlib itself reads all
        // that the server has sent in one go, before a turn begins, which
        // takes most of the time here, so the time alone tells little.)
        assert!(
            window.connection.pending() > 0,
            "the wait took every event before it saw the wake"
        );
        let took = before.elapsed();
        assert!(took < Duration::from_secs(1), "the wake took {took:?}");
        // A deadline that passed already ends a wait with events left.
        let _ = pastes_within(&mut window, Duration::ZERO);
        assert!(
            window.connection.pending() > 0,
            "a wait past its deadline took every event"
        );
        // The next waits take the rest.
        settle(&mut window);
    }

    #[test]
    fn a_copy_behind_a_flood_of_events_holds_the_selection_when_it_returns() {
        let test = "a_copy_behind_a_flood_of_events_holds_the_selection_when_it_returns";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut owner = open_shown("owner");
        let meddler = open_shown("meddler");
        let mut paster = open_shown("paster");
        let noise = meddler.connection.intern(c"TIDEPOOL_TEST_NOISE");
        for _ in 0..300_000 {
            meddler
                .connection
                .set_bytes(owner.window, noise, xlib::XA_STRING, b"x");
        }
        meddler.connection.sync();
        owner.set_selection(Selection::Clipboard, "fresh");
        assert!(
            owner.clipboard.owns(Selection::Clipboard),
            "the copy returned before it held the selection"
        );
        paster.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut owner, &mut paster], 1),
            [(Selection::Clipboard, Some("fresh".to_string()))]
        );
    }

    #[test]
    fn a_stale_write_of_the_time_stamp_property_is_not_taken_for_a_copys_time() {
        let test = "a_stale_write_of_the_time_stamp_property_is_not_taken_for_a_copys_time";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut owner = open_shown("owner");
        let meddler = open_shown("meddler");
        let mut paster = open_shown("paster");
        let stamp = owner.clipboard.stamp_property();
        let clipboard = meddler.connection.intern(c"CLIPBOARD");
        // The meddler writes the owner's time stamp property, an event the
        // owner has not read yet, and then takes the clipboard, later.
        meddler
            .connection
            .touch_property(owner.window, stamp, xlib::XA_INTEGER);
        meddler.connection.sync();
        thread::sleep(Duration::from_millis(5));
        meddler
            .connection
            .set_selection_owner(clipboard, meddler.window, xlib::CurrentTime);
        meddler.connection.sync();
        thread::sleep(Duration::from_millis(5));
        // The owner's copy, later still, takes the time of its own write.
        owner.set_selection(Selection::Clipboard, "mine");
        assert!(
            owner.clipboard.owns(Selection::Clipboard),
            "the copy took the time of the meddler's earlier write"
        );
        paster.request_selection(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut owner, &mut paster], 1),
            [(Selection::Clipboard, Some("mine".to_string()))]
        );
    }

    #[test]
    fn requests_refused_for_the_transfer_cap_spend_none_of_the_budget() {
        let test = "requests_refused_for_the_transfer_cap_spend_none_of_the_budget";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut owner = open_shown("owner");
        let asker = open_shown("asker");
        let text = long_text(owner.clipboard.part() + 1);
        // Room for 16 transfers and four refused ones, and not one byte
        // more.
        owner.clipboard.serve_limit_for_test(text.len() * 20);
        owner.set_selection(Selection::Clipboard, &text);
        owner.set_selection(Selection::Primary, "p");
        let atom = |name: &str| {
            let name = std::ffi::CString::new(name).expect("a name");
            asker.connection.intern(&name)
        };
        let (clipboard, utf8_string) = (atom("CLIPBOARD"), atom("UTF8_STRING"));
        // Sixteen transfers that the asker never takes a part of, and four
        // requests more, which the cap refuses.
        for index in 0..20 {
            asker.connection.convert_selection(
                clipboard,
                utf8_string,
                atom(&format!("TIDEPOOL_TEST_{index}")),
                asker.window,
                xlib::CurrentTime,
            );
        }
        let primary_into = atom("TIDEPOOL_TEST_PRIMARY");
        asker.connection.convert_selection(
            xlib::XA_PRIMARY,
            utf8_string,
            primary_into,
            asker.window,
            xlib::CurrentTime,
        );
        asker.connection.flush();
        let mut answers = Vec::new();
        let limit = Instant::now() + PATIENCE;
        while answers.len() < 21 {
            assert!(Instant::now() < limit, "{} answers", answers.len());
            let _ = pastes_within(&mut owner, Duration::from_millis(20));
            while asker.connection.pending() > 0 {
                let event = asker.connection.next_event();
                if let Incoming::Selection(SelectionEvent::Notify {
                    selection,
                    property,
                    ..
                }) = event.decode()
                {
                    answers.push((selection, property));
                }
            }
        }
        let refused = answers
            .iter()
            .filter(|&&(selection, property)| selection == clipboard && property == 0)
            .count();
        assert_eq!(refused, 4, "{answers:?}");
        assert!(
            answers.contains(&(xlib::XA_PRIMARY, primary_into)),
            "the primary selection was refused: {answers:?}"
        );
    }

    #[test]
    fn a_resting_answer_slot_is_skipped_when_its_turn_comes() {
        let test = "a_resting_answer_slot_is_skipped_when_its_turn_comes";
        let Some(_serial) = x11(test) else {
            return;
        };
        let mut old_owner = open_shown("old owner");
        let mut new_owner = open_shown("new owner");
        let mut paster = open_shown("paster");
        old_owner.set_selection(Selection::Clipboard, "old");
        paster
            .clipboard
            .limit_for_test(MAX_PASTE_BYTES, Duration::from_millis(300));
        paster.request_selection(Selection::Clipboard);
        let first = paster.clipboard.pending_property(Selection::Clipboard);
        assert_eq!(
            paste_in(&mut [&mut paster], 0),
            [(Selection::Clipboard, None)]
        );
        new_owner.set_selection(Selection::Clipboard, "new");
        paster
            .clipboard
            .limit_for_test(MAX_PASTE_BYTES, TIMEOUT_FOR_TESTS);
        // Three pastes, in the other three slots, so that the next turn is
        // the first slot's, which rests.
        for _ in 0..3 {
            paster.request_selection(Selection::Clipboard);
            assert_ne!(
                paster.clipboard.pending_property(Selection::Clipboard),
                first
            );
            assert_eq!(
                paste_in(&mut [&mut new_owner, &mut paster], 1),
                [(Selection::Clipboard, Some("new".to_string()))]
            );
        }
        paster.request_selection(Selection::Clipboard);
        assert_ne!(
            paster.clipboard.pending_property(Selection::Clipboard),
            first,
            "the resting slot was taken"
        );
        // The old owner answers the first request only now.
        settle(&mut old_owner);
        let early = pastes_within(&mut paster, Duration::from_millis(200));
        assert!(early.is_empty(), "the late answer was taken: {early:?}");
        assert_eq!(
            paste_in(&mut [&mut new_owner, &mut paster], 1),
            [(Selection::Clipboard, Some("new".to_string()))]
        );
    }
}
