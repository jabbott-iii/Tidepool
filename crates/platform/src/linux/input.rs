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

//! From X events to [`Event`]s.
//!
//! [`Incoming::decode`] reads an X event out of Xlib's union, and
//! [`Translator`] turns it into this crate's events. The translator needs no
//! X server, so its rules are tested here. What a key press typed is looked
//! up by the window, which hands it to [`push_typed`].
//!
//! Ported in part from nadir-platform `linux/x11/input.rs`: `Incoming` and
//! its decoding, the extra buttons, [`focus_change`], the close message,
//! the held keys and the shortcut rule of [`push_typed`]. Adapted for
//! Tidepool: events are `tidepool_core::input::InputEvent`s, with logical
//! positions and the modifiers held; keys carry their modifiers
//! ([`modifiers`]), wheel steps are lines with positive `y` down, and side
//! buttons are buttons; consecutive motions are reported as the last of
//! them; exposures are reported; buttons are passed on as they come,
//! without Nadir's record of which are down; and Nadir's handling of the
//! copies an input method server sends back is left out, since Tidepool
//! opens only Xlib's own input method (ADR-0039), which sends none.
//!
//! One case can leave a key held. While a window manager holds a keyboard
//! grab, for a shortcut of its own, the window gets no key events. A key
//! that goes up during the grab is seen as held until it goes up again or
//! the window loses the focus, and its next press is a repeat.

use core::ffi::{c_int, c_long, c_uint};

use tidepool_core::geom::{DevicePoint, DeviceRect, DeviceSize, Scale};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton, ScrollDelta};
// The constants are used as `xlib::KeyPress`, not imported by name: in a
// pattern, a bare name that is not upper case reads as a new variable.
use tidepool_sys::linux::x11::{self as xlib, Atom, XEvent};

use super::keymap;
use super::selection::{Request, SelectionEvent};
use crate::events::Event;

/// The X button numbers above [`xlib::Button5`], which `X11/X.h` does not
/// name. X.Org's input drivers, XWayland and Xvfb use them so by
/// convention.
mod extra_buttons {
    use core::ffi::c_uint;

    /// One step of the wheel to the left.
    pub(super) const WHEEL_LEFT: c_uint = 6;
    /// One step of the wheel to the right.
    pub(super) const WHEEL_RIGHT: c_uint = 7;
    /// The first side button.
    pub(super) const BACK: c_uint = 8;
    /// The second side button.
    pub(super) const FORWARD: c_uint = 9;
}

/// An X event, reduced to the kinds and fields we use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Incoming {
    /// A key went down or up, with the modifiers `state` held before it. A
    /// key press of keycode 0 is no key, but text that Xlib's input method
    /// composed.
    Key {
        pressed: bool,
        keycode: c_uint,
        state: c_uint,
    },
    /// A pointer button went down or up, with the pointer at (`x`, `y`) in
    /// device pixels.
    Button {
        pressed: bool,
        button: c_uint,
        x: c_int,
        y: c_int,
        state: c_uint,
    },
    /// The pointer moved to (`x`, `y`).
    Motion { x: c_int, y: c_int, state: c_uint },
    /// The keyboard focus came or went. `mode` says why.
    Focus { gained: bool, mode: c_int },
    /// A rectangle of the window must be drawn again.
    Expose {
        x: c_int,
        y: c_int,
        width: c_int,
        height: c_int,
    },
    /// The window has this size now, or moved.
    Configure { width: c_int, height: c_int },
    /// A message from another client, of format 32, the only one we read.
    /// `first` is the first item of its data.
    ClientMessage { message_type: Atom, first: c_long },
    /// A property or selection event, for the clipboard.
    Selection(SelectionEvent),
    /// An event we have no use for, such as `MapNotify`.
    Other,
}

impl Incoming {
    /// Reads an event out of the union. A client message of a format other
    /// than 32 is [`Incoming::Other`], and so is any type not listed here.
    ///
    /// # Safety
    ///
    /// `type_` must be initialized, and so must every field this reads for
    /// that type:
    ///
    /// - for [`KeyPress`](xlib::KeyPress), [`KeyRelease`](xlib::KeyRelease),
    ///   [`ButtonPress`](xlib::ButtonPress),
    ///   [`ButtonRelease`](xlib::ButtonRelease),
    ///   [`MotionNotify`](xlib::MotionNotify), [`FocusIn`](xlib::FocusIn),
    ///   [`FocusOut`](xlib::FocusOut), [`Expose`](xlib::Expose),
    ///   [`ConfigureNotify`](xlib::ConfigureNotify),
    ///   [`PropertyNotify`](xlib::PropertyNotify),
    ///   [`SelectionClear`](xlib::SelectionClear),
    ///   [`SelectionRequest`](xlib::SelectionRequest) and
    ///   [`SelectionNotify`](xlib::SelectionNotify), every field of the
    ///   member the type selects;
    /// - for [`ClientMessage`](xlib::ClientMessage), `xclient.format`, and,
    ///   when that is 32, `xclient.message_type` and all five `long`s of
    ///   `xclient.data`.
    ///
    /// An event that Xlib read from the server is like that, as is a copy of
    /// one that an input method put back. libX11's `_XWireToEvent` writes
    /// the fields every event starts with (`type`, `serial`, `send_event`,
    /// `display`), every field of the member for the types in the first
    /// list, and, for a client message, `window` and `format` always, but
    /// `message_type` and the data only for formats 8, 16 and 32, and only
    /// 20 bytes of the data for 8 and 16. It zeroes nothing: the rest of the
    /// 192 bytes is whatever Xlib's queue held there, which may be
    /// uninitialized memory, and is never read here.
    pub(super) unsafe fn decode(event: &XEvent) -> Self {
        // SAFETY: the caller promises that the type is initialized. Every
        // member starts with it.
        let type_ = unsafe { event.type_ };
        match type_ {
            xlib::KeyPress | xlib::KeyRelease => {
                // SAFETY: the type selects `xkey`, every field of which the
                // caller promises is initialized.
                let key = unsafe { event.xkey };
                Self::Key {
                    pressed: type_ == xlib::KeyPress,
                    keycode: key.keycode,
                    state: key.state,
                }
            }
            xlib::ButtonPress | xlib::ButtonRelease => {
                // SAFETY: the type selects `xbutton`, every field of which the
                // caller promises is initialized.
                let button = unsafe { event.xbutton };
                Self::Button {
                    pressed: type_ == xlib::ButtonPress,
                    button: button.button,
                    x: button.x,
                    y: button.y,
                    state: button.state,
                }
            }
            xlib::MotionNotify => {
                // SAFETY: the type selects `xmotion`, every field of which the
                // caller promises is initialized.
                let motion = unsafe { event.xmotion };
                Self::Motion {
                    x: motion.x,
                    y: motion.y,
                    state: motion.state,
                }
            }
            xlib::FocusIn | xlib::FocusOut => {
                // SAFETY: the type selects `xfocus`, every field of which the
                // caller promises is initialized.
                let focus = unsafe { event.xfocus };
                Self::Focus {
                    gained: type_ == xlib::FocusIn,
                    mode: focus.mode,
                }
            }
            xlib::Expose => {
                // SAFETY: the type selects `xexpose`, every field of which the
                // caller promises is initialized.
                let expose = unsafe { event.xexpose };
                Self::Expose {
                    x: expose.x,
                    y: expose.y,
                    width: expose.width,
                    height: expose.height,
                }
            }
            xlib::ConfigureNotify => {
                // SAFETY: the type selects `xconfigure`, every field of which the
                // caller promises is initialized.
                let configure = unsafe { event.xconfigure };
                Self::Configure {
                    width: configure.width,
                    height: configure.height,
                }
            }
            xlib::ClientMessage => {
                // Field by field, never the whole member: what Xlib leaves
                // of it depends on the format.
                //
                // SAFETY: the type selects `xclient`, whose `format` the
                // caller promises is initialized. Only that field is read.
                let format = unsafe { event.xclient.format };
                if format != 32 {
                    return Self::Other;
                }
                // SAFETY: for format 32 the caller promises that
                // `message_type` is initialized. Only that field is read.
                let message_type = unsafe { event.xclient.message_type };
                // SAFETY: for format 32 the caller promises that all five
                // `long`s of the data are initialized. Only the first is
                // read.
                let first = unsafe { event.xclient.data.l[0] };
                Self::ClientMessage {
                    message_type,
                    first,
                }
            }
            xlib::PropertyNotify => {
                // SAFETY: the type selects `xproperty`, every field of which
                // the caller promises is initialized.
                let property = unsafe { event.xproperty };
                Self::Selection(SelectionEvent::Property {
                    serial: property.serial,
                    window: property.window,
                    atom: property.atom,
                    time: property.time,
                    deleted: property.state == xlib::PropertyDelete,
                    sent: property.send_event != xlib::False,
                })
            }
            xlib::SelectionClear => {
                // SAFETY: the type selects `xselectionclear`, every field of
                // which the caller promises is initialized.
                let clear = unsafe { event.xselectionclear };
                Self::Selection(SelectionEvent::Clear {
                    window: clear.window,
                    selection: clear.selection,
                    time: clear.time,
                    sent: clear.send_event != xlib::False,
                })
            }
            xlib::SelectionRequest => {
                // SAFETY: the type selects `xselectionrequest`, every field
                // of which the caller promises is initialized.
                let request = unsafe { event.xselectionrequest };
                Self::Selection(SelectionEvent::Request(Request {
                    owner: request.owner,
                    requestor: request.requestor,
                    selection: request.selection,
                    target: request.target,
                    property: request.property,
                    time: request.time,
                    sent: request.send_event != xlib::False,
                }))
            }
            xlib::SelectionNotify => {
                // SAFETY: the type selects `xselection`, every field of which
                // the caller promises is initialized.
                let notify = unsafe { event.xselection };
                Self::Selection(SelectionEvent::Notify {
                    requestor: notify.requestor,
                    selection: notify.selection,
                    target: notify.target,
                    property: notify.property,
                })
            }
            _ => Self::Other,
        }
    }
}

/// What an X button number means.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pointer {
    /// A button.
    Button(MouseButton),
    /// One step of the wheel, in lines: positive `y` is down, toward the
    /// end of the content, and positive `x` is right.
    Wheel { x: f32, y: f32 },
}

/// Returns what an X button number means, if anything. The wheel is four
/// "buttons", each pressed and released at once for one step.
fn pointer(button: c_uint) -> Option<Pointer> {
    Some(match button {
        xlib::Button1 => Pointer::Button(MouseButton::Left),
        xlib::Button2 => Pointer::Button(MouseButton::Middle),
        xlib::Button3 => Pointer::Button(MouseButton::Right),
        // Away from the user: up.
        xlib::Button4 => Pointer::Wheel { x: 0.0, y: -1.0 },
        // Toward the user: down.
        xlib::Button5 => Pointer::Wheel { x: 0.0, y: 1.0 },
        extra_buttons::WHEEL_LEFT => Pointer::Wheel { x: -1.0, y: 0.0 },
        extra_buttons::WHEEL_RIGHT => Pointer::Wheel { x: 1.0, y: 0.0 },
        extra_buttons::BACK => Pointer::Button(MouseButton::Back),
        extra_buttons::FORWARD => Pointer::Button(MouseButton::Forward),
        _ => return None,
    })
}

/// The modifiers that the `state` of a key, button or motion event says are
/// held: Shift, Control, the first extra modifier as Alt and the fourth as
/// Super, as the usual keyboard map assigns them. Caps Lock, Num Lock and
/// the fifth modifier, which most layouts give to Alt Gr, are not
/// modifiers here.
pub(super) fn modifiers(state: c_uint) -> Modifiers {
    let mut modifiers = Modifiers::NONE;
    modifiers.set(Modifiers::SHIFT, state & xlib::ShiftMask != 0);
    modifiers.set(Modifiers::CTRL, state & xlib::ControlMask != 0);
    modifiers.set(Modifiers::ALT, state & xlib::Mod1Mask != 0);
    modifiers.set(Modifiers::SUPER, state & xlib::Mod4Mask != 0);
    modifiers
}

/// The modifier a key is, if it is one, and the key on the other side of
/// the keyboard that is the same modifier. Right Alt counts as Alt, also
/// where the layout makes it Alt Gr.
fn modifier_of(key: Key) -> Option<(Modifiers, Key)> {
    Some(match key {
        Key::LeftShift => (Modifiers::SHIFT, Key::RightShift),
        Key::RightShift => (Modifiers::SHIFT, Key::LeftShift),
        Key::LeftControl => (Modifiers::CTRL, Key::RightControl),
        Key::RightControl => (Modifiers::CTRL, Key::LeftControl),
        Key::LeftAlt => (Modifiers::ALT, Key::RightAlt),
        Key::RightAlt => (Modifiers::ALT, Key::LeftAlt),
        Key::LeftSuper => (Modifiers::SUPER, Key::RightSuper),
        Key::RightSuper => (Modifiers::SUPER, Key::LeftSuper),
        _ => return None,
    })
}

/// Appends what a key press typed, `typed`, which the window looked up
/// after the press, whose modifiers were `state`.
///
/// Nothing is typed with Control, Alt or Super held: the key is a
/// shortcut. The state is the press's own, so a modifier that went down
/// before the window had the focus counts too. Alt Gr is another modifier
/// on X (the fifth, for the third level of a key), and types. Control
/// characters, which Xlib gives for Enter, Tab, Backspace, Escape and
/// Delete, are left out: those are keys, not text.
pub(super) fn push_typed(typed: &str, state: c_uint, events: &mut Vec<Event>) {
    let shortcut = state & (xlib::ControlMask | xlib::Mod1Mask | xlib::Mod4Mask) != 0;
    if shortcut {
        return;
    }
    let text: String = typed.chars().filter(|c| !c.is_control()).collect();
    if !text.is_empty() {
        events.push(Event::Input(InputEvent::Text(text)));
    }
}

/// Whether `incoming` gives or takes the keyboard focus: `None` for any
/// other event, and for a focus event that is no change.
///
/// A keyboard grab, such as a window manager takes for a shortcut or while
/// it switches windows, moves the focus away and back. The window is still
/// the one the user works in, so this is not a change of focus. If another
/// window gets the focus while the grab lasts, a normal event says so.
pub(super) fn focus_change(incoming: Incoming) -> Option<bool> {
    match incoming {
        Incoming::Focus { mode, .. } if mode == xlib::NotifyGrab || mode == xlib::NotifyUngrab => {
            None
        }
        Incoming::Focus { gained, .. } => Some(gained),
        _ => None,
    }
}

/// The atoms that mark the window manager's "close" message.
#[derive(Clone, Copy, Debug)]
pub(super) struct CloseMessage {
    /// `WM_PROTOCOLS`: the type of the message.
    pub(super) wm_protocols: Atom,
    /// `WM_DELETE_WINDOW`: the first item of its data.
    pub(super) wm_delete_window: Atom,
}

/// Turns X events into [`Event`]s, and remembers what that takes: which
/// keys are down, whether the window has the focus, its size and its
/// scale.
pub(super) struct Translator {
    close: CloseMessage,
    scale: Scale,
    size: DeviceSize,
    focused: bool,
    /// Which keys are down, by [`Key::usage`].
    held: [bool; 256],
}

impl Translator {
    /// A translator for a window of `size` device pixels, at `scale`, that
    /// has seen no events yet.
    pub(super) fn new(close: CloseMessage, size: DeviceSize, scale: Scale) -> Self {
        Self {
            close,
            scale,
            size,
            focused: false,
            held: [false; 256],
        }
    }

    /// The size of the inside of the window, in device pixels.
    pub(super) fn size(&self) -> DeviceSize {
        self.size
    }

    /// The window's scale.
    pub(super) fn scale(&self) -> Scale {
        self.scale
    }

    /// Whether the window has the keyboard focus.
    pub(super) fn is_focused(&self) -> bool {
        self.focused
    }

    /// Appends the events that `incoming` amounts to, which may be none.
    /// `events` from `start` on are those of this wait, which a motion may
    /// replace; the ones before are the caller's.
    pub(super) fn handle(&mut self, incoming: Incoming, events: &mut Vec<Event>, start: usize) {
        match incoming {
            Incoming::Key {
                pressed,
                keycode,
                state,
            } => self.key(pressed, keycode, state, events),
            Incoming::Button {
                pressed,
                button,
                x,
                y,
                state,
            } => self.button(pressed, button, self.position(x, y), state, events),
            Incoming::Motion { x, y, state } => {
                let moved = Event::Input(InputEvent::MouseMove {
                    position: self.position(x, y),
                    modifiers: modifiers(state),
                });
                let ours = events.len() > start;
                if ours
                    && let Some(last @ Event::Input(InputEvent::MouseMove { .. })) =
                        events.last_mut()
                {
                    // Only the last of consecutive motions is reported.
                    *last = moved;
                } else {
                    events.push(moved);
                }
            }
            Incoming::Focus { .. } => {
                if let Some(gained) = focus_change(incoming) {
                    self.focus(gained, events);
                }
            }
            Incoming::Expose {
                x,
                y,
                width,
                height,
            } => {
                let rect = DeviceRect::new(x, y, width, height);
                if !rect.is_empty() {
                    events.push(Event::Exposed(rect));
                }
            }
            Incoming::Configure { width, height } => {
                let size = DeviceSize::new(width, height);
                // The same event also reports moves, which do not change
                // the size.
                if width > 0 && height > 0 && size != self.size {
                    self.size = size;
                    events.push(Event::Resized(size));
                }
            }
            Incoming::ClientMessage {
                message_type,
                first,
            } => {
                // An atom is an `unsigned long` that travels as a `long`.
                let is_close = message_type == self.close.wm_protocols
                    && first as Atom == self.close.wm_delete_window;
                if is_close {
                    events.push(Event::CloseRequested);
                }
            }
            // The window gives these to its clipboard.
            Incoming::Selection(_) | Incoming::Other => {}
        }
    }

    /// A position in the window, from device pixels to logical units.
    fn position(&self, x: c_int, y: c_int) -> tidepool_core::geom::Point {
        self.scale.to_logical(DevicePoint::new(x, y))
    }

    fn key(&mut self, pressed: bool, keycode: c_uint, state: c_uint, events: &mut Vec<Event>) {
        let Some(key) = keymap::from_x_keycode(keycode) else {
            return;
        };
        let index = usize::from(key.usage());
        let was_held = self.held[index];
        // A release of a key we never saw go down, which happens when the
        // key was pressed while another window had the focus, is not
        // reported: every release follows a press.
        if !pressed && !was_held {
            return;
        }
        self.held[index] = pressed;
        // The state is from before the event: a modifier's own press is not
        // in it yet, and its own release still is.
        let mut held = modifiers(state);
        if let Some((modifier, twin)) = modifier_of(key) {
            held.set(modifier, pressed || self.held[usize::from(twin.usage())]);
        }
        events.push(Event::Input(if pressed {
            // With detectable auto-repeat, a held key repeats as presses
            // only.
            InputEvent::KeyDown {
                key,
                modifiers: held,
                repeat: was_held,
            }
        } else {
            InputEvent::KeyUp {
                key,
                modifiers: held,
            }
        }));
    }

    fn button(
        &mut self,
        pressed: bool,
        button: c_uint,
        position: tidepool_core::geom::Point,
        state: c_uint,
        events: &mut Vec<Event>,
    ) {
        let modifiers = modifiers(state);
        match pointer(button) {
            Some(Pointer::Button(button)) => events.push(Event::Input(if pressed {
                InputEvent::MouseDown {
                    button,
                    position,
                    modifiers,
                }
            } else {
                InputEvent::MouseUp {
                    button,
                    position,
                    modifiers,
                }
            })),
            // The release that follows each step at once says nothing new.
            Some(Pointer::Wheel { x, y }) if pressed => {
                events.push(Event::Input(InputEvent::Wheel {
                    delta: ScrollDelta::Lines { x, y },
                    position,
                    modifiers,
                }))
            }
            Some(Pointer::Wheel { .. }) | None => {}
        }
    }

    fn focus(&mut self, gained: bool, events: &mut Vec<Event>) {
        if gained == self.focused {
            return;
        }
        self.focused = gained;
        if gained {
            events.push(Event::Input(InputEvent::FocusGained));
            return;
        }
        events.push(Event::Input(InputEvent::FocusLost));
        // After the news, so that a reader can tell these releases from the
        // ones the user made. Without the focus we will not see them go up.
        for &key in Key::ALL {
            let index = usize::from(key.usage());
            if self.held[index] {
                self.held[index] = false;
                events.push(Event::Input(InputEvent::KeyUp {
                    key,
                    modifiers: self.held_modifiers(),
                }));
            }
        }
    }

    /// The modifiers whose keys are down, as far as this translator saw.
    fn held_modifiers(&self) -> Modifiers {
        let mut held = Modifiers::NONE;
        for &key in Key::ALL {
            if self.held[usize::from(key.usage())]
                && let Some((modifier, _)) = modifier_of(key)
            {
                held.insert(modifier);
            }
        }
        held
    }
}

#[cfg(test)]
mod tests {
    use tidepool_core::geom::Point;
    use tidepool_sys::linux::x11::{
        Bool, ButtonPress, ButtonRelease, ClientMessage, ConfigureNotify, ControlMask, Expose,
        False, FocusIn, FocusOut, KeyPress, KeyRelease, Mod1Mask, Mod4Mask, MotionNotify,
        NotifyGrab, NotifyUngrab, PropertyDelete, PropertyNotify, SelectionClear, SelectionNotify,
        SelectionRequest, ShiftMask, XButtonEvent, XClientMessageData, XClientMessageEvent,
        XConfigureEvent, XExposeEvent, XFocusChangeEvent, XKeyEvent, XMotionEvent, XPropertyEvent,
        XSelectionClearEvent, XSelectionEvent, XSelectionRequestEvent,
    };

    use super::*;

    const WM_PROTOCOLS: Atom = 300;
    const WM_DELETE_WINDOW: Atom = 301;
    /// `X11/X.h`: `#define NotifyNormal 0`
    const NOTIFY_NORMAL: c_int = 0;
    /// `X11/X.h`: `#define NotifyWhileGrabbed 3`
    const NOTIFY_WHILE_GRABBED: c_int = 3;
    /// `X11/X.h`: `#define Mod5Mask (1<<7)`, which most layouts give to
    /// Alt Gr.
    const MOD5_MASK: c_uint = 1 << 7;
    /// `X11/X.h`: `#define LockMask (1<<1)`: Caps Lock.
    const LOCK_MASK: c_uint = 1 << 1;

    // X keycodes: the evdev code plus 8.
    const KEYCODE_W: c_uint = 17 + 8;
    const KEYCODE_A: c_uint = 30 + 8;
    const KEYCODE_LEFT_SHIFT: c_uint = 42 + 8;
    const KEYCODE_RIGHT_SHIFT: c_uint = 54 + 8;
    const KEYCODE_LEFT_CONTROL: c_uint = 29 + 8;
    const KEYCODE_MUTE: c_uint = 113 + 8;

    fn translator(scale: f32) -> Translator {
        let close = CloseMessage {
            wm_protocols: WM_PROTOCOLS,
            wm_delete_window: WM_DELETE_WINDOW,
        };
        Translator::new(
            close,
            DeviceSize::new(640, 360),
            Scale::new(scale).expect("a valid scale"),
        )
    }

    /// Feeds the events to a translator, as one wait, and returns what
    /// comes out.
    fn run(translator: &mut Translator, incoming: &[Incoming]) -> Vec<Event> {
        let mut events = Vec::new();
        for &event in incoming {
            translator.handle(event, &mut events, 0);
        }
        events
    }

    fn key(pressed: bool, keycode: c_uint, state: c_uint) -> Incoming {
        Incoming::Key {
            pressed,
            keycode,
            state,
        }
    }

    fn down(key: Key, modifiers: Modifiers, repeat: bool) -> Event {
        Event::Input(InputEvent::KeyDown {
            key,
            modifiers,
            repeat,
        })
    }

    fn up(key: Key, modifiers: Modifiers) -> Event {
        Event::Input(InputEvent::KeyUp { key, modifiers })
    }

    fn focus(gained: bool, mode: c_int) -> Incoming {
        Incoming::Focus { gained, mode }
    }

    const NONE: Modifiers = Modifiers::NONE;
    const SHIFT: Modifiers = Modifiers::SHIFT;

    // -- keys ---------------------------------------------------------------

    #[test]
    fn a_key_goes_down_and_up() {
        let events = run(
            &mut translator(1.0),
            &[key(true, KEYCODE_W, 0), key(false, KEYCODE_W, 0)],
        );
        assert_eq!(events, [down(Key::W, NONE, false), up(Key::W, NONE)]);
    }

    #[test]
    fn presses_of_a_key_that_is_down_are_repeats() {
        let events = run(
            &mut translator(1.0),
            &[
                key(true, KEYCODE_W, 0),
                key(true, KEYCODE_W, 0),
                key(true, KEYCODE_A, 0),
                key(true, KEYCODE_W, 0),
                key(false, KEYCODE_W, 0),
                key(true, KEYCODE_W, 0),
            ],
        );
        assert_eq!(
            events,
            [
                down(Key::W, NONE, false),
                down(Key::W, NONE, true),
                down(Key::A, NONE, false),
                down(Key::W, NONE, true),
                up(Key::W, NONE),
                // Down again after a release: a new press.
                down(Key::W, NONE, false),
            ]
        );
    }

    #[test]
    fn a_release_without_a_press_and_an_unknown_key_are_dropped() {
        let events = run(
            &mut translator(1.0),
            &[
                key(false, KEYCODE_W, 0),
                key(true, KEYCODE_MUTE, 0),
                key(false, KEYCODE_MUTE, 0),
                // What Xlib's input method composed: no key.
                key(true, 0, 0),
                key(true, 7, 0),
                key(true, 300, 0),
            ],
        );
        assert_eq!(events, []);
    }

    #[test]
    fn keys_carry_the_modifiers_their_state_says() {
        let events = run(
            &mut translator(1.0),
            &[
                key(true, KEYCODE_A, ShiftMask | ControlMask),
                key(false, KEYCODE_A, Mod1Mask | Mod4Mask),
                key(true, KEYCODE_A, LOCK_MASK | MOD5_MASK),
            ],
        );
        assert_eq!(
            events,
            [
                down(Key::A, Modifiers::SHIFT | Modifiers::CTRL, false),
                up(Key::A, Modifiers::ALT | Modifiers::SUPER),
                // Caps Lock and Alt Gr are no modifiers of ours.
                down(Key::A, NONE, false),
            ]
        );
    }

    #[test]
    fn a_modifier_key_is_its_own_modifier_going_down_and_not_going_up() {
        // X reports the state from before each event.
        let events = run(
            &mut translator(1.0),
            &[
                key(true, KEYCODE_LEFT_SHIFT, 0),
                key(true, KEYCODE_A, ShiftMask),
                key(false, KEYCODE_A, ShiftMask),
                key(false, KEYCODE_LEFT_SHIFT, ShiftMask),
            ],
        );
        assert_eq!(
            events,
            [
                down(Key::LeftShift, SHIFT, false),
                down(Key::A, SHIFT, false),
                up(Key::A, SHIFT),
                up(Key::LeftShift, NONE),
            ]
        );
    }

    #[test]
    fn a_modifier_stays_held_while_its_other_key_is_down() {
        let events = run(
            &mut translator(1.0),
            &[
                key(true, KEYCODE_LEFT_SHIFT, 0),
                key(true, KEYCODE_RIGHT_SHIFT, ShiftMask),
                key(false, KEYCODE_LEFT_SHIFT, ShiftMask),
                key(false, KEYCODE_RIGHT_SHIFT, ShiftMask),
            ],
        );
        assert_eq!(
            events,
            [
                down(Key::LeftShift, SHIFT, false),
                down(Key::RightShift, SHIFT, false),
                up(Key::LeftShift, SHIFT),
                up(Key::RightShift, NONE),
            ]
        );
    }

    #[test]
    fn the_modifier_of_each_modifier_key() {
        let cases = [
            (Key::LeftShift, Modifiers::SHIFT),
            (Key::RightShift, Modifiers::SHIFT),
            (Key::LeftControl, Modifiers::CTRL),
            (Key::RightControl, Modifiers::CTRL),
            (Key::LeftAlt, Modifiers::ALT),
            (Key::RightAlt, Modifiers::ALT),
            (Key::LeftSuper, Modifiers::SUPER),
            (Key::RightSuper, Modifiers::SUPER),
        ];
        for (key, modifier) in cases {
            let (found, twin) = modifier_of(key).expect("a modifier key");
            assert_eq!(found, modifier, "{key:?}");
            assert_eq!(
                modifier_of(twin).map(|(_, back)| back),
                Some(key),
                "{key:?}"
            );
        }
        let others = Key::ALL.iter().filter(|&&key| modifier_of(key).is_some());
        assert_eq!(others.count(), cases.len());
    }

    #[test]
    fn the_state_bits_are_the_four_modifiers() {
        assert_eq!(modifiers(0), NONE);
        assert_eq!(modifiers(ShiftMask), Modifiers::SHIFT);
        assert_eq!(modifiers(ControlMask), Modifiers::CTRL);
        assert_eq!(modifiers(Mod1Mask), Modifiers::ALT);
        assert_eq!(modifiers(Mod4Mask), Modifiers::SUPER);
        assert_eq!(
            modifiers(ShiftMask | ControlMask | Mod1Mask | Mod4Mask),
            Modifiers::ALL
        );
        // Every other bit, buttons included, is no modifier of ours.
        let others = !(ShiftMask | ControlMask | Mod1Mask | Mod4Mask);
        assert_eq!(modifiers(others), NONE);
    }

    // -- typed text -----------------------------------------------------------

    fn typed(text: &str, state: c_uint) -> Vec<Event> {
        let mut events = Vec::new();
        push_typed(text, state, &mut events);
        events
    }

    fn text(text: &str) -> Event {
        Event::Input(InputEvent::Text(text.to_string()))
    }

    #[test]
    fn typed_text_comes_through_without_control_characters() {
        assert_eq!(typed("a", 0), [text("a")]);
        assert_eq!(typed("é", ShiftMask), [text("é")]);
        assert_eq!(typed("ナ", 0), [text("ナ")]);
        // Alt Gr types.
        assert_eq!(typed("€", MOD5_MASK), [text("€")]);
        // Enter, Tab, Backspace, Escape and Delete type nothing.
        for control in ["\r", "\t", "\u{8}", "\u{1b}", "\u{7f}", ""] {
            assert_eq!(typed(control, 0), [], "{control:?}");
        }
        assert_eq!(typed("a\u{0}b", 0), [text("ab")]);
    }

    #[test]
    fn a_shortcut_types_nothing() {
        for state in [ControlMask, Mod1Mask, Mod4Mask, ControlMask | ShiftMask] {
            assert_eq!(typed("s", state), [], "{state:#x}");
        }
    }

    // -- the pointer ----------------------------------------------------------

    fn button(pressed: bool, button: c_uint, state: c_uint) -> Incoming {
        Incoming::Button {
            pressed,
            button,
            x: 30,
            y: 45,
            state,
        }
    }

    #[test]
    fn buttons_and_side_buttons_go_down_and_up_at_logical_positions() {
        let at = Point::new(20.0, 30.0);
        let events = run(
            &mut translator(1.5),
            &[
                button(true, 1, ControlMask),
                button(false, 1, 0),
                button(true, 2, 0),
                button(true, 3, 0),
                button(true, 8, 0),
                button(false, 9, 0),
                button(true, 10, 0),
            ],
        );
        let pressed = |button, modifiers| {
            Event::Input(InputEvent::MouseDown {
                button,
                position: at,
                modifiers,
            })
        };
        let released = |button| {
            Event::Input(InputEvent::MouseUp {
                button,
                position: at,
                modifiers: NONE,
            })
        };
        assert_eq!(
            events,
            [
                pressed(MouseButton::Left, Modifiers::CTRL),
                released(MouseButton::Left),
                pressed(MouseButton::Middle, NONE),
                pressed(MouseButton::Right, NONE),
                pressed(MouseButton::Back, NONE),
                released(MouseButton::Forward),
            ]
        );
    }

    #[test]
    fn wheel_steps_are_lines_with_down_positive() {
        let events = run(
            &mut translator(1.0),
            &[
                button(true, 4, 0),
                button(false, 4, 0),
                button(true, 5, ShiftMask),
                button(false, 5, ShiftMask),
                button(true, 6, 0),
                button(true, 7, 0),
            ],
        );
        let step = |x, y, modifiers| {
            Event::Input(InputEvent::Wheel {
                delta: ScrollDelta::Lines { x, y },
                position: Point::new(30.0, 45.0),
                modifiers,
            })
        };
        assert_eq!(
            events,
            [
                step(0.0, -1.0, NONE),
                step(0.0, 1.0, SHIFT),
                step(-1.0, 0.0, NONE),
                step(1.0, 0.0, NONE),
            ]
        );
    }

    fn motion(x: c_int, y: c_int) -> Incoming {
        Incoming::Motion { x, y, state: 0 }
    }

    fn moved(x: f32, y: f32) -> Event {
        Event::Input(InputEvent::MouseMove {
            position: Point::new(x, y),
            modifiers: NONE,
        })
    }

    #[test]
    fn consecutive_motions_are_reported_as_the_last() {
        let events = run(
            &mut translator(2.0),
            &[
                motion(2, 2),
                motion(4, 4),
                motion(6, 8),
                key(true, KEYCODE_W, 0),
                motion(10, 10),
                Incoming::Other,
                motion(12, 14),
            ],
        );
        assert_eq!(
            events,
            [
                moved(3.0, 4.0),
                down(Key::W, NONE, false),
                // An event that reports nothing does not part motions.
                moved(6.0, 7.0),
            ]
        );
    }

    #[test]
    fn a_motion_does_not_replace_one_from_before_the_wait() {
        let mut translator = translator(1.0);
        let mut events = vec![moved(1.0, 1.0)];
        translator.handle(motion(5, 5), &mut events, 1);
        assert_eq!(events, [moved(1.0, 1.0), moved(5.0, 5.0)]);
    }

    // -- the window -----------------------------------------------------------

    #[test]
    fn focus_comes_and_goes_without_the_noise_of_grabs() {
        let mut translator = translator(1.0);
        let events = run(
            &mut translator,
            &[
                focus(true, NOTIFY_NORMAL),
                focus(true, NOTIFY_NORMAL),
                focus(false, NotifyGrab),
                focus(true, NotifyUngrab),
                focus(false, NOTIFY_WHILE_GRABBED),
            ],
        );
        assert_eq!(
            events,
            [
                Event::Input(InputEvent::FocusGained),
                Event::Input(InputEvent::FocusLost),
            ]
        );
        assert!(!translator.is_focused());
    }

    #[test]
    fn losing_the_focus_releases_every_key_that_is_down() {
        let events = run(
            &mut translator(1.0),
            &[
                focus(true, NOTIFY_NORMAL),
                key(true, KEYCODE_LEFT_CONTROL, 0),
                key(true, KEYCODE_W, ControlMask),
                focus(false, NOTIFY_NORMAL),
                // Seen as down no more.
                key(false, KEYCODE_W, 0),
            ],
        );
        assert_eq!(
            events,
            [
                Event::Input(InputEvent::FocusGained),
                down(Key::LeftControl, Modifiers::CTRL, false),
                down(Key::W, Modifiers::CTRL, false),
                Event::Input(InputEvent::FocusLost),
                // In usage order: W before the left Control, which is still
                // down then.
                up(Key::W, Modifiers::CTRL),
                up(Key::LeftControl, NONE),
            ]
        );
    }

    #[test]
    fn a_new_size_is_reported_once_and_a_move_not_at_all() {
        let mut translator = translator(1.0);
        let configure = |width, height| Incoming::Configure { width, height };
        let events = run(
            &mut translator,
            &[
                configure(640, 360),
                configure(800, 600),
                configure(800, 600),
                configure(0, 600),
                configure(-1, 5),
            ],
        );
        assert_eq!(events, [Event::Resized(DeviceSize::new(800, 600))]);
        assert_eq!(translator.size(), DeviceSize::new(800, 600));
    }

    #[test]
    fn an_exposure_is_reported_in_device_pixels() {
        let expose = |x, y, width, height| Incoming::Expose {
            x,
            y,
            width,
            height,
        };
        let events = run(
            &mut translator(2.0),
            &[expose(0, 0, 640, 360), expose(5, 6, 0, 10)],
        );
        assert_eq!(events, [Event::Exposed(DeviceRect::new(0, 0, 640, 360))]);
    }

    #[test]
    fn only_the_window_managers_delete_message_is_a_close_request() {
        let message = |message_type, first| Incoming::ClientMessage {
            message_type,
            first,
        };
        let delete = WM_DELETE_WINDOW as c_long;
        let events = run(
            &mut translator(1.0),
            &[
                message(WM_PROTOCOLS, delete),
                message(WM_PROTOCOLS, delete + 1),
                message(WM_PROTOCOLS + 5, delete),
            ],
        );
        assert_eq!(events, [Event::CloseRequested]);
    }

    // -- decoding ---------------------------------------------------------------

    /// An event with every byte zero but those `fill` writes.
    fn event(fill: impl FnOnce(&mut XEvent)) -> XEvent {
        let mut event = XEvent { pad: [0; 24] };
        fill(&mut event);
        event
    }

    fn decode(event: &XEvent) -> Incoming {
        // SAFETY: every byte of the event is initialized.
        unsafe { Incoming::decode(event) }
    }

    #[test]
    fn x_events_are_decoded_by_their_type() {
        let same_screen: Bool = 1;
        let key_event = |type_| XKeyEvent {
            type_,
            serial: 0,
            send_event: False,
            display: core::ptr::null_mut(),
            window: 0,
            root: 0,
            subwindow: 0,
            time: 0,
            x: 0,
            y: 0,
            x_root: 0,
            y_root: 0,
            state: ShiftMask,
            keycode: KEYCODE_A,
            same_screen,
        };
        assert_eq!(
            decode(&event(|event| event.xkey = key_event(KeyPress))),
            key(true, KEYCODE_A, ShiftMask)
        );
        assert_eq!(
            decode(&event(|event| event.xkey = key_event(KeyRelease))),
            key(false, KEYCODE_A, ShiftMask)
        );

        let button_event = |type_| XButtonEvent {
            type_,
            serial: 0,
            send_event: False,
            display: core::ptr::null_mut(),
            window: 0,
            root: 0,
            subwindow: 0,
            time: 0,
            x: 30,
            y: 45,
            x_root: 0,
            y_root: 0,
            state: ControlMask,
            button: 3,
            same_screen,
        };
        assert_eq!(
            decode(&event(|event| event.xbutton = button_event(ButtonPress))),
            button(true, 3, ControlMask)
        );
        assert_eq!(
            decode(&event(|event| event.xbutton = button_event(ButtonRelease))),
            button(false, 3, ControlMask)
        );

        let motion_event = XMotionEvent {
            type_: MotionNotify,
            serial: 0,
            send_event: False,
            display: core::ptr::null_mut(),
            window: 0,
            root: 0,
            subwindow: 0,
            time: 0,
            x: 7,
            y: 9,
            x_root: 0,
            y_root: 0,
            state: 0,
            is_hint: 0,
            same_screen,
        };
        assert_eq!(
            decode(&event(|event| event.xmotion = motion_event)),
            motion(7, 9)
        );

        let focus_event = |type_, mode| XFocusChangeEvent {
            type_,
            serial: 0,
            send_event: False,
            display: core::ptr::null_mut(),
            window: 0,
            mode,
            detail: 0,
        };
        assert_eq!(
            decode(&event(
                |event| event.xfocus = focus_event(FocusIn, NotifyGrab)
            )),
            focus(true, NotifyGrab)
        );
        assert_eq!(
            decode(&event(
                |event| event.xfocus = focus_event(FocusOut, NOTIFY_NORMAL)
            )),
            focus(false, NOTIFY_NORMAL)
        );

        let expose_event = XExposeEvent {
            type_: Expose,
            serial: 0,
            send_event: False,
            display: core::ptr::null_mut(),
            window: 0,
            x: 1,
            y: 2,
            width: 3,
            height: 4,
            count: 0,
        };
        assert_eq!(
            decode(&event(|event| event.xexpose = expose_event)),
            Incoming::Expose {
                x: 1,
                y: 2,
                width: 3,
                height: 4
            }
        );

        let configure_event = XConfigureEvent {
            type_: ConfigureNotify,
            serial: 0,
            send_event: False,
            display: core::ptr::null_mut(),
            event: 0,
            window: 0,
            x: 10,
            y: 10,
            width: 800,
            height: 600,
            border_width: 0,
            above: 0,
            override_redirect: False,
        };
        assert_eq!(
            decode(&event(|event| event.xconfigure = configure_event)),
            Incoming::Configure {
                width: 800,
                height: 600
            }
        );

        let client_event = XClientMessageEvent {
            type_: ClientMessage,
            serial: 0,
            send_event: 1,
            display: core::ptr::null_mut(),
            window: 0,
            message_type: WM_PROTOCOLS,
            format: 32,
            data: XClientMessageData {
                l: [WM_DELETE_WINDOW as c_long, 0, 0, 0, 0],
            },
        };
        assert_eq!(
            decode(&event(|event| event.xclient = client_event)),
            Incoming::ClientMessage {
                message_type: WM_PROTOCOLS,
                first: WM_DELETE_WINDOW as c_long
            }
        );
        // Any other format is no message of ours, whatever the rest holds:
        // here bytes as Xlib could leave them, which `decode` never reads.
        let poison = c_long::from_ne_bytes([0xa5; 8]);
        for format in [8, 16, 0, 33, -1] {
            let other = XClientMessageEvent {
                format,
                message_type: poison as Atom,
                data: XClientMessageData { l: [poison; 5] },
                ..client_event
            };
            assert_eq!(
                decode(&event(|event| event.xclient = other)),
                Incoming::Other,
                "format {format}"
            );
        }

        // The property and selection events, for the clipboard.
        let property_event = XPropertyEvent {
            type_: PropertyNotify,
            serial: 41,
            send_event: 1,
            display: core::ptr::null_mut(),
            window: 7,
            atom: 300,
            time: 1234,
            state: PropertyDelete,
        };
        assert_eq!(
            decode(&event(|event| event.xproperty = property_event)),
            Incoming::Selection(SelectionEvent::Property {
                serial: 41,
                window: 7,
                atom: 300,
                time: 1234,
                deleted: true,
                sent: true,
            })
        );
        let new_value = XPropertyEvent {
            state: 0,
            send_event: False,
            ..property_event
        };
        assert!(matches!(
            decode(&event(|event| event.xproperty = new_value)),
            Incoming::Selection(SelectionEvent::Property {
                deleted: false,
                sent: false,
                ..
            })
        ));
        let clear_event = XSelectionClearEvent {
            type_: SelectionClear,
            serial: 0,
            send_event: False,
            display: core::ptr::null_mut(),
            window: 7,
            selection: 1,
            time: 99,
        };
        assert_eq!(
            decode(&event(|event| event.xselectionclear = clear_event)),
            Incoming::Selection(SelectionEvent::Clear {
                window: 7,
                selection: 1,
                time: 99,
                sent: false,
            })
        );
        let request_event = XSelectionRequestEvent {
            type_: SelectionRequest,
            serial: 0,
            send_event: 1,
            display: core::ptr::null_mut(),
            owner: 7,
            requestor: 8,
            selection: 1,
            target: 301,
            property: 302,
            time: 5,
        };
        assert_eq!(
            decode(&event(|event| event.xselectionrequest = request_event)),
            Incoming::Selection(SelectionEvent::Request(Request {
                owner: 7,
                requestor: 8,
                selection: 1,
                target: 301,
                property: 302,
                time: 5,
                sent: true,
            }))
        );
        let notify_event = XSelectionEvent {
            type_: SelectionNotify,
            serial: 0,
            send_event: 1,
            display: core::ptr::null_mut(),
            requestor: 8,
            selection: 1,
            target: 301,
            property: 0,
            time: 5,
        };
        assert_eq!(
            decode(&event(|event| event.xselection = notify_event)),
            Incoming::Selection(SelectionEvent::Notify {
                requestor: 8,
                selection: 1,
                target: 301,
                property: 0,
            })
        );

        // `MapNotify`, which a window gets when it is shown (`X11/X.h`:
        // `#define MapNotify 19`), and nothing at all.
        assert_eq!(decode(&event(|event| event.type_ = 19)), Incoming::Other);
        assert_eq!(decode(&event(|_| {})), Incoming::Other);
    }
}
