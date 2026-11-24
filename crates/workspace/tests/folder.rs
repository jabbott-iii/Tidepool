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

//! `workspace::folder`: opening a folder, and its ignore files read and
//! matched as Git reads them (M4.7, ADR-0067).

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use tidepool_workspace::WorkspaceError;
use tidepool_workspace::folder::{Folder, Ignore, IgnoreFile, IgnoreLimits, Match};

/// A fresh, empty folder for one test, removed when it is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("tidepool-folder-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("make the scratch folder");
        Scratch(dir)
    }

    /// Writes `text` at `path` under the folder, making its folders.
    fn write(&self, path: &str, text: &str) {
        let file = self.0.join(path);
        fs::create_dir_all(file.parent().expect("a file in a folder")).expect("make its folders");
        fs::write(file, text).expect("write the file");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Each case: a root `.gitignore`, a path in the root, whether it is a
/// folder, and whether Git ignores it. Every answer is Git 2.43.0's: each
/// case was made in a fresh repository on 2026-11-23, with the path made
/// as a file or a folder, and `git check-ignore -q --no-index -- ./<path>`
/// asked (the `./` keeps Git from reading `-w` as an option and `:` as
/// pathspec magic).
const GIT_CASES: &[(&str, &str, bool, bool)] = &[
    ("target\n", "target", true, true),
    ("target\n", "a/target", true, true),
    ("target\n", "target", false, true),
    ("/target\n", "target", true, true),
    ("/target\n", "a/target", true, false),
    ("target/\n", "target", true, true),
    ("target/\n", "target", false, false),
    ("target/\n", "a/target", true, true),
    ("a/b\n", "a/b", false, true),
    ("a/b\n", "x/a/b", false, false),
    ("/a/b\n", "a/b", false, true),
    ("*.log\n", "x.log", false, true),
    ("*.log\n", "d/x.log", false, true),
    ("*.log\n", ".log", false, true),
    ("*.log\n!keep.log\n", "keep.log", false, false),
    ("!keep.log\n*.log\n", "keep.log", false, true),
    ("a?c\n", "abc", false, true),
    ("a?c\n", "ac", false, false),
    ("a[/]b\n", "a/b", false, false),
    ("x**y\n", "xzy", false, true),
    ("x**y\n", "x/y", false, false),
    ("**/deep\n", "deep", false, true),
    ("**/deep\n", "a/b/deep", false, true),
    ("**/deep\n", "xdeep", false, false),
    ("top/**\n", "top", true, false),
    ("top/**\n", "top/a", false, true),
    ("top/**\n", "top/a/b", true, true),
    ("mid/**/end\n", "mid/end", false, true),
    ("mid/**/end\n", "mid/a/b/end", false, true),
    ("mid/**/end\n", "mid/xend", false, false),
    ("**\n", "z", false, true),
    ("**\n", "a/z", false, true),
    ("a/**b\n", "a/xb", false, true),
    ("a/**b\n", "a/x/b", false, false),
    ("**/foo/\n", "foo", true, true),
    ("**/foo/\n", "a/foo", true, true),
    ("**/foo/\n", "a/foo", false, false),
    ("doc/**/*.pdf\n", "doc/a.pdf", false, true),
    ("doc/**/*.pdf\n", "doc/x/y/a.pdf", false, true),
    ("doc/**/*.pdf\n", "other/doc/a.pdf", false, false),
    ("\\#hash\n", "#hash", false, true),
    ("#hash\n", "#hash", false, false),
    ("\\!bang\n", "!bang", false, true),
    ("trail\\ \n", "trail ", false, true),
    ("trail\\ \n", "trail", false, false),
    ("tr2  \n", "tr2", false, true),
    ("sp ace\n", "sp ace", false, true),
    ("cr\r\n", "cr", false, true),
    ("\u{feff}bom\n", "bom", false, true),
    ("[!a]x\n", "bx", false, true),
    ("[!a]x\n", "ax", false, false),
    ("[^b]y\n", "cy", false, true),
    ("[]]z\n", "]z", false, true),
    ("[a-]w\n", "-w", false, true),
    ("[a-]w\n", "aw", false, true),
    ("[a-c]d\n", "bd", false, true),
    ("[c-a]d\n", "bd", false, false),
    ("[\\]]e\n", "]e", false, true),
    ("[[:digit:]]n\n", "5n", false, true),
    ("[[:alpha:]]n\n", "5n", false, false),
    ("[[:upper:]]u\n", "Uu", false, true),
    ("[[:upper:]]u\n", "uu", false, false),
    ("[[:space:]]s\n", "\ts", false, true),
    ("[[:space:]]s\n", "\u{b}s", false, false),
    ("[[:space:]]s\n", "\u{c}s", false, false),
    ("[[:punct:]]p\n", "_p", false, true),
    ("[[:xdigit:]]h\n", "fh", false, true),
    ("[[:xdigit:]]h\n", "gh", false, false),
    ("[[:graph:]]g\n", " g", false, false),
    ("[[:print:]]g\n", " g", false, true),
    ("[[:blank:]]b\n", "\tb", false, true),
    ("[[:nope:]]q\n", "nq", false, false),
    ("[[:a]\n", ":", false, true),
    ("[[:a]\n", "[", false, true),
    ("un[closed\n", "un[closed", false, false),
    ("bs\\\n", "bs\\", false, false),
    ("bs\\\n", "bs", false, false),
    ("\\*star\n", "*star", false, true),
    ("\\*star\n", "xstar", false, false),
    ("CASE\n", "case", false, false),
    ("case\n", "case", false, true),
    ("/\n", "a", true, false),
    ("!\n", "a", false, false),
    ("   \n", "a", false, false),
    ("\u{e9}\n", "\u{e9}", false, true),
    ("?\n", "\u{e9}", false, false),
    ("??\n", "\u{e9}", false, true),
    ("*\n", "a", false, true),
    ("*\n", "a/b", false, true),
    ("a/*\n", "a/b", false, true),
    ("a/*\n", "a/b/c", false, true),
    ("a/*/c\n", "a/b/c", false, true),
    ("a/*/c\n", "a/b/x/c", false, false),
    ("**/\n", "d", true, true),
    ("**/\n", "f", false, false),
    ("*.[oa]\n", "lib.a", false, true),
    ("*.[oa]\n", "lib.c", false, false),
    ("x/\ny\n!x/z\n", "x/z", false, true),
];

#[test]
fn every_case_is_what_git_says() {
    let mut wrong = Vec::new();
    for &(text, path, is_dir, ignored) in GIT_CASES {
        let file = IgnoreFile::parse(
            text.as_bytes(),
            Path::new(".gitignore"),
            &IgnoreLimits::DEFAULT,
        )
        .unwrap();
        let mut ignore = Ignore::new();
        ignore.set_git_ignore(Path::new(""), file);
        if ignore.is_ignored(Path::new(path), is_dir) != ignored {
            wrong.push(format!(
                "{text:?} {path:?} dir={is_dir}: Git says {ignored}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_folder_opens_by_its_canonical_path() {
    let scratch = Scratch::new("open");
    fs::create_dir_all(scratch.0.join("w/sub")).unwrap();
    let folder = Folder::open(scratch.0.join("w/sub/..")).unwrap();
    let canonical = tidepool_core::path::canonicalize(scratch.0.join("w")).unwrap();
    assert_eq!(folder.root(), canonical);
    assert_eq!(folder.config_dir(), canonical.join(".tidepool"));
    assert_eq!(
        folder.settings_file(),
        canonical.join(".tidepool/settings.toml")
    );
    assert_eq!(
        folder.relative(&canonical.join("sub/a.rs")),
        Some(Path::new("sub/a.rs"))
    );
    assert_eq!(folder.relative(&canonical), Some(Path::new("")));
    assert_eq!(folder.relative(&canonical.join("../x")), None);
    assert_eq!(folder.relative(&scratch.0.join("elsewhere")), None);

    scratch.write("file.txt", "x");
    assert!(matches!(
        Folder::open(scratch.0.join("file.txt")),
        Err(WorkspaceError::NotAFolder { .. })
    ));
    assert!(matches!(
        Folder::open(scratch.0.join("missing")),
        Err(WorkspaceError::Path(_))
    ));
    assert!(matches!(Folder::open(""), Err(WorkspaceError::Path(_))));
}

/// A tree with ignore files at three levels, in a scratch folder named
/// for the test.
fn tree(name: &str) -> (Scratch, Folder) {
    let scratch = Scratch::new(name);
    scratch.write(
        ".gitignore",
        "*.log\nbuild/\n!build/keep/\nlogs/*\n!logs/keep.txt\n",
    );
    scratch.write("sub/.gitignore", "!a.log\nb.txt\n");
    scratch.write("sub/inner/.gitignore", "!c.txt\n");
    scratch.write(".git/info/exclude", "c.txt\nlocal/\n");
    scratch.write(".tidepoolignore", "!b.txt\nnotes.md\n");
    scratch.write("sub/.tidepoolignore", "!b.txt\n");
    let folder = Folder::open(&scratch.0).expect("open the scratch folder");
    (scratch, folder)
}

#[test]
fn which_rule_wins() {
    let (_scratch, folder) = tree("wins");
    let ignored = |path: &str, is_dir: bool| {
        let (ignore, errors) = folder.ignore_for(Path::new(path), &IgnoreLimits::DEFAULT);
        assert!(errors.is_empty(), "{errors:?}");
        ignore.is_ignored(Path::new(path), is_dir)
    };
    // `.gitignore` files alone, as Git 2.43.0 answered the same tree.
    assert!(ignored("a.log", false));
    assert!(!ignored("sub/a.log", false), "a deeper `.gitignore` wins");
    assert!(
        !ignored("sub/x/a.log", false),
        "below `sub`, `sub`'s still wins"
    );
    assert!(!ignored("sub/inner/a.log", false));
    assert!(ignored("build", true));
    assert!(
        ignored("build/keep", true),
        "nothing includes again inside an ignored folder"
    );
    assert!(ignored("build/keep/x", false));
    assert!(!ignored("logs/keep.txt", false));
    assert!(ignored("logs/other", false));
    // The exclude file is beneath every `.gitignore`.
    assert!(ignored("c.txt", false));
    assert!(ignored("sub/c.txt", false));
    assert!(!ignored("sub/inner/c.txt", false));
    assert!(ignored("local", true));
    assert!(!ignored("local", false), "`local/` takes folders only");
    // `.tidepoolignore` is over every `.gitignore`.
    assert!(!ignored("b.txt", false));
    assert!(
        !ignored("sub/b.txt", false),
        "`sub`'s `.gitignore` loses to a `.tidepoolignore`"
    );
    assert!(ignored("notes.md", false));
    assert!(ignored("sub/notes.md", false));
    // `.git` always, and everything in it.
    assert!(ignored(".git", true));
    assert!(ignored(".git/info/exclude", false));
    assert!(ignored("sub/.git", false), "a submodule's `.git` file");
    // The root, and paths that are not plain, are never ignored.
    let (ignore, _) = folder.ignore_for(Path::new(""), &IgnoreLimits::DEFAULT);
    assert!(!ignore.is_ignored(Path::new(""), true));
    assert!(!ignore.is_ignored(Path::new("../a.log"), false));
    assert!(!ignore.is_ignored(Path::new("/a.log"), false));
}

#[test]
fn a_walker_reads_each_folder_once() {
    let (_scratch, folder) = tree("walker");
    let mut ignore = Ignore::new();
    let limits = IgnoreLimits::DEFAULT;
    assert!(
        folder
            .read_ignore_files(Path::new(""), &mut ignore, &limits)
            .is_empty()
    );
    // `sub`'s files not read yet: only the root's rules.
    assert!(ignore.is_ignored(Path::new("sub/a.log"), false));
    assert!(
        folder
            .read_ignore_files(Path::new("sub"), &mut ignore, &limits)
            .is_empty()
    );
    assert!(!ignore.is_ignored(Path::new("sub/a.log"), false));
    // A folder with no ignore files, or a path that is not plain, reads
    // nothing and is no error.
    assert!(
        folder
            .read_ignore_files(Path::new("none"), &mut ignore, &limits)
            .is_empty()
    );
    assert!(
        folder
            .read_ignore_files(Path::new("../x"), &mut ignore, &limits)
            .is_empty()
    );
}

#[test]
fn files_that_cannot_be_used_leave_no_rules_and_name_themselves() {
    let scratch = Scratch::new("bad");
    scratch.write(".gitignore", "a\n");
    scratch.write("big/.gitignore", &"x\n".repeat(40));
    scratch.write("long/.gitignore", &format!("ok\n{}\n", "y".repeat(40)));
    scratch.write("many/.gitignore", "1\n2\n3\n4\n5\n");
    scratch.write("dir/.gitignore/inside", "");
    let folder = Folder::open(&scratch.0).unwrap();
    let mut limits = IgnoreLimits::DEFAULT;
    limits.max_file_bytes = 60;
    limits.max_pattern_bytes = 32;
    limits.max_patterns = 4;
    let mut ignore = Ignore::new();
    let errors =
        |dir: &str, ignore: &mut Ignore| folder.read_ignore_files(Path::new(dir), ignore, &limits);
    assert!(errors("", &mut ignore).is_empty());
    let big = errors("big", &mut ignore);
    assert!(
        matches!(big[..], [WorkspaceError::TooLarge { limit: 60, .. }]),
        "{big:?}"
    );
    let long = errors("long", &mut ignore);
    assert!(
        matches!(
            long[..],
            [WorkspaceError::PatternTooLong {
                line: 2,
                limit: 32,
                ..
            }]
        ),
        "{long:?}"
    );
    let many = errors("many", &mut ignore);
    assert!(
        matches!(many[..], [WorkspaceError::TooManyPatterns { limit: 4, .. }]),
        "{many:?}"
    );
    let dir = errors("dir", &mut ignore);
    assert!(
        matches!(dir[..], [WorkspaceError::NotRegularFile { .. }]),
        "{dir:?}"
    );
    assert!(dir[0].to_string().contains(".gitignore"), "{}", dir[0]);
    // The files not used leave no rules; the root's still apply.
    assert!(!ignore.is_ignored(Path::new("long/ok"), false));
    assert!(ignore.is_ignored(Path::new("long/a"), false));
}

/// Makes a symbolic link at `at` to `target`, or says why it cannot.
fn link(target: &Path, at: &Path) -> Result<(), String> {
    // `fs::soft_link` is deprecated in favour of one function per OS, but
    // those would need a platform `cfg`, which `workspace` must not have
    // (`AGENTS.md` rule 5); this one works wherever links do.
    #[allow(deprecated)]
    fs::soft_link(target, at).map_err(|error| error.to_string())
}

#[test]
fn a_linked_ignore_file_is_not_read_as_git_does_not_read_one() {
    let scratch = Scratch::new("link");
    scratch.write("rules", "x.txt\n");
    fs::create_dir_all(scratch.0.join("sub")).unwrap();
    if let Err(reason) = link(Path::new("../rules"), &scratch.0.join("sub/.gitignore")) {
        let _ = writeln!(
            io::stderr(),
            "SKIPPED: a_linked_ignore_file_is_not_read_as_git_does_not_read_one: no link: {reason}"
        );
        return;
    }
    let folder = Folder::open(&scratch.0).unwrap();
    let (ignore, errors) = folder.ignore_for(Path::new("sub/x.txt"), &IgnoreLimits::DEFAULT);
    assert!(
        matches!(errors[..], [WorkspaceError::NotRegularFile { .. }]),
        "{errors:?}"
    );
    assert!(!ignore.is_ignored(Path::new("sub/x.txt"), false));
}

#[test]
fn ignore_files_are_not_read_through_a_linked_folder() {
    let scratch = Scratch::new("linkdir");
    scratch.write("outside/.gitignore", "secret\n");
    fs::create_dir_all(scratch.0.join("root/real")).expect("make the root");
    if let Err(reason) = link(&scratch.0.join("outside"), &scratch.0.join("root/linked")) {
        let _ = writeln!(
            io::stderr(),
            "SKIPPED: ignore_files_are_not_read_through_a_linked_folder: no link: {reason}"
        );
        return;
    }
    let folder = Folder::open(scratch.0.join("root")).expect("open the root");
    let (ignore, errors) = folder.ignore_for(Path::new("linked/secret"), &IgnoreLimits::DEFAULT);
    assert!(errors.is_empty(), "{errors:?}");
    assert!(
        !ignore.is_ignored(Path::new("linked/secret"), false),
        "the outside file is not read"
    );
    assert_eq!(ignore.total_bytes(), 0);
    // A file where a folder is expected reads nothing either.
    scratch.write("root/file", "");
    let mut ignore = Ignore::new();
    let none = folder.read_ignore_files(Path::new("file/x"), &mut ignore, &IgnoreLimits::DEFAULT);
    assert!(none.is_empty());
    assert_eq!(ignore.total_bytes(), 0);
}

#[test]
fn the_ignore_files_together_stay_within_their_budget() {
    let scratch = Scratch::new("budget");
    scratch.write(".gitignore", &"a\n".repeat(20));
    scratch.write(".tidepoolignore", &"b\n".repeat(20));
    scratch.write("sub/.gitignore", &"c\n".repeat(20));
    let folder = Folder::open(&scratch.0).expect("open the folder");
    let mut limits = IgnoreLimits::DEFAULT;
    limits.max_total_bytes = 90;
    let mut ignore = Ignore::new();
    assert!(
        folder
            .read_ignore_files(Path::new(""), &mut ignore, &limits)
            .is_empty()
    );
    assert_eq!(ignore.total_bytes(), 80);
    let refused = folder.read_ignore_files(Path::new("sub"), &mut ignore, &limits);
    assert!(
        matches!(
            refused[..],
            [WorkspaceError::IgnoreBudget { limit: 90, .. }]
        ),
        "{refused:?}"
    );
    assert!(refused[0].to_string().contains("sub"), "{}", refused[0]);
    assert!(
        !ignore.is_ignored(Path::new("sub/c"), false),
        "the refused file left no rules"
    );
    assert_eq!(ignore.total_bytes(), 80);
    // Reading a folder again replaces its files, so it stays within the
    // budget rather than counting them twice.
    assert!(
        folder
            .read_ignore_files(Path::new(""), &mut ignore, &limits)
            .is_empty()
    );
    assert_eq!(ignore.total_bytes(), 80);
}

#[test]
fn a_file_alone_says_what_its_last_match_says() {
    let file =
        IgnoreFile::parse(b"*.o\n!keep.o\n", Path::new("x"), &IgnoreLimits::DEFAULT).unwrap();
    assert_eq!(file.len(), 2);
    assert!(!file.is_empty());
    assert_eq!(file.matched(b"a.o", false), Match::Ignore);
    assert_eq!(file.matched(b"d/keep.o", false), Match::Include);
    assert_eq!(file.matched(b"a.c", false), Match::None);
    assert_eq!(file.bytes(), 12);
    assert_eq!(
        format!("{file:?}"),
        "IgnoreFile { patterns: 2, bytes: 12 }",
        "Debug shows no pattern"
    );
}

#[test]
fn hostile_patterns_match_in_linear_time() {
    // A backtracking matcher takes time exponential in the number of `*`
    // for `*a*a…*ab` on a run of `a`s; this one takes the run's length
    // times the pattern's.
    let mut text = String::new();
    for n in 1..=40 {
        text.push_str(&"*a".repeat(n));
        text.push_str("b\n");
    }
    text.push_str("**/**/**/**/**/**/**/**/x\n[[:alpha:][:digit:]]*[!/]?*?*?*?*z\n");
    let file = IgnoreFile::parse(text.as_bytes(), Path::new("x"), &IgnoreLimits::DEFAULT).unwrap();
    let path = "a/".repeat(500) + &"a".repeat(3000);
    let start = std::time::Instant::now();
    assert_eq!(file.matched(path.as_bytes(), false), Match::None);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}
