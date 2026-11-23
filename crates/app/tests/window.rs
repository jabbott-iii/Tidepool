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

//! The binary's main loop under an X server (M3.8, ADR-0012): it reaches
//! its first frame, then waits without using the processor. Like
//! `tidepool-gui`'s window tests, this runs only with `TIDEPOOL_IT_X11=1`,
//! under `xvfb-run` (ADR-0039, ADR-0041), and prints `SKIPPED:` otherwise.
//!
//! It runs only the binary under test, which Cargo builds for this test,
//! never an outside program, and stops it when it is done. The processor
//! time it reads is the binary's, from `/proc/<pid>/stat`, which Linux
//! has; the window opens only on Linux, so nowhere else gets this far.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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

/// The binary under test, running; it is killed when this is dropped, so
/// a failed assertion leaves no window behind.
struct Running {
    child: Child,
    /// What it writes to standard error, read on a thread of its own so
    /// that a full pipe never stops it.
    stderr: mpsc::Receiver<String>,
}

impl Running {
    /// What it has written to standard error so far.
    fn stderr(&self) -> String {
        self.stderr.try_iter().collect()
    }

    /// Waits for it to exit, once its standard output has closed, and
    /// returns all it wrote to standard error: the reading thread ends,
    /// and the channel with it, when the stream closes.
    fn exited(&mut self) -> String {
        let _ = self.child.wait();
        self.stderr.iter().collect()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// How long the binary has to reach its first frame. A dev build under
/// Xvfb on a busy machine is far slower than the budget, which `cargo tp
/// -- --print-startup-time` checks on the reference machine.
const FIRST_FRAME: Duration = Duration::from_secs(20);

/// Starts the binary with `--print-startup-time` and waits for its startup
/// line, starting it again while it exits first, as it does when the
/// server refuses the connection while stock `xvfb-run`'s Xvfb resets
/// after its last client leaves.
fn start() -> (Running, String) {
    let mut last = String::new();
    for _ in 0..20 {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tidepool"))
            .arg("--print-startup-time")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary under test runs");
        let stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let (err_tx, err_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut chunk = [0; 4096];
            while let Ok(n @ 1..) = stderr.read(&mut chunk) {
                let _ = err_tx.send(String::from_utf8_lossy(&chunk[..n]).into_owned());
            }
        });
        let (line_tx, line_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = line_tx.send(line);
        });
        let mut running = Running {
            child,
            stderr: err_rx,
        };
        match line_rx.recv_timeout(FIRST_FRAME) {
            Ok(line) if !line.is_empty() => return (running, line),
            // Standard output closed with no line: the binary exited.
            Ok(_) => {
                last = running.exited();
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => panic!(
                "no first frame in {FIRST_FRAME:?}; standard error: {}",
                running.stderr()
            ),
        }
    }
    panic!("the binary exited before its first frame 20 times; last standard error: {last}")
}

/// The processor time `pid` has used, user and system, in clock ticks:
/// the 14th and 15th fields of `/proc/<pid>/stat`, counted after the
/// command's name, which is in parentheses and may hold spaces.
fn ticks(pid: u32) -> u64 {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).expect("the binary is running");
    let after_name = &stat[stat.rfind(')').expect("a name in parentheses") + 1..];
    let fields: Vec<&str> = after_name.split_ascii_whitespace().collect();
    // `fields[0]` is the 3rd field, the state.
    let field = |n: usize| -> u64 { fields[n - 3].parse().expect("a count of ticks") };
    field(14) + field(15)
}

#[test]
fn the_loop_reaches_its_first_frame_then_uses_no_processor_while_idle() {
    if !x11("the_loop_reaches_its_first_frame_then_uses_no_processor_while_idle") {
        return;
    }
    let (running, line) = start();
    let ms = line
        .strip_prefix("first frame: ")
        .and_then(|rest| rest.strip_suffix(" ms\n"))
        .and_then(|ms| ms.parse::<f64>().ok());
    assert!(
        ms.is_some_and(|ms| ms > 0.0),
        "{line:?}; standard error: {}",
        running.stderr()
    );

    // The backdrop's build may still be landing if the first present gave
    // up waiting for it; give it time, then nothing should run.
    thread::sleep(Duration::from_secs(2));
    let pid = running.child.id();
    let before = ticks(pid);
    thread::sleep(Duration::from_secs(1));
    let used = ticks(pid) - before;
    // A tick is 10 ms on Linux's usual `CLK_TCK` of 100; one is allowed
    // for a stray wake, such as the logger's thread flushing.
    assert!(
        used <= 1,
        "the idle binary used {used} ticks of processor time in a second; standard error: {}",
        running.stderr()
    );
}
