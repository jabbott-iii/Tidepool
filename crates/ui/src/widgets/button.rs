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

//! Buttons: each a tile of jabbott.dev's header (`.nav-tile`) in the
//! accent of its [`ButtonKind`], 30 or 24 units high (ADR-0058). [`Button`]
//! describes the tile, its glow, and where it is hit.

use tidepool_core::geom::{Point, Rect, Size};
use tidepool_core::input::{InputEvent, Key, Modifiers, MouseButton};

use crate::a11y::{Role, Semantics};
use crate::draw::{Corners, DrawList, PaintContext};
use crate::input::{ActionKind, EventContext, Handled, is_modifier};
use crate::metrics::TextMetrics;
use crate::theme::{Theme, Token};
use crate::tree::Widget;

use super::{
    BRACKET_THICKNESS, BUTTON_ARM, BUTTON_ARM_SMALL, BUTTON_HEIGHT, BUTTON_HEIGHT_SMALL,
    BUTTON_LABEL, BUTTON_LIFT, BUTTON_LIFT_SMALL, BUTTON_PADDING, BUTTON_PADDING_SMALL,
    BUTTON_REACH_AT_REST, Control, Look, baseline, dimmed, outline,
};

/// The corner glow's ellipse, as the site's `radial-gradient(90% 160% …)`
/// sizes it: how far it reaches from the corner, across and down, as a
/// fraction of the tile's width and of its height.
const GLOW_ACROSS: f32 = 0.9;

/// See [`GLOW_ACROSS`].
const GLOW_DOWN: f32 = 1.6;

/// The site's glow at its corner: the accent at 16%
/// (`color-mix(in srgb, accent 16%, transparent)`).
const GLOW_PEAK: f64 = 0.16;

/// How far out along its gradient the site's glow is transparent: 70%.
const GLOW_END: f64 = 0.7;

/// The most coats a glow is painted in, which bounds what a button paints.
/// The bundled theme's coats of 5/255 need 9; a coat fainter than 3/255
/// reaches less than the site's 16% within this many.
const GLOW_MAX_COATS: usize = 16;

/// How far out along the glow's gradient each coat of a `-haze` token of
/// `alpha` (0 to 255) reaches, the first and largest first, from the
/// corner (0) to where the gradient is transparent ([`GLOW_END`]).
///
/// Coat `k + 1` reaches where the gradient's alpha,
/// `GLOW_PEAK × (1 − reach / GLOW_END)`, is half-way between what `k` and
/// `k + 1` coats make one over another, so that each point is covered by
/// the number of coats that comes nearest the gradient there. Coats stop
/// where that half-way level passes [`GLOW_PEAK`], or at
/// [`GLOW_MAX_COATS`]: 9 coats of the bundled 5/255, 3 of 13/255 (5%),
/// none of a coat stronger than twice the peak or of a transparent one.
/// Only `+ − × ÷` in `f64`, so the reaches are the same on every CPU.
fn glow_reaches(alpha: u8) -> impl Iterator<Item = f32> {
    let coat = f64::from(alpha) / 255.0;
    let mut below = 0.0;
    (0..GLOW_MAX_COATS).map_while(move |_| {
        let above = below + coat * (1.0 - below);
        let middle = (below + above) / 2.0;
        below = above;
        // `as` rounds to the nearest `f32`; the reach is from 0 to 0.7.
        (coat > 0.0 && middle < GLOW_PEAK).then(|| (GLOW_END * (1.0 - middle / GLOW_PEAK)) as f32)
    })
}

/// What a button is for, and so the accent of its tile.
///
/// Each kind's tile is the same `panel` inside a `line-2` border; its
/// accent colors the bracket corners and the focus outline, and gives the
/// glow, the lit border and the lit label their tokens. A tile is lit
/// while hovered, pressed or showing focus ([`Button`] describes the
/// states).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ButtonKind {
    /// The default: the neutral tile, in `text-2`, its glow `neutral-haze`;
    /// lit, a `neutral-line` border and a `neutral-label` label.
    #[default]
    Ghost,
    /// Secondary: the teal tile, in `teal`, its glow `teal-haze`; lit, a
    /// `teal-line` border and a `teal-label` label.
    Teal,
    /// Primary: the amber tile, in `amber`, its glow `amber-haze`; lit, an
    /// `amber-line` border and an `amber-label` label.
    Amber,
    /// Quiet: the neutral tile with no glow.
    Slate,
}

/// The tokens of a kind's tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Accent {
    /// The bracket corners and the focus outline.
    color: Token,
    /// The label, lit.
    label: Token,
    /// The border, lit.
    line: Token,
    /// A coat of the corner glow, if the tile has one.
    haze: Option<Token>,
}

impl ButtonKind {
    const fn accent(self) -> Accent {
        let neutral = Accent {
            color: Token::Text2,
            label: Token::NeutralLabel,
            line: Token::NeutralLine,
            haze: Some(Token::NeutralHaze),
        };
        match self {
            Self::Ghost => neutral,
            Self::Teal => Accent {
                color: Token::Teal,
                label: Token::TealLabel,
                line: Token::TealLine,
                haze: Some(Token::TealHaze),
            },
            Self::Amber => Accent {
                color: Token::Amber,
                label: Token::AmberLabel,
                line: Token::AmberLine,
                haze: Some(Token::AmberHaze),
            },
            Self::Slate => Accent {
                haze: None,
                ..neutral
            },
        }
    }
}

/// How tall a button is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ButtonSize {
    /// [`BUTTON_HEIGHT`] high.
    #[default]
    Regular,
    /// [`BUTTON_HEIGHT_SMALL`] high.
    Small,
}

impl ButtonSize {
    const fn height(self) -> f32 {
        match self {
            Self::Regular => BUTTON_HEIGHT,
            Self::Small => BUTTON_HEIGHT_SMALL,
        }
    }

    const fn padding(self) -> f32 {
        match self {
            Self::Regular => BUTTON_PADDING,
            Self::Small => BUTTON_PADDING_SMALL,
        }
    }

    const fn arm(self) -> f32 {
        match self {
            Self::Regular => BUTTON_ARM,
            Self::Small => BUTTON_ARM_SMALL,
        }
    }

    const fn lift(self) -> f32 {
        match self {
            Self::Regular => BUTTON_LIFT,
            Self::Small => BUTTON_LIFT_SMALL,
        }
    }
}

/// A button with a label, which emits [`ActionKind::Activate`] when it is
/// clicked (pressed and released on it with the primary button), or, while
/// it has focus, when Enter goes down or Space comes up.
///
/// It paints a tile of jabbott.dev's header in its kind's accent, as
/// below. Its label is painted uppercase in [`BUTTON_LABEL`], centered,
/// and cut to its tile: as wide as its label and padding when its parent
/// gives it that, a label wider than the space it is given paints nothing
/// outside it. It is cut rather than ended with an ellipsis, as `ui`'s
/// [`TextMetrics`] cannot shape one, and a label that long is its owner's
/// to shorten. Its accessible name is the label as written. It takes
/// focus.
///
/// ```
/// use tidepool_ui::a11y::Role;
/// use tidepool_ui::widgets::{Button, ButtonKind, ButtonSize, Control};
///
/// let save = Button::new("Save", ButtonKind::Teal).size(ButtonSize::Small);
/// assert_eq!(save.semantics().role, Role::Button);
/// assert_eq!(save.semantics().name, "Save");
/// ```
///
/// # The tile
///
/// A tile is `panel` inside a 1 unit `line-2` border, with bracket corners
/// in its accent over the border at its top-left and bottom-right, and a
/// glow of its accent in its top-right corner. At rest its label is `text`
/// and each bracket's arms reach [`BUTTON_REACH_AT_REST`] of
/// [`BUTTON_ARM`] ([`BUTTON_ARM_SMALL`]). *Lit*, as it is while hovered,
/// pressed or showing focus, the arms reach their whole length, and the
/// border and the label take the accent's `-line` and `-label` tokens: the
/// accent at 55%, and the accent mixed 55% with white. While hovered it
/// also rises [`BUTTON_LIFT`] ([`BUTTON_LIFT_SMALL`]); pressed, it stays
/// down, so a press reads as pushing it back. Showing keyboard focus, it
/// is lit but does not rise, and paints the
/// [focus outline](super::focus_outline) in its accent around itself;
/// hovered as well, it rises, and its outline with it. Disabled, it is the
/// rest look at [`DISABLED_OPACITY`](super::DISABLED_OPACITY).
///
/// The site raises a tile for keyboard focus too. A button does not, as
/// the pointer can be anywhere then: in a column of buttons with no gap, a
/// focused button raised over the one above would show its own tile where
/// a click reaches that one (ADR-0058).
///
/// # Why the glow is a stack of pills
///
/// The site's glow is `radial-gradient(90% 160% at 100% 0%, accent 16%,
/// transparent 70%)`: an ellipse centered on the tile's top-right corner,
/// 90% of the tile's width and 160% of its height out from it, fading from
/// the accent at 16% at the corner to nothing 70% of the way out. The draw
/// list has no gradient (ADR-0058), so a button stands in coats of its
/// `-haze` token: in the bundled theme, nine of the accent at 5/255 (2%),
/// which make 16.3% where all nine overlap. Each coat is a pill (a fully
/// rounded rect) centered on the corner and cut to the tile. Its edge is
/// the gradient's ellipse where the gradient's alpha is half-way between
/// what `k` and `k + 1` coats make, so each point of the tile is covered by
/// the number of coats that comes nearest the gradient there. The coats
/// and their reaches follow from the token's alpha, so a theme with
/// another one still glows at about the site's 16%: three coats of 5%
/// make 14.3%, where nine would make 37%. At most 16 coats are painted.
///
/// Coats that faint keep the step from one ring to the next to about 3 to
/// 4 of a color channel's 256 levels, where four coats of 11/255 showed
/// rings. Pills follow the ellipse's curve, where rects would step in
/// square corners: over tiles 60 to 130 units wide, the root-mean-square
/// difference from the gradient is 0.004 to 0.009 in alpha with nine
/// pills, 0.013 to 0.014 with nine rects, and 0.009 to 0.012 with four
/// pills. The site's blurred shadow under a risen tile is left out.
///
/// # Hit testing and damage
///
/// A risen button paints above its node's bounds, which do not move: the
/// pointer hits it where it was laid out, so the strip it rose from still
/// hovers it and the strip it rose into does not. Were the bounds to rise
/// with it, a pointer resting in that bottom strip would leave it, the
/// button would sink back under the pointer, and the hover would flicker.
/// As only the pointer over it raises it, it sinks back once the pointer
/// moves on to the strip it rose into, where a click reaches whatever is
/// laid out there. The tree damages what a node
/// paints, inside its bounds or not, so the frame that raises a button or
/// lowers it damages both places.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    label: String,
    shown: String,
    kind: ButtonKind,
    size: ButtonSize,
    activator: Activator,
}

impl Button {
    /// A regular button of `kind` labelled `label`.
    pub fn new(label: impl Into<String>, kind: ButtonKind) -> Self {
        let label = label.into();
        Self {
            shown: label.to_uppercase(),
            label,
            kind,
            size: ButtonSize::Regular,
            activator: Activator::default(),
        }
    }

    /// The same, of `size`.
    #[must_use]
    pub fn size(self, size: ButtonSize) -> Self {
        Self { size, ..self }
    }

    /// Its label, as written.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Labels it `label`.
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
        self.shown = self.label.to_uppercase();
    }

    /// Its kind.
    pub const fn kind(&self) -> ButtonKind {
        self.kind
    }

    /// Makes it `kind`.
    pub fn set_kind(&mut self, kind: ButtonKind) {
        self.kind = kind;
    }

    /// Its size.
    pub const fn button_size(&self) -> ButtonSize {
        self.size
    }

    /// Paints the tile at `rect`, lit or not: its `panel`, its glow, its
    /// border and its bracket corners.
    fn paint_tile(&self, theme: &Theme, rect: Rect, lit: bool, list: &mut DrawList) {
        let c = |token| theme.color(token);
        let accent = self.kind.accent();
        list.rect(rect, c(Token::Panel));
        if let Some(haze) = accent.haze {
            // Each coat is centered on the top-right corner, cut to the
            // tile; the tile's border goes over them, as the site's
            // background runs under its border.
            let (right, top) = (rect.x + rect.width, rect.y);
            list.push_clip(rect);
            for reach in glow_reaches(c(haze).a) {
                let across = GLOW_ACROSS * reach * rect.width;
                let down = GLOW_DOWN * reach * rect.height;
                let coat = Rect::new(right - across, top - down, 2.0 * across, 2.0 * down);
                list.pill(coat, c(haze));
            }
            list.pop_clip();
        }
        let (border, arm) = if lit {
            (accent.line, self.size.arm())
        } else {
            (Token::Line2, self.size.arm() * BUTTON_REACH_AT_REST)
        };
        list.border(rect, 1.0, c(border));
        list.brackets(
            rect,
            arm,
            BRACKET_THICKNESS,
            Corners::TOP_LEFT_AND_BOTTOM_RIGHT,
            c(accent.color),
        );
    }
}

impl Widget for Button {
    fn measure(&self, _available: Size, metrics: &dyn TextMetrics) -> Size {
        let width = metrics.width(&self.shown, &BUTTON_LABEL);
        Size::new(width + 2.0 * self.size.padding(), self.size.height())
    }

    fn paint(&self, cx: &PaintContext<'_>, list: &mut DrawList) {
        let state = cx.state();
        let theme = cx.theme();
        let metrics = cx.metrics();
        let accent = self.kind.accent();
        let look = Look::of(state);
        // Focus lights the tile only while it shows (`:focus-visible`),
        // and never a disabled one's. Only the pointer raises it: keyboard
        // focus leaves the pointer anywhere, and a tile raised over the
        // button above would show where a click reaches that one
        // (ADR-0058).
        let focus = state.focus_visible && !state.disabled;
        let lit = look != Look::Rest || focus;
        let risen = look == Look::Hover;
        let bounds = cx.bounds();
        let rect = if risen {
            Rect::new(
                bounds.x,
                bounds.y - self.size.lift(),
                bounds.width,
                bounds.height,
            )
        } else {
            bounds
        };
        dimmed(cx, list, |list| {
            self.paint_tile(theme, rect, lit, list);
            let label = if lit { accent.label } else { Token::Text };
            let width = metrics.width(&self.shown, &BUTTON_LABEL);
            let origin = Point::new(
                rect.x + (rect.width - width) / 2.0,
                baseline(rect, metrics, &BUTTON_LABEL),
            );
            // Cut to the tile: a label wider than the space its parent
            // gives it paints nothing outside it.
            list.push_clip(rect);
            list.text(origin, self.shown.clone(), BUTTON_LABEL, theme.color(label));
            list.pop_clip();
        });
        if state.focus_visible {
            outline(list, rect, theme.color(accent.color));
        }
    }

    fn event(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        self.activator.activate(cx, event)
    }
}

impl Control for Button {
    fn semantics(&self) -> Semantics {
        Semantics::new(Role::Button, self.label.clone())
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// What a clickable control (a button, a chip, a link) remembers between
/// events to know when it is activated, and which events it keeps from its
/// ancestors.
///
/// It is activated by a click, the release of a primary press it captured
/// while the pointer is over it (on the [hit](crate::Tree::hit_test) path,
/// so not where a clip or a node above it hides it); by Enter going down,
/// not repeating; or by Space coming up after it went down on the control,
/// as a browser's button is, so that the space Space types, which comes
/// between, is the control's whatever the owner does on the activation
/// (moving focus to a field, say). Either key counts with no modifier but
/// Shift. Space's activation is called off if, while it is held, keyboard
/// focus moves or the window loses it
/// ([`EventContext::focus_moves`]), so a release the platform reports for
/// a key held as the window lost focus activates nothing, or another key
/// but a modifier goes down, or the pointer presses on the control.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Activator {
    /// Space went down on the control, and has not come up: the count of
    /// focus moves then.
    space: Option<u64>,
}

impl Activator {
    /// Takes `event`, and returns whether it activated the control and
    /// whether the control keeps it from its ancestors: a primary press or
    /// release, Enter and Space going down (repeating or not), Space going
    /// up after it went down here, and the space it types.
    pub(crate) fn event(&mut self, cx: &EventContext<'_>, event: &InputEvent) -> (bool, Handled) {
        let plain = |modifiers: Modifiers| modifiers.difference(Modifiers::SHIFT).is_empty();
        match *event {
            InputEvent::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                self.space = None;
                (false, Handled::Yes)
            }
            InputEvent::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                let state = cx.state();
                (state.pressed && state.hovered, Handled::Yes)
            }
            InputEvent::KeyDown {
                key: Key::Enter | Key::NumpadEnter,
                modifiers,
                repeat,
            } if plain(modifiers) => {
                self.space = None;
                (!repeat, Handled::Yes)
            }
            InputEvent::KeyDown {
                key: Key::Space,
                modifiers,
                repeat,
            } if plain(modifiers) => {
                if !repeat {
                    self.space = Some(cx.focus_moves());
                }
                (false, Handled::Yes)
            }
            InputEvent::KeyUp {
                key: Key::Space, ..
            } => match self.space.take() {
                Some(moves) => (moves == cx.focus_moves(), Handled::Yes),
                None => (false, Handled::No),
            },
            InputEvent::Text(ref text) if text == " " => (false, Handled::Yes),
            InputEvent::KeyDown { key, .. } => {
                if !is_modifier(key) {
                    self.space = None;
                }
                (false, Handled::No)
            }
            _ => (false, Handled::No),
        }
    }

    /// Takes `event` as [`event`](Self::event) does, emitting
    /// [`ActionKind::Activate`] when it activates the control.
    pub(crate) fn activate(&mut self, cx: &mut EventContext<'_>, event: &InputEvent) -> Handled {
        let (activated, handled) = self.event(cx, event);
        if activated {
            cx.emit(ActionKind::Activate);
        }
        handled
    }
}

#[cfg(test)]
mod tests {
    use tidepool_core::color::Color;

    use super::*;

    /// What `count` coats of `alpha` (0 to 255) make, one over another.
    fn coats(alpha: u8, count: usize) -> f64 {
        let coat = f64::from(alpha) / 255.0;
        (0..count).fold(0.0, |below, _| below + coat * (1.0 - below))
    }

    #[test]
    fn the_glow_s_coats_follow_from_the_haze_s_alpha() {
        // The bundled theme's coats of 5/255: nine, reaching as the
        // button's documentation says, which make 16.3%.
        let theme = Theme::tidepool_dark();
        for haze in [Token::NeutralHaze, Token::TealHaze, Token::AmberHaze] {
            assert_eq!(theme.color(haze).a, 5, "{haze}");
        }
        let reaches: Vec<f32> = glow_reaches(5).collect();
        let documented = [
            0.657, 0.572, 0.489, 0.407, 0.327, 0.249, 0.172, 0.096, 0.022,
        ];
        assert_eq!(reaches.len(), documented.len(), "{reaches:?}");
        for (reach, documented) in reaches.iter().zip(documented) {
            assert!((reach - documented).abs() < 0.001, "{reaches:?}");
        }
        assert!((coats(5, 9) - 0.163).abs() < 0.0005);
        // About 5% (13/255): three coats, 14.5%, where nine would make
        // 37.6%.
        assert_eq!(glow_reaches(13).count(), 3);
        assert!((coats(13, 3) - 0.145).abs() < 0.0005);
        assert!((coats(13, 9) - 0.376).abs() < 0.0005);
        // Any coat: each reach inside the gradient and smaller than the
        // last, at most `GLOW_MAX_COATS` of them, which make the site's
        // 16% to within half a coat unless they stop at the most.
        for alpha in 0..=u8::MAX {
            let reaches: Vec<f32> = glow_reaches(alpha).collect();
            assert!(reaches.len() <= GLOW_MAX_COATS, "{alpha}");
            assert!(
                reaches.iter().all(|&reach| reach > 0.0 && reach <= 0.7),
                "{alpha}: {reaches:?}"
            );
            assert!(
                reaches.windows(2).all(|pair| pair[1] < pair[0]),
                "{alpha}: {reaches:?}"
            );
            let made = coats(alpha, reaches.len());
            let half = f64::from(alpha) / 255.0 / 2.0;
            if alpha == 0 {
                assert!(reaches.is_empty());
            } else if reaches.len() < GLOW_MAX_COATS {
                assert!(
                    (made - GLOW_PEAK).abs() <= half,
                    "{alpha}: {} coats make {made}",
                    reaches.len()
                );
            } else {
                assert!(made < GLOW_PEAK, "{alpha}: {made}");
            }
        }
    }

    /// `top` over `below` by source-over, as `gfx` blends it.
    fn over(top: Color, below: Color) -> Color {
        top.premultiply().over(below.premultiply()).unpremultiply()
    }

    /// WCAG 2.x's relative luminance, as `cargo xtask theme` computes it.
    fn luminance(color: Color) -> f64 {
        let linear = |channel: u8| {
            let c = f64::from(channel) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    /// WCAG 2.x's contrast ratio of `a` and `b`.
    fn contrast(a: Color, b: Color) -> f64 {
        let (a, b) = (luminance(a) + 0.05, luminance(b) + 0.05);
        a.max(b) / a.min(b)
    }

    /// What is wrong with `theme`'s stand-ins for the glows: the surfaces
    /// the theme's `[[contrast.use]]` checks the tiles' labels on, in place
    /// of the glows, which `cargo xtask theme` cannot composite.
    ///
    /// Each kind's tile as painted where its glow is strongest, `panel`
    /// over `panel-solid` (the lightest surface a button sits on: a
    /// dialog's, a toast's) under every coat of its glow, must be no
    /// lighter than its stand-in over `panel-solid`, so that the check's
    /// pairs bound the painted ones (the labels are light, so a lighter
    /// surface is the harder one); and its labels, at rest and lit, must
    /// meet 4.5:1 on it.
    fn stand_in_problems(theme: &Theme) -> Vec<String> {
        let c = |token| theme.color(token);
        let host = c(Token::PanelSolid);
        let tile = over(c(Token::Panel), host);
        let mut problems = Vec::new();
        for (kind, stand_in) in [
            (ButtonKind::Ghost, Some(Token::Line2)),
            (ButtonKind::Teal, Some(Token::TealFill)),
            (ButtonKind::Amber, Some(Token::AmberHalo)),
            (ButtonKind::Slate, None),
        ] {
            let accent = kind.accent();
            let mut painted = tile;
            if let Some(haze) = accent.haze {
                for _ in glow_reaches(c(haze).a) {
                    painted = over(c(haze), painted);
                }
            }
            let (name, checked) = match stand_in {
                Some(token) => (token.name(), over(c(token), host)),
                None => ("panel", tile),
            };
            if luminance(checked) < luminance(painted) {
                problems.push(format!(
                    "{kind:?}: `{name}` over `panel-solid` is {checked}, darker than the \
                     glow painted, {painted}"
                ));
            }
            for label in [Token::Text, accent.label] {
                let ratio = contrast(c(label), painted);
                if ratio < 4.5 {
                    problems.push(format!("{kind:?}: `{label}` on {painted} is {ratio:.2}:1"));
                }
            }
        }
        problems
    }

    #[test]
    fn the_contrast_check_s_stand_ins_bound_the_painted_glows() {
        assert_eq!(
            stand_in_problems(&Theme::tidepool_dark()),
            Vec::<String>::new()
        );
        // A `teal-fill` of 10% is darker than the teal glow painted, so it
        // would no longer bound it.
        let from = "teal-fill = { value = \"rgba(92,203,203,0.20)\"";
        assert!(crate::theme::TIDEPOOL_DARK_TOML.contains(from));
        let text = crate::theme::TIDEPOOL_DARK_TOML.replacen(
            from,
            "teal-fill = { value = \"rgba(92,203,203,0.10)\"",
            1,
        );
        let theme = Theme::parse(&text, crate::theme::LIMITS).expect("a theme");
        let problems = stand_in_problems(&theme);
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("`teal-fill`")),
            "{problems:?}"
        );
    }

    #[test]
    fn each_kind_is_a_tile_in_its_accent() {
        let cases = [
            (ButtonKind::Ghost, Token::Text2, Some(Token::NeutralHaze)),
            (ButtonKind::Teal, Token::Teal, Some(Token::TealHaze)),
            (ButtonKind::Amber, Token::Amber, Some(Token::AmberHaze)),
            (ButtonKind::Slate, Token::Text2, None),
        ];
        for (kind, color, haze) in cases {
            let accent = kind.accent();
            assert_eq!((accent.color, accent.haze), (color, haze), "{kind:?}");
        }
        // Slate is the ghost's tile without its glow.
        assert_eq!(
            ButtonKind::Slate.accent(),
            Accent {
                haze: None,
                ..ButtonKind::Ghost.accent()
            }
        );
        // The parts scale from the site's 40 px tile, in whole units.
        for (size, arm, lift) in [
            (ButtonSize::Regular, 12.0, 2.0),
            (ButtonSize::Small, 10.0, 1.0),
        ] {
            assert_eq!((size.arm(), size.lift()), (arm, lift), "{size:?}");
            assert_eq!(size.arm() * BUTTON_REACH_AT_REST, arm / 2.0);
        }
    }
}
