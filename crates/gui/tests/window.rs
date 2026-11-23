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

//! The GUI's window under an X server: the first frame presented, the
//! backdrop arriving from its thread, and an idle window presenting
//! nothing (M3.8, ADR-0059). Like `tidepool-platform`'s window tests, these
//! run only with `TIDEPOOL_IT_X11=1`, under `xvfb-run` (ADR-0039,
//! ADR-0041), and print `SKIPPED:` otherwise.

use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use tidepool_core::geom::Size;
use tidepool_gui::{Gui, GuiError, first_frame};
use tidepool_platform::events::Event;
use tidepool_platform::window::WindowConfig;
use tidepool_ui::Tree;

/// Says whether the window tests may run, as `TIDEPOOL_IT_X11=1` allows;
/// if not, says on standard error, as the gate counts it, that `test`
/// skipped.
fn x11(test: &str) -> bool {
    let reason = match std::env::var_os("TIDEPOOL_IT_X11") {
        Some(value) if value == "1" => return true,
        Some(value) => format!("TIDEPOOL_IT_X11 is {value:?}, not 1"),
        None => "TIDEPOOL_IT_X11 is not set".to_string(),
    };
    // One write, so that the line reaches the stream whole.
    let _ = io::stderr().write_all(format!("SKIPPED: {test}: {reason}\n").as_bytes());
    false
}

/// Opens a small window, trying again while the server refuses the
/// connection, as stock `xvfb-run`'s Xvfb does while it resets after its
/// last client leaves.
fn open() -> Gui {
    let config = WindowConfig {
        title: "tidepool-gui test".to_string(),
        size: Size::new(160.0, 120.0),
    };
    for _ in 1..20 {
        match Gui::open(&config) {
            Err(GuiError::Platform(tidepool_platform::Error::ConnectionFailed { .. })) => {
                thread::sleep(Duration::from_millis(50));
            }
            done => return done.unwrap_or_else(|error| panic!("{error}")),
        }
    }
    Gui::open(&config).unwrap_or_else(|error| panic!("{error}"))
}

/// Runs turns of a main loop, each waiting on the window and rendering
/// `tree`, until `done` holds before a turn. `done` is given the `Gui`
/// and whether a turn presented anything. Fails, saying `what`, after 5 s,
/// and on a close request.
fn run_until(gui: &mut Gui, tree: &mut Tree, what: &str, done: impl Fn(&Gui, bool) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let (mut events, mut presented) = (Vec::new(), false);
    while !done(gui, presented) {
        assert!(Instant::now() < deadline, "{what} in 5 s");
        events.clear();
        let wait = gui
            .next_deadline()
            .map_or(deadline, |next| next.min(deadline));
        gui.wait_events(Some(wait), &mut events)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            !events.contains(&Event::CloseRequested),
            "nothing asked the window to close"
        );
        presented |= gui.render(tree).unwrap_or_else(|error| panic!("{error}"));
    }
}

#[test]
fn the_first_frame_is_presented_the_backdrop_arrives_and_an_idle_window_presents_nothing() {
    if !x11("the_first_frame_is_presented_the_backdrop_arrives_and_an_idle_window_presents_nothing")
    {
        return;
    }
    let mut gui = open();
    let mut tree = first_frame::tree();
    assert!(gui.next_deadline().is_some() || gui.backdrop_ready());
    run_until(
        &mut gui,
        &mut tree,
        "no frame was presented",
        |_, presented| presented,
    );
    let size = gui.size();
    assert!(size.width > 0 && size.height > 0, "{size:?}");
    assert!(gui.scale().factor() > 0.0);

    // The backdrop is built on a thread of its own, and wakes the window
    // when it is done; it may have been what the first present waited for.
    run_until(
        &mut gui,
        &mut tree,
        "the backdrop did not arrive",
        |gui, _| gui.backdrop_ready(),
    );
    assert_eq!(gui.next_deadline(), None, "the first present waits no more");

    // Nothing asks for a frame now, so the waits run out their deadline
    // and the renders present nothing. A build's wake can come after its
    // result was taken, so one wake may end a wait early.
    let mut events = Vec::new();
    let started = Instant::now();
    let until = started + Duration::from_millis(100);
    while Instant::now() < until {
        events.clear();
        gui.wait_events(Some(until), &mut events)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            events.iter().all(|event| *event == Event::Woken),
            "{events:?}"
        );
        let presented = gui
            .render(&mut tree)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!presented, "an idle window presents nothing");
    }
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(90), "{waited:?}");
}
