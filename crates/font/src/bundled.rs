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

//! The bundled faces: Saira and JetBrains Mono (ADR-0004), compiled into
//! the binary from `assets/fonts/` and parsed when [`load`]ed, at
//! startup.
//!
//! New in Tidepool; not ported (Nadir draws text with OpenGL).
//!
//! The files are the ones `docs/02-dependency-policy.md` → Assets lists,
//! under the SIL OFL 1.1, whose text is beside them in `assets/fonts/`.
//! Compiling them in means no file is read and no path is looked for at
//! run time; the binary grows by their 1.3 MB.
//!
//! ```
//! # fn main() -> Result<(), tidepool_font::FontError> {
//! use tidepool_font::bundled::{self, Face};
//!
//! let faces = bundled::load()?;
//! assert_eq!(faces.len(), Face::ALL.len());
//! assert_eq!(faces.fallback(), [Face::MonoRegular.id()]);
//! let saira = faces.font(Face::SairaSemiBold.id()).expect("loaded");
//! assert_eq!(saira.os2().map(|os2| os2.weight_class), Some(600));
//! # Ok(())
//! # }
//! ```

use crate::fallback::{FaceId, Faces};
use crate::ttf::Font;
use crate::{FontError, Limits};

/// A bundled face. [`load`] adds each at the id [`Face::id`] gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Face {
    /// Saira Regular, 400: UI body text.
    SairaRegular,
    /// Saira Medium, 500: section titles and the brand's subline.
    SairaMedium,
    /// Saira SemiBold, 600: labels, buttons, eyebrows and the brand.
    SairaSemiBold,
    /// Saira Bold, 700.
    SairaBold,
    /// JetBrains Mono Regular, 400: code, the terminal, paths and counts;
    /// and the fallback for characters Saira lacks.
    MonoRegular,
    /// JetBrains Mono Italic, 400: comments.
    MonoItalic,
    /// JetBrains Mono SemiBold, 600: emphasis in code.
    MonoSemiBold,
}

impl Face {
    /// Every bundled face, in the order of their ids.
    pub const ALL: [Self; 7] = [
        Self::SairaRegular,
        Self::SairaMedium,
        Self::SairaSemiBold,
        Self::SairaBold,
        Self::MonoRegular,
        Self::MonoItalic,
        Self::MonoSemiBold,
    ];

    /// The id [`load`] gives the face: its place in [`Face::ALL`].
    pub const fn id(self) -> FaceId {
        FaceId(self as u16)
    }

    /// The family, as the face's `name` table gives it: its typographic
    /// family (name ID 16), else its family (1), which for Saira Medium,
    /// SemiBold and JetBrains Mono SemiBold names the weight too.
    pub const fn family(self) -> &'static str {
        match self {
            Self::SairaRegular | Self::SairaMedium | Self::SairaSemiBold | Self::SairaBold => {
                "Saira"
            }
            Self::MonoRegular | Self::MonoItalic | Self::MonoSemiBold => "JetBrains Mono",
        }
    }

    /// The weight, as the face's `OS/2` table gives it.
    pub const fn weight(self) -> u16 {
        match self {
            Self::SairaRegular | Self::MonoRegular | Self::MonoItalic => 400,
            Self::SairaMedium => 500,
            Self::SairaSemiBold | Self::MonoSemiBold => 600,
            Self::SairaBold => 700,
        }
    }

    /// Whether the face is italic.
    pub const fn italic(self) -> bool {
        matches!(self, Self::MonoItalic)
    }

    /// The file's path under `assets/fonts/`.
    pub const fn path(self) -> &'static str {
        match self {
            Self::SairaRegular => "saira/Saira-Regular.ttf",
            Self::SairaMedium => "saira/Saira-Medium.ttf",
            Self::SairaSemiBold => "saira/Saira-SemiBold.ttf",
            Self::SairaBold => "saira/Saira-Bold.ttf",
            Self::MonoRegular => "jetbrains-mono/JetBrainsMono-Regular.ttf",
            Self::MonoItalic => "jetbrains-mono/JetBrainsMono-Italic.ttf",
            Self::MonoSemiBold => "jetbrains-mono/JetBrainsMono-SemiBold.ttf",
        }
    }

    /// The file's bytes, compiled in.
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Self::SairaRegular => include_bytes!("../../../assets/fonts/saira/Saira-Regular.ttf"),
            Self::SairaMedium => include_bytes!("../../../assets/fonts/saira/Saira-Medium.ttf"),
            Self::SairaSemiBold => {
                include_bytes!("../../../assets/fonts/saira/Saira-SemiBold.ttf")
            }
            Self::SairaBold => include_bytes!("../../../assets/fonts/saira/Saira-Bold.ttf"),
            Self::MonoRegular => {
                include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
            }
            Self::MonoItalic => {
                include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf")
            }
            Self::MonoSemiBold => {
                include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-SemiBold.ttf")
            }
        }
    }

    /// Parses the face with [`Limits::DEFAULT`], borrowing its bytes.
    ///
    /// # Errors
    ///
    /// What [`Font::parse`] returns; never, for the files bundled, which a
    /// test parses.
    pub fn parse(self) -> Result<Font, FontError> {
        Font::parse(self.bytes(), Limits::DEFAULT)
    }
}

/// Parses every bundled face into a new [`Faces`], each at its
/// [`Face::id`], with the fallback chain JetBrains Mono Regular. A system
/// face, when one is configured, is added after them and put after
/// JetBrains Mono in the chain ([`Faces::set_fallback`]).
///
/// Parsing the seven takes well under a millisecond: 0.04 ms in a release
/// build on the development machine.
///
/// # Errors
///
/// What [`Font::parse`] returns; never, for the files bundled, which a
/// test parses.
pub fn load() -> Result<Faces, FontError> {
    let mut faces = Faces::new();
    for face in Face::ALL {
        let id = faces.add(face.parse()?)?;
        debug_assert_eq!(
            id,
            face.id(),
            "the faces are added in the order of their ids"
        );
    }
    faces.set_fallback(&[Face::MonoRegular.id()])?;
    Ok(faces)
}
