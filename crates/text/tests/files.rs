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

//! Loading and saving real files through the public API: unchanged files
//! round-trip byte for byte (BOMs, UTF-16, every line ending, no trailing
//! newline), a file that is not valid UTF-8 is never written back, refused
//! saves change nothing, and the large-file threshold. On Unix, also modes
//! and symbolic links (ADR-0025); on Unix and Windows, a relative link
//! behind a link to a directory, and a link target with a root or a drive
//! alone (ADR-0034).
//!
//! Each test has its own scratch directory under `target/tmp`, removed when
//! it passes.

use std::fs;
use std::io::{self, Write as _};
use std::ops::Deref;
use std::path::{Path, PathBuf};

use tidepool_text::file::{self, LARGE_FILE_BYTES, TEMP_PREFIX};
use tidepool_text::{Encoding, FileStep, Format, LineBreak, LineEndings, Loaded, Rope, TextError};

/// A test's scratch directory. It is removed when the test passes; a
/// failing test leaves it behind for inspection.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            // Best effort: a leftover directory only takes space.
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

impl Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

/// Returns a fresh, empty directory for one test, under `target/tmp`. The
/// process id keeps concurrent runs of this binary out of each other's way.
fn scratch_dir(test: &str) -> Scratch {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("tidepool-text-files")
        .join(format!("{test}-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("remove the previous run's directory");
    }
    fs::create_dir_all(&dir).expect("create the test directory");
    Scratch(dir)
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

fn load(path: &Path) -> Loaded {
    file::load(path).unwrap_or_else(|error| panic!("load {}: {error}", path.display()))
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("list the directory")
        .map(|entry| {
            let entry = entry.expect("read an entry");
            entry.file_name().to_string_lossy().into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Whether `dir` holds a temporary file of a save.
fn has_temp(dir: &Path) -> bool {
    names(dir).iter().any(|name| name.starts_with(TEMP_PREFIX))
}

fn utf16(text: &str, big_endian: bool) -> Vec<u8> {
    let mut bytes = if big_endian {
        vec![0xFE, 0xFF]
    } else {
        vec![0xFF, 0xFE]
    };
    for unit in text.encode_utf16() {
        let pair = if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        };
        bytes.extend_from_slice(&pair);
    }
    bytes
}

const UTF8_BOM: Format = Format::new(Encoding::Utf8, true);

/// One unchanged-file case: its bytes, its format, its line endings
/// (LF, CR LF, CR) and its trailing break.
struct Case {
    name: &'static str,
    bytes: Vec<u8>,
    format: Format,
    endings: [usize; 3],
    trailing: Option<LineBreak>,
}

fn cases() -> Vec<Case> {
    let case = |name, bytes: &[u8], format, endings, trailing| Case {
        name,
        bytes: bytes.to_vec(),
        format,
        endings,
        trailing,
    };
    let mut cases = vec![
        case("empty", b"", Format::UTF8, [0, 0, 0], None),
        case(
            "utf8",
            b"fn main() {}\n",
            Format::UTF8,
            [1, 0, 0],
            Some(LineBreak::Lf),
        ),
        case(
            "utf8-bom",
            b"\xEF\xBB\xBFhello\r\n",
            UTF8_BOM,
            [0, 1, 0],
            Some(LineBreak::CrLf),
        ),
        case("bom-only", b"\xEF\xBB\xBF", UTF8_BOM, [0, 0, 0], None),
        case(
            "lf",
            b"a\nb\nc\n",
            Format::UTF8,
            [3, 0, 0],
            Some(LineBreak::Lf),
        ),
        case(
            "crlf",
            b"a\r\nb\r\nc\r\n",
            Format::UTF8,
            [0, 3, 0],
            Some(LineBreak::CrLf),
        ),
        case(
            "lone-cr",
            b"a\rb\rc\r",
            Format::UTF8,
            [0, 0, 3],
            Some(LineBreak::Cr),
        ),
        case(
            "mixed",
            b"a\nb\r\nc\rd\r\n\n",
            Format::UTF8,
            [2, 2, 1],
            Some(LineBreak::Lf),
        ),
        case(
            "no-trailing-newline",
            b"first\nlast",
            Format::UTF8,
            [1, 0, 0],
            None,
        ),
        case(
            "crlf-no-trailing-newline",
            b"first\r\nlast",
            Format::UTF8,
            [0, 1, 0],
            None,
        ),
        case(
            "non-ascii",
            "Grüße, 世界 🌊\u{FEFF}\u{FFFD}\n".as_bytes(),
            Format::UTF8,
            [1, 0, 0],
            Some(LineBreak::Lf),
        ),
    ];
    let text = "héllo 😀\r\nwörld\rend";
    cases.push(Case {
        name: "utf16le",
        bytes: utf16(text, false),
        format: Format::new(Encoding::Utf16Le, true),
        endings: [0, 1, 1],
        trailing: None,
    });
    cases.push(Case {
        name: "utf16be",
        bytes: utf16("line\n\u{10FFFF}\n", true),
        format: Format::new(Encoding::Utf16Be, true),
        endings: [2, 0, 0],
        trailing: Some(LineBreak::Lf),
    });
    // Long enough for many leaves, with CR LFs at every distance from a seam.
    let long: String = (0..20_000).map(|i| format!("{i}é\r\n")).collect();
    cases.push(Case {
        name: "long-crlf",
        bytes: long.into_bytes(),
        format: UTF8_BOM,
        endings: [0, 20_000, 0],
        trailing: Some(LineBreak::CrLf),
    });
    if let Some(last) = cases.last_mut() {
        last.bytes.splice(0..0, [0xEF, 0xBB, 0xBF]);
    }
    cases
}

#[test]
fn unchanged_files_save_byte_for_byte() {
    let dir = scratch_dir("unchanged");
    for case in cases() {
        let path = dir.join(format!("{}.txt", case.name));
        write(&path, &case.bytes);
        let loaded = load(&path);
        let name = case.name;
        assert_eq!(loaded.format(), Some(case.format), "{name}");
        assert_eq!(loaded.detected(), case.format, "{name}");
        assert!(!loaded.read_only(), "{name}");
        let [lf, crlf, cr] = case.endings;
        assert_eq!(
            loaded.line_endings(),
            LineEndings { lf, crlf, cr },
            "{name}"
        );
        assert_eq!(loaded.trailing_break(), case.trailing, "{name}");
        assert_eq!(loaded.file_len(), case.bytes.len() as u64, "{name}");
        assert!(!loaded.is_large(), "{name}");
        assert_eq!(loaded.decoded().original_bytes(), case.bytes, "{name}");

        let format = loaded.format().expect("a valid file has a format");
        file::save(&path, loaded.text(), format).expect("save");
        assert!(read(&path) == case.bytes, "{name} changed on save");
        // And once more, from what the save wrote.
        let again = load(&path);
        assert_eq!(again.text(), loaded.text(), "{name}");
        file::save(&path, again.text(), format).expect("save again");
        assert!(
            read(&path) == case.bytes,
            "{name} changed on the second save"
        );
    }
    assert!(!has_temp(&dir), "{:?}", names(&dir));
}

#[test]
fn the_mixed_file_reports_its_endings() {
    let dir = scratch_dir("mixed");
    let path = dir.join("mixed.txt");
    write(&path, b"a\r\nb\r\nc\nd\re");
    let endings = load(&path).line_endings();
    assert!(endings.is_mixed());
    assert_eq!(endings.total(), 4);
    assert_eq!(endings.dominant(), LineBreak::CrLf);
}

#[test]
fn edited_text_saves_with_its_breaks_as_they_are() {
    let dir = scratch_dir("edited");
    let path = dir.join("edited.txt");
    write(&path, b"\xEF\xBB\xBFone\r\ntwo\r\n");
    let loaded = load(&path);
    let mut text = loaded.text().clone();
    let at = text.len();
    text.insert(at, "three").expect("insert at the end");
    file::save(&path, &text, loaded.format().expect("valid")).expect("save");
    assert_eq!(read(&path), b"\xEF\xBB\xBFone\r\ntwo\r\nthree");
}

#[test]
fn invalid_utf8_loads_read_only_and_is_never_written() {
    let dir = scratch_dir("invalid");
    let cases: [&[u8]; 4] = [
        b"ok \xFF\xFE bad \xC0\x80 \xED\xA0\x80\r\nend \xF0\x9F",
        b"\x80",
        b"\xEF\xBB\xBFbom then \xFF",
        // UTF-16 with a lone surrogate and an odd last byte.
        b"\xFF\xFEa\0\x00\xD8b\0!",
    ];
    for (i, bytes) in cases.into_iter().enumerate() {
        let path = dir.join(format!("invalid-{i}.bin"));
        write(&path, bytes);
        let loaded = load(&path);
        assert!(loaded.read_only(), "case {i}");
        assert_eq!(loaded.format(), None, "case {i}");
        assert_eq!(loaded.decoded().format(), None, "case {i}");
        assert!(loaded.decoded().replaced().len() > 0, "case {i}");
        assert_eq!(loaded.decoded().original_bytes(), bytes, "case {i}");
        assert_eq!(loaded.file_len(), bytes.len() as u64, "case {i}");
        // Loading wrote nothing.
        assert_eq!(read(&path), bytes, "case {i}");
    }
    assert_eq!(names(&dir).len(), cases.len());
}

#[test]
fn a_successful_save_leaves_no_temp_file() {
    let dir = scratch_dir("no-temp");
    let path = dir.join("a.txt");
    write(&path, b"old");
    for round in 0..5 {
        file::save(&path, &Rope::from(format!("round {round}")), Format::UTF8).expect("save");
        assert_eq!(names(&dir), ["a.txt"]);
    }
    assert_eq!(read(&path), b"round 4");
}

#[test]
fn a_new_file_is_created() {
    let dir = scratch_dir("new");
    let path = dir.join("new.txt");
    let text = Rope::from("brand new\r\n");
    file::save(&path, &text, Format::UTF8).expect("save");
    assert_eq!(read(&path), b"brand new\r\n");
    assert_eq!(names(&dir), ["new.txt"]);

    let path = dir.join("new-utf16.txt");
    file::save(&path, &text, Format::new(Encoding::Utf16Be, true)).expect("save");
    assert_eq!(read(&path), utf16("brand new\r\n", true));
    assert_eq!(*load(&path).text(), "brand new\r\n");
}

#[test]
fn a_new_file_in_a_missing_directory_is_an_error() {
    let dir = scratch_dir("missing-dir");
    let path = dir.join("missing").join("new.txt");
    let error = file::save(&path, &Rope::from("x"), Format::UTF8).expect_err("no directory");
    assert!(
        matches!(
            error,
            TextError::Io {
                step: FileStep::CreateTemp,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(names(&dir).is_empty());
}

#[test]
fn a_directory_is_refused() {
    let dir = scratch_dir("directory");
    let sub = dir.join("sub");
    fs::create_dir(&sub).expect("create a directory");
    let error = file::load(&sub).expect_err("a directory does not load");
    assert!(
        matches!(error, TextError::NotRegularFile { directory: true }),
        "{error:?}"
    );
    let error = file::save(&sub, &Rope::from("x"), Format::UTF8).expect_err("nor saves");
    assert!(
        matches!(error, TextError::NotRegularFile { directory: true }),
        "{error:?}"
    );
    assert_eq!(names(&dir), ["sub"]);
    assert!(names(&sub).is_empty());
}

#[test]
fn a_device_does_not_load() {
    let null = Path::new("/dev/null");
    if !null.exists() {
        let _ = writeln!(
            io::stderr(),
            "SKIPPED: a_device_does_not_load: this system has no /dev/null"
        );
        return;
    }
    let error = file::load(null).expect_err("a device does not load");
    assert!(
        matches!(error, TextError::NotRegularFile { directory: false }),
        "{error:?}"
    );
}

#[test]
fn a_missing_file_does_not_load() {
    let dir = scratch_dir("missing");
    let error = file::load(dir.join("nothing.txt")).expect_err("nothing to load");
    assert!(
        matches!(
            error,
            TextError::Io {
                step: FileStep::Metadata,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn a_read_only_file_is_refused_and_unchanged() {
    let dir = scratch_dir("read-only");
    let path = dir.join("locked.txt");
    write(&path, b"keep me");
    let original = fs::metadata(&path).expect("stat").permissions();
    let mut read_only = original.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&path, read_only).expect("make it read-only");
    let writable_anyway = fs::OpenOptions::new().write(true).open(&path).is_ok();

    let result = file::save(&path, &Rope::from("changed"), Format::UTF8);

    let after = read(&path);
    // Writable again, so that the scratch directory can be removed.
    fs::set_permissions(&path, original).expect("restore the permissions");
    if writable_anyway {
        // Root may write a read-only file, so the save goes through.
        let _ = writeln!(
            io::stderr(),
            "SKIPPED: a_read_only_file_is_refused_and_unchanged: this user can write \
             read-only files"
        );
        return;
    }
    let error = result.expect_err("a read-only file is not saved");
    assert!(matches!(error, TextError::NotWritable { .. }), "{error:?}");
    assert_eq!(after, b"keep me");
    assert_eq!(names(&dir), ["locked.txt"]);
}

#[test]
fn the_large_file_threshold_is_16_mib() {
    assert_eq!(LARGE_FILE_BYTES, 16 * 1024 * 1024);
    let dir = scratch_dir("large");
    let line = b"0123456789abcdef0123456789abcde\n";
    let mut bytes = line.repeat(LARGE_FILE_BYTES as usize / line.len());
    assert_eq!(bytes.len() as u64, LARGE_FILE_BYTES);

    let path = dir.join("exactly.txt");
    write(&path, &bytes);
    let loaded = load(&path);
    assert_eq!(loaded.file_len(), LARGE_FILE_BYTES);
    assert!(!loaded.is_large());
    assert_eq!(loaded.text().len() as u64, LARGE_FILE_BYTES);
    drop(loaded);

    bytes.push(b'!');
    let path = dir.join("one-more.txt");
    write(&path, &bytes);
    let loaded = load(&path);
    assert_eq!(loaded.file_len(), LARGE_FILE_BYTES + 1);
    assert!(loaded.is_large());
    assert!(!loaded.read_only(), "large is not read-only here");
    assert_eq!(loaded.trailing_break(), None);
}

#[test]
fn file_errors_hold_no_path() {
    let dir = scratch_dir("messages");
    let path = dir.join("secret-name.txt");
    let error = file::load(&path).expect_err("missing");
    let message = error.to_string();
    assert!(!message.contains("secret-name"), "{message}");
    assert!(
        message.starts_with("could not read the metadata of the path: "),
        "{message}"
    );
}

/// Says on standard error, as the gate counts it, that `test` skipped.
fn skipped(test: &str, reason: &str) {
    let _ = writeln!(io::stderr(), "SKIPPED: {test}: {reason}");
}

/// `relative` with this OS's separator: Windows keeps a link's target as it
/// is written.
fn rel(relative: &str) -> PathBuf {
    relative.split('/').collect()
}

/// Whether `std` parses Windows' prefixes here: asked at run time, not
/// through `cfg`, as `core::path`'s tests ask it.
fn windows_paths() -> bool {
    matches!(
        Path::new(r"C:\").components().next(),
        Some(std::path::Component::Prefix(_))
    )
}

/// Makes a symbolic link to a directory. Windows tells links to directories
/// from links to files, and `fs::soft_link` makes only the second, so the
/// two OSes need a function each: ADR-0025 allows `cfg(unix)` in the tests
/// of links, and ADR-0034 `cfg(windows)` in those that link to directories.
#[cfg(unix)]
fn directory_link(target: &Path, at: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

/// Makes a symbolic link to a directory (ADR-0034).
#[cfg(windows)]
fn directory_link(target: &Path, at: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, at)
}

/// Makes a symbolic link to a file, beside [`directory_link`].
#[cfg(unix)]
fn file_link(target: &Path, at: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

/// Makes a symbolic link to a file, beside [`directory_link`] (ADR-0034).
#[cfg(windows)]
fn file_link(target: &Path, at: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, at)
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .expect("lstat")
        .file_type()
        .is_symlink()
}

/// ADR-0034: a relative link reached through a link to a directory is
/// followed from the directory the OS reaches. `w/a` links to `b/c/d`, and
/// `w/b/c/d/l` to `../../x`, so the OS opens `w/a/l` as `w/b/x`. Before
/// ADR-0034 a save joined the target onto `w/a`, the link's directory as
/// given, which the decoy `x` beside `w` stands for. The kernel applied the
/// `..` after the link on Unix anyway, so this passed there before the fix;
/// Win32 applies them to the text, so on Windows the save wrote the decoy,
/// and this test would have failed.
#[test]
fn a_relative_link_behind_a_directory_link_saves_where_the_os_opens_it() {
    let test = "a_relative_link_behind_a_directory_link_saves_where_the_os_opens_it";
    let dir = scratch_dir("behind-a-directory-link");
    let w = dir.join("w");
    let d = w.join("b").join("c").join("d");
    fs::create_dir_all(&d).expect("mkdir");
    let l = d.join("l");
    if let Err(error) =
        directory_link(&rel("b/c/d"), &w.join("a")).and_then(|()| file_link(&rel("../../x"), &l))
    {
        return skipped(test, &format!("cannot make a link here: {error}"));
    }
    let through = w.join("a").join("l");

    // Nothing there yet: the save creates the file where the OS opens it.
    file::save(&through, &Rope::from("first"), Format::UTF8).expect("save");
    assert_eq!(read(&w.join("b").join("x")), b"first");
    assert_eq!(names(&dir), ["w"]);
    assert_eq!(names(&w), ["a", "b"]);
    assert!(is_link(&l));
    assert_eq!(fs::read_link(&l).expect("readlink"), rel("../../x"));

    // Again, with a decoy where the path as given would lead.
    write(&dir.join("x"), b"decoy");
    file::save(&through, &Rope::from("second"), Format::UTF8).expect("save");
    assert_eq!(read(&w.join("b").join("x")), b"second");
    assert_eq!(read(&dir.join("x")), b"decoy");
    assert!(is_link(&l));
    assert_eq!(names(&w.join("b")), ["c", "x"]);
    assert_eq!(names(&dir), ["w", "x"]);
}

/// ADR-0034: a link target with a root or a drive but not the rest of an
/// absolute path (`\x`, `c:x`) means something that depends on the
/// current drive, so where Windows' path rules apply a save refuses it, as
/// `core::path::resolve` does, and its message does not name the target.
/// Elsewhere both are ordinary names, and the save writes through them.
///
/// On Windows this may print `SKIPPED`: making the link stores a
/// drive-relative target (`c:x`) as an absolute path, which is no longer
/// the case under test, and an account may not make links at all. The
/// refusal itself is tested on `follow` in `src/file.rs`, with no link.
#[test]
fn a_target_with_a_root_or_a_drive_alone_is_refused_where_windows_reads_it() {
    let test = "a_target_with_a_root_or_a_drive_alone_is_refused_where_windows_reads_it";
    let dir = scratch_dir("root-or-drive-target");
    let mut skips = Vec::new();
    for (name, target) in [("rooted", r"\x"), ("drive", "c:x")] {
        let link = dir.join(name);
        // `fs::soft_link` is deprecated in favour of one function per OS,
        // but those need a `cfg`, which this test does not; on Windows it
        // makes a link to a file, which is what is wanted here.
        #[allow(deprecated)]
        let made = fs::soft_link(target, &link);
        if let Err(error) = made {
            skips.push(format!("cannot make the link `{name}` here: {error}"));
            continue;
        }
        // Windows may store such a target as an absolute path instead,
        // which would then be followed somewhere outside this test.
        let stored = fs::read_link(&link).expect("readlink");
        if stored != Path::new(target) {
            skips.push(format!("the OS stored the target of `{name}` otherwise"));
            continue;
        }
        let saved = file::save(&link, &Rope::from("through"), Format::UTF8);
        if windows_paths() {
            let error = saved.expect_err("refused");
            assert!(
                matches!(&error, TextError::Io { step: FileStep::ReadLink, error }
                    if error.kind() == io::ErrorKind::InvalidInput),
                "{error:?}"
            );
            let message = error.to_string();
            assert!(!message.contains(target), "{message}");
        } else {
            saved.expect("an ordinary name here");
            assert_eq!(read(&dir.join(target)), b"through");
        }
        assert!(is_link(&link));
    }
    if !skips.is_empty() {
        skipped(test, &skips.join("; "));
    }
}

/// ADR-0034: where Windows' path rules apply, the directory a relative link
/// is in is taken from the whole path, as Win32 reads it, not from its
/// parent alone. Win32 keeps a trailing space on a name inside a path and
/// trims it from a path's last name, so the parent `w\space ` of the plain
/// `w\space \l` opened on its own is `w\space`. Elsewhere the names are
/// ordinary and the save writes there too.
#[test]
fn a_link_below_a_name_win32_trims_at_the_end_saves_beside_it() {
    let test = "a_link_below_a_name_win32_trims_at_the_end_saves_beside_it";
    let dir = scratch_dir("trimmed-name");
    // On Windows this is `\\?\…`, through which a trailing space is kept.
    let exact = fs::canonicalize(&*dir).expect("canonicalize");
    let w = exact.join("w");
    fs::create_dir_all(w.join("space ")).expect("mkdir");
    fs::create_dir(w.join("space")).expect("mkdir");
    if let Err(error) = file_link(Path::new("x"), &w.join("space ").join("l")) {
        return skipped(test, &format!("cannot make a link here: {error}"));
    }
    // The plain path, in which Win32 keeps the inner space.
    let through = dir.join("w").join("space ").join("l");
    file::save(&through, &Rope::from("text"), Format::UTF8).expect("save");
    assert_eq!(read(&w.join("space ").join("x")), b"text");
    assert_eq!(names(&w.join("space ")), ["l", "x"]);
    assert!(names(&w.join("space")).is_empty());
    assert!(is_link(&w.join("space ").join("l")));
}

/// A link whose target ends in a separator asks for a directory. Saving a
/// file through it is an error, whether that name is a file or missing,
/// and writes nothing. For a file, the OS refuses the path when the save
/// reads its metadata. For a missing name, `save` refuses it before the
/// temporary file: Unix's rename would refuse it too, but NT's drops the
/// separator and makes a file `x` (ADR-0034).
#[test]
fn a_link_to_a_name_with_a_final_separator_saves_no_file() {
    let test = "a_link_to_a_name_with_a_final_separator_saves_no_file";
    let dir = scratch_dir("separator-target");
    let mut target = std::ffi::OsString::from("x");
    target.push(std::path::MAIN_SEPARATOR_STR);
    // `fs::soft_link` is deprecated in favour of one function per OS, but
    // those need a `cfg`, which this test does not.
    #[allow(deprecated)]
    let made = fs::soft_link(&target, dir.join("l"));
    if let Err(error) = made {
        return skipped(test, &format!("cannot make a link here: {error}"));
    }
    // `x` missing: refused as Unix's rename refuses it, and no file named
    // `x` is made.
    let error = file::save(dir.join("l"), &Rope::from("new"), Format::UTF8).expect_err("missing");
    assert!(
        matches!(
            &error,
            TextError::Io { step: FileStep::Rename, error } if error.kind() == io::ErrorKind::NotADirectory
        ),
        "{error:?}"
    );
    assert_eq!(names(&dir), ["l"]);
    // `x` a file: it is not replaced.
    write(&dir.join("x"), b"old");
    file::save(dir.join("l"), &Rope::from("new"), Format::UTF8).expect_err("a file");
    assert_eq!(read(&dir.join("x")), b"old");
    assert_eq!(names(&dir), ["l", "x"]);
}

// ADR-0034 keeps the lexical join on Unix because the kernel resolves a
// path through `/proc` magic links (`/proc/<pid>/root`, `/proc/<pid>/cwd`,
// `/proc/self/fd/N`) and mounts as it opens it, while `realpath(3)` reads a
// magic link's target as text. No test here can show the difference: it
// needs a mount namespace. Reproduced under `unshare -Urm`: make
// `base/d/l -> x` and `base/d/x`, open `base/d` as descriptor 3, mount a
// tmpfs over `base` holding another `base/d/x`, then save through
// `/proc/self/fd/3/l`. The lexical join writes the hidden `x` that
// `cat /proc/self/fd/3/l` reads; joining onto `realpath` of the link's
// directory wrote the covering one.

/// Modes and symbolic links: ADR-0025 allows `cfg(unix)` in these tests.
#[cfg(unix)]
mod unix {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).expect("stat").permissions().mode() & 0o7777
    }

    #[test]
    fn modes_are_kept() {
        let dir = scratch_dir("modes");
        for wanted in [0o600, 0o755, 0o640, 0o444 | 0o200] {
            let path = dir.join(format!("mode-{wanted:o}.txt"));
            write(&path, b"old");
            fs::set_permissions(&path, fs::Permissions::from_mode(wanted)).expect("chmod");
            let loaded = load(&path);
            file::save(&path, &Rope::from("new"), loaded.format().expect("valid")).expect("save");
            assert_eq!(mode(&path), wanted, "{wanted:o}");
            assert_eq!(read(&path), b"new");
        }
        assert!(!has_temp(&dir));
    }

    /// ADR-0025: the new file has the saver's owner and group, so it does
    /// not keep setuid or setgid, as a write in place would not.
    #[test]
    fn setuid_and_setgid_are_cleared() {
        let dir = scratch_dir("setid");
        for (before, after) in [(0o4755, 0o755), (0o2755, 0o755), (0o6750, 0o750)] {
            let path = dir.join(format!("mode-{before:o}.txt"));
            write(&path, b"old");
            fs::set_permissions(&path, fs::Permissions::from_mode(before)).expect("chmod");
            assert_eq!(mode(&path), before, "{before:o} was set");
            let loaded = load(&path);
            file::save(&path, &Rope::from("new"), loaded.format().expect("valid")).expect("save");
            assert_eq!(mode(&path), after, "{before:o}");
            assert_eq!(read(&path), b"new");
        }
        assert!(!has_temp(&dir));
    }

    #[test]
    fn a_link_is_saved_through() {
        let dir = scratch_dir("link");
        let target = dir.join("target.txt");
        let link = dir.join("link.txt");
        write(&target, b"old\n");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).expect("chmod");
        symlink("target.txt", &link).expect("make a link");

        let loaded = load(&link);
        assert_eq!(*loaded.text(), "old\n");
        file::save(&link, &Rope::from("new\n"), Format::UTF8).expect("save");

        assert_eq!(read(&target), b"new\n");
        assert!(
            fs::symlink_metadata(&link)
                .expect("lstat")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_link(&link).expect("readlink"),
            Path::new("target.txt")
        );
        assert_eq!(mode(&target), 0o640);
        assert_eq!(names(&dir), ["link.txt", "target.txt"]);
    }

    #[test]
    fn a_relative_link_in_another_directory_resolves_from_its_own() {
        let dir = scratch_dir("relative-link");
        let links = dir.join("links");
        let files = dir.join("files");
        fs::create_dir_all(links.join("deeper")).expect("mkdir");
        fs::create_dir_all(&files).expect("mkdir");
        let target = files.join("real.txt");
        write(&target, b"old");
        // links/deeper/link -> ../hop -> ../files/real.txt
        symlink("../files/real.txt", links.join("hop")).expect("link");
        symlink("../hop", links.join("deeper").join("link")).expect("link");

        let path = links.join("deeper").join("link");
        file::save(&path, &Rope::from("new"), Format::UTF8).expect("save");

        assert_eq!(read(&target), b"new");
        assert_eq!(fs::read_link(&path).expect("readlink"), Path::new("../hop"));
        assert_eq!(names(&files), ["real.txt"]);
        assert_eq!(names(&links), ["deeper", "hop"]);
        assert_eq!(names(&links.join("deeper")), ["link"]);
    }

    #[test]
    fn an_absolute_link_is_followed() {
        let dir = scratch_dir("absolute-link");
        let target = dir.join("real.txt");
        write(&target, b"old");
        let link = dir.join("link");
        symlink(&target, &link).expect("link");
        file::save(&link, &Rope::from("new"), Format::UTF8).expect("save");
        assert_eq!(read(&target), b"new");
        assert!(
            fs::symlink_metadata(&link)
                .expect("lstat")
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_dangling_link_creates_its_target() {
        let dir = scratch_dir("dangling");
        let link = dir.join("link.txt");
        symlink("made-by-save.txt", &link).expect("link");
        file::save(&link, &Rope::from("hello"), Format::UTF8).expect("save");
        assert_eq!(read(&dir.join("made-by-save.txt")), b"hello");
        assert!(
            fs::symlink_metadata(&link)
                .expect("lstat")
                .file_type()
                .is_symlink()
        );
        assert_eq!(names(&dir), ["link.txt", "made-by-save.txt"]);
    }

    #[test]
    fn a_link_loop_is_an_error_and_changes_nothing() {
        let dir = scratch_dir("loop");
        let a = dir.join("a");
        let b = dir.join("b");
        symlink("b", &a).expect("link");
        symlink("a", &b).expect("link");
        let error = file::save(&a, &Rope::from("x"), Format::UTF8).expect_err("a loop");
        assert!(
            matches!(error, TextError::TooManyLinks { max: 40 }),
            "{error:?}"
        );
        assert_eq!(fs::read_link(&a).expect("readlink"), Path::new("b"));
        assert_eq!(fs::read_link(&b).expect("readlink"), Path::new("a"));
        assert_eq!(names(&dir), ["a", "b"]);
    }

    #[test]
    fn a_save_follows_forty_links_and_no_more() {
        let dir = scratch_dir("forty");
        write(&dir.join("file.txt"), b"old");
        // link-1 -> file.txt, link-k -> link-(k-1).
        symlink("file.txt", dir.join("link-1")).expect("link");
        for k in 2..=41 {
            symlink(format!("link-{}", k - 1), dir.join(format!("link-{k}"))).expect("link");
        }
        file::save(dir.join("link-40"), &Rope::from("new"), Format::UTF8).expect("40 links");
        assert_eq!(read(&dir.join("file.txt")), b"new");
        let error =
            file::save(dir.join("link-41"), &Rope::from("x"), Format::UTF8).expect_err("41");
        assert!(
            matches!(error, TextError::TooManyLinks { max: 40 }),
            "{error:?}"
        );
        assert_eq!(read(&dir.join("file.txt")), b"new");
        assert!(!has_temp(&dir));
    }

    #[test]
    fn a_link_to_a_directory_is_refused() {
        let dir = scratch_dir("link-to-dir");
        fs::create_dir(dir.join("sub")).expect("mkdir");
        symlink("sub", dir.join("link")).expect("link");
        let error =
            file::save(dir.join("link"), &Rope::from("x"), Format::UTF8).expect_err("a directory");
        assert!(
            matches!(error, TextError::NotRegularFile { directory: true }),
            "{error:?}"
        );
        assert!(names(&dir.join("sub")).is_empty());
    }

    #[test]
    fn a_new_file_gets_the_default_mode() {
        let dir = scratch_dir("default-mode");
        let path = dir.join("new.txt");
        file::save(&path, &Rope::from("x"), Format::UTF8).expect("save");
        // What `std` gives any new file in this directory.
        let reference = dir.join("reference.txt");
        write(&reference, b"x");
        assert_eq!(mode(&path), mode(&reference));
    }
}
