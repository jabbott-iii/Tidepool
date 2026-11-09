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

//! Tests of `core::path::canonicalize`, `resolve` and `under` on real
//! files, through the public API only.
//!
//! New in Tidepool. Each test works in a directory of its own under
//! `target/tmp`, and builds what it expects from that directory's
//! canonical path (its anchor), so the same assertions hold on every OS.
//! There is no `cfg` here (ADR-0033): where an OS differs, the test lets
//! the OS decide by making every candidate and asking `canonicalize`.
//!
//! A `resolve` plan is checked the way callers rely on it: planned before
//! the file exists, then made, then compared with `canonicalize`.
//!
//! Links are made with `fs::soft_link`, which needs no `cfg` and makes a
//! file link on Windows. A test that needs a link it cannot make, or a
//! link to a directory it cannot follow, says `SKIPPED:` and returns.

use std::cell::Cell;
use std::fs;
use std::io::{self, Write as _};
use std::ops::Deref;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use tidepool_core::path::{self, PathError};
use tidepool_core::testing::check;

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

/// Returns a fresh, empty directory for one test, under `target/tmp`, and
/// its canonical path, which is checked to be in the plain form. The
/// process id keeps concurrent runs of this binary out of each other's
/// way.
fn scratch_dir(test: &str) -> (Scratch, PathBuf) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("tidepool-core-path")
        .join(format!("{test}-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("remove the previous run's directory");
    }
    fs::create_dir_all(&dir).expect("create the test directory");
    let anchor = path::canonicalize(&dir).expect("canonicalize the test directory");
    assert!(!is_verbatim(&anchor), "{}", path::display(&anchor));
    (Scratch(dir), anchor)
}

/// Whether `path` starts with one of Windows' `\\?\` prefixes.
fn is_verbatim(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix)) if prefix.kind().is_verbatim()
    )
}

/// `relative` under `base`, one name at a time: a Windows link target is
/// kept as written, so it must use the OS's own separator.
fn under_base(base: &Path, relative: &str) -> PathBuf {
    let mut path = base.to_path_buf();
    path.extend(relative.split('/'));
    path
}

/// `relative` as a path of its own, with the OS's separator.
fn rel(relative: &str) -> PathBuf {
    relative.split('/').collect()
}

/// Makes a symbolic link at `at` to `target`, or says why it cannot.
fn link(target: &Path, at: &Path) -> Result<(), String> {
    // `fs::soft_link` is deprecated in favour of one function per OS, but
    // those would need a platform `cfg`, which `core` must not have
    // (`AGENTS.md` rule 5); this one works wherever links do. On Windows it
    // makes a file link even to a directory, which is still a link.
    #[allow(deprecated)]
    fs::soft_link(target, at).map_err(|error| error.to_string())
}

/// Says on standard error, as the gate counts it, that `test` skipped.
fn skipped(test: &str, reason: &str) {
    let _ = writeln!(io::stderr(), "SKIPPED: {test}: {reason}");
}

/// Writes `contents` to `file`, making its directories.
fn make_file(file: &Path, contents: &str) {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).expect("make the file's directories");
    }
    fs::write(file, contents).expect("write the file");
}

/// The source of `error` if it is an [`PathError::Io`] for `given`.
fn io_source<'a>(error: &'a PathError, given: &Path) -> &'a io::Error {
    match error {
        PathError::Io { path, source } => {
            assert_eq!(path, given, "the error names the path as given");
            source
        }
        other => panic!(
            "expected an I/O error for {}, got {other}",
            path::display(given)
        ),
    }
}

#[test]
fn an_existing_path_canonicalizes_to_one_plain_absolute_spelling() {
    let (dir, anchor) = scratch_dir("existing");
    make_file(&under_base(&dir, "a/b/f.txt"), "f");
    let given = under_base(&dir, "a/./b/../b/f.txt");
    let canonical = path::canonicalize(&given).unwrap();
    assert_eq!(canonical, under_base(&anchor, "a/b/f.txt"));
    assert!(canonical.is_absolute() && !is_verbatim(&canonical));
    assert_eq!(path::canonicalize(&canonical).unwrap(), canonical);
    assert_eq!(path::resolve(&given).unwrap(), canonical);
    assert_eq!(path::resolve(&canonical).unwrap(), canonical);
    assert_eq!(
        path::under(&canonical, &anchor),
        Some(rel("a/b/f.txt").as_path())
    );
    // A directory too.
    assert_eq!(path::canonicalize(dir.join("a")).unwrap(), anchor.join("a"));
}

#[test]
fn a_relative_path_is_taken_from_the_current_directory() {
    // `cargo test` runs a test binary in its package's directory. This
    // test never changes the current directory, which the others share.
    let package = path::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap();
    assert_eq!(path::canonicalize(".").unwrap(), package);
    assert_eq!(
        path::canonicalize("Cargo.toml").unwrap(),
        package.join("Cargo.toml")
    );
    let missing = format!("no-such-{}.txt", std::process::id());
    assert_eq!(
        path::resolve(rel(&format!("src/../{missing}"))).unwrap(),
        package.join(&missing)
    );
    assert_eq!(path::resolve(&missing).unwrap(), package.join(&missing));
}

#[test]
fn a_missing_path_is_an_error_that_names_it() {
    let (dir, _anchor) = scratch_dir("missing");
    let given = under_base(&dir, "nope/missing.txt");
    let error = path::canonicalize(&given).unwrap_err();
    let source = io_source(&error, &given);
    assert_eq!(source.kind(), io::ErrorKind::NotFound);
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(
        error.to_string(),
        format!("cannot resolve `{}`: {source}", path::display(&given))
    );
    for error in [
        path::canonicalize("").unwrap_err(),
        path::resolve("").unwrap_err(),
    ] {
        assert!(matches!(error, PathError::Empty), "{error}");
        assert_eq!(error.to_string(), "the path is empty");
    }
}

#[test]
fn a_name_under_a_file_is_not_a_directory() {
    let (dir, _anchor) = scratch_dir("under-file");
    make_file(&dir.join("file.txt"), "f");
    for relative in ["file.txt/x", "file.txt/x/y.txt"] {
        let given = under_base(&dir, relative);
        let error = path::resolve(&given).unwrap_err();
        assert_eq!(
            io_source(&error, &given).kind(),
            io::ErrorKind::NotADirectory,
            "{error}"
        );
        assert!(path::canonicalize(&given).is_err());
    }
}

#[test]
fn a_missing_tail_resolves_where_making_it_puts_it() {
    let (dir, anchor) = scratch_dir("missing-tail");
    fs::create_dir_all(under_base(&dir, "real/sub")).unwrap();
    fs::create_dir_all(dir.join("nest")).unwrap();
    // The input, the directories that making it needs, the file it names,
    // and where that file is, all relative to the test directory.
    let cases: [(&str, &[&str], &str, &str); 6] = [
        ("new.txt", &[], "new.txt", "new.txt"),
        ("n/new", &["n"], "n/new", "n/new"),
        ("n2/../new2", &["n2"], "new2", "new2"),
        ("n3/./a/../new3", &["n3/a"], "n3/new3", "n3/new3"),
        ("real/sub/../../new7", &[], "new7", "new7"),
        // Out of a directory that exists, through one that does not.
        (
            "nest/m8/../../new8.txt",
            &["nest/m8"],
            "new8.txt",
            "new8.txt",
        ),
    ];
    for (input, directories, file, at) in cases {
        let given = under_base(&dir, input);
        let planned = path::resolve(&given).unwrap();
        assert_eq!(planned, under_base(&anchor, at), "{input}");
        assert!(
            path::canonicalize(&given).is_err(),
            "{input} does not exist yet"
        );
        for directory in directories {
            fs::create_dir_all(under_base(&dir, directory)).unwrap();
        }
        make_file(&under_base(&dir, file), input);
        assert_eq!(path::canonicalize(&given).unwrap(), planned, "{input}");
        assert_eq!(path::resolve(&given).unwrap(), planned, "{input}");
    }
}

#[test]
fn a_resolved_path_is_what_canonicalize_gives_once_its_directories_exist() {
    let (dir, _anchor) = scratch_dir("directories");
    let case = Cell::new(0u32);
    check(
        "a resolved path is what canonicalize gives once its directories exist",
        100,
        |rng| {
            // Names, `.` and `..`, never above the case's directory.
            let mut parts = Vec::new();
            let mut depth = 0u32;
            for _ in 0..rng.range(0..8u32) {
                match rng.range(0..4u32) {
                    0 => parts.push("x"),
                    1 => parts.push("y"),
                    2 => parts.push("."),
                    _ if depth > 0 => parts.push(".."),
                    _ => parts.push("x"),
                }
                depth = match parts.last() {
                    Some(&"..") => depth - 1,
                    Some(&".") => depth,
                    _ => depth + 1,
                };
            }
            parts
        },
        |parts| {
            let base = dir.join(format!("case-{}", case.get()));
            case.set(case.get() + 1);
            fs::create_dir(&base).map_err(|error| error.to_string())?;
            let mut given = base.clone();
            given.extend(parts);
            let planned = path::resolve(&given).map_err(|error| error.to_string())?;
            // Each name in turn, through the `.` and `..` before it:
            // `create_dir_all` cannot make a path that ends in `.`.
            let mut making = base;
            for part in parts {
                making.push(part);
                if !matches!(*part, "." | "..") {
                    match fs::create_dir(&making) {
                        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => {
                            return Err(format!("make {}: {error}", path::display(&making)));
                        }
                        _ => {}
                    }
                }
            }
            let made = path::canonicalize(&given).map_err(|error| error.to_string())?;
            if planned == made {
                Ok(())
            } else {
                Err(format!(
                    "planned {}, made {}",
                    path::display(&planned),
                    path::display(&made)
                ))
            }
        },
    );
}

#[test]
fn links_to_files_are_followed() {
    let test = "links_to_files_are_followed";
    let (dir, anchor) = scratch_dir("file-links");
    make_file(&dir.join("file.txt"), "f");
    fs::create_dir(dir.join("sub")).unwrap();
    let links = [
        (PathBuf::from("file.txt"), dir.join("relative")),
        (anchor.join("file.txt"), dir.join("absolute")),
        (rel("../file.txt"), under_base(&dir, "sub/up")),
    ];
    for (target, at) in &links {
        if let Err(reason) = link(target, at) {
            return skipped(test, &format!("cannot make a symbolic link here: {reason}"));
        }
    }
    for (_, at) in &links {
        let canonical = path::canonicalize(at).unwrap();
        assert_eq!(canonical, anchor.join("file.txt"), "{}", path::display(at));
        assert_eq!(path::resolve(at).unwrap(), canonical);
        assert_eq!(fs::read_to_string(&canonical).unwrap(), "f");
    }
}

#[test]
fn dangling_links_lead_where_saving_would_create_the_file() {
    let test = "dangling_links_lead_where_saving_would_create_the_file";
    let (dir, anchor) = scratch_dir("dangling");
    fs::create_dir(dir.join("sub")).unwrap();
    // The link, its target, and the file the chain ends at.
    let links = [
        ("dangling", PathBuf::from("nowhere.txt"), "nowhere.txt"),
        ("hop", PathBuf::from("dangling"), "nowhere.txt"),
        ("sub/up", rel("../up.txt"), "up.txt"),
    ];
    for (at, target, _) in &links {
        if let Err(reason) = link(target, &under_base(&dir, at)) {
            return skipped(test, &format!("cannot make a symbolic link here: {reason}"));
        }
    }
    let planned: Vec<PathBuf> = links
        .iter()
        .map(|(at, _, end)| {
            let given = under_base(&dir, at);
            let error = path::canonicalize(&given).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
            let planned = path::resolve(&given).unwrap();
            assert_eq!(planned, under_base(&anchor, end), "{at}");
            planned
        })
        .collect();
    for (_, _, end) in &links {
        make_file(&under_base(&dir, end), end);
    }
    for ((at, _, _), planned) in links.iter().zip(&planned) {
        assert_eq!(
            &path::canonicalize(under_base(&dir, at)).unwrap(),
            planned,
            "{at}"
        );
    }
}

#[test]
fn links_to_directories_and_dotdot_after_them() {
    let test = "links_to_directories_and_dotdot_after_them";
    let (dir, anchor) = scratch_dir("directory-links");
    fs::create_dir_all(under_base(&dir, "real/sub")).unwrap();
    if let Err(reason) = link(&rel("real/sub"), &dir.join("dirlink"))
        .and_then(|()| link(Path::new("ddir"), &dir.join("dangling-dir")))
    {
        return skipped(test, &format!("cannot make a symbolic link here: {reason}"));
    }

    // `..` after a link: physical on Unix (the parent of the target),
    // textual on Windows (the parent of the link). Making both candidates
    // lets `canonicalize` say which this OS means.
    let dotdot = [
        ("dirlink/../new5.txt", &[][..], "new5.txt"),
        (
            "m11/m12/../../dirlink/../new11.txt",
            &["m11/m12"][..],
            "new11.txt",
        ),
    ];
    for (input, directories, name) in dotdot {
        let given = under_base(&dir, input);
        let planned = path::resolve(&given).unwrap();
        let candidates = [
            under_base(&anchor, &format!("real/{name}")),
            anchor.join(name),
        ];
        assert!(
            candidates.contains(&planned),
            "{input}: {}",
            path::display(&planned)
        );
        for directory in directories {
            fs::create_dir_all(under_base(&dir, directory)).unwrap();
        }
        make_file(&under_base(&dir, &format!("real/{name}")), name);
        make_file(&dir.join(name), name);
        assert_eq!(path::canonicalize(&given).unwrap(), planned, "{input}");
    }

    // Through a link to a directory, which Windows cannot follow when the
    // link is a file link.
    if let Err(error) = fs::read_dir(dir.join("dirlink")) {
        return skipped(
            test,
            &format!("the cases through a link to a directory: it cannot be listed here: {error}"),
        );
    }
    let through = [
        ("dirlink/new4.txt", "real/sub/new4.txt"),
        ("dangling-dir/x/new6.txt", "ddir/x/new6.txt"),
    ];
    for (input, at) in through {
        let given = under_base(&dir, input);
        let planned = path::resolve(&given).unwrap();
        assert_eq!(planned, under_base(&anchor, at), "{input}");
        make_file(&under_base(&dir, at), input);
        assert_eq!(path::canonicalize(&given).unwrap(), planned, "{input}");
    }
}

#[test]
fn link_loops_and_long_chains_are_errors() {
    let test = "link_loops_and_long_chains_are_errors";
    let (dir, _anchor) = scratch_dir("loops");
    let mut links = vec![
        (PathBuf::from("b"), dir.join("a")),
        (PathBuf::from("a"), dir.join("b")),
        (PathBuf::from("end-nothing"), dir.join("c0")),
    ];
    // A chain of 41 links that leads nowhere: one more than `resolve`
    // follows, and than Linux's limit; macOS stops at 32.
    links.extend((1..41).map(|at| {
        (
            PathBuf::from(format!("c{}", at - 1)),
            dir.join(format!("c{at}")),
        )
    }));
    for (target, at) in &links {
        if let Err(reason) = link(target, at) {
            return skipped(test, &format!("cannot make a symbolic link here: {reason}"));
        }
    }
    // A loop of links that exist: the OS sees it, so it is the OS's loop
    // error, never `resolve`'s own limit.
    for relative in ["a", "a/x"] {
        let given = under_base(&dir, relative);
        let error = path::resolve(&given).unwrap_err();
        let source = io_source(&error, &given);
        assert_ne!(
            source.kind(),
            io::ErrorKind::NotFound,
            "{relative}: {error}"
        );
        assert!(path::canonicalize(&given).is_err(), "{relative}");
    }
    // The chain: the OS's loop error where the OS stops first (Linux after
    // 40 links, macOS after 32), or `TooManyLinks` where it follows more
    // (Windows, 63) and `resolve` follows the rest itself.
    for relative in ["c40", "c40/x.txt"] {
        let given = under_base(&dir, relative);
        let error = path::resolve(&given).unwrap_err();
        assert_ne!(error.kind(), io::ErrorKind::NotFound, "{relative}: {error}");
        match &error {
            PathError::Io { path, .. } | PathError::TooManyLinks { path } => {
                assert_eq!(path, &given);
            }
            other => panic!("{relative}: {other}"),
        }
        assert!(path::canonicalize(&given).is_err(), "{relative}");
    }
}

#[test]
fn forty_links_resolve_and_the_forty_first_is_too_many() {
    let test = "forty_links_resolve_and_the_forty_first_is_too_many";
    let (dir, anchor) = scratch_dir("forty-links");
    // `d0 -> end.txt` and `dK -> m/../d(K-1)`, with `m` missing: the OS
    // stops at `m` after one link, so `resolve` follows the chain itself.
    let mut links = vec![(PathBuf::from("end.txt"), dir.join("d0"))];
    links.extend((1..=40).map(|at| {
        (
            rel(&format!("m/../d{}", at - 1)),
            dir.join(format!("d{at}")),
        )
    }));
    // A loop of two such links.
    links.push((rel("m/../b"), dir.join("a")));
    links.push((rel("m/../a"), dir.join("b")));
    for (target, at) in &links {
        if let Err(reason) = link(target, at) {
            return skipped(test, &format!("cannot make a symbolic link here: {reason}"));
        }
    }
    // `d39` is 40 links, the most `resolve` follows.
    assert_eq!(
        path::resolve(dir.join("d39")).unwrap(),
        anchor.join("end.txt")
    );
    let given = dir.join("d40");
    match path::resolve(&given) {
        Err(PathError::TooManyLinks { path }) => assert_eq!(path, given),
        other => panic!("41 links: {other:?}"),
    }
    let given = dir.join("a");
    match path::resolve(&given) {
        Err(PathError::TooManyLinks { path }) => assert_eq!(path, given),
        // Where the OS applies `..` by its text (Windows), it sees the loop
        // itself.
        Err(PathError::Io { path, source })
            if windows_paths() && source.kind() != io::ErrorKind::NotFound =>
        {
            assert_eq!(path, given);
        }
        other => panic!("a loop: {other:?}"),
    }
}

#[test]
fn names_win32_would_rewrite_keep_the_verbatim_form_and_open_the_same_file() {
    let test = "names_win32_would_rewrite_keep_the_verbatim_form_and_open_the_same_file";
    let (dir, anchor) = scratch_dir("rewritten-names");
    // On Windows this is `\\?\C:\…`, through which a name is made as it is
    // spelled; elsewhere it is the anchor.
    let exact = fs::canonicalize(&*dir).unwrap();
    let mut refused = Vec::new();
    for name in ["nul.txt", "trailing.", "space "] {
        let file = exact.join(name);
        if let Err(error) = fs::write(&file, name) {
            refused.push(format!("`{name}`: {error}"));
            continue;
        }
        let canonical = path::canonicalize(&file).unwrap();
        assert_eq!(fs::read_to_string(&canonical).unwrap(), name, "{name}");
        assert_eq!(path::resolve(&file).unwrap(), canonical, "{name}");
        assert_eq!(
            path::under(&canonical, &anchor),
            Some(Path::new(name)),
            "{name}"
        );
        assert_eq!(path::canonicalize(&canonical).unwrap(), canonical, "{name}");
    }
    if !refused.is_empty() {
        skipped(
            test,
            &format!("the file system refuses {}", refused.join(", ")),
        );
    }
}

#[test]
fn a_long_path_canonicalizes_plain_and_still_opens() {
    let (dir, anchor) = scratch_dir("long");
    let names: Vec<String> = (0..6).map(|at| format!("{at}{}", "d".repeat(49))).collect();
    let relative: PathBuf = names.iter().collect();
    let file = dir.join(&relative).join("f.txt");
    make_file(&file, "deep");
    let canonical = path::canonicalize(&file).unwrap();
    assert!(canonical.as_os_str().len() > 300);
    assert_eq!(canonical, anchor.join(&relative).join("f.txt"));
    assert!(!is_verbatim(&canonical), "{}", path::display(&canonical));
    assert_eq!(fs::read_to_string(&canonical).unwrap(), "deep");
    assert_eq!(path::canonicalize(&canonical).unwrap(), canonical);
    // And a missing name below it.
    assert_eq!(
        path::resolve(dir.join(&relative).join("new.txt")).unwrap(),
        anchor.join(&relative).join("new.txt")
    );
}

#[test]
fn case_is_what_the_file_system_stores() {
    let (dir, anchor) = scratch_dir("case");
    make_file(&dir.join("Mixed.txt"), "m");
    let typed = dir.join("mIXED.TXT");
    if fs::metadata(&typed).is_ok() {
        // A file system that folds case: the name as stored on macOS and
        // Windows, and as typed on Linux, whose `realpath` keeps the bytes
        // it is given in a directory that folds case. Either opens it.
        let canonical = path::canonicalize(&typed).unwrap();
        let spellings = [anchor.join("Mixed.txt"), anchor.join("mIXED.TXT")];
        assert!(
            spellings.contains(&canonical),
            "{}",
            path::display(&canonical)
        );
        assert_eq!(fs::read_to_string(&canonical).unwrap(), "m");
        assert_eq!(path::resolve(&typed).unwrap(), canonical);
        // A missing name below a directory typed in another case: the walk
        // gives the directory as the OS's canonicalization does.
        fs::create_dir(dir.join("Folder")).unwrap();
        let typed_folder = dir.join("fOLDER");
        assert_eq!(
            path::resolve(typed_folder.join("new.txt")).unwrap(),
            path::canonicalize(&typed_folder).unwrap().join("new.txt")
        );
    } else {
        // One that does not: another file, which does not exist yet.
        assert_eq!(path::resolve(&typed).unwrap(), anchor.join("mIXED.TXT"));
        assert!(path::canonicalize(&typed).is_err());
    }
    assert_eq!(
        path::canonicalize(dir.join("Mixed.txt")).unwrap(),
        anchor.join("Mixed.txt")
    );
}

#[test]
fn a_final_separator_asks_for_a_directory() {
    let test = "a_final_separator_asks_for_a_directory";
    let (dir, anchor) = scratch_dir("final-separator");
    make_file(&dir.join("file.txt"), "f");
    fs::create_dir(dir.join("dir")).unwrap();
    // `/` is a separator on every OS.
    let with = |name: &str, suffix: &str| {
        let mut text = dir.join(name).into_os_string();
        text.push(suffix);
        PathBuf::from(text)
    };
    // A file with a final separator is not a directory, as the OS says.
    let given = with("file.txt", "/");
    let error = path::resolve(&given).unwrap_err();
    assert_eq!(
        io_source(&error, &given).kind(),
        io::ErrorKind::NotADirectory
    );
    assert!(path::canonicalize(&given).is_err());
    // POSIX reads a final `/.` the same way; Win32 drops it, and the OS
    // decides.
    let given = with("file.txt", "/.");
    match path::canonicalize(&given) {
        Err(_) => {
            let error = path::resolve(&given).unwrap_err();
            assert_eq!(
                io_source(&error, &given).kind(),
                io::ErrorKind::NotADirectory
            );
        }
        Ok(canonical) => assert_eq!(path::resolve(&given).unwrap(), canonical),
    }
    if !windows_paths() {
        assert!(
            path::canonicalize(&given).is_err(),
            "POSIX: `file/.` is not a directory"
        );
    } else {
        // Win32 trims a final space or dot and keeps the separator before
        // it, which then asks for a directory too.
        for suffix in [r"\ ", r"\...", r"\ ."] {
            let given = with("file.txt", suffix);
            let error = path::resolve(&given).unwrap_err();
            let kind = io_source(&error, &given).kind();
            assert_eq!(kind, io::ErrorKind::NotADirectory, "{suffix:?}");
            assert!(path::canonicalize(&given).is_err(), "{suffix:?}");
        }
    }
    // A directory with either is itself.
    for suffix in ["/", "/."] {
        let given = with("dir", suffix);
        assert_eq!(path::canonicalize(&given).unwrap(), anchor.join("dir"));
        assert_eq!(path::resolve(&given).unwrap(), anchor.join("dir"));
    }
    // A missing last name with a final separator is planned as a directory.
    let given = with("new", "/");
    let planned = path::resolve(&given).unwrap();
    assert_eq!(planned, anchor.join("new"));
    fs::create_dir(dir.join("new")).unwrap();
    assert_eq!(path::canonicalize(&given).unwrap(), planned);
    // The target of a link that is the last name asks the same way.
    let mut target = rel("y/../file.txt").into_os_string();
    target.push(std::path::MAIN_SEPARATOR_STR);
    if let Err(reason) = link(Path::new(&target), &dir.join("a")) {
        return skipped(test, &format!("cannot make a symbolic link here: {reason}"));
    }
    let given = dir.join("a");
    let error = path::resolve(&given).unwrap_err();
    let kind = io_source(&error, &given).kind();
    if windows_paths() {
        // Windows may refuse the target's final `\` itself; then `resolve`
        // gives the OS's error, as `canonicalize` does.
        let refused = path::canonicalize(&given).is_err_and(|refused| refused.kind() == kind);
        assert!(kind == io::ErrorKind::NotADirectory || refused, "{error}");
    } else {
        assert_eq!(kind, io::ErrorKind::NotADirectory);
    }
}

/// Guards against `resolve` taking time that grows faster than its input.
/// It once rescanned the whole path after each `..` that cancelled a
/// missing name: 800 such pairs took 5 s in a release build, and 2,000
/// would take minutes in a debug one. The walk takes milliseconds.
#[test]
fn missing_names_and_dotdot_take_time_in_proportion() {
    let test = "missing_names_and_dotdot_take_time_in_proportion";
    let (dir, anchor) = scratch_dir("linear");
    let pairs = 2_000;
    let mut alternating = dir.as_os_str().to_owned();
    alternating.push("/m/..".repeat(pairs));
    alternating.push("/x");
    let mut nested = dir.as_os_str().to_owned();
    nested.push("/m".repeat(pairs));
    nested.push("/..".repeat(pairs));
    nested.push("/x");
    // Through a name that exists, which the OS does not stop at.
    fs::create_dir(dir.join("a")).unwrap();
    let mut existing = dir.as_os_str().to_owned();
    existing.push("/a/..".repeat(pairs));
    existing.push("/x");
    for given in [
        PathBuf::from(alternating),
        PathBuf::from(nested),
        PathBuf::from(existing),
    ] {
        let started = Instant::now();
        let planned = path::resolve(&given);
        let took = started.elapsed();
        assert!(
            took < Duration::from_secs(10),
            "{pairs} pairs took {took:?}"
        );
        match planned {
            Ok(planned) => assert_eq!(planned, anchor.join("x")),
            // macOS's `realpath` refuses a path over 1,024 bytes, and then
            // so does `resolve`, which asks it first.
            Err(error)
                if path::canonicalize(&given)
                    .is_err_and(|refused| refused.kind() == error.kind())
                    && error.kind() != io::ErrorKind::NotFound =>
            {
                return skipped(
                    test,
                    &format!(
                        "the OS refuses a path of {} bytes: {error}",
                        given.as_os_str().len()
                    ),
                );
            }
            Err(error) => panic!("{error}"),
        }
    }
}

/// Guards the same through links: each of 40 links leads to the next
/// through 150 missing names and `..`, as a repository could check them in.
/// That took 5 s in a release build when every `..` started over.
#[test]
fn links_through_missing_names_take_time_in_proportion() {
    let test = "links_through_missing_names_take_time_in_proportion";
    let (dir, anchor) = scratch_dir("linear-links");
    for at in 0..40 {
        let mut target = "m/../".repeat(150);
        if at + 1 < 40 {
            target.push_str(&format!("l{}", at + 1));
        } else {
            target.push_str("end.txt");
        }
        if let Err(reason) = link(&rel(&target), &dir.join(format!("l{at}"))) {
            return skipped(test, &format!("cannot make a symbolic link here: {reason}"));
        }
    }
    let started = Instant::now();
    let planned = path::resolve(dir.join("l0")).unwrap();
    let took = started.elapsed();
    assert!(took < Duration::from_secs(10), "40 links took {took:?}");
    assert_eq!(planned, anchor.join("end.txt"));
}

/// Whether `std` parses Windows' prefixes here: asked at run time, not
/// through `cfg` (ADR-0033).
fn windows_paths() -> bool {
    matches!(
        Path::new(r"C:\").components().next(),
        Some(Component::Prefix(_))
    )
}

/// `base` with `name` appended by text, the separator of this OS between:
/// `join` would read a name such as `c:x` as a drive on Windows.
fn appended(base: &Path, name: &str) -> PathBuf {
    let mut text = base.as_os_str().to_owned();
    text.push(std::path::MAIN_SEPARATOR_STR);
    text.push(name);
    PathBuf::from(text)
}

#[test]
fn a_windows_name_resolve_will_not_guess_at_is_invalid_input() {
    let (dir, anchor) = scratch_dir("not-guessed");
    // On Windows this is `\\?\C:\…`; elsewhere it is the anchor.
    let exact = fs::canonicalize(&*dir).unwrap();
    for base in [&*dir, exact.as_path()] {
        let given = appended(base, "c:x");
        let resolved = path::resolve(&given);
        if windows_paths() {
            // A stream `x` of a file `c`, which Windows would read as the
            // drive `C:` and the path `x`.
            let error = resolved.unwrap_err();
            assert_eq!(
                io_source(&error, &given).kind(),
                io::ErrorKind::InvalidInput
            );
        } else {
            // An ordinary name elsewhere.
            assert_eq!(resolved.unwrap(), appended(&anchor, "c:x"));
        }
    }
    if windows_paths() {
        // A `.` that only a verbatim path keeps.
        let given = appended(&appended(&exact, "missing"), r".\x");
        let error = path::resolve(&given).unwrap_err();
        assert_eq!(
            io_source(&error, &given).kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn a_name_win32_would_trim_inside_a_path_is_probed_as_it_is() {
    let (dir, anchor) = scratch_dir("trimmed-inside");
    // Made through `\\?\` on Windows, so that the space stays; and beside
    // it the name Win32 would read it as.
    let exact = fs::canonicalize(&*dir).unwrap();
    fs::create_dir(exact.join("space ")).unwrap();
    fs::create_dir(exact.join("space")).unwrap();
    fs::write(exact.join("space ").join("a.txt"), "a").unwrap();
    let directory = path::canonicalize(exact.join("space ")).unwrap();
    let child = path::canonicalize(exact.join("space ").join("a.txt")).unwrap();
    assert_eq!(fs::read_to_string(&child).unwrap(), "a");
    if windows_paths() {
        // Win32 trims a trailing space only from a path's last name (it
        // drops one final dot from every name): the directory keeps `\\?\`,
        // the file below it is plain, so forms alternate down a tree.
        assert!(is_verbatim(&directory), "{}", path::display(&directory));
        assert!(!is_verbatim(&child), "{}", path::display(&child));
    }
    assert_eq!(
        path::under(&child, &anchor),
        Some(rel("space /a.txt").as_path())
    );
    // The sibling of a plain child resolves in the directory with the
    // space, where writing it puts it, not in `space`.
    let sibling = child.with_file_name("b.txt");
    let planned = path::resolve(&sibling).unwrap();
    assert_eq!(
        path::under(&planned, &anchor),
        Some(rel("space /b.txt").as_path())
    );
    if windows_paths() {
        // Spelled as a device path, which Win32 normalizes as well.
        let mut device = std::ffi::OsString::from(r"\\.\");
        device.push(sibling.as_os_str());
        assert_eq!(path::resolve(PathBuf::from(device)).unwrap(), planned);
    }
    fs::write(exact.join("space ").join("b.txt"), "b").unwrap();
    assert_eq!(
        path::canonicalize(exact.join("space ").join("b.txt")).unwrap(),
        planned
    );
    assert_eq!(path::resolve(&sibling).unwrap(), planned);
}

/// Guards against `resolve` repeating the OS's work for each link with an
/// absolute target. It once had the OS canonicalize each such target
/// whole, which follows the rest of the chain again: 40 chained links into
/// a directory 300 deep took 30 times as long as `canonicalize` of the made
/// path. Walking each target from its root takes about as long as the OS.
#[test]
fn absolute_links_into_a_deep_tree_cost_about_what_canonicalize_costs() {
    let test = "absolute_links_into_a_deep_tree_cost_about_what_canonicalize_costs";
    let (dir, _anchor) = scratch_dir("absolute-links");
    // Deep enough to show the cost, and short enough for macOS, whose
    // `realpath` refuses a path over 1,024 bytes.
    let mut deep = dir.to_path_buf();
    deep.extend(std::iter::repeat_n("a", 300));
    fs::create_dir_all(&deep).unwrap();
    // `l0` beside the tree and `l1`..`l39` at its bottom, each an absolute
    // link to the next; the last leads to `missing`, which does not exist.
    for at in 0..40 {
        let place = if at == 0 {
            dir.join("l0")
        } else {
            deep.join(format!("l{at}"))
        };
        let target = if at + 1 < 40 {
            deep.join(format!("l{}", at + 1))
        } else {
            deep.join("missing")
        };
        if let Err(reason) = link(&target, &place) {
            return skipped(test, &format!("cannot make a symbolic link here: {reason}"));
        }
    }
    let given = dir.join("l0");
    let started = Instant::now();
    let planned = path::resolve(&given).unwrap();
    let resolving = started.elapsed();
    assert_eq!(planned, path::canonicalize(&deep).unwrap().join("missing"));
    make_file(&deep.join("missing"), "m");
    let started = Instant::now();
    let made = path::canonicalize(&given).unwrap();
    let canonicalizing = started.elapsed();
    assert_eq!(made, planned);
    assert!(
        resolving < canonicalizing * 10 + Duration::from_millis(500),
        "resolve took {resolving:?}, canonicalize {canonicalizing:?}"
    );
}
