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

//! The clipboard of an X11 window: owning the `CLIPBOARD` and `PRIMARY`
//! selections and answering for them, and asking their owners for their
//! text (ADR-0042).
//!
//! New in Tidepool; Nadir has no clipboard. It follows ICCCM 2.0's
//! selections ("Peer-to-Peer Communication by Means of Selections"), but
//! for two things: it refuses `MULTIPLE`, and it asks at `CurrentTime`
//! (ADR-0042 says why). `docs/03-platforms.md` → Clipboard describes it.
//!
//! - **Owning:** [`Clipboard::set`] keeps the text and writes nothing to
//!   `TIDEPOOL_TIMESTAMP`, replacing whatever another client left there;
//!   the [`PropertyNotify`](tidepool_sys::linux::x11::PropertyNotify) of
//!   that write, told by its request number from another client's earlier
//!   one, carries the server's time, at which the window claims the
//!   selection. The window waits for the server so that the claim is made
//!   before `set_selection` returns. It answers each
//!   [`SelectionRequest`](tidepool_sys::linux::x11::SelectionRequest) for
//!   it until a [`SelectionClear`](tidepool_sys::linux::x11::SelectionClear)
//!   says another client took it.
//! - **Sending:** a text longer than one part goes by `INCR`: the window
//!   writes the next part each time the requestor deletes the property,
//!   and ends with a part of no bytes. It watches the requestor's window
//!   for that ([`PropertyChangeMask`]) for as long as a transfer to it
//!   runs. It begins at most [`MAX_SERVED`] bytes of text a second.
//! - **Asking:** [`Clipboard::request`] asks for `UTF8_STRING` into one of
//!   the window's own properties for the selection, a different one each
//!   time, and the
//!   [`SelectionNotify`](tidepool_sys::linux::x11::SelectionNotify) that
//!   names it says the answer is there: the text, or `INCR`, after which
//!   the property's new values are the next parts, for at most a minute
//!   in all. However many writes come between two waits, the property is
//!   read once ([`Clipboard::read_parts`]).
//!
//! Every client of the display can set any property of any window and
//! send any window any event, so nothing read here is trusted: a paste is
//! bounded in size and time, and an event another client sent
//! (`send_event`) never makes the window claim, give up or answer for a
//! selection. Every answer the window writes goes to the window the
//! server named; a window that is gone makes the server refuse the
//! request, which the log reports.

use core::ffi::{c_long, c_ulong};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tidepool_core::log::{debug, warn};
use tidepool_sys::linux::x11::{
    Atom, CurrentTime, NoEventMask, PropertyChangeMask, Time, Window, XA_ATOM, XA_INTEGER,
    XA_PRIMARY,
};

use super::connection::Connection;
use crate::clipboard::{MAX_PASTE_BYTES, Selection};
use crate::events::Event;

/// How long a paste waits for the owner's answer, and for each part after
/// the first, and how long a transfer waits for the requestor to take
/// each part.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The most bytes of text in one part, unless the server takes fewer.
const MAX_PART: usize = 256 * 1024;

/// The size of a `ChangeProperty` request before its data
/// (`X11/Xproto.h`: `sz_xChangePropertyReq`).
const CHANGE_PROPERTY_HEADER: usize = 24;

/// The most `INCR` transfers that run at once; a request for another is
/// refused.
const MAX_TRANSFERS: usize = 16;

/// How long a paste by `INCR` may take in all, in [`TIMEOUT`]s (a
/// minute), however often its parts come: any client can write the next
/// part, and a paste that never ends would keep its selection from
/// pasting, since later requests join it.
pub(super) const RECEIVE_TIMEOUTS: u32 = 12;

/// The answer properties of each selection, which requests take in turn,
/// so that a late answer to one request is never read as the answer to the
/// next.
const ANSWER_SLOTS: usize = 4;

/// The most bytes of text the window begins to send in each
/// [`SERVE_PERIOD`], whole or by `INCR`: a request costs its client 24
/// bytes and the window the whole text, so beyond this a request is
/// refused. The first of each period is always answered.
const MAX_SERVED: usize = MAX_PASTE_BYTES;

/// The period [`MAX_SERVED`] counts in.
const SERVE_PERIOD: Duration = Duration::from_secs(1);

/// A property or selection event, which only the clipboard reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelectionEvent {
    /// A property of `window` changed, or was deleted. `serial` is the
    /// last request of this connection that the server had processed
    /// when it made the event.
    Property {
        serial: c_ulong,
        window: Window,
        atom: Atom,
        time: Time,
        deleted: bool,
        /// Whether another client sent it.
        sent: bool,
    },
    /// `window` no longer owns `selection`, since `time`.
    Clear {
        window: Window,
        selection: Atom,
        time: Time,
        sent: bool,
    },
    /// A client asks the owner of a selection for it.
    Request(Request),
    /// The answer to a request of `requestor`'s: in `property`, or refused
    /// with 0. An owner's answer is always sent; the server's, when no
    /// client owns the selection, is not.
    Notify {
        requestor: Window,
        selection: Atom,
        target: Atom,
        property: Atom,
    },
}

/// A request for a selection's value: `requestor` asks the owner of
/// `selection` for it as `target`, into `property` (0 from an obsolete
/// client), at `time`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Request {
    pub(super) owner: Window,
    pub(super) requestor: Window,
    pub(super) selection: Atom,
    pub(super) target: Atom,
    pub(super) property: Atom,
    pub(super) time: Time,
    /// Whether another client sent it.
    pub(super) sent: bool,
}

/// The atoms the clipboard uses, looked up once for each window.
struct Atoms {
    clipboard: Atom,
    utf8_string: Atom,
    targets: Atom,
    timestamp: Atom,
    incr: Atom,
    /// The window's property that a zero-length write gets the time from.
    stamp: Atom,
    /// The window's properties that each selection's answers come in, by
    /// [`Selection::index`].
    answers: [[Atom; ANSWER_SLOTS]; 2],
}

impl Atoms {
    fn intern(connection: &Connection) -> Self {
        Self {
            clipboard: connection.intern(c"CLIPBOARD"),
            utf8_string: connection.intern(c"UTF8_STRING"),
            targets: connection.intern(c"TARGETS"),
            timestamp: connection.intern(c"TIMESTAMP"),
            incr: connection.intern(c"INCR"),
            stamp: connection.intern(c"TIDEPOOL_TIMESTAMP"),
            answers: [
                [
                    connection.intern(c"TIDEPOOL_CLIPBOARD_0"),
                    connection.intern(c"TIDEPOOL_CLIPBOARD_1"),
                    connection.intern(c"TIDEPOOL_CLIPBOARD_2"),
                    connection.intern(c"TIDEPOOL_CLIPBOARD_3"),
                ],
                [
                    connection.intern(c"TIDEPOOL_PRIMARY_0"),
                    connection.intern(c"TIDEPOOL_PRIMARY_1"),
                    connection.intern(c"TIDEPOOL_PRIMARY_2"),
                    connection.intern(c"TIDEPOOL_PRIMARY_3"),
                ],
            ],
        }
    }

    /// The selection's atom.
    fn of(&self, selection: Selection) -> Atom {
        match selection {
            Selection::Clipboard => self.clipboard,
            Selection::Primary => XA_PRIMARY,
        }
    }

    /// The selection an atom names, if it is one of ours.
    fn selection(&self, atom: Atom) -> Option<Selection> {
        Selection::ALL
            .into_iter()
            .find(|&selection| self.of(selection) == atom)
    }
}

/// A selection the window owns.
struct Owned {
    text: Arc<str>,
    /// The time the window took it at.
    since: Time,
}

/// Where a paste of one selection is.
enum Paste {
    /// None is asked for.
    Idle,
    /// Asked for, with the answer to come in `property`, and waiting for
    /// the owner's answer; or, with a `property` of 0, waiting to ask until
    /// the window holds what it set.
    Waiting { property: Atom, deadline: Instant },
    /// The owner sends the text by `INCR` in `property`: these bytes so
    /// far. The next part is due by `deadline`, which is never later than
    /// `ends`; `announced` says a write announced a part not read yet.
    Receiving {
        property: Atom,
        bytes: Vec<u8>,
        deadline: Instant,
        ends: Instant,
        announced: bool,
    },
}

impl Paste {
    fn deadline(&self) -> Option<Instant> {
        match self {
            Self::Idle => None,
            Self::Waiting { deadline, .. } | Self::Receiving { deadline, .. } => Some(*deadline),
        }
    }

    /// The property the answer comes in, once asked.
    fn property(&self) -> Option<Atom> {
        match self {
            Self::Waiting { property, .. } | Self::Receiving { property, .. } if *property != 0 => {
                Some(*property)
            }
            _ => None,
        }
    }
}

/// An `INCR` transfer of an owned text to a requestor.
struct Transfer {
    requestor: Window,
    property: Atom,
    text: Arc<str>,
    /// How many bytes of the text were written.
    sent: usize,
    deadline: Instant,
}

/// What a test can read of how the clipboard worked.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Counts {
    /// `INCR` transfers this window began as the owner.
    pub(super) sent_incr: usize,
    /// Pastes this window received by `INCR`.
    pub(super) received_incr: usize,
    /// Parts this window wrote by `INCR`, the last empty one included.
    pub(super) parts_sent: usize,
    /// Reads of a part of an `INCR` paste.
    pub(super) parts_read: usize,
}

/// The clipboard of one window.
pub(super) struct Clipboard {
    atoms: Atoms,
    window: Window,
    /// The most bytes of text in one property write.
    part: usize,
    /// The most bytes a paste takes.
    limit: usize,
    timeout: Duration,
    /// The selections the window owns, by [`Selection::index`].
    owned: [Option<Owned>; 2],
    /// The texts waiting for the time to take their selection at.
    claiming: [Option<Arc<str>>; 2],
    /// Whether a zero-length write waits for its `PropertyNotify`.
    stamping: bool,
    /// The request number of the latest such write: the event it causes
    /// has at least this serial, and an earlier event, which another
    /// client's write or an earlier one of ours caused, is no time stamp
    /// of this claim.
    stamp_serial: c_ulong,
    pastes: [Paste; 2],
    /// The answer slot each selection's next request takes first.
    next_slot: [usize; 2],
    /// When a paste last ended without its answer in each slot: a late
    /// answer may still come there, so the slot rests for a while.
    spoiled: [[Option<Instant>; ANSWER_SLOTS]; 2],
    transfers: Vec<Transfer>,
    /// The bytes of text begun in the current period, which began at
    /// `period`, and the most that may be.
    served: usize,
    period: Option<Instant>,
    serve_limit: usize,
    #[cfg(test)]
    pub(super) counts: Counts,
}

impl Clipboard {
    /// The clipboard of `window`, which selects [`PropertyChangeMask`].
    pub(super) fn new(connection: &Connection, window: Window) -> Self {
        let part = MAX_PART
            .min(connection.max_request_data(CHANGE_PROPERTY_HEADER))
            .max(1);
        Self {
            atoms: Atoms::intern(connection),
            window,
            part,
            limit: MAX_PASTE_BYTES,
            timeout: TIMEOUT,
            owned: [None, None],
            claiming: [None, None],
            stamping: false,
            stamp_serial: 0,
            pastes: [Paste::Idle, Paste::Idle],
            next_slot: [0, 0],
            spoiled: [[None; ANSWER_SLOTS]; 2],
            transfers: Vec::new(),
            served: 0,
            period: None,
            serve_limit: MAX_SERVED,
            #[cfg(test)]
            counts: Counts::default(),
        }
    }

    /// Sets the most bytes a paste takes and the timeout, for a test.
    #[cfg(test)]
    pub(super) fn limit_for_test(&mut self, limit: usize, timeout: Duration) {
        self.limit = limit;
        self.timeout = timeout;
    }

    /// The most bytes in one part.
    #[cfg(test)]
    pub(super) fn part(&self) -> usize {
        self.part
    }

    /// Sets the most bytes of text begun each second, for a test.
    #[cfg(test)]
    pub(super) fn serve_limit_for_test(&mut self, limit: usize) {
        self.serve_limit = limit;
    }

    /// Whether the window owns `selection` now, as far as it knows.
    #[cfg(test)]
    pub(super) fn owns(&self, selection: Selection) -> bool {
        self.owned[selection.index()].is_some()
    }

    /// The property the pending paste of `selection` is answered in.
    #[cfg(test)]
    pub(super) fn pending_property(&self, selection: Selection) -> Option<Atom> {
        self.pastes[selection.index()].property()
    }

    /// How many transfers run to `requestor`.
    #[cfg(test)]
    pub(super) fn transfers_to(&self, requestor: Window) -> usize {
        self.transfers
            .iter()
            .filter(|transfer| transfer.requestor == requestor)
            .count()
    }

    /// The window's time stamp property.
    #[cfg(test)]
    pub(super) fn stamp_property(&self) -> Atom {
        self.atoms.stamp
    }

    /// Makes `text` the selection's, and asks the server for the time to
    /// take it at; [`Clipboard::handle`] takes it when the time comes, which
    /// the window waits for.
    pub(super) fn set(&mut self, connection: &Connection, selection: Selection, text: &str) {
        self.claiming[selection.index()] = Some(Arc::from(text));
        // Each copy writes anew, so that its time comes from an event of
        // its own, after any that another client's write left queued.
        self.stamp_serial = connection.next_request();
        connection.touch_property(self.window, self.atoms.stamp, XA_INTEGER);
        self.stamping = true;
    }

    /// Whether a claim waits for the time the server gives it.
    pub(super) fn is_stamping(&self) -> bool {
        self.stamping
    }

    /// Asks for the selection's text, unless that is asked for already;
    /// [`Clipboard::handle`] reports the answer.
    pub(super) fn request(&mut self, connection: &Connection, selection: Selection, now: Instant) {
        let index = selection.index();
        if !matches!(self.pastes[index], Paste::Idle) {
            return;
        }
        let property = if self.claiming[index].is_none() {
            self.ask(connection, selection, now)
        } else {
            0
        };
        self.pastes[index] = Paste::Waiting {
            property,
            deadline: now + self.timeout,
        };
    }

    /// Sends the request for the selection's text, into the next answer
    /// slot that no late answer may still come to, and returns its
    /// property.
    fn ask(&mut self, connection: &Connection, selection: Selection, now: Instant) -> Atom {
        let index = selection.index();
        let rest = self.timeout * RECEIVE_TIMEOUTS;
        let spoiled = &self.spoiled[index];
        let fresh = (0..ANSWER_SLOTS)
            .map(|step| (self.next_slot[index] + step) % ANSWER_SLOTS)
            .find(|&slot| spoiled[slot].is_none_or(|at| now.duration_since(at) >= rest));
        // Every slot rests: the one that has rested longest.
        let slot = fresh.unwrap_or_else(|| {
            (0..ANSWER_SLOTS)
                .min_by_key(|&slot| spoiled[slot])
                .unwrap_or(0)
        });
        self.spoiled[index][slot] = None;
        self.next_slot[index] = (slot + 1) % ANSWER_SLOTS;
        let property = self.atoms.answers[index][slot];
        connection.convert_selection(
            self.atoms.of(selection),
            self.atoms.utf8_string,
            property,
            self.window,
            CurrentTime,
        );
        property
    }

    /// The earliest time something here times out, if anything can.
    pub(super) fn deadline(&self) -> Option<Instant> {
        self.pastes
            .iter()
            .filter_map(Paste::deadline)
            .chain(self.transfers.iter().map(|transfer| transfer.deadline))
            .min()
    }

    /// Ends every paste and transfer whose time is up: a paste with no
    /// text.
    pub(super) fn expire(
        &mut self,
        connection: &Connection,
        now: Instant,
        events: &mut Vec<Event>,
    ) {
        for selection in Selection::ALL {
            if self.pastes[selection.index()]
                .deadline()
                .is_some_and(|deadline| deadline <= now)
            {
                debug!("no answer came for the {selection:?} selection in time: nothing pastes");
                self.finish(selection, None, events);
            }
        }
        let mut index = 0;
        while let Some(transfer) = self.transfers.get(index) {
            if transfer.deadline <= now {
                debug!(
                    "window {:#x} took no part of its paste in time: the transfer ends",
                    transfer.requestor
                );
                self.end_transfer(connection, index);
            } else {
                index += 1;
            }
        }
    }

    /// Handles a property or selection event.
    pub(super) fn handle(
        &mut self,
        connection: &Connection,
        event: SelectionEvent,
        now: Instant,
        events: &mut Vec<Event>,
    ) {
        match event {
            SelectionEvent::Property {
                serial,
                window,
                atom,
                time,
                deleted,
                sent,
            } => {
                if sent {
                    // The server reports every change; a copy another
                    // client sent is no change.
                    return;
                }
                if window == self.window && atom == self.atoms.stamp && !deleted {
                    // Only the event of the latest write of ours.
                    if serial >= self.stamp_serial {
                        self.stamped(connection, time, now);
                    }
                    return;
                }
                if window == self.window && !deleted {
                    // A part to read once the events at hand are handled
                    // (`read_parts`): one read, however many writes.
                    for paste in &mut self.pastes {
                        if let Paste::Receiving {
                            property,
                            announced,
                            ..
                        } = paste
                            && *property == atom
                        {
                            *announced = true;
                        }
                    }
                }
                if deleted
                    && let Some(index) = self
                        .transfers
                        .iter()
                        .position(|t| t.requestor == window && t.property == atom)
                {
                    self.send_part(connection, index, now);
                }
            }
            SelectionEvent::Clear {
                window,
                selection,
                time,
                sent,
            } => {
                if sent || window != self.window {
                    return;
                }
                let Some(selection) = self.atoms.selection(selection) else {
                    return;
                };
                let atom = self.atoms.of(selection);
                let window = self.window;
                let slot = &mut self.owned[selection.index()];
                // A clear from before the window took the selection again
                // is about an older claim. One at the very time it took it
                // at may be about a claim the server took in the same
                // millisecond, just before the window's: the server says
                // whether the window holds it still.
                let lost = slot.as_ref().is_some_and(|owned| {
                    !earlier(time, owned.since)
                        && (time != owned.since || connection.selection_owner(atom) != window)
                });
                if lost {
                    debug!("another client took the {selection:?} selection");
                    *slot = None;
                }
            }
            SelectionEvent::Request(mut request) => {
                if request.sent {
                    debug!("a request for a selection that a client sent itself is not answered");
                    return;
                }
                // An obsolete client gives no property: the target is the
                // property's name (ICCCM, "Requesting a Selection").
                if request.property == 0 {
                    request.property = request.target;
                }
                let answered = self.serve(connection, request, now);
                connection.send_selection_notify(
                    request.requestor,
                    request.selection,
                    request.target,
                    if answered { request.property } else { 0 },
                    request.time,
                );
            }
            SelectionEvent::Notify {
                requestor,
                selection,
                target,
                property,
            } => {
                if requestor != self.window || target != self.atoms.utf8_string {
                    return;
                }
                let Some(selection) = self.atoms.selection(selection) else {
                    return;
                };
                let paste = &self.pastes[selection.index()];
                let Paste::Waiting { property: ours, .. } = *paste else {
                    return;
                };
                if ours == 0 {
                    return;
                }
                if property != 0 && property != ours {
                    // The late answer to an earlier request, in its own
                    // slot, which rests: it is not this one's.
                    debug!("a late answer for the {selection:?} selection is not taken");
                    return;
                }
                self.answered(connection, selection, property, now, events);
            }
        }
    }

    /// The selection whose answers come in the window's property `atom`.
    fn answer_of(&self, atom: Atom) -> Option<Selection> {
        Selection::ALL
            .into_iter()
            .find(|selection| self.atoms.answers[selection.index()].contains(&atom))
    }

    /// The server's time came: takes every selection waiting for it, then
    /// asks for those the window wanted to paste once it held them.
    fn stamped(&mut self, connection: &Connection, time: Time, now: Instant) {
        if !self.stamping {
            return;
        }
        self.stamping = false;
        for selection in Selection::ALL {
            let Some(text) = self.claiming[selection.index()].take() else {
                continue;
            };
            let atom = self.atoms.of(selection);
            connection.set_selection_owner(atom, self.window, time);
            if connection.selection_owner(atom) == self.window {
                self.owned[selection.index()] = Some(Owned { text, since: time });
            } else {
                self.owned[selection.index()] = None;
                warn!("another client took the {selection:?} selection first: the copy is lost");
            }
        }
        for selection in Selection::ALL {
            if let Paste::Waiting { property: 0, .. } = self.pastes[selection.index()] {
                let property = self.ask(connection, selection, now);
                self.pastes[selection.index()] = Paste::Waiting {
                    property,
                    deadline: now + self.timeout,
                };
            }
        }
    }

    /// Answers a request for an owned selection, and returns whether it
    /// wrote the answer to the request's property of its requestor.
    fn serve(&mut self, connection: &Connection, request: Request, now: Instant) -> bool {
        let Request {
            owner,
            requestor,
            selection,
            target,
            property,
            time,
            sent: _,
        } = request;
        if owner != self.window {
            return false;
        }
        // Another client can ask for an answer in any property of this
        // window too, its time stamp or title among them: only those meant
        // for answers are written.
        if requestor == self.window && self.answer_of(property).is_none() {
            return false;
        }
        // A new request into a property ends a transfer into it, answered
        // or not: the requestor no longer waits for that one, whose parts
        // would land on the new answer.
        if let Some(index) = self
            .transfers
            .iter()
            .position(|t| t.requestor == requestor && t.property == property)
        {
            self.end_transfer(connection, index);
        }
        let Some(selection) = self.atoms.selection(selection) else {
            return false;
        };
        let Some(owned) = &self.owned[selection.index()] else {
            return false;
        };
        if time != CurrentTime && earlier(time, owned.since) {
            return false;
        }
        let (text, since) = (Arc::clone(&owned.text), owned.since);
        if target == self.atoms.targets {
            let targets = [
                self.atoms.targets,
                self.atoms.timestamp,
                self.atoms.utf8_string,
            ];
            // An atom is an `unsigned long` that travels as a `long`.
            return connection.set_longs(
                requestor,
                property,
                XA_ATOM,
                &targets.map(|a| a as c_long),
            );
        }
        if target == self.atoms.timestamp {
            return connection.set_longs(requestor, property, XA_INTEGER, &[since as c_long]);
        }
        if target != self.atoms.utf8_string {
            return false;
        }
        let whole = text.len() <= self.part;
        if !whole && self.transfers.len() >= MAX_TRANSFERS {
            debug!("{MAX_TRANSFERS} transfers of a selection run already: a request is refused");
            return false;
        }
        // Charged only for text that will be sent.
        if !self.charge(text.len(), now) {
            debug!("the selection's text was asked for too often: a request is refused");
            return false;
        }
        if whole {
            return connection.set_bytes(
                requestor,
                property,
                self.atoms.utf8_string,
                text.as_bytes(),
            );
        }
        // The window's own properties it watches already, with the mask it
        // chose; another's it watches from now until the transfer ends.
        if requestor != self.window && !self.transfers.iter().any(|t| t.requestor == requestor) {
            connection.select_input(requestor, PropertyChangeMask);
        }
        // ICCCM: the value is a lower bound of the size, in 32 bits.
        let size = c_long::try_from(text.len())
            .unwrap_or(c_long::MAX)
            .min(0x7fff_ffff);
        connection.set_longs(requestor, property, self.atoms.incr, &[size]);
        self.transfers.push(Transfer {
            requestor,
            property,
            text,
            sent: 0,
            deadline: now + self.timeout,
        });
        #[cfg(test)]
        {
            self.counts.sent_incr += 1;
        }
        true
    }

    /// Counts `len` bytes of text against the period's budget, and returns
    /// whether they may be sent: the first text of a period always may.
    fn charge(&mut self, len: usize, now: Instant) -> bool {
        if self
            .period
            .is_none_or(|period| now.duration_since(period) >= SERVE_PERIOD)
        {
            self.period = Some(now);
            self.served = 0;
        }
        if self.served != 0 && self.served.saturating_add(len) > self.serve_limit {
            return false;
        }
        self.served = self.served.saturating_add(len);
        true
    }

    /// The requestor took the last part of a transfer: writes the next, or
    /// the empty one that ends it.
    fn send_part(&mut self, connection: &Connection, index: usize, now: Instant) {
        let Some(transfer) = self.transfers.get_mut(index) else {
            return;
        };
        let rest = transfer
            .text
            .as_bytes()
            .get(transfer.sent..)
            .unwrap_or_default();
        let part = rest.get(..rest.len().min(self.part)).unwrap_or_default();
        connection.set_bytes(
            transfer.requestor,
            transfer.property,
            self.atoms.utf8_string,
            part,
        );
        #[cfg(test)]
        {
            self.counts.parts_sent += 1;
        }
        if part.is_empty() {
            self.end_transfer(connection, index);
            return;
        }
        transfer.sent += part.len();
        transfer.deadline = now + self.timeout;
    }

    /// Ends a transfer, and stops watching its requestor's window if no
    /// other transfer goes to it.
    fn end_transfer(&mut self, connection: &Connection, index: usize) {
        if index >= self.transfers.len() {
            return;
        }
        let ended = self.transfers.swap_remove(index);
        let watched = self
            .transfers
            .iter()
            .any(|t| t.requestor == ended.requestor);
        if ended.requestor != self.window && !watched {
            connection.select_input(ended.requestor, NoEventMask);
        }
    }

    /// The owner answered a paste of `selection` in `property`, or refused.
    fn answered(
        &mut self,
        connection: &Connection,
        selection: Selection,
        property: Atom,
        now: Instant,
        events: &mut Vec<Event>,
    ) {
        if property == 0 {
            self.finish(selection, None, events);
            return;
        }
        let ours = property;
        // One more byte than the limit, to tell a text at the limit from
        // one over it.
        let Some(read) = connection.get_property(self.window, ours, true, self.limit + 1) else {
            self.finish(selection, None, events);
            return;
        };
        if read.type_ == self.atoms.incr && read.format == 32 {
            // Reading it deleted it, which tells the owner to begin.
            self.pastes[selection.index()] = Paste::Receiving {
                property: ours,
                bytes: Vec::new(),
                deadline: now + self.timeout,
                ends: now + self.timeout * RECEIVE_TIMEOUTS,
                announced: false,
            };
            #[cfg(test)]
            {
                self.counts.received_incr += 1;
            }
            return;
        }
        let fits = read.bytes_after == 0 && read.bytes.len() <= self.limit;
        if read.bytes_after != 0 {
            // Too large to have been read whole, and so not deleted.
            connection.delete_property(self.window, ours);
        }
        let text = (read.type_ == self.atoms.utf8_string && read.format == 8 && fits)
            .then(|| text(read.bytes, self.limit))
            .flatten();
        if text.is_none() {
            debug!(
                "the {selection:?} selection's answer is no UTF-8 text of at most {} bytes",
                self.limit
            );
        }
        self.finish(selection, text, events);
    }

    /// Reads the part that writes announced since the last call, once for
    /// each selection however many writes there were, so that another
    /// client's flood of writes costs one round trip a wait, not one each.
    /// The window calls it after each turn of taking events. Returns
    /// whether it read any: then it waited for the server, which may have
    /// queued more events meanwhile.
    pub(super) fn read_parts(
        &mut self,
        connection: &Connection,
        now: Instant,
        events: &mut Vec<Event>,
    ) -> bool {
        let mut read = false;
        for selection in Selection::ALL {
            let announced = match &mut self.pastes[selection.index()] {
                Paste::Receiving { announced, .. } => core::mem::take(announced),
                _ => false,
            };
            if announced {
                self.receive_part(connection, selection, now, events);
                read = true;
            }
        }
        read
    }

    /// The owner wrote the next part of an `INCR` paste of `selection`.
    fn receive_part(
        &mut self,
        connection: &Connection,
        selection: Selection,
        now: Instant,
        events: &mut Vec<Event>,
    ) {
        let index = selection.index();
        let Paste::Receiving {
            property,
            bytes,
            ends,
            ..
        } = &self.pastes[index]
        else {
            return;
        };
        if now >= *ends {
            debug!("the {selection:?} selection took more than its time: nothing pastes");
            self.finish(selection, None, events);
            return;
        }
        let room = self.limit.saturating_sub(bytes.len());
        let ours = *property;
        #[cfg(test)]
        {
            self.counts.parts_read += 1;
        }
        let Some(read) = connection.get_property(self.window, ours, true, room + 1) else {
            self.finish(selection, None, events);
            return;
        };
        if read.type_ == 0 {
            // The property is gone already: nothing to read.
            return;
        }
        if read.type_ != self.atoms.utf8_string || read.format != 8 {
            debug!("a part of the {selection:?} selection is no UTF-8 text: nothing pastes");
            self.finish(selection, None, events);
            return;
        }
        if read.bytes_after != 0 || read.bytes.len() > room {
            debug!(
                "the {selection:?} selection is more than {} bytes: nothing pastes",
                self.limit
            );
            self.finish(selection, None, events);
            return;
        }
        if read.bytes.is_empty() {
            let Paste::Receiving { bytes, .. } =
                core::mem::replace(&mut self.pastes[index], Paste::Idle)
            else {
                return;
            };
            let text = text(bytes, self.limit);
            if text.is_none() {
                debug!(
                    "the {selection:?} selection is more than {} bytes as text: nothing pastes",
                    self.limit
                );
            }
            self.finish(selection, text, events);
            return;
        }
        if let Paste::Receiving {
            bytes,
            deadline,
            ends,
            ..
        } = &mut self.pastes[index]
        {
            bytes.extend_from_slice(&read.bytes);
            *deadline = (now + self.timeout).min(*ends);
        }
    }

    /// Ends the paste of `selection` with `text`. A slot whose paste ended
    /// with none rests, since its answer may still come.
    fn finish(&mut self, selection: Selection, text: Option<String>, events: &mut Vec<Event>) {
        let index = selection.index();
        let paste = core::mem::replace(&mut self.pastes[index], Paste::Idle);
        if text.is_none()
            && let Some(property) = paste.property()
            && let Some(slot) = self.atoms.answers[index]
                .iter()
                .position(|&atom| atom == property)
        {
            self.spoiled[index][slot] = Some(Instant::now());
        }
        events.push(Event::Pasted { selection, text });
    }
}

/// The text of a paste's bytes, with U+FFFD for each sequence that is not
/// UTF-8, or `None` if that is more than `limit` bytes: each such
/// replacement is 3 bytes, so a text can grow up to three times. Valid
/// UTF-8 keeps its buffer.
fn text(bytes: Vec<u8>, limit: usize) -> Option<String> {
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|invalid| String::from_utf8_lossy(invalid.as_bytes()).into_owned());
    (text.len() <= limit).then_some(text)
}

/// Whether the server time `a` is earlier than `b`. The server's time is a
/// 32-bit count of milliseconds that wraps about every 49.7 days, and the
/// half of its values after a time are later than it (the core protocol,
/// "Common Types": `TIMESTAMP`).
fn earlier(a: Time, b: Time) -> bool {
    // The low 32 bits are the time; Xlib keeps it in an `unsigned long`.
    let (a, b) = (a as u32, b as u32);
    (a.wrapping_sub(b) as i32) < 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pastes_text_is_bounded_after_its_replacements() {
        assert_eq!(text(b"tide".to_vec(), 4).as_deref(), Some("tide"));
        assert_eq!(text(b"tides".to_vec(), 4), None);
        // Two bytes that are not UTF-8 become 6 bytes of U+FFFD.
        assert_eq!(
            text(b"a\xff\xfe".to_vec(), 7).as_deref(),
            Some("a\u{fffd}\u{fffd}")
        );
        assert_eq!(text(b"a\xff\xfe".to_vec(), 6), None);
        assert_eq!(text(Vec::new(), 0).as_deref(), Some(""));
    }

    #[test]
    fn times_compare_across_the_wrap() {
        assert!(earlier(1, 2));
        assert!(!earlier(2, 1));
        assert!(!earlier(5, 5));
        // Just after the wrap is later than just before it.
        assert!(earlier(0xffff_fff0, 0x10));
        assert!(!earlier(0x10, 0xffff_fff0));
        // Only the low 32 bits count.
        assert!(!earlier(0x1_0000_0005, 5));
    }
}
