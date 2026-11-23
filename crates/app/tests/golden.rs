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

//! The golden scenes of the shell (M3.4, ADR-0060, ADR-0044,
//! `docs/04-quality.md` → "Golden frames"): `editor::shell` in a `ui` tree,
//! painted by `tidepool-gui`'s [`Painter`] over jabbott.dev's backdrop, as
//! the window shows it, with the activity bar's icons registered as the
//! binary registers them. One test per scene, named after it: the shell at
//! 1280 × 800 and 1920 × 1080, with the command palette open, and at scale
//! 2.
//!
//! When [`DIR_VAR`] names a directory, each scene writes what it painted
//! there as `<scene>.pam`; `cargo xtask golden` sets it, and compares each
//! file with `tests/golden/<scene>.qoi`, exactly. Unset, as in `cargo
//! test`, the scenes paint and write nothing. The PAM file holds the
//! framebuffer's premultiplied bytes in RGBA order, rows from the top, as
//! `tidepool-gui`'s scenes write it.
//!
//! The shell is told the time, which paints nothing: no pointer rests
//! anywhere, so no tooltip opens.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use tidepool_core::geom::{DevicePoint, DeviceSize, Scale, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers};
use tidepool_editor::shell::{Panels, Shell, View};
use tidepool_gfx::backdrop::{Backdrop, BackdropStyle};
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_gui::{Icon, Painter, icons};
use tidepool_ui::Tree;
use tidepool_ui::metrics::TextMetrics;

/// Names the directory the scenes write to; `cargo xtask golden` sets it.
const DIR_VAR: &str = "TIDEPOOL_GOLDEN_DIR";

/// The outline `view`'s button shows, as the binary registers it.
fn icon(view: View) -> Icon {
    match view {
        View::Explorer => icons::files(),
        View::Search => icons::search(),
        View::SourceControl => icons::branch(),
        View::Toolchains => icons::wrench(),
        View::Devices => icons::device(),
        View::Agents => icons::spark(),
    }
}

/// A painter with the activity bar's icons.
fn painter() -> Painter {
    let mut painter = Painter::new().expect("the bundled faces load");
    for view in View::ALL {
        painter.set_icon(view.icon(), icon(view));
    }
    painter
}

/// Builds the shell, lets `prepare` act on it after a first layout, with
/// the painter's metrics, and paints the whole window of `size` logical units at `factor` over the
/// backdrop, as `name`.
fn render(
    name: &str,
    size: Size,
    factor: f32,
    prepare: impl FnOnce(&mut Tree, &mut Shell, &dyn TextMetrics, Size),
) {
    let scale = Scale::new(factor).expect("a scale");
    let mut painter = painter();
    let mut tree = Tree::new();
    let mut shell = Shell::new(&mut tree, Panels::default()).expect("the shell builds");
    let _ = tree.frame(size, &painter.metrics(scale));
    prepare(&mut tree, &mut shell, &painter.metrics(scale), size);
    tree.invalidate_all();
    let frame = tree
        .frame(size, &painter.metrics(scale))
        .expect("a whole frame");
    let device = DeviceSize::new((size.width * factor) as i32, (size.height * factor) as i32);
    let mut backdrop = Backdrop::new();
    backdrop
        .ensure(device, scale, &BackdropStyle::jabbott_dev())
        .expect("a backdrop");
    let mut target = Framebuffer::new(device).expect("a window");
    let painted = painter
        .paint(&mut target, &backdrop.base().view(), &frame, scale)
        .expect("painted");
    assert_eq!(painted.rects(), [target.bounds()], "the frame is whole");
    assert!(
        tree.frame(size, &painter.metrics(scale)).is_none(),
        "the shell is idle after it"
    );
    assert_ne!(
        target.pixel(DevicePoint::new(device.width / 2, 4)),
        backdrop.base().pixel(DevicePoint::new(device.width / 2, 4)),
        "the title strip covers the backdrop"
    );
    save(name, &target);
}

/// Writes `frame` as `<name>.pam` into the directory [`DIR_VAR`] names, if
/// it names one.
fn save(name: &str, frame: &Framebuffer) {
    let Some(dir) = env::var_os(DIR_VAR).filter(|dir| !dir.is_empty()) else {
        return;
    };
    let path = PathBuf::from(dir).join(format!("{name}.pam"));
    fs::write(&path, pam(frame)).unwrap_or_else(|error| {
        panic!("cannot write {}: {error}", path.display());
    });
}

/// `frame` as a PAM file: RGBA, rows from the top, the premultiplied bytes
/// unchanged.
fn pam(frame: &Framebuffer) -> Vec<u8> {
    let size = frame.size();
    let mut file = format!(
        "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n",
        size.width, size.height
    )
    .into_bytes();
    let row_bytes = frame.stride();
    for row in frame.bytes().chunks_exact(row_bytes) {
        for &[b, g, r, a] in row.as_chunks::<4>().0 {
            file.extend_from_slice(&[r, g, b, a]);
        }
    }
    file
}

#[test]
fn shell_1280x800() {
    render(
        "shell_1280x800",
        Size::new(1280.0, 800.0),
        1.0,
        |_, _, _, _| {},
    );
}

#[test]
fn shell_1920x1080() {
    render(
        "shell_1920x1080",
        Size::new(1920.0, 1080.0),
        1.0,
        |_, _, _, _| {},
    );
}

#[test]
fn shell_palette_1280x800() {
    render(
        "shell_palette_1280x800",
        Size::new(1280.0, 800.0),
        1.0,
        |tree, shell, metrics, size| {
            let now = Instant::now();
            let open = InputEvent::KeyDown {
                key: Key::P,
                modifiers: Modifiers::CTRL.union(Modifiers::SHIFT),
                repeat: false,
            };
            shell.handle(tree, &open, now).expect("handled");
            let _ = tree.frame(size, metrics);
            for c in ["t", "o", "g"] {
                shell
                    .handle(tree, &InputEvent::Text(c.to_string()), now)
                    .expect("handled");
                let _ = tree.frame(size, metrics);
            }
            assert!(shell.palette().is_some());
        },
    );
}

#[test]
fn shell_960x600_scale_2() {
    render(
        "shell_960x600_scale_2",
        Size::new(960.0, 600.0),
        2.0,
        |_, _, _, _| {},
    );
}
