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

//! `cargo xtask golden [--bless <scene>...]`: draws the golden scenes and
//! compares each with its reference, `tests/golden/<scene>.qoi` (ADR-0044,
//! `docs/04-quality.md` → "Golden frames"). Without `--bless` it is also a
//! step of `ci` ([`check`]).
//!
//! Ported from nadir xtask `golden.rs`. Adapted for Tidepool: the scenes
//! are CPU scenes, so no renderer is named or checked; each drawing crate
//! has one tolerance ([`Scenes::tolerance`]), 0 for `gfx`, and no pixel may
//! differ by more, where Nadir let 0.1% of them; each crate's scenes write
//! into a directory of their own, so that a name two crates draw is
//! caught; `--bless` writes the references of the scenes it names, and no
//! others; and what a run finds is a [`Report`], which `ci` prints as it
//! prints its other steps.
//!
//! The scenes are tests of the crates that draw: `tests/golden.rs`, one
//! test per scene, named after it. The drawing crates are the workspace's
//! members with a test target named `golden`, which `cargo metadata`
//! lists; each has a row in [`SCENES`] with its tolerance. This tool may
//! depend only on `tidepool-core` and `tidepool-codec`, so it runs them
//! with `cargo test` (ADR-0017, extended by ADR-0044), naming a directory
//! in [`DIR_VAR`], and each scene writes what it drew there as a PAM
//! (Netpbm) file: the framebuffer's premultiplied bytes in RGBA order, rows
//! from the top. The PAM reader, the comparison, the diff images and the
//! QOI files are this tool's.
//!
//! What it writes: `target/golden/`, which it empties at the start of each
//! run that has a drawing crate (the scenes' PAM files, and an `actual` and
//! a `diff` image for each scene that fails); and with `--bless`, the references of the scenes
//! named. It refuses a symbolic link on the way to either, checking again
//! before it writes; it creates each image as a new file, which the OS
//! refuses at a link, and writes each reference to a new file that it
//! renames over the old one, which replaces a link instead of writing
//! through it.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use tidepool_codec::qoi::{self, Channels, ColorSpace, Header};
use tidepool_codec::{CodecError, Limits};
use tidepool_core::cli::Args;

use crate::cargo;
use crate::check::{self, Report};
use crate::error::Error;
use crate::metadata::{self, Metadata, Package, Resolve};
use crate::walk;

/// The command's name.
pub(crate) const NAME: &str = "golden";

/// The references, relative to the repository root.
const REFERENCES: &str = "tests/golden";

/// What this tool writes, relative to the repository root. It is emptied
/// at the start of each run that has a drawing crate, and otherwise left
/// as it is ([`run`]).
const OUTPUT: &str = "target/golden";

/// The directory inside [`OUTPUT`] that the scenes write to, one directory
/// per drawing crate, named after its package.
const RENDERED: &str = "rendered";

/// Names the directory a crate's scenes write to. Unset or empty, they
/// draw but write nothing.
pub(crate) const DIR_VAR: &str = "TIDEPOOL_GOLDEN_DIR";

/// The largest side of a scene or a reference, in pixels. Scenes are small
/// on purpose; the largest so far is 1,344 × 472 (`font_mono_scale_2`).
const MAX_SIDE: u32 = 4096;

/// The most bytes of pixels one scene or reference may hold:
/// [`MAX_SIDE`]² RGBA, 64 MiB.
const MAX_PIXEL_BYTES: usize = 4096 * 4096 * 4;

/// The longest PAM or QOI file read: [`MAX_PIXEL_BYTES`] and a header. A
/// QOI file is at most 5 bytes a pixel, but one that large is no golden.
const MAX_FILE_BYTES: u64 = MAX_PIXEL_BYTES as u64 + 1024;

/// The longest PAM header read, in bytes.
const MAX_HEADER_BYTES: usize = 1024;

/// The longest scene name, in bytes.
const MAX_NAME_BYTES: usize = 64;

/// A crate whose `tests/golden.rs` draws scenes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Scenes {
    /// Its package.
    pub(crate) package: &'static str,
    /// The largest difference in a channel that still counts as the same.
    pub(crate) tolerance: u8,
}

impl Scenes {
    /// The arguments of the cargo command that draws its scenes.
    fn args(&self) -> [&'static str; 5] {
        ["test", "--package", self.package, "--test", "golden"]
    }
}

/// The crates that draw scenes, in the order they run, with their
/// tolerances: `gfx` exactly (M2.4); `font` within 1, for the coverage
/// its scenes blend themselves (M2.5); `gui` exactly, painting `ui`'s
/// draw lists with `gfx` and `font` (M3.5, ADR-0048); and the `tidepool`
/// binary exactly, painting `editor`'s shell, as the window shows it
/// (M3.4, ADR-0060), and its editor view (M4.1, ADR-0063), with `gui`'s
/// painter. A crate draws scenes when it is a member of the workspace
/// with a test target named `golden` ([`select`]); each such crate needs
/// its row here.
pub(crate) const SCENES: &[Scenes] = &[
    Scenes {
        package: "tidepool-gfx",
        tolerance: 0,
    },
    Scenes {
        package: "tidepool-font",
        tolerance: 1,
    },
    Scenes {
        package: "tidepool-gui",
        tolerance: 0,
    },
    Scenes {
        package: "tidepool",
        tolerance: 0,
    },
];

/// The name of the test target that holds a crate's scenes.
const TEST: &str = "golden";

/// The step of `ci`: draws every scene and compares it with its reference,
/// blessing nothing.
///
/// # Errors
///
/// The errors of [`metadata::load`]; [`Error::Spawn`] if cargo cannot be
/// started; [`Error::Read`] or [`Error::Write`] if `target/golden/` or
/// `tests/golden/` cannot be read or written, or one of them, or a
/// directory on the way, is a symbolic link.
pub(crate) fn check(root: &Path) -> Result<Report, Error> {
    golden(root, &BTreeSet::new())
}

/// Finds the crates that draw scenes in the workspace at `root`, then
/// [`run`]s them, blessing the scenes in `bless`.
fn golden(root: &Path, bless: &BTreeSet<String>) -> Result<Report, Error> {
    let metadata = metadata::load(root, &root.join("Cargo.toml"), Resolve::No)?;
    let Selection { drawing, report } = select(&metadata, SCENES);
    run(root, &drawing, bless, report, &mut draw)
}

/// The crates of a workspace that draw scenes, and what choosing them
/// found.
#[derive(Debug, Default, PartialEq, Eq)]
struct Selection {
    /// The crates to run, in the order of [`SCENES`].
    drawing: Vec<Scenes>,
    /// A note for each row of the table that is not in the workspace, and a
    /// violation for each member that has a `golden` test and no row, or a
    /// row and no `golden` test.
    report: Report,
}

/// Chooses the crates of `table` that draw scenes in the workspace
/// `metadata` describes: those that are members with a test target named
/// [`TEST`]. A row whose package is not a member is noted and skipped, as
/// in the fixtures of `xtask`'s own tests; a member with that test and no
/// row, or a row whose member has no such test, is a violation.
fn select(metadata: &Metadata, table: &[Scenes]) -> Selection {
    let has_scenes = |package: &Package| {
        package
            .targets
            .iter()
            .any(|target| target.name == TEST && target.kind == "test")
    };
    let members: BTreeMap<&str, &Package> = metadata
        .packages
        .iter()
        .filter(|package| package.is_member)
        .map(|package| (package.name.as_str(), package))
        .collect();
    let mut selection = Selection::default();
    for row in table {
        match members.get(row.package) {
            Some(package) if has_scenes(package) => selection.drawing.push(*row),
            Some(_) => selection.report.violations.push(format!(
                "{} has a row in SCENES (xtask/src/golden.rs), but no test target named \
                 `{TEST}` (tests/{TEST}.rs)",
                row.package
            )),
            None => selection.report.notes.push(format!(
                "{NAME}: {} is not a member of this workspace, so it draws nothing",
                row.package
            )),
        }
    }
    for (name, package) in &members {
        if has_scenes(package) && !table.iter().any(|row| row.package == *name) {
            selection.report.violations.push(format!(
                "{name} has a test target named `{TEST}`, but no row in SCENES \
                 (xtask/src/golden.rs), which gives its tolerance"
            ));
        }
    }
    selection
}

/// Reads the arguments that follow `golden`: `--bless` and the names of the
/// scenes to bless, which are returned, or none.
///
/// # Errors
///
/// [`Error::Usage`] for a name without `--bless`, `--bless` without a name,
/// a name that is not a scene's, or any other option.
pub(crate) fn parse(mut args: Args) -> Result<BTreeSet<String>, Error> {
    let bless = args.flag("--bless");
    let names = args
        .finish()
        .map_err(|error| Error::Usage(error.to_string()))?;
    if let (false, Some(name)) = (bless, names.first()) {
        return Err(Error::Usage(format!(
            "`{NAME}` takes scene names only after `--bless`, but got `{name}`"
        )));
    }
    if bless && names.is_empty() {
        return Err(Error::Usage(format!(
            "`--bless` needs the names of the scenes to bless, once their images in {OUTPUT}/ \
             have been looked at"
        )));
    }
    if let Some(name) = names.iter().find(|name| !is_scene_name(name)) {
        return Err(Error::Usage(format!(
            "`{name}` is not a scene name: 1 to {MAX_NAME_BYTES} of a-z, 0-9 and _"
        )));
    }
    Ok(names.into_iter().collect())
}

/// Runs `golden`, blessing the scenes in `bless`, and prints what it found
/// as `ci` prints a step. Returns whether it passed.
///
/// # Errors
///
/// [`Error::Output`] if standard output cannot be written.
pub(crate) fn command(
    bless: &BTreeSet<String>,
    root: &Path,
    out: &mut dyn Write,
) -> Result<bool, Error> {
    // The scenes' test output goes to the terminal directly.
    out.flush().map_err(Error::Output)?;
    let result = golden(root, bless);
    let passed = check::print(NAME, result, out).map_err(Error::Output)?;
    out.flush().map_err(Error::Output)?;
    Ok(passed)
}

/// Runs the cargo command that draws `scenes`' scenes, into `dir`. Returns
/// whether it succeeded.
fn draw(root: &Path, scenes: &Scenes, dir: &Path) -> Result<bool, Error> {
    cargo::run_with_env(root, &scenes.args(), &[(DIR_VAR, dir.as_os_str())])
}

/// What draws one crate's scenes into a directory: [`draw`], or a fake in
/// the tests. Returns whether the command succeeded.
type Draw<'a> = dyn FnMut(&Path, &Scenes, &Path) -> Result<bool, Error> + 'a;

/// Empties [`OUTPUT`], has `draw` draw each of `scenes` into its directory,
/// then compares what they drew with the references, blesses the scenes
/// in `bless`, and writes the images of the scenes that fail. What it finds
/// is added to `report`, which a violation already in it keeps from
/// blessing. With no `scenes`, [`OUTPUT`] is left as it is.
fn run(
    root: &Path,
    scenes: &[Scenes],
    bless: &BTreeSet<String>,
    mut report: Report,
    draw: &mut Draw<'_>,
) -> Result<Report, Error> {
    let mut failed_commands = Vec::new();
    let drawn = if scenes.is_empty() {
        BTreeMap::new()
    } else {
        let rendered = prepare_output(root, scenes)?;
        for crate_scenes in scenes {
            if !draw(root, crate_scenes, &rendered.join(crate_scenes.package))? {
                failed_commands.push(cargo::display(&crate_scenes.args()));
            }
        }
        for command in &failed_commands {
            report
                .violations
                .push(format!("`{command}` failed; see its output above"));
        }
        read_rendered(root, &rendered, scenes, &mut report.violations)?
    };
    let references = read_references(root, &mut report.violations)?;
    let unknown: Vec<&String> = bless
        .iter()
        .filter(|name| !drawn.contains_key(name.as_str()))
        .collect();
    for name in &unknown {
        report
            .violations
            .push(format!("cannot bless `{name}`: no scene drew it"));
    }
    // Blessing writes nothing unless the run is otherwise sound: every
    // command passed, every file read, and every name is a scene's.
    let blessing = !bless.is_empty() && report.violations.is_empty();
    if !bless.is_empty() && !blessing {
        report
            .notes
            .push(format!("{NAME}: blessing nothing, for the reasons below"));
    }
    if scenes.is_empty() {
        report
            .notes
            .push(format!("{NAME}: no crate in this workspace draws scenes"));
    } else {
        report.notes.push(format!(
            "{NAME}: {} drawn by {}",
            plural(drawn.len(), "scene"),
            scenes
                .iter()
                .map(|crate_scenes| crate_scenes.package)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let names: BTreeSet<&str> = drawn
        .keys()
        .chain(references.keys())
        .map(String::as_str)
        .collect();
    // Crates that draw and drew nothing, with nothing to compare: their
    // command did not run their scenes. A workspace with no such crate and
    // no references has nothing to compare, and passes.
    if names.is_empty() && !scenes.is_empty() && report.violations.is_empty() {
        report.violations.push(
            "no scene drew anything, and there are no references: a run that compares nothing \
             does not pass"
                .to_string(),
        );
    }
    let width = names.iter().map(|name| name.len()).max().unwrap_or(0);
    let mut written = false;
    // The references to bless, written once every scene is judged.
    let mut pending = Vec::new();
    for name in names {
        let scene = drawn.get(name);
        // One reference at a time is held, never all of them.
        let reference = match references.get(name) {
            Some(path) => Some(read_bytes(path, MAX_FILE_BYTES)?),
            None => None,
        };
        let verdict = judge(scene, reference.as_deref());
        match (scene, blessing && bless.contains(name)) {
            // Only an exact match is already blessed: within a tolerance
            // above 0, the reference may still differ.
            (Some(_), true) if verdict == (Verdict::Same { largest: 0 }) => {
                report
                    .notes
                    .push(format!("  {name:<width$}  ok: already its reference"));
            }
            (Some(scene), true) => {
                let path = root.join(REFERENCES).join(format!("{name}.qoi"));
                let file = encode_for(&path, scene.width, scene.height, &scene.pixels)?;
                let kind = if verdict == Verdict::NoReference {
                    "new"
                } else {
                    "updated"
                };
                // Filled in once the reference is written.
                report.notes.push(String::new());
                pending.push((report.notes.len() - 1, name, kind, file));
            }
            _ if verdict == (Verdict::Same { largest: 0 }) => {
                report.notes.push(format!("  {name:<width$}  ok"));
            }
            _ if let Verdict::Same { largest } = verdict => {
                report.notes.push(format!(
                    "  {name:<width$}  ok, within the tolerance: differs by up to {largest}"
                ));
            }
            _ => {
                if let Some(scene) = scene {
                    save_images(root, name, scene, reference.as_deref(), &verdict)?;
                    written = true;
                }
                let scenes_failed = !failed_commands.is_empty();
                report.violations.push(format!(
                    "{name}: {}",
                    failure(name, &verdict, scenes_failed)
                ));
            }
        }
    }
    write_blessed(
        pending,
        width,
        &mut |name, file| write_reference(root, name, file),
        &mut report,
    );
    if written {
        report.notes.push(format!(
            "{NAME}: once a failing scene's images show what it should draw, bless it: cargo \
             xtask {NAME} --bless <scene>"
        ));
    }
    Ok(report)
}

/// A reference to bless: the slot of its line among the notes, the scene's
/// name, whether it is `new` or `updated`, and the QOI file.
type Pending<'a> = (usize, &'a str, &'static str, Vec<u8>);

/// What writes a reference: [`write_reference`], or a fake in the tests.
type WriteReference<'a> = dyn FnMut(&str, &[u8]) -> Result<(), Error> + 'a;

/// Writes the `pending` references in order with `write`, filling in each
/// one's line among `report`'s notes. The first that cannot be written
/// stops the rest, with a violation that names the references written
/// before it.
fn write_blessed(
    pending: Vec<Pending<'_>>,
    width: usize,
    write: &mut WriteReference<'_>,
    report: &mut Report,
) {
    let mut blessed: Vec<&str> = Vec::new();
    let mut stopped = false;
    for (slot, name, kind, file) in pending {
        let line = if stopped {
            "not blessed: an earlier reference could not be written".to_string()
        } else if let Err(error) = write(name, &file) {
            stopped = true;
            let before = if blessed.is_empty() {
                "none".to_string()
            } else {
                blessed.join(", ")
            };
            report.violations.push(format!(
                "cannot bless `{name}`: {error}; the references blessed before it: {before}"
            ));
            "not blessed: its reference could not be written".to_string()
        } else {
            blessed.push(name);
            format!("blessed: {kind} {REFERENCES}/{name}.qoi")
        };
        if let Some(note) = report.notes.get_mut(slot) {
            *note = format!("  {name:<width$}  {line}");
        }
    }
}

/// Whether `name` can name a scene: 1 to [`MAX_NAME_BYTES`] bytes of
/// `a-z`, `0-9` and `_`. Names are then safe in a path and a message.
fn is_scene_name(name: &str) -> bool {
    (1..=MAX_NAME_BYTES).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// `count` and `noun`, with an `s` unless `count` is 1.
fn plural(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        count => format!("{count} {noun}s"),
    }
}

/// An error that `path`, a directory or file this tool reads or writes by
/// its fixed name, is a symbolic link.
fn link_error(root: &Path, path: &Path) -> Error {
    Error::Read {
        path: path.to_path_buf(),
        source: io::Error::other(format!(
            "{} is a symbolic link, which xtask does not follow",
            walk::relative(root, path)
        )),
    }
}

/// Fails if `root/<relative>`, or a directory on the way to it, is a
/// symbolic link. What does not exist is not a link.
fn refuse_links(root: &Path, relative: &str) -> Result<(), Error> {
    let mut at = root.to_path_buf();
    for component in relative.split('/') {
        at.push(component);
        if fs::symlink_metadata(&at).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(link_error(root, &at));
        }
    }
    Ok(())
}

/// Empties [`OUTPUT`] and makes a directory in it for each crate's scenes.
/// Returns the directory that holds those.
fn prepare_output(root: &Path, scenes: &[Scenes]) -> Result<PathBuf, Error> {
    refuse_links(root, OUTPUT)?;
    let output = root.join(OUTPUT);
    match fs::remove_dir_all(&output) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(Error::Write {
                path: output,
                source: error,
            });
        }
        _ => {}
    }
    let rendered = output.join(RENDERED);
    for crate_scenes in scenes {
        let dir = rendered.join(crate_scenes.package);
        fs::create_dir_all(&dir).map_err(|source| Error::Write { path: dir, source })?;
    }
    Ok(rendered)
}

/// Reads `path`, at most `limit` bytes of it. A file that grows while it is
/// read is still cut off at the bound.
fn read_bytes(path: &Path, limit: u64) -> Result<Vec<u8>, Error> {
    let read_error = |source| Error::Read {
        path: path.to_path_buf(),
        source,
    };
    let file = fs::File::open(path).map_err(read_error)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(read_error)?;
    if bytes.len() as u64 > limit {
        return Err(read_error(io::Error::other(format!(
            "the file is larger than the {limit}-byte limit"
        ))));
    }
    Ok(bytes)
}

/// The entries of the directory `dir`, sorted by name: each name, whether
/// it is a regular file (never following a link), and whether it is a
/// link.
fn entries(dir: &Path) -> Result<Vec<(std::ffi::OsString, bool, bool)>, Error> {
    let read_error = |source| Error::Read {
        path: dir.to_path_buf(),
        source,
    };
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        let file_type = entry.file_type().map_err(read_error)?;
        entries.push((
            entry.file_name(),
            file_type.is_file(),
            file_type.is_symlink(),
        ));
    }
    entries.sort();
    Ok(entries)
}

/// The name of the scene a file named `file` in a directory of scenes or
/// references holds, if it is `<scene>.<extension>` with a scene name.
fn scene_of<'a>(file: &'a OsStr, extension: &str) -> Option<&'a str> {
    file.to_str()?
        .strip_suffix(extension)
        .and_then(|stem| stem.strip_suffix('.'))
        .filter(|name| is_scene_name(name))
}

/// What one scene drew: RGBA8 pixels, premultiplied, from the top row.
#[derive(Debug, PartialEq, Eq)]
struct Rendered {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    /// The tolerance of the crate that drew it.
    tolerance: u8,
}

/// Reads what the scenes wrote into `rendered`, one directory per crate.
/// A file that is not a scene's PAM, and a scene that two crates drew, are
/// violations.
fn read_rendered(
    root: &Path,
    rendered: &Path,
    scenes: &[Scenes],
    violations: &mut Vec<String>,
) -> Result<BTreeMap<String, Rendered>, Error> {
    let mut drawn: BTreeMap<String, Rendered> = BTreeMap::new();
    let mut drawn_by: BTreeMap<String, &str> = BTreeMap::new();
    for crate_scenes in scenes {
        let dir = rendered.join(crate_scenes.package);
        for (file, is_file, _) in entries(&dir)? {
            let path = dir.join(&file);
            let shown = walk::relative(root, &path);
            let Some(name) = scene_of(&file, "pam").filter(|_| is_file) else {
                violations.push(format!(
                    "{shown}: not a scene's image; a scene writes a regular file named \
                     <scene>.pam, with a name of 1 to {MAX_NAME_BYTES} of a-z, 0-9 and _"
                ));
                continue;
            };
            let scene = match parse_pam(&read_bytes(&path, MAX_FILE_BYTES)?) {
                Ok((width, height, pixels)) => Rendered {
                    width,
                    height,
                    pixels,
                    tolerance: crate_scenes.tolerance,
                },
                Err(problem) => {
                    violations.push(format!("{shown}: {problem}"));
                    continue;
                }
            };
            if let Some(first) = drawn_by.get(name) {
                violations.push(format!(
                    "{name}: drawn by both {first} and {}; scene names must be unique",
                    crate_scenes.package
                ));
                continue;
            }
            drawn_by.insert(name.to_string(), crate_scenes.package);
            drawn.insert(name.to_string(), scene);
        }
    }
    Ok(drawn)
}

/// Finds the references: each `tests/golden/<scene>.qoi`, by scene name.
/// Any other entry there is a violation. No references at all, or no
/// directory, is an empty map. Each is read when it is compared.
fn read_references(
    root: &Path,
    violations: &mut Vec<String>,
) -> Result<BTreeMap<String, PathBuf>, Error> {
    refuse_links(root, REFERENCES)?;
    let dir = root.join(REFERENCES);
    if fs::symlink_metadata(&dir).is_err_and(|error| error.kind() == io::ErrorKind::NotFound) {
        return Ok(BTreeMap::new());
    }
    let mut references = BTreeMap::new();
    for (file, is_file, is_link) in entries(&dir)? {
        let path = dir.join(&file);
        let shown = walk::relative(root, &path);
        match scene_of(&file, "qoi") {
            Some(name) if is_file => {
                references.insert(name.to_string(), path);
            }
            _ if is_link => violations.push(format!(
                "{shown}: a symbolic link, which xtask does not follow; a reference is a regular \
                 file"
            )),
            _ => violations.push(format!(
                "{shown}: not a reference; a reference is a regular file named <scene>.qoi, \
                 with a name of 1 to {MAX_NAME_BYTES} of a-z, 0-9 and _; rename it or remove it"
            )),
        }
    }
    Ok(references)
}

/// Reads a PAM file as the scenes write it: `P7`, then `WIDTH`, `HEIGHT`,
/// `DEPTH 4`, `MAXVAL 255` and `TUPLTYPE RGB_ALPHA`, each once, in any
/// order, with comment lines allowed, then `ENDHDR` and exactly width ×
/// height × 4 bytes. Returns the size and the pixels, or what is wrong.
fn parse_pam(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    const END: &[u8] = b"ENDHDR\n";
    let search = &bytes[..bytes.len().min(MAX_HEADER_BYTES)];
    let end = search
        .windows(END.len())
        .enumerate()
        .find(|&(at, window)| window == END && (at == 0 || search[at - 1] == b'\n'))
        .map(|(at, _)| at)
        .ok_or_else(|| format!("no `ENDHDR` line in the first {MAX_HEADER_BYTES} bytes"))?;
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| "the header is not UTF-8")?;
    let mut lines = header.lines();
    if lines.next() != Some("P7") {
        return Err("not a PAM file: the first line is not `P7`".to_string());
    }
    let mut fields: BTreeMap<&str, &str> = BTreeMap::new();
    for line in lines {
        if line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once(' ')
            .ok_or_else(|| format!("the header line `{line}` is not `<KEY> <value>`"))?;
        if !matches!(key, "WIDTH" | "HEIGHT" | "DEPTH" | "MAXVAL" | "TUPLTYPE") {
            return Err(format!("the header has an unknown field `{key}`"));
        }
        if fields.insert(key, value).is_some() {
            return Err(format!("the header gives `{key}` twice"));
        }
    }
    let field = |key: &str| {
        fields
            .get(key)
            .copied()
            .ok_or_else(|| format!("the header has no `{key}`"))
    };
    let side = |key: &str| -> Result<u32, String> {
        let value = field(key)?;
        match value.parse::<u32>() {
            Ok(side) if (1..=MAX_SIDE).contains(&side) && !value.starts_with('+') => Ok(side),
            _ => Err(format!(
                "`{key}` is `{value}`, not a whole number from 1 to {MAX_SIDE}"
            )),
        }
    };
    let (width, height) = (side("WIDTH")?, side("HEIGHT")?);
    for (key, wanted) in [("DEPTH", "4"), ("MAXVAL", "255"), ("TUPLTYPE", "RGB_ALPHA")] {
        let value = field(key)?;
        if value != wanted {
            return Err(format!("`{key}` is `{value}`, not `{wanted}`"));
        }
    }
    // At most 4096 × 4096 × 4, which fits a `usize` on every target.
    let len = width as usize * height as usize * 4;
    let pixels = &bytes[end + END.len()..];
    if pixels.len() != len {
        return Err(format!(
            "the pixels are {} bytes, not {width} × {height} × 4",
            pixels.len()
        ));
    }
    Ok((width, height, pixels.to_vec()))
}

/// The limits a reference is decoded within.
const fn reference_limits() -> Limits {
    Limits {
        max_input_bytes: MAX_FILE_BYTES as usize,
        max_image_dimension: MAX_SIDE,
        max_decoded_bytes: MAX_PIXEL_BYTES,
        ..Limits::DEFAULT
    }
}

/// How a scene compares with its reference.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// No pixel differs by more than the tolerance.
    Same {
        /// The largest difference in any channel: 0 when the scene is
        /// exactly its reference.
        largest: u8,
    },
    /// Some pixels differ by more than the tolerance.
    Differs {
        /// How many.
        over: usize,
        /// How many pixels there are.
        total: usize,
        /// The largest difference in any channel.
        largest: u8,
    },
    /// The reference has another size.
    OtherSize { width: u32, height: u32 },
    /// The reference does not decode.
    BadReference(String),
    /// There is no reference for the scene.
    NoReference,
    /// There is a reference, but no scene drew it.
    NotRendered,
}

/// Compares what a scene drew with its reference, when there are both.
fn judge(scene: Option<&Rendered>, reference: Option<&[u8]>) -> Verdict {
    let (Some(scene), Some(reference)) = (scene, reference) else {
        return match scene {
            Some(_) => Verdict::NoReference,
            None => Verdict::NotRendered,
        };
    };
    let image = match qoi::decode(reference, reference_limits()) {
        Ok(image) => image,
        Err(error) => return Verdict::BadReference(error.to_string()),
    };
    let Header { width, height, .. } = image.header;
    if (width, height) != (scene.width, scene.height) {
        return Verdict::OtherSize { width, height };
    }
    let (over, largest) = count_over(&image.pixels, &scene.pixels, scene.tolerance);
    if over == 0 {
        Verdict::Same { largest }
    } else {
        Verdict::Differs {
            over,
            total: scene.pixels.len() / 4,
            largest,
        }
    }
}

/// The number of pixels with a channel that differs by more than
/// `tolerance`, and the largest difference in any channel.
fn count_over(expected: &[u8], actual: &[u8], tolerance: u8) -> (usize, u8) {
    let mut over = 0;
    let mut largest = 0;
    for (expected, actual) in expected
        .as_chunks::<4>()
        .0
        .iter()
        .zip(actual.as_chunks::<4>().0)
    {
        let delta = delta(expected, actual);
        largest = largest.max(delta);
        over += usize::from(delta > tolerance);
    }
    (over, largest)
}

/// The largest difference in any channel of two pixels.
fn delta(expected: &[u8; 4], actual: &[u8; 4]) -> u8 {
    expected
        .iter()
        .zip(actual)
        .map(|(&expected, &actual)| expected.abs_diff(actual))
        .max()
        .unwrap_or(0)
}

/// An image of where two images differ: magenta where a pixel differs by
/// more than `tolerance`, and elsewhere the expected pixel in a dim gray.
/// It is opaque everywhere, so that viewers show it whatever the scene's
/// alpha.
fn diff_image(expected: &[u8], actual: &[u8], tolerance: u8) -> Vec<u8> {
    let expected = expected.as_chunks::<4>().0;
    let actual = actual.as_chunks::<4>().0;
    let mut image = Vec::with_capacity(expected.len() * 4);
    for (expected, actual) in expected.iter().zip(actual) {
        if delta(expected, actual) > tolerance {
            image.extend_from_slice(&[255, 0, 255, 255]);
        } else {
            let [r, g, b, _] = expected.map(u16::from);
            // At most 765 / 12, which fits a byte.
            let gray = u8::try_from((r + g + b) / 12).unwrap_or(u8::MAX);
            image.extend_from_slice(&[gray, gray, gray, 255]);
        }
    }
    image
}

/// `pixels` of `width` × `height` as a QOI file: RGBA, sRGB.
fn encode(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, CodecError> {
    let header = Header {
        width,
        height,
        channels: Channels::Rgba,
        color_space: ColorSpace::Srgb,
    };
    qoi::encode(&header, pixels)
}

/// [`encode`] for the file at `path`. The PAM reader checks every scene's
/// size, so a failure here is a bug in this tool.
fn encode_for(path: &Path, width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, Error> {
    encode(width, height, pixels).map_err(|error| Error::Write {
        path: path.to_path_buf(),
        source: io::Error::other(format!("the image does not encode: {error}")),
    })
}

/// Writes `bytes` to a new file at `path`. Nothing may be there, not even
/// a link, which the OS checks as it creates the file
/// (`O_EXCL`, `CREATE_NEW`), so nothing is written through a link. A file
/// it created and could not fill is removed.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let write_error = |source| Error::Write {
        path: path.to_path_buf(),
        source,
    };
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(write_error)?;
    if let Err(source) = file.write_all(bytes) {
        drop(file);
        // Best effort: the error is the write's. `create_new` made this
        // file, so the remove takes nothing that was there before.
        let _ = fs::remove_file(path);
        return Err(write_error(source));
    }
    Ok(())
}

/// Writes `file`, the reference of the scene `name`: into a new file
/// beside it, `.<name>.qoi.tmp`, then renamed over it. A rename replaces a
/// link, or a hard link, at that name rather than writing through it. A
/// temp file left by a run that stopped is not a reference, and so fails
/// the next run until it is removed.
fn write_reference(root: &Path, name: &str, file: &[u8]) -> Result<(), Error> {
    refuse_links(root, REFERENCES)?;
    let dir = root.join(REFERENCES);
    fs::create_dir_all(&dir).map_err(|source| Error::Write {
        path: dir.clone(),
        source,
    })?;
    let temp = dir.join(format!(".{name}.qoi.tmp"));
    write_new(&temp, file)?;
    let path = dir.join(format!("{name}.qoi"));
    fs::rename(&temp, &path).map_err(|source| {
        // Best effort: the error is the rename's.
        let _ = fs::remove_file(&temp);
        Error::Write { path, source }
    })
}

/// Writes `<name>.actual.qoi` for a scene that failed, and
/// `<name>.diff.qoi` when its reference decodes and has its size, into
/// [`OUTPUT`], which must still not be a link.
fn save_images(
    root: &Path,
    name: &str,
    scene: &Rendered,
    reference: Option<&[u8]>,
    verdict: &Verdict,
) -> Result<(), Error> {
    refuse_links(root, OUTPUT)?;
    let output = root.join(OUTPUT);
    let path = output.join(format!("{name}.actual.qoi"));
    let actual = encode_for(&path, scene.width, scene.height, &scene.pixels)?;
    write_new(&path, &actual)?;
    if let (Verdict::Differs { .. }, Some(reference)) = (verdict, reference) {
        // `judge` decoded it already: the bytes have not changed.
        let Ok(expected) = qoi::decode(reference, reference_limits()) else {
            return Ok(());
        };
        let path = output.join(format!("{name}.diff.qoi"));
        let diff = diff_image(&expected.pixels, &scene.pixels, scene.tolerance);
        let diff = encode_for(&path, scene.width, scene.height, &diff)?;
        write_new(&path, &diff)?;
    }
    Ok(())
}

/// What a failing scene reports, after its name. `scenes_failed` says
/// whether a command that drew scenes failed.
fn failure(name: &str, verdict: &Verdict, scenes_failed: bool) -> String {
    let actual = format!("{OUTPUT}/{name}.actual.qoi");
    match verdict {
        Verdict::Differs {
            over,
            total,
            largest,
        } => format!(
            "{over} of {total} pixels differ by more than the tolerance, by up to {largest}; \
             see {OUTPUT}/{name}.diff.qoi and {actual}"
        ),
        Verdict::OtherSize { width, height } => {
            format!("the reference is {width} × {height}, a size the scene is not; see {actual}")
        }
        Verdict::BadReference(error) => {
            format!("the reference does not decode: {error}; see {actual}")
        }
        Verdict::NoReference => {
            format!("no reference; look at {actual}, then run `cargo xtask {NAME} --bless {name}`")
        }
        Verdict::NotRendered if scenes_failed => {
            "not drawn: its test failed before it wrote what it drew, as cargo says above"
                .to_string()
        }
        Verdict::NotRendered => format!(
            "no scene drew it; remove {REFERENCES}/{name}.qoi if its scene is gone, or restore \
             the scene"
        ),
        Verdict::Same { .. } => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::testing::{TempTree, symlink};

    /// A PAM file as the scenes write it.
    fn pam(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut file = format!(
            "P7\nWIDTH {width}\nHEIGHT {height}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n"
        )
        .into_bytes();
        file.extend_from_slice(pixels);
        file
    }

    /// A 3 × 2 image, every pixel different.
    fn pixels() -> Vec<u8> {
        (0..24u8).map(|byte| byte * 10).collect()
    }

    /// A reference for [`pixels`].
    fn reference() -> Vec<u8> {
        encode(3, 2, &pixels()).unwrap()
    }

    /// One crate of scenes, exact.
    const GFX: &[Scenes] = &[Scenes {
        package: "tidepool-gfx",
        tolerance: 0,
    }];

    /// A fake [`draw`] that writes `files` (name, bytes) into the
    /// directory and returns `passes`.
    fn drawing(
        files: Vec<(&'static str, Vec<u8>)>,
        passes: bool,
    ) -> impl FnMut(&Path, &Scenes, &Path) -> Result<bool, Error> {
        move |_, _, dir| {
            for (name, bytes) in &files {
                fs::write(dir.join(name), bytes).unwrap();
            }
            Ok(passes)
        }
    }

    fn run_with(
        tree: &TempTree,
        scenes: &[Scenes],
        bless: &[&str],
        mut draw: impl FnMut(&Path, &Scenes, &Path) -> Result<bool, Error>,
    ) -> Report {
        let bless = bless.iter().map(|name| (*name).to_string()).collect();
        run(tree.path(), scenes, &bless, Report::default(), &mut draw).unwrap()
    }

    #[test]
    fn the_reader_takes_what_the_scenes_write_and_nothing_else() {
        assert_eq!(parse_pam(&pam(3, 2, &pixels())), Ok((3, 2, pixels())));
        // Fields in another order, and comments.
        let reordered =
            b"P7\n# drawn\nTUPLTYPE RGB_ALPHA\nMAXVAL 255\nDEPTH 4\nHEIGHT 1\nWIDTH 1\nENDHDR\n\
              \x01\x02\x03\x04";
        assert_eq!(parse_pam(reordered), Ok((1, 1, vec![1, 2, 3, 4])));
        let header = "P7\nWIDTH 1\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\n";
        let with = |header: &str, data: &[u8]| {
            let mut file = header.as_bytes().to_vec();
            file.extend_from_slice(b"ENDHDR\n");
            file.extend_from_slice(data);
            parse_pam(&file).unwrap_err()
        };
        let one = [0u8; 4];
        assert_eq!(
            with(&header.replace("P7", "P6"), &one),
            "not a PAM file: the first line is not `P7`"
        );
        assert_eq!(
            with(&header.replace("DEPTH 4", "DEPTH 3"), &one),
            "`DEPTH` is `3`, not `4`"
        );
        assert_eq!(
            with(&header.replace("MAXVAL 255", "MAXVAL 65535"), &one),
            "`MAXVAL` is `65535`, not `255`"
        );
        assert_eq!(
            with(&header.replace("RGB_ALPHA", "RGB"), &one),
            "`TUPLTYPE` is `RGB`, not `RGB_ALPHA`"
        );
        for bad in ["0", "4097", "-1", "+1", "x", ""] {
            assert_eq!(
                with(&header.replace("WIDTH 1", &format!("WIDTH {bad}")), &one),
                format!("`WIDTH` is `{bad}`, not a whole number from 1 to 4096")
            );
        }
        assert_eq!(
            with(&header.replace("HEIGHT 1\n", ""), &one),
            "the header has no `HEIGHT`"
        );
        assert_eq!(
            with(&format!("{header}WIDTH 1\n"), &one),
            "the header gives `WIDTH` twice"
        );
        assert_eq!(
            with(&format!("{header}SCALE 2\n"), &one),
            "the header has an unknown field `SCALE`"
        );
        assert_eq!(
            with(&format!("{header}WIDTH\n"), &one),
            "the header line `WIDTH` is not `<KEY> <value>`"
        );
        assert_eq!(
            with(header, &[0; 3]),
            "the pixels are 3 bytes, not 1 × 1 × 4"
        );
        assert_eq!(
            with(header, &[0; 5]),
            "the pixels are 5 bytes, not 1 × 1 × 4"
        );
        assert_eq!(
            parse_pam(b"P7\nWIDTH 1\n").unwrap_err(),
            "no `ENDHDR` line in the first 1024 bytes"
        );
        let mut long = b"P7\n".to_vec();
        long.extend(std::iter::repeat_n(b'#', 2000));
        long.extend_from_slice(b"\nENDHDR\n");
        assert_eq!(
            parse_pam(&long).unwrap_err(),
            "no `ENDHDR` line in the first 1024 bytes"
        );
        assert_eq!(
            parse_pam(b"P7\n\xff\nENDHDR\n").unwrap_err(),
            "the header is not UTF-8"
        );
        // `ENDHDR` inside a comment does not end the header; the line does.
        let mut commented = b"P7\n# not ENDHDR\n".to_vec();
        commented.extend_from_slice(&pam(1, 1, &[5, 6, 7, 8])[3..]);
        assert_eq!(parse_pam(&commented), Ok((1, 1, vec![5, 6, 7, 8])));
    }

    #[test]
    fn scene_names_are_lowercase_ascii_digits_and_underscores() {
        for name in ["gfx_fill_rect", "a", "x2", &"a".repeat(64)] {
            assert!(is_scene_name(name), "{name}");
        }
        for name in ["", "Gfx", "a-b", "a.b", "../a", "a b", "é", &"a".repeat(65)] {
            assert!(!is_scene_name(name), "{name}");
        }
    }

    #[test]
    fn bless_takes_scene_names_and_only_with_the_flag() {
        let parse_of = |args: &[&str]| parse(Args::new(args.iter().copied()).unwrap());
        assert_eq!(parse_of(&[]).unwrap(), BTreeSet::new());
        assert_eq!(
            parse_of(&["--bless", "b", "a", "b"]).unwrap(),
            BTreeSet::from(["a".to_string(), "b".to_string()])
        );
        let usage = |args: &[&str]| match parse_of(args) {
            Err(Error::Usage(message)) => message,
            other => panic!("expected a usage error, got {other:?}"),
        };
        assert_eq!(
            usage(&["gfx_line"]),
            "`golden` takes scene names only after `--bless`, but got `gfx_line`"
        );
        assert_eq!(
            usage(&["--bless"]),
            "`--bless` needs the names of the scenes to bless, once their images in \
             target/golden/ have been looked at"
        );
        assert_eq!(
            usage(&["--bless", "../x"]),
            "`../x` is not a scene name: 1 to 64 of a-z, 0-9 and _"
        );
        assert_eq!(
            usage(&["--check"]),
            "unexpected argument `--check` (argument 1)"
        );
    }

    #[test]
    fn matching_scenes_pass_and_write_no_images() {
        let tree = TempTree::new("golden-match");
        tree.write("tests/golden/one.qoi", reference());
        let report = run_with(
            &tree,
            GFX,
            &[],
            drawing(vec![("one.pam", pam(3, 2, &pixels()))], true),
        );
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes,
            ["golden: 1 scene drawn by tidepool-gfx", "  one  ok"]
        );
        // Only the scene's own file is left in the output.
        let output = tree.path().join(OUTPUT);
        assert_eq!(
            entries(&output).unwrap(),
            [(std::ffi::OsString::from(RENDERED), false, false)]
        );
    }

    #[test]
    fn a_changed_pixel_fails_with_an_actual_and_a_diff_image() {
        let tree = TempTree::new("golden-changed");
        tree.write("tests/golden/one.qoi", reference());
        let mut changed = pixels();
        changed[5] = changed[5].wrapping_add(1); // pixel 1, green, by 1
        let report = run_with(
            &tree,
            GFX,
            &[],
            drawing(vec![("one.pam", pam(3, 2, &changed))], true),
        );
        assert_eq!(
            report.violations,
            [
                "one: 1 of 6 pixels differ by more than the tolerance, by up to 1; see \
                 target/golden/one.diff.qoi and target/golden/one.actual.qoi"
            ]
        );
        let output = tree.path().join(OUTPUT);
        let actual = qoi::decode(
            &fs::read(output.join("one.actual.qoi")).unwrap(),
            reference_limits(),
        )
        .unwrap();
        assert_eq!(actual.pixels, changed);
        let diff = qoi::decode(
            &fs::read(output.join("one.diff.qoi")).unwrap(),
            reference_limits(),
        )
        .unwrap();
        assert_eq!(diff.pixels[4..8], [255, 0, 255, 255]);
        // Elsewhere, the reference in dim gray: pixel 0 is (0, 10, 20).
        assert_eq!(diff.pixels[..4], [2, 2, 2, 255]);
        assert!(diff.pixels.chunks(4).all(|pixel| pixel[3] == 255));
        // The reference is unchanged.
        assert_eq!(
            fs::read(tree.path().join("tests/golden/one.qoi")).unwrap(),
            reference()
        );
    }

    #[test]
    fn a_tolerance_lets_small_differences_pass() {
        let tree = TempTree::new("golden-tolerance");
        tree.write("tests/golden/one.qoi", reference());
        let mut changed = pixels();
        changed[5] += 1;
        changed[10] -= 1;
        let loose = [Scenes {
            package: "tidepool-font",
            tolerance: 1,
        }];
        let report = run_with(
            &tree,
            &loose,
            &[],
            drawing(vec![("one.pam", pam(3, 2, &changed))], true),
        );
        assert_eq!(report.violations, Vec::<String>::new());
        changed[5] += 1;
        let report = run_with(
            &tree,
            &loose,
            &[],
            drawing(vec![("one.pam", pam(3, 2, &changed))], true),
        );
        assert_eq!(
            report.violations,
            [
                "one: 1 of 6 pixels differ by more than the tolerance, by up to 2; see \
                 target/golden/one.diff.qoi and target/golden/one.actual.qoi"
            ]
        );
    }

    #[test]
    fn bless_rewrites_a_reference_that_matches_only_within_the_tolerance() {
        let tree = TempTree::new("golden-bless-tolerance");
        tree.write("tests/golden/one.qoi", reference());
        tree.write("tests/golden/two.qoi", reference());
        let mut changed = pixels();
        changed[5] += 1;
        let loose = [Scenes {
            package: "tidepool-font",
            tolerance: 1,
        }];
        let scenes = || {
            drawing(
                vec![
                    ("one.pam", pam(3, 2, &changed)),
                    ("two.pam", pam(3, 2, &pixels())),
                ],
                true,
            )
        };
        let report = run_with(&tree, &loose, &[], scenes());
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes,
            [
                "golden: 2 scenes drawn by tidepool-font",
                "  one  ok, within the tolerance: differs by up to 1",
                "  two  ok",
            ]
        );
        let report = run_with(&tree, &loose, &["one", "two"], scenes());
        assert_eq!(
            report.notes,
            [
                "golden: 2 scenes drawn by tidepool-font",
                "  one  blessed: updated tests/golden/one.qoi",
                "  two  ok: already its reference",
            ]
        );
        assert_eq!(
            fs::read(tree.path().join("tests/golden/one.qoi")).unwrap(),
            encode(3, 2, &changed).unwrap()
        );
    }

    #[test]
    fn missing_strange_and_unreadable_references_fail() {
        let tree = TempTree::new("golden-references");
        tree.write("tests/golden/gone.qoi", reference());
        tree.write("tests/golden/sized.qoi", encode(2, 3, &pixels()).unwrap());
        tree.write("tests/golden/broken.qoi", b"qoif");
        tree.write("tests/golden/Upper.qoi", reference());
        tree.write("tests/golden/notes.txt", "x");
        tree.write("tests/golden/sub/one.qoi", reference());
        let report = run_with(
            &tree,
            GFX,
            &[],
            drawing(
                vec![
                    ("new.pam", pam(3, 2, &pixels())),
                    ("sized.pam", pam(3, 2, &pixels())),
                    ("broken.pam", pam(3, 2, &pixels())),
                ],
                true,
            ),
        );
        let violations = report.violations.join("\n");
        for expected in [
            "tests/golden/Upper.qoi: not a reference",
            "tests/golden/notes.txt: not a reference",
            "tests/golden/sub: not a reference",
            "broken: the reference does not decode: QOI error",
            "gone: no scene drew it; remove tests/golden/gone.qoi",
            "new: no reference; look at target/golden/new.actual.qoi, then run `cargo xtask \
             golden --bless new`",
            "sized: the reference is 2 × 3, a size the scene is not; see \
             target/golden/sized.actual.qoi",
        ] {
            assert!(
                violations.contains(expected),
                "{expected}\nnot in:\n{violations}"
            );
        }
        assert_eq!(report.violations.len(), 7, "{violations}");
        let output = tree.path().join(OUTPUT);
        for image in ["new.actual.qoi", "sized.actual.qoi", "broken.actual.qoi"] {
            assert!(output.join(image).is_file(), "{image}");
        }
        assert!(!output.join("sized.diff.qoi").exists());
    }

    #[test]
    fn bad_scene_files_duplicates_and_failed_commands_fail() {
        let tree = TempTree::new("golden-scenes");
        tree.write("tests/golden/one.qoi", reference());
        let two = [
            Scenes {
                package: "tidepool-gfx",
                tolerance: 0,
            },
            Scenes {
                package: "tidepool-font",
                tolerance: 1,
            },
        ];
        let report = run_with(&tree, &two, &[], |_: &Path, scenes: &Scenes, dir: &Path| {
            fs::write(dir.join("one.pam"), pam(3, 2, &pixels())).unwrap();
            if scenes.package == "tidepool-gfx" {
                fs::write(dir.join("bad.pam"), b"P6\n").unwrap();
                fs::write(dir.join("stray.png"), b"").unwrap();
                Ok(true)
            } else {
                Ok(false)
            }
        });
        let violations = report.violations.join("\n");
        for expected in [
            "`cargo test --package tidepool-font --test golden` failed; see its output above",
            "target/golden/rendered/tidepool-gfx/bad.pam: no `ENDHDR` line",
            "target/golden/rendered/tidepool-gfx/stray.png: not a scene's image",
            "one: drawn by both tidepool-gfx and tidepool-font; scene names must be unique",
        ] {
            assert!(
                violations.contains(expected),
                "{expected}\nnot in:\n{violations}"
            );
        }
        assert_eq!(report.violations.len(), 4, "{violations}");
    }

    #[test]
    fn a_reference_not_drawn_after_a_failed_command_says_so() {
        let tree = TempTree::new("golden-not-drawn");
        tree.write("tests/golden/one.qoi", reference());
        let report = run_with(&tree, GFX, &[], drawing(Vec::new(), false));
        assert_eq!(
            report.violations,
            [
                "`cargo test --package tidepool-gfx --test golden` failed; see its output above",
                "one: not drawn: its test failed before it wrote what it drew, as cargo says above"
            ]
        );
    }

    #[test]
    fn bless_writes_only_the_scenes_named() {
        let tree = TempTree::new("golden-bless");
        let mut changed = pixels();
        changed[0] = 99;
        tree.write("tests/golden/kept.qoi", reference());
        tree.write("tests/golden/updated.qoi", reference());
        tree.write("tests/golden/unnamed.qoi", reference());
        let scenes = || {
            drawing(
                vec![
                    ("kept.pam", pam(3, 2, &pixels())),
                    ("updated.pam", pam(3, 2, &changed)),
                    ("unnamed.pam", pam(3, 2, &changed)),
                    ("new.pam", pam(3, 2, &changed)),
                ],
                true,
            )
        };
        let report = run_with(&tree, GFX, &["kept", "new", "updated"], scenes());
        assert_eq!(
            report.notes,
            [
                "golden: 4 scenes drawn by tidepool-gfx",
                "  kept     ok: already its reference",
                "  new      blessed: new tests/golden/new.qoi",
                "  updated  blessed: updated tests/golden/updated.qoi",
                "golden: once a failing scene's images show what it should draw, bless it: \
                 cargo xtask golden --bless <scene>",
            ]
        );
        // The unnamed scene that differs still fails, and is not written.
        assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
        assert!(report.violations[0].starts_with("unnamed: 1 of 6 pixels differ"));
        let read = |name: &str| fs::read(tree.path().join(format!("tests/golden/{name}.qoi")));
        let blessed = encode(3, 2, &changed).unwrap();
        assert_eq!(read("kept").unwrap(), reference());
        assert_eq!(read("updated").unwrap(), blessed);
        assert_eq!(read("new").unwrap(), blessed);
        assert_eq!(read("unnamed").unwrap(), reference());
        // Run again: every named scene now matches.
        let report = run_with(&tree, GFX, &[], scenes());
        assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    }

    #[test]
    fn bless_writes_nothing_when_the_run_is_not_sound() {
        let tree = TempTree::new("golden-bless-refused");
        tree.write("tests/golden/one.qoi", reference());
        let mut changed = pixels();
        changed[0] = 99;
        // A name no scene drew.
        let report = run_with(
            &tree,
            GFX,
            &["one", "missing"],
            drawing(vec![("one.pam", pam(3, 2, &changed))], true),
        );
        assert_eq!(
            report.notes[0],
            "golden: blessing nothing, for the reasons below"
        );
        assert!(
            report
                .violations
                .contains(&"cannot bless `missing`: no scene drew it".to_string()),
            "{:?}",
            report.violations
        );
        assert_eq!(
            fs::read(tree.path().join("tests/golden/one.qoi")).unwrap(),
            reference()
        );
        // A command that failed.
        let report = run_with(
            &tree,
            GFX,
            &["one"],
            drawing(vec![("one.pam", pam(3, 2, &changed))], false),
        );
        assert_eq!(
            report.notes[0],
            "golden: blessing nothing, for the reasons below"
        );
        assert_eq!(
            fs::read(tree.path().join("tests/golden/one.qoi")).unwrap(),
            reference()
        );
    }

    /// A target of `kind` named `name`.
    fn target(name: &str, kind: &str) -> crate::metadata::Target {
        crate::metadata::Target {
            name: name.to_string(),
            kind: kind.to_string(),
            src_path: PathBuf::from(format!("/work/tidepool/tests/{name}.rs")),
        }
    }

    #[test]
    fn the_drawing_crates_are_the_members_with_a_golden_test() {
        use crate::metadata::testing::{graph, member, third_party};
        let with_test = |name: &str, test: &str, kind: &str| {
            let mut package = member(name);
            package.targets.push(target(name, "lib"));
            package.targets.push(target(test, kind));
            package
        };
        let table = [
            Scenes {
                package: "tidepool-gfx",
                tolerance: 0,
            },
            Scenes {
                package: "tidepool-font",
                tolerance: 1,
            },
            Scenes {
                package: "tidepool-ui",
                tolerance: 0,
            },
        ];
        let mut outside = third_party("tidepool-ui", "1.0.0");
        outside.targets.push(target("golden", "test"));
        let selection = select(
            &graph(vec![
                with_test("tidepool-gfx", "golden", "test"),
                // A `golden` that is not a test, and a test of another name.
                with_test("tidepool-font", "golden", "bench"),
                with_test("tidepool-text", "properties", "test"),
                // Not a member, though it has the test.
                outside,
                with_test("tidepool-gui", "golden", "test"),
            ]),
            &table,
        );
        assert_eq!(selection.drawing, [table[0]]);
        assert_eq!(
            selection.report.notes,
            ["golden: tidepool-ui is not a member of this workspace, so it draws nothing"]
        );
        assert_eq!(
            selection.report.violations,
            [
                "tidepool-font has a row in SCENES (xtask/src/golden.rs), but no test target \
                 named `golden` (tests/golden.rs)",
                "tidepool-gui has a test target named `golden`, but no row in SCENES \
                 (xtask/src/golden.rs), which gives its tolerance",
            ]
        );
        // The real table names only crates that exist, each once.
        let mut packages: Vec<&str> = SCENES.iter().map(|row| row.package).collect();
        packages.sort_unstable();
        packages.dedup();
        assert_eq!(packages.len(), SCENES.len());
    }

    #[test]
    fn a_workspace_that_draws_nothing_passes_unless_it_has_references() {
        let tree = TempTree::new("golden-no-crates");
        let report = run_with(&tree, &[], &[], drawing(Vec::new(), true));
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes,
            ["golden: no crate in this workspace draws scenes"]
        );
        tree.write("tests/golden/one.qoi", reference());
        let report = run_with(&tree, &[], &[], drawing(Vec::new(), true));
        assert_eq!(
            report.violations,
            [
                "one: no scene drew it; remove tests/golden/one.qoi if its scene is gone, or \
              restore the scene"
            ]
        );
    }

    #[test]
    fn a_violation_found_before_the_run_keeps_it_from_blessing() {
        let tree = TempTree::new("golden-prior");
        let mut prior = Report::default();
        prior.violations.push("a crate has no row".to_string());
        let bless = BTreeSet::from(["one".to_string()]);
        let mut draw = drawing(vec![("one.pam", pam(3, 2, &pixels()))], true);
        let report = run(tree.path(), GFX, &bless, prior, &mut draw).unwrap();
        assert_eq!(
            report.notes[0],
            "golden: blessing nothing, for the reasons below"
        );
        assert_eq!(report.violations[0], "a crate has no row");
        assert!(!tree.path().join("tests/golden/one.qoi").exists());
    }

    #[test]
    fn a_reference_that_cannot_be_written_stops_the_rest_and_says_what_was() {
        let mut report = Report {
            notes: vec![
                String::new(),
                "  kept  ok".to_string(),
                String::new(),
                String::new(),
            ],
            ..Report::default()
        };
        let pending = vec![
            (0, "first", "new", vec![1]),
            (2, "second", "updated", vec![2]),
            (3, "third", "new", vec![3]),
        ];
        let mut written = Vec::new();
        let mut write = |name: &str, file: &[u8]| {
            if name == "second" {
                return Err(Error::Write {
                    path: PathBuf::from("tests/golden/second.qoi"),
                    source: io::Error::other("disk full"),
                });
            }
            written.push((name.to_string(), file.to_vec()));
            Ok(())
        };
        write_blessed(pending, 6, &mut write, &mut report);
        assert_eq!(written, [("first".to_string(), vec![1])]);
        assert_eq!(
            report.notes,
            [
                "  first   blessed: new tests/golden/first.qoi",
                "  kept  ok",
                "  second  not blessed: its reference could not be written",
                "  third   not blessed: an earlier reference could not be written",
            ]
        );
        assert_eq!(
            report.violations,
            [
                "cannot bless `second`: cannot write `tests/golden/second.qoi`: disk full; the \
              references blessed before it: first"
            ]
        );
    }

    #[test]
    fn bless_replaces_a_hard_link_rather_than_writing_through_it() {
        let tree = TempTree::new("golden-hard-link");
        let outside = TempTree::new("golden-hard-link-outside");
        outside.write("kept.qoi", reference());
        fs::create_dir_all(tree.path().join("tests/golden")).unwrap();
        if let Err(reason) = fs::hard_link(
            outside.path().join("kept.qoi"),
            tree.path().join("tests/golden/one.qoi"),
        ) {
            let _ = writeln!(
                std::io::stderr(),
                "SKIPPED: bless_replaces_a_hard_link_rather_than_writing_through_it: cannot \
                 make a hard link here: {reason}"
            );
            return;
        }
        let mut changed = pixels();
        changed[0] = 99;
        let report = run_with(
            &tree,
            GFX,
            &["one"],
            drawing(vec![("one.pam", pam(3, 2, &changed))], true),
        );
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            fs::read(tree.path().join("tests/golden/one.qoi")).unwrap(),
            encode(3, 2, &changed).unwrap()
        );
        assert_eq!(
            fs::read(outside.path().join("kept.qoi")).unwrap(),
            reference()
        );
        // No temp file is left.
        assert_eq!(
            entries(&tree.path().join("tests/golden")).unwrap(),
            [(std::ffi::OsString::from("one.qoi"), true, false)]
        );
    }

    #[test]
    fn images_are_not_written_through_an_output_swapped_for_a_link() {
        let tree = TempTree::new("golden-swapped");
        let outside = TempTree::new("golden-swapped-outside");
        tree.write("tests/golden/one.qoi", reference());
        let mut changed = pixels();
        changed[0] = 99;
        let root = tree.path();
        let moved = outside.path().join("moved");
        let mut made_link = Ok(());
        let mut draw = |_: &Path, _: &Scenes, dir: &Path| {
            fs::write(dir.join("one.pam"), pam(3, 2, &changed)).unwrap();
            // The output is moved away and a link left in its place, as a
            // child could do while it runs.
            fs::rename(root.join(OUTPUT), &moved).unwrap();
            made_link = symlink(&tree, OUTPUT, &moved);
            Ok(true)
        };
        let result = run(
            tree.path(),
            GFX,
            &BTreeSet::new(),
            Report::default(),
            &mut draw,
        );
        if let Err(reason) = made_link {
            let _ = writeln!(
                std::io::stderr(),
                "SKIPPED: images_are_not_written_through_an_output_swapped_for_a_link: cannot \
                 make a link here: {reason}"
            );
            return;
        }
        let error = result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("target/golden is a symbolic link"),
            "{error}"
        );
        assert!(!moved.join("one.actual.qoi").exists());
    }

    #[test]
    fn the_output_is_emptied_and_no_link_is_followed() {
        let tree = TempTree::new("golden-links");
        tree.write("target/golden/old.diff.qoi", "stale");
        let report = run_with(&tree, GFX, &[], drawing(Vec::new(), true));
        assert!(!tree.path().join("target/golden/old.diff.qoi").exists());
        assert_eq!(report.notes, ["golden: 0 scenes drawn by tidepool-gfx"]);
        // Comparing nothing is not a pass.
        assert_eq!(
            report.violations,
            [
                "no scene drew anything, and there are no references: a run that compares nothing \
              does not pass"
            ]
        );

        let outside = TempTree::new("golden-links-outside");
        outside.write("keep.txt", "kept");
        let linked = TempTree::new("golden-linked-output");
        if let Err(reason) = symlink(&linked, "target/golden", outside.path()) {
            // Links need a privilege on Windows that a runner may lack.
            let _ = writeln!(
                std::io::stderr(),
                "SKIPPED: the_output_is_emptied_and_no_link_is_followed: cannot make a link \
                 here: {reason}"
            );
            return;
        }
        let mut draw = drawing(Vec::new(), true);
        let error = run(
            linked.path(),
            GFX,
            &BTreeSet::new(),
            Report::default(),
            &mut draw,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("target/golden is a symbolic link"),
            "{error}"
        );
        assert!(outside.path().join("keep.txt").is_file());

        let references = TempTree::new("golden-linked-references");
        symlink(&references, "tests/golden", outside.path()).unwrap();
        let mut draw = drawing(Vec::new(), true);
        let error = run(
            references.path(),
            GFX,
            &BTreeSet::new(),
            Report::default(),
            &mut draw,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("tests/golden is a symbolic link"),
            "{error}"
        );

        let one = TempTree::new("golden-linked-reference");
        one.write("tests/golden/real.qoi", reference());
        symlink(
            &one,
            "tests/golden/one.qoi",
            &outside.path().join("keep.txt"),
        )
        .unwrap();
        let report = run_with(
            &one,
            GFX,
            &["one"],
            drawing(vec![("one.pam", pam(3, 2, &pixels()))], true),
        );
        assert!(
            report.violations[0].starts_with("tests/golden/one.qoi: a symbolic link"),
            "{:?}",
            report.violations
        );
        assert_eq!(
            fs::read_to_string(outside.path().join("keep.txt")).unwrap(),
            "kept"
        );
    }
}
