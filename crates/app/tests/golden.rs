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

//! The golden scenes of the shell (M3.4, ADR-0060) and of the editor view
//! (M4.1, ADR-0063), as ADR-0044 and `docs/04-quality.md` → "Golden
//! frames" describe them: `editor`'s widgets in a `ui` tree, painted by
//! `tidepool-gui`'s [`Painter`]. One test per scene, named after it.
//!
//! - **The shell** (`shell_*`), over jabbott.dev's backdrop, as the window
//!   shows it, with the activity bar's icons registered as the binary
//!   registers them: at 1280 × 800 and 1920 × 1080, with the command
//!   palette open, and at scale 2. The shell is told the time, which
//!   paints nothing: no pointer rests anywhere, so no tooltip opens.
//! - **The editor view** (`editor_view_*`), alone, filling the window. It
//!   is opaque, `editor-bg` under everything it draws, so each scene
//!   paints over the theme's `bg` and checks that over another base it
//!   paints the same pixels. Its texts are written here, not read from
//!   disk: [`fixture`], Rust-like code with tab-indented lines, lines
//!   longer than the window, and a line of hidden characters (NUL, U+202E,
//!   U+200B, U+2028) and the flag of Wales, shown with a selection over
//!   two lines and a second caret, with keyboard focus, at 800 × 480 with
//!   soft wrap off and on, and at 400 × 240 at scale 2; [`ten_thousand`]
//!   lines, scrolled to the end and sideways, with pointer focus, which
//!   shows the cursor and no focus outline; and a line past
//!   [`MAX_LINE_BYTES`], scrolled sideways to its cut marker.
//!
//! When [`DIR_VAR`] names a directory, each scene writes what it painted
//! there as `<scene>.pam`; `cargo xtask golden` sets it, and compares each
//! file with `tests/golden/<scene>.qoi`, exactly. Unset, as in `cargo
//! test`, the scenes paint and write nothing. The PAM file holds the
//! framebuffer's premultiplied bytes in RGBA order, rows from the top, as
//! `tidepool-gui`'s scenes write it.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use tidepool_core::geom::{DevicePoint, DeviceSize, Point, Scale, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, ScrollDelta};
use tidepool_editor::shell::{Panels, Shell, View};
use tidepool_editor::view::{CUT_MARKER, EditorView, MAX_LINE_BYTES};
use tidepool_gfx::backdrop::{Backdrop, BackdropStyle};
use tidepool_gfx::framebuffer::Framebuffer;
use tidepool_gui::{Icon, Painter, icons};
use tidepool_text::{Cursor, Rope, Selection};
use tidepool_ui::Tree;
use tidepool_ui::draw::Command;
use tidepool_ui::layout::Axis;
use tidepool_ui::metrics::TextMetrics;
use tidepool_ui::theme::Token;
use tidepool_ui::widgets::ScrollBar;

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

/// The flag of Wales: a black flag, five tag characters and a cancel tag,
/// one grapheme cluster.
const WALES: &str = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0077}\u{E006C}\u{E0073}\u{E007F}";

/// The editor view's scenes' Rust-like code, 23 lines with no line break
/// at the end: two lines indented with a tab (8 and 9); a line of hidden
/// characters, NUL, U+202E (right-to-left override), U+200B (zero-width
/// space) and U+2028 (line separator), each shown as a marker, and the
/// flag of Wales, shown whole (4); a line of prose longer than an 800-unit
/// window, which soft wrap breaks at a space (19); and a line of digits
/// with no space in its string, which it breaks after `= ` and then inside
/// the string (20). The selection and the caret [`mark`] puts are in the
/// first eleven lines, which every scene of it shows.
fn fixture() -> String {
    let hidden = format!(
        "const HIDDEN: &str = \"nul \0, rlo \u{202E}, zwsp \u{200B}, ls \u{2028}, {WALES}\";"
    );
    [
        "use std::fmt;",
        "",
        "/// Characters that would hide or reorder code show as markers.",
        hidden.as_str(),
        "",
        "/// A point on the grid, its fields indented with a tab.",
        "struct Point {",
        "\tx: i32,",
        "\ty: i32,",
        "}",
        "",
        "impl fmt::Display for Point {",
        "    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {",
        "        write!(f, \"({}, {})\", self.x, self.y)",
        "    }",
        "}",
        "",
        "fn main() {",
        "    let message = \"This line runs past the right edge of the window, so the view \
         scrolls sideways to show the rest of it.\";",
        "    let digits = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\
         0123456789abcdef0123456789abcdef\";",
        "    let origin = Point { x: 0, y: 0 };",
        "    println!(\"{origin} {message} {digits} {HIDDEN}\");",
        "}",
    ]
    .join("\n")
}

/// Puts the fixture's selection and gives the view keyboard focus: a
/// selection over two lines, from `Point` on line 7 to after the `x` on
/// line 8, its head there; and a second caret after `use std::` on line
/// 1.
fn mark(tree: &mut Tree, view: EditorView) {
    let text = view.text(tree).expect("its text").to_string();
    let anchor = text.find("Point {").expect("line 7");
    let head = text.find("\tx: i32").expect("line 8") + "\tx".len();
    let caret = text.find("std::").expect("line 1") + "std::".len();
    view.set_selection(
        tree,
        Selection::new(Cursor::new(anchor, head), [Cursor::caret(caret)]),
    )
    .expect("a selection over two lines and a caret");
    tree.set_focus(view.text_node(), true)
        .expect("keyboard focus");
}

/// 10,000 lines of Rust-like code, with no line break at the end: a
/// function every ten lines, each a signature, a comment of 112 to 115
/// bytes, seven `let`s of 57 to 60, and a closing brace.
fn ten_thousand() -> String {
    (0..10_000)
        .map(|n: usize| match n % 10 {
            0 => format!("fn step_{n}(input: &[u32]) -> u32 {{"),
            1 => format!(
                "    // Step {n} folds the values of its input, each times its place, into the \
                 one number that the next step reads."
            ),
            9 => "}".to_string(),
            k => format!("    let value_{k} = input.get({k}).copied().unwrap_or({n}) * {k};"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Draws frames and updates the view, as the main loop does, until
/// neither has anything left to do.
fn settle(tree: &mut Tree, view: EditorView, size: Size, metrics: &dyn TextMetrics) {
    for _ in 0..8 {
        let drew = tree.frame(size, metrics).is_some();
        let changed = view.update(tree, &[], metrics).expect("updated");
        if !drew && !changed {
            return;
        }
    }
    panic!("the view never settled");
}

/// A wheel event at the middle of the view's text area: `x` notches
/// across and `y` down.
fn notch(tree: &Tree, view: EditorView, x: f32, y: f32) -> InputEvent {
    let area = tree.bounds(view.text_node()).expect("laid out");
    InputEvent::Wheel {
        delta: ScrollDelta::Lines { x, y },
        position: Point::new(area.x + area.width / 2.0, area.y + area.height / 2.0),
        modifiers: Modifiers::NONE,
    }
}

/// The offset of the view's scrollbar on `axis`, and the most it can be.
fn bar(tree: &Tree, view: EditorView, axis: Axis) -> (f32, f32) {
    let bar = tree
        .widget::<ScrollBar>(view.scroll_bar(axis))
        .expect("a scrollbar");
    (bar.offset(), bar.max_offset())
}

/// Builds a view of `text` filling a window of `size` logical units, lets
/// `prepare` act on it after a first layout, with the painter's metrics,
/// settles it, and paints the whole window at `factor` as `name`. The
/// view is opaque, so over the theme's `bg` and over `teal` it paints the
/// same pixels.
fn render_view(
    name: &str,
    size: Size,
    factor: f32,
    text: &str,
    prepare: impl FnOnce(&mut Tree, EditorView, &dyn TextMetrics),
) {
    let scale = Scale::new(factor).expect("a scale");
    let mut painter = Painter::new().expect("the bundled faces load");
    let mut tree = Tree::new();
    let root = tree.root();
    let view =
        EditorView::insert(&mut tree, root, "main.rs", Rope::from(text)).expect("the view builds");
    {
        let metrics = painter.metrics(scale);
        let _ = tree.frame(size, &metrics);
        prepare(&mut tree, view, &metrics);
        settle(&mut tree, view, size, &metrics);
    }
    tree.invalidate_all();
    let frame = tree
        .frame(size, &painter.metrics(scale))
        .expect("a whole frame");
    let device = DeviceSize::new((size.width * factor) as i32, (size.height * factor) as i32);
    let paint = |painter: &mut Painter, token: Token| {
        let mut base = Framebuffer::new(device).expect("a base");
        base.clear(tree.theme().color(token).premultiply());
        let mut target = Framebuffer::new(device).expect("a window");
        let painted = painter
            .paint(&mut target, &base.view(), &frame, scale)
            .expect("painted");
        assert_eq!(painted.rects(), [target.bounds()], "the frame is whole");
        (target, base)
    };
    let (target, base) = paint(&mut painter, Token::Bg);
    let (over_teal, _) = paint(&mut painter, Token::Teal);
    assert_ne!(target, base, "the view drew something");
    assert!(
        target == over_teal,
        "nothing of the base shows through the view"
    );
    let metrics = painter.metrics(scale);
    assert!(tree.frame(size, &metrics).is_none(), "idle after it");
    assert!(
        !view.update(&mut tree, &[], &metrics).expect("updated"),
        "idle after it"
    );
    save(name, &target);
}

#[test]
fn editor_view_800x480() {
    render_view(
        "editor_view_800x480",
        Size::new(800.0, 480.0),
        1.0,
        &fixture(),
        |tree, view, _| {
            mark(tree, view);
            let state = tree.state(view.text_node()).expect("a state");
            assert!(state.focused && state.focus_visible, "{state:?}");
        },
    );
}

#[test]
fn editor_view_wrap_800x480() {
    render_view(
        "editor_view_wrap_800x480",
        Size::new(800.0, 480.0),
        1.0,
        &fixture(),
        |tree, view, _| {
            mark(tree, view);
            view.set_wrap(tree, true).expect("wrap");
            let hidden = tree
                .style(view.scroll_bar(Axis::Horizontal))
                .map(|style| style.hidden);
            assert_eq!(hidden, Some(true), "no horizontal scrollbar");
        },
    );
}

#[test]
fn editor_view_scrolled_800x480() {
    render_view(
        "editor_view_scrolled_800x480",
        Size::new(800.0, 480.0),
        1.0,
        &ten_thousand(),
        |tree, view, metrics| {
            let rope = view.text(tree).expect("its text").clone();
            assert_eq!(rope.len_lines(), 10_000);
            // A caret 40 bytes into line 9,996, a `let`, and pointer focus.
            let caret = rope.line_to_byte(9_995).expect("line 9,996") + 40;
            view.set_selection(tree, Selection::caret(caret))
                .expect("a caret");
            tree.set_focus(view.text_node(), false).expect("focus");
            // The last line at the bottom, then two notches sideways.
            assert!(
                view.scroll_into_view(tree, rope.len(), metrics)
                    .expect("the end")
            );
            settle(tree, view, Size::new(800.0, 480.0), metrics);
            for _ in 0..2 {
                view.handle(tree, &notch(tree, view, 1.0, 0.0), metrics)
                    .expect("handled");
                settle(tree, view, Size::new(800.0, 480.0), metrics);
            }
            let position = view.position(tree).expect("a position");
            assert_eq!(position.x, 2.0 * 3.0 * 21.0, "{position:?}");
            assert!(position.line > 9_970, "{position:?}");
            let (offset, most) = bar(tree, view, Axis::Vertical);
            assert_eq!(offset, most, "the vertical thumb at its end");
            let (offset, most) = bar(tree, view, Axis::Horizontal);
            assert!(offset > 0.0 && offset < most, "{offset} of {most}");
            let state = tree.state(view.text_node()).expect("a state");
            assert!(state.focused && !state.focus_visible, "{state:?}");
        },
    );
}

#[test]
fn editor_view_cut_800x480() {
    // `static TABLE: [u16; 2600] = [0, 1, …, 2599];`: 14,519 bytes of
    // ASCII, cut at byte 10,000.
    let numbers: Vec<String> = (0..2600).map(|n: u32| n.to_string()).collect();
    let table = format!("static TABLE: [u16; 2600] = [{}];", numbers.join(", "));
    assert!(table.len() > MAX_LINE_BYTES + 4_000, "{}", table.len());
    let text = format!("/// The first 2,600 numbers.\n{table}\n\nfn main() {{}}");
    render_view(
        "editor_view_cut_800x480",
        Size::new(800.0, 480.0),
        1.0,
        &text,
        |tree, view, metrics| {
            // A caret at the table's end, past the cut, shows at the
            // marker; then a notch on, to the line's end.
            let end = text.find("];").expect("the table's end") + "];".len();
            view.set_selection(tree, Selection::caret(end))
                .expect("a caret");
            tree.set_focus(view.text_node(), true).expect("focus");
            assert!(view.scroll_into_view(tree, end, metrics).expect("the end"));
            settle(tree, view, Size::new(800.0, 480.0), metrics);
            view.handle(tree, &notch(tree, view, 1.0, 0.0), metrics)
                .expect("handled");
            settle(tree, view, Size::new(800.0, 480.0), metrics);
            let (offset, most) = bar(tree, view, Axis::Horizontal);
            assert!(offset > 0.0 && offset == most, "{offset} of {most}");
            tree.invalidate_all();
            let frame = tree
                .frame(Size::new(800.0, 480.0), metrics)
                .expect("a frame");
            let cut = frame.list.commands().iter().any(|command| {
                matches!(command, Command::Text { text, color, .. }
                    if text == CUT_MARKER && *color == tree.theme().color(Token::Text3))
            });
            assert!(cut, "the cut marker shows");
        },
    );
}

#[test]
fn editor_view_400x240_scale_2() {
    render_view(
        "editor_view_400x240_scale_2",
        Size::new(400.0, 240.0),
        2.0,
        &fixture(),
        |tree, view, _| mark(tree, view),
    );
}
