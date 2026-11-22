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

//! The suites of `tidepool-ui`: what M3.3's second security review
//! measured by hand.
//!
//! - `ui/palette/repaint/4kib_64_matches`: the command palette, 800 × 600,
//!   showing ten items, each a 4 KiB label with 64 ranges its query
//!   matched, repainted whole (`Tree::invalidate_all`, then
//!   `Tree::frame`), its text measured with `gui`'s `FontMetrics` over the
//!   bundled faces at scale 1, as the window measures it. A row's matched
//!   runs are measured once each, and none past the row's right edge, so
//!   this costs what the rows show, not their labels' length: measuring
//!   each run's whole prefix took 162 ms a frame in the review's probe.
//!
//! Before it is measured, the suite checks, untimed, that the palette
//! shows its ten items, and that a frame draws fewer matched runs than
//! the labels hold, so the rows were cut at their edges.

use tidepool_core::geom::{Scale, Size};
use tidepool_font::bundled;
use tidepool_gui::FontMetrics;
use tidepool_ui::Tree;
use tidepool_ui::a11y::Role;
use tidepool_ui::draw::Command;
use tidepool_ui::widgets::{MAX_MATCHES, PALETTE_ROWS, Palette, Row};

use crate::{BenchError, Bencher, Suite};

/// The ui suites, in the order a run takes them.
pub const SUITES: &[Suite] = &[Suite {
    name: "ui/palette/repaint/4kib_64_matches",
    run: palette_repaint,
}];

/// The window the palette is in.
const WINDOW: Size = Size::new(800.0, 600.0);

/// Repainting a palette of ten rows of 4 KiB labels with 64 matches each.
fn palette_repaint(b: &mut Bencher) -> Result<(), BenchError> {
    let faces = bundled::load()?;
    let metrics = FontMetrics::new(&faces, Scale::ONE);
    // `a` and a combining acute, then `b`: 4 bytes, two clusters.
    let label = "a\u{301}b".repeat(4096 / 4);
    let ranges: Vec<std::ops::Range<usize>> =
        (0..MAX_MATCHES).map(|k| k * 60 + 1..k * 60 + 31).collect();
    let items: Vec<Row> = (0..PALETTE_ROWS)
        .map(|_| Row::new(label.clone()).matched(ranges.iter().cloned()))
        .collect();
    let mut tree = Tree::new();
    let palette = Palette::new("Command palette")
        .open(&mut tree)
        .map_err(|error| failed(&error.to_string()))?;
    Palette::set_items(&mut tree, palette, items).map_err(|error| failed(&error.to_string()))?;
    let frame = tree
        .frame(WINDOW, &metrics)
        .ok_or_else(|| failed("the first frame draws"))?;
    let rows = tree
        .accessibility()
        .nodes()
        .iter()
        .filter(|node| node.role == Role::ListItem)
        .count();
    check(rows == PALETTE_ROWS, "the palette shows its ten items")?;
    let runs = frame
        .list
        .commands()
        .iter()
        .filter(|command| matches!(command, Command::Text { text, .. } if text.contains('b')))
        .count();
    check(
        runs > 0 && runs < PALETTE_ROWS * MAX_MATCHES,
        "each row draws only the runs that reach its right edge",
    )?;
    b.iter(|| {
        tree.invalidate_all();
        tree.frame(WINDOW, &metrics)
    });
    Ok(())
}

/// An error naming what a suite's untimed check found wrong, unless it
/// holds.
fn check(holds: bool, what: &str) -> Result<(), BenchError> {
    if holds { Ok(()) } else { Err(failed(what)) }
}

/// The error of a suite's untimed check that found `what` wrong.
fn failed(what: &str) -> BenchError {
    BenchError::Check(format!("its check failed: {what}"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::harness::{Settings, run_suite};

    /// Settings that finish at once, for unoptimized test builds.
    const QUICK: Settings = Settings {
        warm_up: Duration::ZERO,
        sample_target: Duration::from_micros(10),
        budget: Duration::ZERO,
        min_samples: 2,
        max_samples: 2,
    };

    #[test]
    fn each_suite_checks_its_palette_and_measures_one_routine() {
        for suite in SUITES {
            let summary = run_suite(suite, &QUICK).expect("the suite runs");
            assert_eq!(summary.samples, 2, "{}", suite.name);
        }
    }
}
