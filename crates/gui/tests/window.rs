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

//! The GUI's window under an X server. Like `tidepool-platform`'s window
//! tests, these run only with `TIDEPOOL_IT_X11=1`, under `xvfb-run`
//! (ADR-0039, ADR-0041), and print `SKIPPED:` otherwise.

use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use tidepool_core::geom::Size;
use tidepool_gui::{Gui, GuiError};
use tidepool_platform::window::WindowConfig;

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

#[test]
fn the_first_expose_presents_a_frame_and_an_idle_window_presents_nothing() {
    if !x11("the_first_expose_presents_a_frame_and_an_idle_window_presents_nothing") {
        return;
    }
    let mut gui = open();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let turn = gui
            .turn(Some(deadline))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!turn.close_requested);
        if turn.presented {
            break;
        }
        assert!(Instant::now() < deadline, "no frame was presented in 5 s");
    }
    let size = gui.size();
    assert!(size.width > 0 && size.height > 0, "{size:?}");

    // Nothing asks for a frame now, so a turn waits out its deadline and
    // presents nothing.
    let started = Instant::now();
    let idle = gui
        .turn(Some(started + Duration::from_millis(100)))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(idle, tidepool_gui::Turn::default());
    assert!(
        started.elapsed() >= Duration::from_millis(90),
        "{:?}",
        started.elapsed()
    );
}
