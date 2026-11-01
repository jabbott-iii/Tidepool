//! `cargo xtask ucd [--check]`: the grapheme tables of `tidepool-text` are
//! what the Unicode data kept in the repository generates (ADR-0018).
//!
//! New in Tidepool; nothing here is ported from Nadir.
//!
//! The inputs are the official Unicode 17.0.0 files in [`DIR`], each pinned
//! by its SHA-256 in [`PINS`]. Before any of them is read, each directory
//! below the root on the way to [`DIR`] and to [`OUTPUT`] must be missing
//! or a directory, and not a symbolic link: a link is never followed, so
//! nothing is read or written through one. Before anything is parsed,
//! every pinned file must be a regular file (not missing, not a symbolic
//! link), at most [`MAX_INPUT_BYTES`], UTF-8, and match its pin. Then three
//! of them are parsed into one byte of packed properties per code point:
//!
//! - `GraphemeBreakProperty.txt`: `Grapheme_Cluster_Break` (bits 0-3);
//! - `DerivedCoreProperties.txt`: `Indic_Conjunct_Break` (bits 4-5), from
//!   its `InCB` lines only;
//! - `emoji-data.txt`: `Extended_Pictographic` (bit 6).
//!
//! The values of the bits are the constants of `class` in
//! `crates/text/src/pos/grapheme.rs`, which the generated file names
//! (`EXTEND | INCB_EXTEND`). Each file's first line and `@missing` default
//! must be the expected ones, every data line must parse, no code point may
//! get one property twice, and each `# Total code points: N` line must
//! count the code points kept since the one before it (unless none were).
//! No code point may be both `ZWJ` and `Extended_Pictographic`, which the
//! grapheme state machine cannot represent, and U+000A and U+000D must be
//! exactly `LF` and `CR`, which its ASCII fast path assumes.
//!
//! Without `--check` it writes [`OUTPUT`] when that changed: to a temporary
//! file beside it, renamed over it, after making each missing directory on
//! the way one level at a time and checking it again. With `--check`, the
//! `ci` step, it fails when [`OUTPUT`] is missing or differs. Neither opens
//! a socket or starts a process. The inputs are pinned, so their parser is
//! not a fuzz target.

use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::Path;

use tidepool_core::hash::{sha256, to_hex};

use crate::check::Report;
use crate::error::Error;
use crate::walk;

/// The directory of the Unicode data, relative to the repository root.
pub(crate) const DIR: &str = "crates/text/ucd/17.0.0";

/// The generated file, relative to the repository root.
pub(crate) const OUTPUT: &str = "crates/text/src/pos/grapheme/tables.rs";

/// The directory of [`OUTPUT`].
const OUTPUT_DIR: &str = "crates/text/src/pos/grapheme";

/// The Unicode version of [`DIR`].
const VERSION: &str = "17.0.0";

/// The version `emoji-data.txt` gives itself: Unicode's, without the update
/// number.
const EMOJI_VERSION: &str = "17.0";

/// The `Grapheme_Cluster_Break` file.
const GCB_FILE: &str = "GraphemeBreakProperty.txt";

/// The `Extended_Pictographic` file.
const EMOJI_FILE: &str = "emoji-data.txt";

/// The `Indic_Conjunct_Break` file.
const DERIVED_FILE: &str = "DerivedCoreProperties.txt";

/// Each file in [`DIR`] and the SHA-256 of the official file, computed once
/// from the downloaded bytes with `tidepool_core::hash::sha256` and checked
/// with `sha256sum` (2026-11-01). The three property files come first, in
/// the order the generated file lists them; the conformance test (read only
/// by tests) and the license are pinned too, so that they cannot change
/// unnoticed either.
const PINS: [(&str, &str); 5] = [
    (
        GCB_FILE,
        "d6b51d1d2ae5c33b451b7ed994b48f1f4dc62b2272a5831e7fd418514a6bae89",
    ),
    (
        EMOJI_FILE,
        "2cb2bb9455cda83e8481541ecf5b6dfda66a3bb89efa3fa7c5297eccf607b72b",
    ),
    (
        DERIVED_FILE,
        "24c7fed1195c482faaefd5c1e7eb821c5ee1fb6de07ecdbaa64b56a99da22c08",
    ),
    (
        "GraphemeBreakTest.txt",
        "e2d134d2c52919bace503ebb6a551c1855fe1a1faec18478c78fff254a1793ec",
    ),
    (
        "license.txt",
        "e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96",
    ),
];

/// The largest input file read, in bytes. The largest pinned one,
/// `DerivedCoreProperties.txt`, is about 1.1 MB.
const MAX_INPUT_BYTES: u64 = 4 << 20;

/// The largest output read or generated, in bytes. The tables are about
/// 0.1 MB.
const MAX_OUTPUT_BYTES: u64 = 1 << 20;

/// The number of code points, U+0000 to U+10FFFF.
const CODE_POINTS: usize = 0x11_0000;

/// The `Grapheme_Cluster_Break` bits of a packed value.
const GCB_MASK: u8 = 0x0F;

/// The `Indic_Conjunct_Break` bits of a packed value.
const INCB_MASK: u8 = 0x30;

/// The `Extended_Pictographic` bit.
const EXT_PICT: u8 = 0x40;

/// `Grapheme_Cluster_Break=CR`.
const CR: u8 = 1;

/// `Grapheme_Cluster_Break=LF`.
const LF: u8 = 2;

/// `Grapheme_Cluster_Break=ZWJ`.
const ZWJ: u8 = 5;

/// Each `Grapheme_Cluster_Break` value but `Other` (0, never written): its
/// name in the data, its bits, and the constant of `class` that names it.
const GCB_VALUES: [(&str, u8, &str); 13] = [
    ("CR", CR, "CR"),
    ("LF", LF, "LF"),
    ("Control", 3, "CONTROL"),
    ("Extend", 4, "EXTEND"),
    ("ZWJ", ZWJ, "ZWJ"),
    ("Regional_Indicator", 6, "REGIONAL_INDICATOR"),
    ("Prepend", 7, "PREPEND"),
    ("SpacingMark", 8, "SPACING_MARK"),
    ("L", 9, "L"),
    ("V", 10, "V"),
    ("T", 11, "T"),
    ("LV", 12, "LV"),
    ("LVT", 13, "LVT"),
];

/// Each `Indic_Conjunct_Break` value but `None` (0), as for [`GCB_VALUES`].
const INCB_VALUES: [(&str, u8, &str); 3] = [
    ("Linker", 0x10, "INCB_LINKER"),
    ("Consonant", 0x20, "INCB_CONSONANT"),
    ("Extend", 0x30, "INCB_EXTEND"),
];

/// The properties `emoji-data.txt` lists. Only `Extended_Pictographic` is
/// kept; any other name is an error, so that a renamed property is noticed.
const EMOJI_PROPERTIES: [&str; 6] = [
    "Emoji",
    "Emoji_Presentation",
    "Emoji_Modifier",
    "Emoji_Modifier_Base",
    "Emoji_Component",
    "Extended_Pictographic",
];

/// The comment that ends a section of a property file with its count.
const TOTAL: &str = "# Total code points:";

/// `cargo xtask ucd --check`: [`OUTPUT`] is what the files in [`DIR`]
/// generate. A missing input or output, a linked one, one past a directory
/// that is linked or is not a directory, or an input that fails its pin is
/// a violation.
///
/// # Errors
///
/// [`Error::Read`] if a file cannot be read (or is too large or not UTF-8),
/// and [`Error::Doc`] if a property file does not parse.
pub(crate) fn check(root: &Path) -> Result<Report, Error> {
    let mut report = Report::default();
    let Some((tables, output_dir)) = generate(root, &mut report)? else {
        return Ok(report);
    };
    let path = root.join(OUTPUT);
    match output_entry(&path, &output_dir)? {
        Entry::File => {
            let current = read_output(&path)?;
            if current != tables.as_bytes() {
                report.violations.push(format!(
                    "{OUTPUT} is not what {DIR} generates (first difference at line {}); run \
                     cargo xtask ucd",
                    first_difference(tables.as_bytes(), &current)
                ));
            }
        }
        Entry::Missing => report
            .violations
            .push(format!("{OUTPUT} is missing; run cargo xtask ucd")),
        unusable => report.violations.push(unusable_output(&unusable)),
    }
    Ok(report)
}

/// `cargo xtask ucd`: writes [`OUTPUT`] if it is not what the files in
/// [`DIR`] generate. Nothing is written when there is a violation: an input
/// is missing, linked or fails its pin, a directory on the way to an input
/// or the output is a link or not a directory, or the output is a link or
/// not a regular file.
///
/// # Errors
///
/// As for [`check`], and [`Error::Write`] if the file or a directory on the
/// way to it cannot be made or written.
pub(crate) fn write(root: &Path) -> Result<Report, Error> {
    let mut report = Report::default();
    let Some((tables, output_dir)) = generate(root, &mut report)? else {
        return Ok(report);
    };
    let path = root.join(OUTPUT);
    match output_entry(&path, &output_dir)? {
        Entry::File if read_output(&path)? == tables.as_bytes() => {
            report.notes.push(format!("{OUTPUT} is up to date"));
        }
        Entry::File | Entry::Missing => {
            // Walked again just before writing, making what is missing one
            // level at a time: a link that appeared since the first walk is
            // refused too.
            if let Err(violation) = directories(root, OUTPUT_DIR, true)? {
                report.violations.push(violation);
                return Ok(report);
            }
            replace(&path, &tables)?;
            report.notes.push(format!("wrote {OUTPUT}"));
        }
        unusable => report.violations.push(unusable_output(&unusable)),
    }
    Ok(report)
}

/// What is at a path, without following a link there.
#[derive(Debug, PartialEq, Eq)]
enum Entry {
    /// Nothing.
    Missing,
    /// A symbolic link.
    Link,
    /// A directory.
    Dir,
    /// Neither a regular file nor a directory (a device, a socket, a pipe).
    NotFile,
    /// A regular file.
    File,
}

/// What is at `path`, read with `symlink_metadata`.
fn entry(path: &Path) -> Result<Entry, Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Ok(Entry::Link),
        Ok(metadata) if metadata.is_file() => Ok(Entry::File),
        Ok(metadata) if metadata.is_dir() => Ok(Entry::Dir),
        Ok(_) => Ok(Entry::NotFile),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(Entry::Missing)
        }
        Err(source) => Err(Error::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// What the directories on the way to a path are, when none is a
/// violation.
#[derive(Debug, PartialEq, Eq)]
enum Walk {
    /// Each one is a directory, and none is a symbolic link.
    Present,
    /// One is missing, and so is everything past it.
    Missing,
}

/// Walks `dir` (relative to `root`, `/`-separated) one component at a time,
/// shallowest first, with `symlink_metadata`. `root` itself and what is
/// above it are not checked. Stops at the first component that is a
/// symbolic link or not a directory, and returns its violation; nothing
/// past it is looked at. A missing component ends the walk with
/// [`Walk::Missing`], unless `create`: then it is made with
/// `fs::create_dir`, never `create_dir_all`, and checked again before the
/// walk goes into it.
///
/// # Errors
///
/// [`Error::Read`] naming a component that cannot be examined;
/// [`Error::Write`] naming all of `dir` in `root`, the directory being
/// made, if a component of it cannot be made.
fn directories(root: &Path, dir: &str, create: bool) -> Result<Result<Walk, String>, Error> {
    let ends = dir.match_indices('/').map(|(end, _)| end);
    for shown in ends.chain([dir.len()]).map(|end| &dir[..end]) {
        let path = root.join(shown);
        let mut found = entry(&path)?;
        if found == Entry::Missing && create {
            let write_error = |source| Error::Write {
                path: root.join(dir),
                source,
            };
            match fs::create_dir(&path) {
                Ok(()) => {}
                // Made by someone else in between: checked like any other.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(write_error(source)),
            }
            found = entry(&path)?;
            if found == Entry::Missing {
                return Err(write_error(io::ErrorKind::NotFound.into()));
            }
        }
        match found {
            Entry::Dir => {}
            Entry::Missing => return Ok(Ok(Walk::Missing)),
            Entry::Link => {
                return Ok(Err(format!(
                    "{shown} is a symbolic link, which ucd does not follow"
                )));
            }
            Entry::File | Entry::NotFile => {
                return Ok(Err(format!("{shown} is not a directory")));
            }
        }
    }
    Ok(Ok(Walk::Present))
}

/// What is at [`OUTPUT`], whose directory the walk `dir` found: nothing if
/// that is missing, without looking any further.
fn output_entry(path: &Path, dir: &Walk) -> Result<Entry, Error> {
    match dir {
        Walk::Present => entry(path),
        Walk::Missing => Ok(Entry::Missing),
    }
}

/// The violation for an [`OUTPUT`] that is neither missing nor a regular
/// file.
fn unusable_output(entry: &Entry) -> String {
    match entry {
        Entry::Link => format!(
            "{OUTPUT} is a symbolic link, which ucd does not follow or replace; remove it, then \
             run cargo xtask ucd"
        ),
        _ => format!("{OUTPUT} is not a regular file; remove it, then run cargo xtask ucd"),
    }
}

/// Checks the directories on the way to [`DIR`] and [`OUTPUT`] in `root`,
/// then the pinned files and, if they all pass, generates the tables,
/// adding the notes to `report`. Returns the tables and what the walk of
/// the output's directory found, or `None`, with nothing parsed, if there
/// was a violation, which is then in `report`. A directory that is a
/// violation stops everything: no input is read.
fn generate(root: &Path, report: &mut Report) -> Result<Option<(String, Walk)>, Error> {
    let inputs = directories(root, DIR, false)?;
    let output = directories(root, OUTPUT_DIR, false)?;
    for walk in [&inputs, &output] {
        // `crates` and `crates/text` are on both ways: each is named once.
        if let Err(violation) = walk
            && !report.violations.contains(violation)
        {
            report.violations.push(violation.clone());
        }
    }
    let (Ok(inputs), Ok(output)) = (inputs, output) else {
        return Ok(None);
    };
    let mut texts = Vec::with_capacity(PINS.len());
    for (file, pin) in PINS {
        let found = match inputs {
            Walk::Present => pinned(root, file, pin)?,
            Walk::Missing => Err(format!("{DIR}/{file} is missing")),
        };
        match found {
            Ok(text) => texts.push(text),
            Err(violation) => report.violations.push(violation),
        }
    }
    // Only when every file passed are all five there.
    let [gcb, emoji, derived, _, _] = texts.as_slice() else {
        return Ok(None);
    };
    report.notes.push(format!(
        "checked {} files in {DIR} against their SHA-256 pins",
        PINS.len()
    ));
    let props = properties(gcb, emoji, derived)?;
    let ranges = ranges(&props);
    let tables = render(&ranges, &PINS[..3]);
    if tables.len() as u64 > MAX_OUTPUT_BYTES {
        return Err(Error::Doc {
            path: OUTPUT.to_string(),
            line: 0,
            detail: format!(
                "the tables would be {} bytes, over the {MAX_OUTPUT_BYTES}-byte limit",
                tables.len()
            ),
        });
    }
    report.notes.push(format!(
        "{OUTPUT}: {} ranges (Unicode {VERSION})",
        ranges.len()
    ));
    Ok(Some((tables, output)))
}

/// The text of `file` in [`DIR`], whose directories the walk found
/// present, or the violation that keeps it from being used: it is missing,
/// a link, not a regular file, or not the file `pin` names.
///
/// # Errors
///
/// [`Error::Read`] if it cannot be read, is over [`MAX_INPUT_BYTES`] or is
/// not UTF-8.
fn pinned(root: &Path, file: &str, pin: &str) -> Result<Result<String, String>, Error> {
    let shown = format!("{DIR}/{file}");
    let path = root.join(&shown);
    match entry(&path)? {
        Entry::File => {}
        Entry::Missing => return Ok(Err(format!("{shown} is missing"))),
        Entry::Link => {
            return Ok(Err(format!(
                "{shown} is a symbolic link, which ucd does not follow"
            )));
        }
        Entry::Dir | Entry::NotFile => return Ok(Err(format!("{shown} is not a regular file"))),
    }
    let text = walk::read_required(&path, MAX_INPUT_BYTES)?;
    let digest = to_hex(&sha256(text.as_bytes()));
    if digest != pin {
        return Ok(Err(format!(
            "{shown}: SHA-256 is {digest}, but the pin is {pin} (the file must be the unchanged \
             official one; check git's line-ending conversion and .gitattributes)"
        )));
    }
    Ok(Ok(text))
}

/// Reads the generated file at `path`, as bytes: at most one more than
/// [`MAX_OUTPUT_BYTES`], which is enough to tell that it differs.
fn read_output(path: &Path) -> Result<Vec<u8>, Error> {
    let read_error = |source| Error::Read {
        path: path.to_path_buf(),
        source,
    };
    let file = fs::File::open(path).map_err(read_error)?;
    let mut bytes = Vec::new();
    file.take(MAX_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(read_error)?;
    Ok(bytes)
}

/// The first line, counted from 1, on which `actual` differs from
/// `expected`. A line includes its line feed, so a missing or extra one, or
/// a CR before it, is a difference on that line.
fn first_difference(expected: &[u8], actual: &[u8]) -> usize {
    let mut expected = expected.split_inclusive(|&byte| byte == b'\n');
    let mut actual = actual.split_inclusive(|&byte| byte == b'\n');
    let mut line = 1;
    while let (Some(want), Some(have)) = (expected.next(), actual.next()) {
        if want != have {
            break;
        }
        line += 1;
    }
    line
}

/// Writes `contents` to `path`, whose directory [`directories`] made and
/// checked: to a new file beside it, renamed over it, so that the file is
/// never left half written.
///
/// # Errors
///
/// [`Error::Write`] naming the file that could not be made, written or
/// renamed.
fn replace(path: &Path, contents: &str) -> Result<(), Error> {
    let write_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| Error::Write { path, source }
    };
    let temporary = path.with_extension("rs.tmp");
    // One left by a run that stopped part way. Removing a link removes the
    // link, never what it leads to; `create_new` then refuses anything that
    // appeared in between.
    match fs::remove_file(&temporary) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(write_error(&temporary)(source)),
    }
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        });
    if let Err(source) = written {
        let _ = fs::remove_file(&temporary);
        return Err(write_error(&temporary)(source));
    }
    fs::rename(&temporary, path).map_err(|source| {
        let _ = fs::remove_file(&temporary);
        write_error(path)(source)
    })
}

/// The packed properties of every code point, from the texts of the three
/// property files.
///
/// # Errors
///
/// [`Error::Doc`] naming the file and line for a wrong first line, a
/// missing `@missing` default or version line (line 0), a data line that
/// does not parse, a code point that gets one property twice or a wrong
/// `# Total code points:` count; and naming [`DIR`] (line 0) for a code
/// point that is both `ZWJ` and `Extended_Pictographic`, or a U+000A or
/// U+000D that is not exactly `LF` or `CR`.
fn properties(gcb: &str, emoji: &str, derived: &str) -> Result<Vec<u8>, Error> {
    first_line(
        GCB_FILE,
        gcb,
        &format!("# GraphemeBreakProperty-{VERSION}.txt"),
    )?;
    has_line(GCB_FILE, gcb, "# @missing: 0000..10FFFF; Other")?;
    first_line(EMOJI_FILE, emoji, "# emoji-data.txt")?;
    has_line(EMOJI_FILE, emoji, &format!("# Version: {EMOJI_VERSION}"))?;
    first_line(
        DERIVED_FILE,
        derived,
        &format!("# DerivedCoreProperties-{VERSION}.txt"),
    )?;
    has_line(
        DERIVED_FILE,
        derived,
        "# @missing: 0000..10FFFF; InCB; None",
    )?;

    let mut props = vec![0; CODE_POINTS];
    parse(GCB_FILE, gcb, GCB_MASK, gcb_value, &mut props)?;
    parse(EMOJI_FILE, emoji, EXT_PICT, emoji_value, &mut props)?;
    parse(DERIVED_FILE, derived, INCB_MASK, incb_value, &mut props)?;

    let whole = |detail: String| Error::Doc {
        path: DIR.to_string(),
        line: 0,
        detail,
    };
    if let Some(code_point) = props
        .iter()
        .position(|&value| value & GCB_MASK == ZWJ && value & EXT_PICT != 0)
    {
        return Err(whole(format!(
            "U+{code_point:04X} is both ZWJ and Extended_Pictographic, which the grapheme state \
             machine cannot represent"
        )));
    }
    for (code_point, value, name) in [(0x0A, LF, "LF"), (0x0D, CR, "CR")] {
        if props[code_point] != value {
            return Err(whole(format!(
                "U+{code_point:04X} must be exactly {name}, with no other property, as the ASCII \
                 fast path assumes"
            )));
        }
    }
    Ok(props)
}

/// Fails unless the first line of `file`'s `text` is `expected`.
fn first_line(file: &str, text: &str, expected: &str) -> Result<(), Error> {
    if text.lines().next() == Some(expected) {
        return Ok(());
    }
    Err(Error::Doc {
        path: format!("{DIR}/{file}"),
        line: 1,
        detail: format!("the first line must be `{expected}`"),
    })
}

/// Fails unless some line of `file`'s `text` is `expected`.
fn has_line(file: &str, text: &str, expected: &str) -> Result<(), Error> {
    if text.lines().any(|line| line == expected) {
        return Ok(());
    }
    Err(Error::Doc {
        path: format!("{DIR}/{file}"),
        line: 0,
        detail: format!("no line is `{expected}`"),
    })
}

/// Reads the data lines of `file`'s `text` into `props`. `value` gives the
/// bits a line's fields (the code points first) set, `None` for a line that
/// is not kept, or what is wrong with them. `mask` is the property's bits:
/// a code point that already has any of them is assigned twice.
///
/// A data line is what comes before its first `#`, trimmed; blank ones are
/// skipped. Its fields are separated by `;` and trimmed. Each
/// `# Total code points: N` line must count the code points kept since the
/// one before it, unless there were none.
fn parse(
    file: &str,
    text: &str,
    mask: u8,
    value: fn(&[&str]) -> Result<Option<u8>, String>,
    props: &mut [u8],
) -> Result<(), Error> {
    let doc = |line: usize, detail: String| Error::Doc {
        path: format!("{DIR}/{file}"),
        line,
        detail,
    };
    let mut kept = 0;
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        if let Some(total) = line.strip_prefix(TOTAL) {
            let total = total.trim();
            let Ok(total) = total.parse::<usize>() else {
                return Err(doc(number, format!("`{total}` is not a count")));
            };
            if kept != 0 && kept != total {
                return Err(doc(
                    number,
                    format!(
                        "the total is {total} code points, but the lines since the previous \
                         total have {kept}"
                    ),
                ));
            }
            kept = 0;
            continue;
        }
        let data = line.split_once('#').map_or(line, |(data, _)| data).trim();
        if data.is_empty() {
            continue;
        }
        let fields: Vec<&str> = data.split(';').map(str::trim).collect();
        let Some((first, last)) = fields.first().and_then(|field| code_points(field)) else {
            return Err(doc(
                number,
                format!(
                    "`{}` is not a code point or a range of them (`XXXX` or `XXXX..YYYY`, 4 to 6 \
                     uppercase hex digits, first <= last <= 10FFFF)",
                    fields.first().copied().unwrap_or_default()
                ),
            ));
        };
        let Some(bits) = value(&fields).map_err(|detail| doc(number, detail))? else {
            continue;
        };
        for code_point in first..=last {
            let slot = &mut props[code_point as usize];
            if *slot & mask != 0 {
                return Err(doc(
                    number,
                    format!("U+{code_point:04X} is assigned this property twice"),
                ));
            }
            *slot |= bits;
        }
        kept += (last - first) as usize + 1;
    }
    Ok(())
}

/// The code points `field` names: `XXXX` or `XXXX..YYYY`, each 4 to 6
/// uppercase hex digits, first <= last <= U+10FFFF.
fn code_points(field: &str) -> Option<(u32, u32)> {
    let (first, last) = field.split_once("..").unwrap_or((field, field));
    let (first, last) = (code_point(first)?, code_point(last)?);
    (first <= last).then_some((first, last))
}

/// One code point: 4 to 6 uppercase hex digits, at most 10FFFF.
fn code_point(digits: &str) -> Option<u32> {
    let well_formed = (4..=6).contains(&digits.len())
        && digits
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'A'..=b'F'));
    let value = u32::from_str_radix(digits, 16)
        .ok()
        .filter(|_| well_formed)?;
    (value < CODE_POINTS as u32).then_some(value)
}

/// The bits of a `GraphemeBreakProperty.txt` line: exactly two fields, the
/// second one of [`GCB_VALUES`].
fn gcb_value(fields: &[&str]) -> Result<Option<u8>, String> {
    let [_, name] = fields else {
        return Err(fields_error(2, fields.len()));
    };
    GCB_VALUES
        .iter()
        .find(|(value, ..)| value == name)
        .map(|&(_, bits, _)| Some(bits))
        .ok_or_else(|| format!("unknown Grapheme_Cluster_Break value `{name}`"))
}

/// The bits of an `emoji-data.txt` line: exactly two fields, the second
/// one of [`EMOJI_PROPERTIES`]. Only `Extended_Pictographic` is kept.
fn emoji_value(fields: &[&str]) -> Result<Option<u8>, String> {
    let [_, name] = fields else {
        return Err(fields_error(2, fields.len()));
    };
    if !EMOJI_PROPERTIES.contains(name) {
        return Err(format!("unknown emoji property `{name}`"));
    }
    Ok((*name == "Extended_Pictographic").then_some(EXT_PICT))
}

/// The bits of a `DerivedCoreProperties.txt` line: for an `InCB` line,
/// exactly three fields, the third one of [`INCB_VALUES`]. Other lines are
/// not kept.
fn incb_value(fields: &[&str]) -> Result<Option<u8>, String> {
    match fields {
        [_, "InCB", name] => INCB_VALUES
            .iter()
            .find(|(value, ..)| value == name)
            .map(|&(_, bits, _)| Some(bits))
            .ok_or_else(|| format!("unknown Indic_Conjunct_Break value `{name}`")),
        [_, "InCB", ..] => Err(fields_error(3, fields.len())),
        _ => Ok(None),
    }
}

/// The message for a line with `found` fields instead of `expected`.
fn fields_error(expected: usize, found: usize) -> String {
    format!("expected {expected} fields separated by `;`, found {found}")
}

/// The sorted, disjoint, inclusive ranges of code points whose packed value
/// is not 0, with adjacent code points of equal value merged into one.
fn ranges(props: &[u8]) -> Vec<(u32, u32, u8)> {
    let mut ranges: Vec<(u32, u32, u8)> = Vec::new();
    for (code_point, &value) in (0..).zip(props) {
        if value == 0 {
            continue;
        }
        match ranges.last_mut() {
            Some((_, last, same)) if *same == value && *last + 1 == code_point => {
                *last = code_point;
            }
            _ => ranges.push((code_point, code_point, value)),
        }
    }
    ranges
}

/// The constants of `class` that make up a packed value: its
/// `Grapheme_Cluster_Break`, `Indic_Conjunct_Break` and
/// `Extended_Pictographic` parts, those that are not 0, joined by ` | `.
fn names(value: u8) -> String {
    let gcb = GCB_VALUES
        .iter()
        .filter(|&&(_, bits, _)| bits == value & GCB_MASK);
    let incb = INCB_VALUES
        .iter()
        .filter(|&&(_, bits, _)| bits == value & INCB_MASK);
    let mut parts: Vec<&str> = gcb.chain(incb).map(|&(_, _, name)| name).collect();
    if value & EXT_PICT != 0 {
        parts.push("EXT_PICT");
    }
    parts.join(" | ")
}

/// The generated file: `ranges`, with `sources` (each property file and its
/// SHA-256) in its header. LF line endings, one entry per line, and nothing
/// that depends on when or where it is generated.
fn render(ranges: &[(u32, u32, u8)], sources: &[(&str, &str)]) -> String {
    let width = sources
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    let mut text = format!(
        "// @generated by `cargo xtask ucd` from {DIR}. Do not edit by hand.\n\
         //\n\
         // Sources and their SHA-256:\n"
    );
    for (name, digest) in sources {
        text.push_str(&format!("//   {name:<width$}  {digest}\n"));
    }
    text.push_str(&format!(
        "//\n\
         // Unicode data: Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3\n\
         // ({DIR}/license.txt).\n\
         \n\
         //! Grapheme cluster properties of every code point (ADR-0018); the bits are in \
         `super::class`.\n\
         \n\
         use super::class::*;\n\
         \n\
         /// The Unicode version of these tables.\n\
         pub(crate) const UNICODE_VERSION: (u8, u8, u8) = ({});\n\
         \n\
         /// Sorted, disjoint, inclusive `(first, last, properties)` ranges of every code point \
         whose\n\
         /// properties are not all defaults (Other, InCB=None, not Extended_Pictographic); \
         adjacent\n\
         /// ranges with equal properties are merged.\n\
         pub(crate) static RANGES: [(u32, u32, u8); {}] = [\n",
        VERSION.replace('.', ", "),
        ranges.len()
    ));
    for &(first, last, value) in ranges {
        text.push_str(&format!(
            "    ({first:#06X}, {last:#06X}, {}),\n",
            names(value)
        ));
    }
    text.push_str("];\n");
    text
}

#[cfg(test)]
pub(crate) mod testing {
    //! A scratch repository holding the real Unicode data.

    use std::fs;
    use std::path::Path;

    use super::{DIR, PINS};
    use crate::walk::testing::TempTree;

    /// A new [`TempTree`] for the test `name` with a copy of every pinned
    /// file of the repository's [`DIR`], and nothing else.
    pub(crate) fn tree_with_inputs(name: &str) -> TempTree {
        let tree = TempTree::new(name);
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(DIR);
        for (file, _) in PINS {
            let bytes = fs::read(real.join(file)).expect("the repository has its Unicode data");
            tree.write(&format!("{DIR}/{file}"), bytes);
        }
        tree
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::testing::tree_with_inputs;
    use super::*;
    use crate::walk::testing::{TempTree, symlink};

    /// The smallest property files that pass every check. A line appended
    /// to `GCB` is its line 7.
    const GCB: &str = "# GraphemeBreakProperty-17.0.0.txt\n\
                       # @missing: 0000..10FFFF; Other\n\
                       000D          ; CR # Cc       <control-000D>\n\
                       000A          ; LF # Cc       <control-000A>\n\
                       \n\
                       # Total code points: 2\n";
    const EMOJI: &str = "# emoji-data.txt\n# Version: 17.0\n";
    const DERIVED: &str = "# DerivedCoreProperties-17.0.0.txt\n\
                           # @missing: 0000..10FFFF; InCB; None\n";

    /// The `Error::Doc` that `properties` gives, as (path, line, detail).
    fn doc_error(gcb: &str, emoji: &str, derived: &str) -> (String, usize, String) {
        match properties(gcb, emoji, derived) {
            Err(Error::Doc { path, line, detail }) => (path, line, detail),
            Err(other) => panic!("expected a document error, got {other:?}"),
            Ok(_) => panic!("expected a document error, but the files parsed"),
        }
    }

    /// The error for [`GCB`] followed by `line`, its line 7 on.
    fn gcb_line_error(line: &str) -> (String, usize, String) {
        doc_error(&format!("{GCB}{line}\n"), EMOJI, DERIVED)
    }

    fn gcb_path() -> String {
        format!("{DIR}/{GCB_FILE}")
    }

    /// A tree where `write` has generated the output.
    fn generated_tree(name: &str) -> TempTree {
        let tree = tree_with_inputs(name);
        let report = write(tree.path()).unwrap();
        assert_eq!(report.violations, Vec::<String>::new());
        tree
    }

    fn real_root() -> PathBuf {
        crate::repository_root()
    }

    /// The packed properties the repository's Unicode data gives.
    fn real_properties() -> Vec<u8> {
        let dir = real_root().join(DIR);
        let read = |file| fs::read_to_string(dir.join(file)).unwrap();
        properties(&read(GCB_FILE), &read(EMOJI_FILE), &read(DERIVED_FILE)).unwrap()
    }

    #[test]
    fn the_smallest_files_parse() {
        let props = properties(GCB, EMOJI, DERIVED).unwrap();
        assert_eq!(props.len(), 0x11_0000);
        assert_eq!((props[0x0A], props[0x0D]), (LF, CR));
        assert_eq!(props.iter().filter(|&&value| value != 0).count(), 2);
    }

    #[test]
    fn data_lines_are_cut_at_the_first_hash_and_trimmed() {
        // No space before `#`, as in emoji-data.txt; tabs; a range.
        let emoji = format!(
            "{EMOJI}00A9          ; Extended_Pictographic# E0.6   [1] (©️)       copyright\n\
             \t1F1E6..1F1FF\t;\tEmoji_Component\t# regional indicators\n"
        );
        let props = properties(GCB, &emoji, DERIVED).unwrap();
        assert_eq!(props[0xA9], EXT_PICT);
        // Emoji_Component is read, and not kept.
        assert_eq!(props[0x1F1E6], 0);
        let gcb = format!("{GCB}1F1E6..1F1FF ; Regional_Indicator\n");
        let props = properties(&gcb, EMOJI, DERIVED).unwrap();
        assert_eq!((props[0x1F1E5], props[0x1F200]), (0, 0));
        assert!(props[0x1F1E6..=0x1F1FF].iter().all(|&value| value == 6));
    }

    #[test]
    fn a_code_point_must_be_4_to_6_uppercase_hex_digits_up_to_10ffff() {
        for bad in [
            "00a9", "0A9", "1234567", "00G9", "+0A9", "0x00A9", "", "110000", "00A9..",
        ] {
            let (path, line, detail) = gcb_line_error(&format!("{bad} ; Control"));
            assert_eq!((path.as_str(), line), (gcb_path().as_str(), 7), "{bad}");
            assert!(
                detail.starts_with(&format!("`{bad}` is not a code point")),
                "{bad}: {detail}"
            );
        }
        // A reversed range, and one that ends above U+10FFFF.
        for bad in ["0041..0040", "10FFFE..110000"] {
            let (_, line, detail) = gcb_line_error(&format!("{bad} ; Control"));
            assert_eq!(line, 7, "{bad}");
            assert!(detail.starts_with(&format!("`{bad}`")), "{detail}");
        }
        assert_eq!(code_points("10FFFF"), Some((0x10_FFFF, 0x10_FFFF)));
        assert_eq!(code_points("0000..10FFFF"), Some((0, 0x10_FFFF)));
        assert_eq!(code_points("01F600"), Some((0x1F600, 0x1F600)));
    }

    #[test]
    fn unknown_values_and_wrong_field_counts_are_errors() {
        let (path, line, detail) = gcb_line_error("0041 ; Letter");
        assert_eq!((path, line), (gcb_path(), 7));
        assert_eq!(detail, "unknown Grapheme_Cluster_Break value `Letter`");
        let (_, line, detail) = gcb_line_error("0041 ; Control ; More");
        assert_eq!(line, 7);
        assert_eq!(detail, "expected 2 fields separated by `;`, found 3");
        // Values are matched whole and case-sensitively.
        let (_, _, detail) = gcb_line_error("0041 ; control");
        assert_eq!(detail, "unknown Grapheme_Cluster_Break value `control`");

        let (path, line, detail) =
            doc_error(GCB, &format!("{EMOJI}0041 ; Emoji_Sparkle\n"), DERIVED);
        assert_eq!((path, line), (format!("{DIR}/{EMOJI_FILE}"), 3));
        assert_eq!(detail, "unknown emoji property `Emoji_Sparkle`");

        let derived_path = format!("{DIR}/{DERIVED_FILE}");
        let (path, line, detail) = doc_error(
            GCB,
            EMOJI,
            &format!("{DERIVED}0915 ; InCB; Vowel # Lo DEVANAGARI LETTER KA\n"),
        );
        assert_eq!((path.as_str(), line), (derived_path.as_str(), 3));
        assert_eq!(detail, "unknown Indic_Conjunct_Break value `Vowel`");
        let (_, line, detail) = doc_error(GCB, EMOJI, &format!("{DERIVED}0915 ; InCB\n"));
        assert_eq!(
            (line, detail.as_str()),
            (3, "expected 3 fields separated by `;`, found 2")
        );
        // Other derived properties are not read, whatever their shape.
        let props = properties(
            GCB,
            EMOJI,
            &format!("{DERIVED}0041 ; Alphabetic\n0042 ; Odd ; Shape ; Here\n"),
        )
        .unwrap();
        assert_eq!(props[0x41], 0);
    }

    #[test]
    fn a_code_point_assigned_twice_within_one_property_is_an_error() {
        let (path, line, detail) = gcb_line_error("0300..036F ; Extend\n0341 ; Extend");
        assert_eq!(
            (path, line, detail.as_str()),
            (gcb_path(), 8, "U+0341 is assigned this property twice")
        );
        // The same code point in two properties is not.
        let gcb = format!("{GCB}094D ; Extend\n");
        let derived = format!("{DERIVED}094D ; InCB; Linker\n");
        let emoji = format!("{EMOJI}094D ; Emoji\n");
        let props = properties(&gcb, &emoji, &derived).unwrap();
        assert_eq!(props[0x094D], 4 | 0x10);
        let (_, line, detail) = doc_error(
            GCB,
            EMOJI,
            &format!("{DERIVED}094D ; InCB; Linker\n094D ; InCB; Extend\n"),
        );
        assert_eq!(
            (line, detail.as_str()),
            (4, "U+094D is assigned this property twice")
        );
    }

    #[test]
    fn first_lines_and_defaults_must_be_the_expected_ones() {
        let (path, line, detail) = doc_error(&GCB.replacen("17.0.0", "16.0.0", 1), EMOJI, DERIVED);
        assert_eq!((path, line), (gcb_path(), 1));
        assert_eq!(
            detail,
            "the first line must be `# GraphemeBreakProperty-17.0.0.txt`"
        );
        let (path, line, detail) = doc_error(
            &GCB.replace("# @missing: 0000..10FFFF; Other\n", ""),
            EMOJI,
            DERIVED,
        );
        assert_eq!((path, line), (gcb_path(), 0));
        assert_eq!(detail, "no line is `# @missing: 0000..10FFFF; Other`");
        let (path, line, detail) = doc_error(
            GCB,
            EMOJI,
            &DERIVED.replace("# @missing: 0000..10FFFF; InCB; None\n", ""),
        );
        assert_eq!((path, line), (format!("{DIR}/{DERIVED_FILE}"), 0));
        assert_eq!(detail, "no line is `# @missing: 0000..10FFFF; InCB; None`");
        let (path, line, detail) = doc_error(GCB, &EMOJI.replace("17.0", "16.0"), DERIVED);
        assert_eq!((path, line), (format!("{DIR}/{EMOJI_FILE}"), 0));
        assert_eq!(detail, "no line is `# Version: 17.0`");
        let (_, line, detail) = doc_error(GCB, &EMOJI.replace("emoji", "Emoji"), DERIVED);
        assert_eq!(
            (line, detail.as_str()),
            (1, "the first line must be `# emoji-data.txt`")
        );
        // The empty file has no first line.
        let (_, line, _) = doc_error(GCB, EMOJI, "");
        assert_eq!(line, 1);
    }

    #[test]
    fn each_total_must_count_the_code_points_kept_since_the_last_one() {
        let (path, line, detail) =
            gcb_line_error("0000..0009 ; Control\n000B ; Control\n\n# Total code points: 12");
        assert_eq!((path, line), (gcb_path(), 10));
        assert_eq!(
            detail,
            "the total is 12 code points, but the lines since the previous total have 11"
        );
        let (_, line, detail) = gcb_line_error("0000 ; Control\n# Total code points: lots");
        assert_eq!((line, detail.as_str()), (8, "`lots` is not a count"));
        // A right total passes; one after nothing kept is not checked.
        let gcb = format!(
            "{GCB}# Total code points: 1\n0000..0009 ; Control\n000B ; Control\n\
             # Total code points: 11\n"
        );
        properties(&gcb, EMOJI, DERIVED).unwrap();
        let derived = format!(
            "{DERIVED}0041..005A ; Alphabetic\n# Total code points: 999\n\
             094D ; InCB; Linker\n# Total code points: 1\n"
        );
        properties(GCB, EMOJI, &derived).unwrap();
        // `# Total elements:` lines (emoji-data.txt) are not checked.
        let emoji = format!("{EMOJI}00A9 ; Extended_Pictographic\n# Total elements: 7\n");
        properties(GCB, &emoji, DERIVED).unwrap();
    }

    #[test]
    fn zwj_and_extended_pictographic_together_cannot_be_represented() {
        let emoji = format!("{EMOJI}200D ; Extended_Pictographic\n");
        let gcb = format!("{GCB}200D ; ZWJ\n");
        let (path, line, detail) = doc_error(&gcb, &emoji, DERIVED);
        assert_eq!((path.as_str(), line), (DIR, 0));
        assert_eq!(
            detail,
            "U+200D is both ZWJ and Extended_Pictographic, which the grapheme state machine \
             cannot represent"
        );
    }

    #[test]
    fn lf_and_cr_must_be_exactly_lf_and_cr() {
        let (path, line, detail) = doc_error(&GCB.replace("; LF", "; CR"), EMOJI, DERIVED);
        assert_eq!((path.as_str(), line), (DIR, 0));
        assert_eq!(
            detail,
            "U+000A must be exactly LF, with no other property, as the ASCII fast path assumes"
        );
        let emoji = format!("{EMOJI}000D ; Extended_Pictographic\n");
        let (_, _, detail) = doc_error(GCB, &emoji, DERIVED);
        assert!(detail.starts_with("U+000D must be exactly CR"), "{detail}");
    }

    #[test]
    fn ranges_expand_back_to_every_code_point() {
        let props = real_properties();
        let ranges = ranges(&props);
        let mut expanded = vec![0; CODE_POINTS];
        let mut previous: Option<(u32, u8)> = None;
        for &(first, last, value) in &ranges {
            assert!(first <= last && value != 0, "{first:X}..{last:X}");
            if let Some((end, before)) = previous {
                assert!(end < first, "not sorted and disjoint at {first:X}");
                assert!(
                    end + 1 < first || before != value,
                    "not merged at {first:X}"
                );
            }
            previous = Some((last, value));
            for code_point in first..=last {
                expanded[code_point as usize] = value;
            }
        }
        assert_eq!(expanded, props);
        // Unicode 17.0.0 gives a table of this size (design estimate: 2-3k).
        assert!((1_000..5_000).contains(&ranges.len()), "{}", ranges.len());
    }

    #[test]
    fn ranges_merge_only_adjacent_equal_values() {
        let mut props = vec![0; 16];
        props[1..4].fill(3);
        props[4] = 4;
        props[6..8].fill(4);
        props[15] = 0x44;
        assert_eq!(
            ranges(&props),
            [(1, 3, 3), (4, 4, 4), (6, 7, 4), (15, 15, 0x44)]
        );
        assert_eq!(ranges(&[]), Vec::<(u32, u32, u8)>::new());
    }

    #[test]
    fn every_packed_value_is_named_by_its_parts() {
        assert_eq!(names(3), "CONTROL");
        assert_eq!(names(4 | 0x30), "EXTEND | INCB_EXTEND");
        assert_eq!(names(5 | 0x30), "ZWJ | INCB_EXTEND");
        assert_eq!(names(0x20), "INCB_CONSONANT");
        assert_eq!(names(0x40), "EXT_PICT");
        assert_eq!(names(4 | 0x40), "EXTEND | EXT_PICT");
        assert_eq!(
            names(8 | 0x10 | 0x40),
            "SPACING_MARK | INCB_LINKER | EXT_PICT"
        );
        // Every value and constant name is distinct, and the values match
        // `class` in crates/text/src/pos/grapheme.rs.
        let gcb: Vec<(u8, &str)> = GCB_VALUES
            .iter()
            .map(|&(_, bits, name)| (bits, name))
            .collect();
        assert_eq!(
            gcb,
            [
                (1, "CR"),
                (2, "LF"),
                (3, "CONTROL"),
                (4, "EXTEND"),
                (5, "ZWJ"),
                (6, "REGIONAL_INDICATOR"),
                (7, "PREPEND"),
                (8, "SPACING_MARK"),
                (9, "L"),
                (10, "V"),
                (11, "T"),
                (12, "LV"),
                (13, "LVT"),
            ]
        );
        assert_eq!(
            INCB_VALUES.map(|(_, bits, name)| (bits, name)),
            [
                (0x10, "INCB_LINKER"),
                (0x20, "INCB_CONSONANT"),
                (0x30, "INCB_EXTEND")
            ]
        );
    }

    #[test]
    fn rendering_is_deterministic_with_lf_line_endings() {
        let ranges = [
            (0, 9, 3),
            (0x0A, 0x0A, LF),
            (0x1F1E6, 0x1F1FF, 6),
            (0x10_FFFF, 0x10_FFFF, 0x44),
        ];
        let sources = [
            ("GraphemeBreakProperty.txt", "ab"),
            ("emoji-data.txt", "cd"),
        ];
        let text = render(&ranges, &sources);
        assert_eq!(text, render(&ranges, &sources));
        assert!(!text.contains('\r'));
        assert!(text.ends_with("];\n") && !text.ends_with("\n\n"));
        assert!(text.starts_with(
            "// @generated by `cargo xtask ucd` from crates/text/ucd/17.0.0. Do not edit by hand.\n\
             //\n\
             // Sources and their SHA-256:\n\
             //   GraphemeBreakProperty.txt  ab\n\
             //   emoji-data.txt             cd\n\
             //\n"
        ));
        assert!(text.contains("pub(crate) const UNICODE_VERSION: (u8, u8, u8) = (17, 0, 0);\n"));
        assert!(text.contains(
            "pub(crate) static RANGES: [(u32, u32, u8); 4] = [\n    \
             (0x0000, 0x0009, CONTROL),\n    \
             (0x000A, 0x000A, LF),\n    \
             (0x1F1E6, 0x1F1FF, REGIONAL_INDICATOR),\n    \
             (0x10FFFF, 0x10FFFF, EXTEND | EXT_PICT),\n\
             ];\n"
        ));
        assert!(text.contains("\nuse super::class::*;\n"));
    }

    #[test]
    fn the_versions_agree() {
        assert!(DIR.ends_with(VERSION));
        assert!(VERSION.starts_with(&format!("{EMOJI_VERSION}.")));
        assert_eq!(VERSION.replace('.', ", "), "17, 0, 0");
    }

    #[test]
    fn the_pins_are_the_files_in_the_repository() {
        let real = real_root().join(DIR);
        for (file, pin) in PINS {
            let bytes = fs::read(real.join(file)).unwrap();
            assert_eq!(to_hex(&sha256(&bytes)), pin, "{file}");
        }
    }

    #[test]
    fn the_repository_tables_are_what_the_data_generates() {
        let report = check(&real_root()).unwrap();
        assert_eq!(report.violations, Vec::<String>::new());
        let [checked, generated] = report.notes.as_slice() else {
            panic!("expected two notes: {:?}", report.notes);
        };
        assert_eq!(
            checked,
            &format!("checked 5 files in {DIR} against their SHA-256 pins")
        );
        assert!(
            generated.starts_with(&format!("{OUTPUT}: "))
                && generated.ends_with(" ranges (Unicode 17.0.0)"),
            "{generated}"
        );
    }

    #[test]
    fn check_without_the_output_is_a_violation() {
        let tree = tree_with_inputs("ucd-no-output");
        let report = check(tree.path()).unwrap();
        assert_eq!(
            report.violations,
            [format!("{OUTPUT} is missing; run cargo xtask ucd")]
        );
        assert_eq!(report.notes.len(), 2, "{:?}", report.notes);
    }

    #[test]
    fn write_then_check_then_write_again() {
        let tree = tree_with_inputs("ucd-write");
        let report = write(tree.path()).unwrap();
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(report.notes.last(), Some(&format!("wrote {OUTPUT}")));
        let written = fs::read(tree.path().join(OUTPUT)).unwrap();
        assert!(!tree.path().join(OUTPUT).with_extension("rs.tmp").exists());

        let report = check(tree.path()).unwrap();
        assert_eq!(report.violations, Vec::<String>::new());

        let report = write(tree.path()).unwrap();
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes.last(),
            Some(&format!("{OUTPUT} is up to date"))
        );
        assert_eq!(fs::read(tree.path().join(OUTPUT)).unwrap(), written);
    }

    #[test]
    fn write_replaces_a_stale_output_and_a_leftover_temporary_file() {
        let tree = generated_tree("ucd-stale");
        let output = tree.path().join(OUTPUT);
        let expected = fs::read(&output).unwrap();
        tree.write(OUTPUT, "// old\n");
        tree.write(&format!("{OUTPUT}.tmp"), "left over");
        let report = write(tree.path()).unwrap();
        assert_eq!(report.notes.last(), Some(&format!("wrote {OUTPUT}")));
        assert_eq!(fs::read(&output).unwrap(), expected);
        assert!(!output.with_extension("rs.tmp").exists());
    }

    #[test]
    fn an_edited_output_names_the_first_different_line() {
        let tree = generated_tree("ucd-edited-output");
        let output = tree.path().join(OUTPUT);
        let text = fs::read_to_string(&output).unwrap();
        let edits = [
            // A changed entry: the first range is on line 22.
            (text.replacen("CONTROL", "LF", 1), 22),
            // Line endings converted by git.
            (text.replace('\n', "\r\n"), 1),
            // No final line feed, or an extra line.
            (text.trim_end().to_string(), text.lines().count()),
            (format!("{text}\n"), text.lines().count() + 1),
            // Truncated, or past the size limit.
            (String::new(), 1),
            (
                format!("{text}{}", "x".repeat(2 << 20)),
                text.lines().count() + 1,
            ),
        ];
        assert_eq!(text.lines().nth(21), Some("    (0x0000, 0x0009, CONTROL),"));
        for (edited, line) in edits {
            tree.write(OUTPUT, &edited);
            let report = check(tree.path()).unwrap();
            assert_eq!(
                report.violations,
                [format!(
                    "{OUTPUT} is not what {DIR} generates (first difference at line {line}); run \
                     cargo xtask ucd"
                )]
            );
        }
        // Not UTF-8 either.
        tree.write(OUTPUT, [0xFF, b'\n']);
        let report = check(tree.path()).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert!(report.violations[0].contains("(first difference at line 1)"));
    }

    #[test]
    fn an_edited_input_byte_fails_its_pin_and_nothing_is_written() {
        // Each pinned file in turn, the conformance test and the license
        // included, in a tree whose output `write` would otherwise replace.
        for (index, (file, pin)) in PINS.into_iter().enumerate() {
            let tree = tree_with_inputs(&format!("ucd-edited-input-{index}"));
            tree.write(OUTPUT, "// stale\n");
            let path = tree.path().join(DIR).join(file);
            let mut bytes = fs::read(&path).unwrap();
            // The last lowercase ASCII letter made uppercase: still UTF-8,
            // and the same length, with only the end of the file changed.
            let at = bytes.iter().rposition(u8::is_ascii_lowercase).unwrap();
            bytes[at] = bytes[at].to_ascii_uppercase();
            assert!(std::str::from_utf8(&bytes).is_ok(), "{file}");
            fs::write(&path, &bytes).unwrap();
            let digest = to_hex(&sha256(&bytes));
            assert_ne!(digest, pin, "{file}");
            let expected = format!(
                "{DIR}/{file}: SHA-256 is {digest}, but the pin is {pin} (the file must be the \
                 unchanged official one; check git's line-ending conversion and .gitattributes)"
            );
            for run in [check, write] {
                let report = run(tree.path()).unwrap();
                assert_eq!(report.violations, std::slice::from_ref(&expected), "{file}");
                // Nothing was parsed: no notes.
                assert_eq!(report.notes, Vec::<String>::new(), "{file}");
            }
            assert_eq!(
                fs::read(tree.path().join(OUTPUT)).unwrap(),
                b"// stale\n",
                "{file}"
            );
        }
    }

    #[test]
    fn crlf_inputs_fail_their_pins() {
        // What a Windows checkout with `core.autocrlf` would give without
        // the repository's .gitattributes.
        let tree = tree_with_inputs("ucd-crlf");
        let path = tree.path().join(DIR).join("license.txt");
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, text.replace('\n', "\r\n")).unwrap();
        let report = check(tree.path()).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert!(
            report.violations[0].starts_with(&format!("{DIR}/license.txt: SHA-256 is ")),
            "{:?}",
            report.violations
        );
    }

    #[test]
    fn missing_inputs_are_all_listed() {
        let tree = TempTree::new("ucd-no-inputs");
        let report = check(tree.path()).unwrap();
        let expected: Vec<String> = PINS
            .iter()
            .map(|(file, _)| format!("{DIR}/{file} is missing"))
            .collect();
        assert_eq!(report.violations, expected);
        // A directory where a file should be.
        let tree = tree_with_inputs("ucd-directory-input");
        let path = tree.path().join(DIR).join(GCB_FILE);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(
            check(tree.path()).unwrap().violations,
            [format!("{DIR}/{GCB_FILE} is not a regular file")]
        );
    }

    #[test]
    fn an_input_over_the_size_limit_cannot_be_read() {
        let tree = tree_with_inputs("ucd-large-input");
        tree.write(&format!("{DIR}/license.txt"), vec![b'a'; (4 << 20) + 1]);
        match check(tree.path()) {
            Err(Error::Read { path, source }) => {
                assert!(path.ends_with("license.txt"), "{}", path.display());
                assert!(
                    source.to_string().contains("4194304-byte limit"),
                    "{source}"
                );
            }
            other => panic!("expected a read error, got {other:?}"),
        }
    }

    #[test]
    fn a_linked_input_is_a_violation() {
        let tree = tree_with_inputs("ucd-linked-input");
        let outside = tree_with_inputs("ucd-linked-input-target");
        let link = format!("{DIR}/{GCB_FILE}");
        fs::remove_file(tree.path().join(&link)).unwrap();
        // The link leads to the right bytes: it is refused all the same.
        if let Err(reason) = symlink(&tree, &link, &outside.path().join(&link)) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_linked_input_is_a_violation: cannot make a link here: {reason}"
            );
            return;
        }
        for run in [check, write] {
            let report = run(tree.path()).unwrap();
            assert_eq!(
                report.violations,
                [format!(
                    "{DIR}/{GCB_FILE} is a symbolic link, which ucd does not follow"
                )]
            );
        }
        assert!(!tree.path().join(OUTPUT).exists());
    }

    #[test]
    fn a_linked_or_directory_output_is_a_violation_and_is_not_written() {
        let tree = tree_with_inputs("ucd-linked-output");
        let outside = TempTree::new("ucd-linked-output-target");
        outside.write("tables.rs", "// outside\n");
        let target = outside.path().join("tables.rs");
        if let Err(reason) = symlink(&tree, OUTPUT, &target) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_linked_or_directory_output_is_a_violation_and_is_not_written: \
                 cannot make a link here: {reason}"
            );
        } else {
            let expected = format!(
                "{OUTPUT} is a symbolic link, which ucd does not follow or replace; remove it, \
                 then run cargo xtask ucd"
            );
            for run in [check, write] {
                assert_eq!(
                    run(tree.path()).unwrap().violations,
                    std::slice::from_ref(&expected)
                );
            }
            assert_eq!(fs::read(&target).unwrap(), b"// outside\n");
            fs::remove_file(tree.path().join(OUTPUT)).unwrap();
        }
        fs::create_dir_all(tree.path().join(OUTPUT)).unwrap();
        let expected =
            format!("{OUTPUT} is not a regular file; remove it, then run cargo xtask ucd");
        for run in [check, write] {
            assert_eq!(
                run(tree.path()).unwrap().violations,
                std::slice::from_ref(&expected)
            );
        }
        assert!(tree.path().join(OUTPUT).is_dir());
    }

    /// Every directory below the root on the way to [`DIR`] or to
    /// [`OUTPUT`], shallowest first; the first two are on both ways.
    const DIRECTORIES: [&str; 7] = [
        "crates",
        "crates/text",
        "crates/text/ucd",
        DIR,
        "crates/text/src",
        "crates/text/src/pos",
        OUTPUT_DIR,
    ];

    /// The SHA-256 of each of `files` in `tree`, `None` for one that is
    /// missing.
    fn digests(tree: &TempTree, files: &[String]) -> Vec<(String, Option<String>)> {
        files
            .iter()
            .map(|file| {
                let bytes = fs::read(tree.path().join(file)).ok();
                (file.clone(), bytes.map(|bytes| to_hex(&sha256(&bytes))))
            })
            .collect()
    }

    #[test]
    fn a_linked_output_directory_is_a_violation_and_what_it_leads_to_is_untouched() {
        // What `write` would replace and remove if it went through the link.
        let output = OUTPUT.to_string();
        let temporary = format!("{OUTPUT}.tmp");
        for (index, linked) in ["crates/text/src", "crates/text/src/pos", OUTPUT_DIR]
            .into_iter()
            .enumerate()
        {
            let tree = tree_with_inputs(&format!("ucd-linked-output-dir-{index}"));
            let outside = TempTree::new(&format!("ucd-linked-output-dir-target-{index}"));
            outside.write(&output, "// outside\n");
            outside.write(&temporary, "// outside, left over\n");
            if let Err(reason) = symlink(&tree, linked, &outside.path().join(linked)) {
                let _ = writeln!(
                    io::stderr(),
                    "SKIPPED: a_linked_output_directory_is_a_violation_and_what_it_leads_to_is_\
                     untouched: cannot make a link here: {reason}"
                );
                return;
            }
            let expected = format!("{linked} is a symbolic link, which ucd does not follow");
            for run in [check, write] {
                let report = run(tree.path()).unwrap();
                assert_eq!(report.violations, std::slice::from_ref(&expected));
                // Refused before any input was read.
                assert_eq!(report.notes, Vec::<String>::new(), "{linked}");
            }
            let outside_file = |file: &str| fs::read(outside.path().join(file)).unwrap();
            assert_eq!(outside_file(&output), b"// outside\n", "{linked}");
            assert_eq!(
                outside_file(&temporary),
                b"// outside, left over\n",
                "{linked}"
            );
            let entries = fs::read_dir(outside.path().join(OUTPUT_DIR)).unwrap();
            assert_eq!(entries.count(), 2, "{linked}");
        }
    }

    #[test]
    fn a_linked_input_directory_is_a_violation_and_nothing_is_read_through_it() {
        let files: Vec<String> = PINS
            .iter()
            .map(|(file, _)| format!("{DIR}/{file}"))
            .chain([OUTPUT.to_string(), format!("{OUTPUT}.tmp")])
            .collect();
        // The last two lead to the output as well, and are named once.
        for (index, linked) in [DIR, "crates/text/ucd", "crates/text", "crates"]
            .into_iter()
            .enumerate()
        {
            let tree = tree_with_inputs(&format!("ucd-linked-input-dir-{index}"));
            tree.write(OUTPUT, "// stale\n");
            // Behind the link: the right inputs, which would pass their
            // pins and add notes if they were read, and a stale output.
            let outside = tree_with_inputs(&format!("ucd-linked-input-dir-target-{index}"));
            outside.write(OUTPUT, "// outside\n");
            let before = digests(&outside, &files);
            fs::remove_dir_all(tree.path().join(linked)).unwrap();
            if let Err(reason) = symlink(&tree, linked, &outside.path().join(linked)) {
                let _ = writeln!(
                    io::stderr(),
                    "SKIPPED: a_linked_input_directory_is_a_violation_and_nothing_is_read_through_\
                     it: cannot make a link here: {reason}"
                );
                return;
            }
            let expected = format!("{linked} is a symbolic link, which ucd does not follow");
            for run in [check, write] {
                let report = run(tree.path()).unwrap();
                assert_eq!(report.violations, std::slice::from_ref(&expected));
                assert_eq!(report.notes, Vec::<String>::new(), "{linked}");
            }
            assert_eq!(digests(&outside, &files), before, "{linked}");
            if !OUTPUT.starts_with(&format!("{linked}/")) {
                assert_eq!(
                    fs::read(tree.path().join(OUTPUT)).unwrap(),
                    b"// stale\n",
                    "{linked}"
                );
            }
        }
    }

    #[test]
    fn a_directory_on_the_way_that_is_a_file_is_a_violation() {
        // The list is every directory the walks see, each once.
        assert_eq!(Path::new(OUTPUT).parent(), Some(Path::new(OUTPUT_DIR)));
        let mut walked: Vec<&str> = Vec::new();
        for path in [DIR, OUTPUT_DIR] {
            let ends = path.match_indices('/').map(|(end, _)| end);
            for dir in ends.chain([path.len()]).map(|end| &path[..end]) {
                if !walked.contains(&dir) {
                    walked.push(dir);
                }
            }
        }
        assert_eq!(walked, DIRECTORIES);
        for (index, dir) in DIRECTORIES.into_iter().enumerate() {
            let tree = tree_with_inputs(&format!("ucd-file-in-the-way-{index}"));
            tree.write(OUTPUT, "// stale\n");
            fs::remove_dir_all(tree.path().join(dir)).unwrap();
            tree.write(dir, "not a directory\n");
            // Once only for `crates` and `crates/text`, on both ways.
            let expected = format!("{dir} is not a directory");
            for run in [check, write] {
                let report = run(tree.path()).unwrap();
                assert_eq!(report.violations, std::slice::from_ref(&expected));
                assert_eq!(report.notes, Vec::<String>::new(), "{dir}");
            }
            assert_eq!(
                fs::read(tree.path().join(dir)).unwrap(),
                b"not a directory\n",
                "{dir}"
            );
            if !OUTPUT.starts_with(&format!("{dir}/")) {
                assert_eq!(
                    fs::read(tree.path().join(OUTPUT)).unwrap(),
                    b"// stale\n",
                    "{dir}"
                );
            }
        }
    }

    #[test]
    fn the_walk_that_makes_directories_never_makes_one_through_a_link() {
        // What `write` walks just before writing, once the first walk has
        // passed: here it is called with nothing checked before.
        let tree = TempTree::new("ucd-make-directories");
        tree.write("crates/text/Cargo.toml", "");
        assert_eq!(
            directories(tree.path(), OUTPUT_DIR, false).unwrap(),
            Ok(Walk::Missing)
        );
        for _ in 0..2 {
            assert_eq!(
                directories(tree.path(), OUTPUT_DIR, true).unwrap(),
                Ok(Walk::Present)
            );
            assert!(tree.path().join(OUTPUT_DIR).is_dir());
        }
        assert_eq!(
            fs::read(tree.path().join("crates/text/Cargo.toml")).unwrap(),
            b""
        );

        let tree = TempTree::new("ucd-make-directories-file");
        tree.write("crates/text/src", "not a directory\n");
        assert_eq!(
            directories(tree.path(), OUTPUT_DIR, true).unwrap(),
            Err("crates/text/src is not a directory".to_string())
        );
        assert_eq!(
            fs::read(tree.path().join("crates/text/src")).unwrap(),
            b"not a directory\n"
        );

        let tree = TempTree::new("ucd-make-directories-link");
        let outside = TempTree::new("ucd-make-directories-link-target");
        if let Err(reason) = symlink(&tree, "crates/text/src", outside.path()) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: the_walk_that_makes_directories_never_makes_one_through_a_link: \
                 cannot make a link here: {reason}"
            );
            return;
        }
        assert_eq!(
            directories(tree.path(), OUTPUT_DIR, true).unwrap(),
            Err("crates/text/src is a symbolic link, which ucd does not follow".to_string())
        );
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_write_that_fails_is_a_write_error() {
        // The temporary file cannot be made: a directory is in its place,
        // which `remove_file` does not remove. (A file in place of one of
        // the directories is a violation instead: see the test above.)
        let tree = tree_with_inputs("ucd-write-error");
        let temporary = format!("{OUTPUT}.tmp");
        fs::create_dir_all(tree.path().join(&temporary)).unwrap();
        match write(tree.path()) {
            Err(Error::Write { path, .. }) => {
                assert!(path.ends_with("tables.rs.tmp"), "{}", path.display());
            }
            other => panic!("expected a write error, got {other:?}"),
        }
        assert!(!tree.path().join(OUTPUT).exists());
        assert!(tree.path().join(&temporary).is_dir());
    }

    #[test]
    fn first_difference_counts_lines_from_1() {
        assert_eq!(first_difference(b"a\nb\n", b"a\nc\n"), 2);
        assert_eq!(first_difference(b"a\nb\n", b"a\nb"), 2);
        assert_eq!(first_difference(b"a\nb\n", b"a\nb\nc\n"), 3);
        assert_eq!(first_difference(b"a\n", b"a\r\n"), 1);
        assert_eq!(first_difference(b"a\n", b""), 1);
    }

    #[test]
    fn spot_checks_against_the_unicode_data() {
        // Each looked up by hand in the three files.
        let props = real_properties();
        for (code_point, expected) in [
            (0x0600, "PREPEND"),
            (0x0903, "SPACING_MARK"),
            (0x1100, "L"),
            (0x1160, "V"),
            (0x11A8, "T"),
            (0xAC00, "LV"),
            (0xAC01, "LVT"),
            (0x0915, "INCB_CONSONANT"),
            (0x094D, "EXTEND | INCB_LINKER"),
            (0x1B44, "EXTEND | INCB_LINKER"),
            (0x0300, "EXTEND | INCB_EXTEND"),
            (0x200C, "EXTEND"),
            (0x200D, "ZWJ | INCB_EXTEND"),
            (0x1F1E6, "REGIONAL_INDICATOR"),
            (0x1F600, "EXT_PICT"),
            (0x00A9, "EXT_PICT"),
            (0x1F3FB, "EXTEND | INCB_EXTEND"),
            (0x000D, "CR"),
            (0x000A, "LF"),
            (0x0000, "CONTROL"),
            (0x0041, ""),
        ] {
            assert_eq!(names(props[code_point]), expected, "U+{code_point:04X}");
        }
    }
}
