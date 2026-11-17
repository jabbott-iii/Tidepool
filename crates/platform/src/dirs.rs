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

//! The standard folders of outside tools that Tidepool manages: the
//! Android SDK's ([`android_sdk`]) and rustup's ([`rustup_home`]).
//!
//! New in Tidepool; not ported. Each is where the tool itself looks, from
//! its own variable or its default, so that Tidepool finds and installs
//! the tool where the user's other programs see it
//! (`docs/03-platforms.md` → Paths). They are paths only: nothing here
//! makes a folder or checks that one exists; `tidepool-toolchain` detects
//! and installs the tools (`docs/subsystems/toolchains.md`). Tidepool's own
//! folders are in [`paths`](crate::paths).
//!
//! On Linux a variable that is unset, empty or not an absolute path counts
//! as unset, as for the XDG variables: a relative path would be relative to
//! whatever folder Tidepool was started in. The home directory is found as
//! [`user_dirs`](crate::paths::user_dirs) finds it, and only when the
//! default needs it.

use std::path::PathBuf;

use crate::{Error, backend};

/// The Android SDK's folder: `$ANDROID_HOME` if it is set, else
/// `~/Android/Sdk`, where Android Studio installs it on Linux.
///
/// # Errors
///
/// - [`Error::NoHomeDirectory`] on Linux, if the default is needed and the
///   home directory is not known, or not an absolute path.
/// - [`Error::Unsupported`] on every other system, until macOS (M11.8) and
///   Windows (M12.2).
pub fn android_sdk() -> Result<PathBuf, Error> {
    backend::android_sdk()
}

/// rustup's folder, which holds its toolchains: `$RUSTUP_HOME` if it is
/// set, else `~/.rustup`.
///
/// # Errors
///
/// As [`android_sdk`].
pub fn rustup_home() -> Result<PathBuf, Error> {
    backend::rustup_home()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_folders_are_absolute() {
        for found in [android_sdk(), rustup_home()].into_iter().flatten() {
            assert!(found.is_absolute(), "{}", found.display());
        }
    }
}
