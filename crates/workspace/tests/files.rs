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

//! `workspace::files`, headless, on a real tree: a folder at a time, sorted
//! with folders first, each entry's ignore state from the ignore files
//! above it, rows for what is open, stale reads dropped, and links shown
//! but never followed (M4.11).

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::thread;

use tidepool_workspace::WorkspaceError;
use tidepool_workspace::files::{EntryKind, FileTree, ReadDirJob, Row};
use tidepool_workspace::folder::Folder;

/// A fresh scratch folder for one test, removed when the test passes.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("tidepool-workspace-files")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("make the scratch folder");
        Scratch(dir)
    }

    /// Writes `bytes` at `path` under the folder, making its folders.
    fn file(&self, path: &str, bytes: &[u8]) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
        fs::write(path, bytes).expect("write");
    }

    fn folder(&self) -> Folder {
        Folder::open(&self.0).expect("open the folder")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !thread::panicking() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

/// Runs `job` on another thread, as a worker would, and keeps its result.
fn run(tree: &mut FileTree, job: ReadDirJob) -> bool {
    let done = thread::spawn(move || job.run()).join().expect("the worker");
    tree.finish(done)
}

/// Opens `dir` and reads it.
fn expand(tree: &mut FileTree, dir: &str) {
    let job = tree
        .expand(Path::new(dir))
        .expect("a folder of the tree")
        .expect("a read");
    assert!(run(tree, job));
}

/// Each row as `"  name/ (ignored)"`: indented by depth, a `/` after a
/// folder, a `@` after a link.
fn shown(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            let name = row.path.file_name().expect("a name").to_string_lossy();
            let mark = match row.kind {
                EntryKind::Folder => "/",
                EntryKind::Link => "@",
                _ => "",
            };
            let ignored = if row.ignored { " (ignored)" } else { "" };
            format!("{}{name}{mark}{ignored}", "  ".repeat(row.depth))
        })
        .collect()
}

fn project(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.file(".gitignore", b"target/\n*.log\n");
    scratch.file("Cargo.toml", b"");
    scratch.file("README.md", b"");
    scratch.file("build.log", b"");
    scratch.file("src/main.rs", b"");
    scratch.file("src/lib.rs", b"");
    scratch.file("src/.gitignore", b"generated.rs\n");
    scratch.file("src/generated.rs", b"");
    scratch.file("src/deep/mod.rs", b"");
    scratch.file("target/debug/app", b"");
    scratch.file(".git/HEAD", b"ref: refs/heads/main\n");
    scratch.file("Zebra/z.txt", b"");
    scratch
}

#[test]
fn a_folder_is_read_when_opened_and_its_entries_sorted_and_marked() {
    let scratch = project("sorted");
    let mut tree = FileTree::new(scratch.folder());
    assert!(
        tree.rows().is_empty(),
        "nothing shows before the root is read"
    );
    let job = tree
        .expand(Path::new(""))
        .expect("the root")
        .expect("a read");
    assert_eq!(job.dir(), Path::new(""));
    assert!(tree.is_loading(Path::new("")));
    assert!(
        tree.expand(Path::new("")).expect("the root").is_none(),
        "one read at a time"
    );
    assert!(run(&mut tree, job));
    assert!(!tree.is_loading(Path::new("")));
    assert!(tree.problems(Path::new("")).is_empty());
    assert_eq!(
        shown(&tree.rows()),
        [
            ".git/ (ignored)",
            "src/",
            "target/ (ignored)",
            "Zebra/",
            ".gitignore",
            "build.log (ignored)",
            "Cargo.toml",
            "README.md",
        ]
    );

    // A folder inside, with its own ignore file over the root's.
    expand(&mut tree, "src");
    assert!(tree.is_expanded(Path::new("src")));
    let rows = tree.rows();
    assert_eq!(
        shown(&rows[1..7]),
        [
            "src/",
            "  deep/",
            "  .gitignore",
            "  generated.rs (ignored)",
            "  lib.rs",
            "  main.rs",
        ]
    );
    assert_eq!(rows[1].path, Path::new("src"));
    assert!(rows[1].expanded);
    assert_eq!(rows[2].path, Path::new("src").join("deep"));
    assert_eq!(rows[2].depth, 1);
    assert!(!rows[2].expanded);

    // Inside an ignored folder, everything is ignored.
    expand(&mut tree, "target");
    let listing = tree.listing(Path::new("target")).expect("read");
    assert_eq!(listing.entries().len(), 1);
    assert!(listing.entries()[0].is_ignored());
    assert!(!listing.is_truncated());

    // Closing a folder hides its rows and keeps its listing.
    tree.collapse(Path::new("src"));
    assert!(!tree.is_expanded(Path::new("src")));
    assert!(tree.listing(Path::new("src")).is_some());
    assert!(!shown(&tree.rows()).contains(&"  main.rs".to_string()));
    assert!(
        tree.expand(Path::new("src")).expect("src").is_none(),
        "read already"
    );
    assert!(shown(&tree.rows()).contains(&"  main.rs".to_string()));
}

#[test]
fn a_read_replaced_by_a_later_one_is_stale() {
    let scratch = project("stale");
    let mut tree = FileTree::new(scratch.folder());
    let first = tree
        .expand(Path::new(""))
        .expect("the root")
        .expect("a read");
    scratch.file("added.txt", b"");
    let second = tree.refresh(Path::new("")).expect("the root");
    // The later read finishes first; the earlier one is then dropped.
    assert!(run(&mut tree, second));
    assert!(!run(&mut tree, first));
    let names: Vec<_> = tree
        .listing(Path::new(""))
        .expect("read")
        .entries()
        .iter()
        .map(|entry| entry.name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"added.txt".to_string()), "{names:?}");
}

#[test]
fn a_folder_that_cannot_be_read_says_why_and_can_be_read_again() {
    let scratch = project("unreadable");
    let mut tree = FileTree::new(scratch.folder());
    expand(&mut tree, "");
    let job = tree
        .expand(Path::new("Zebra"))
        .expect("Zebra")
        .expect("a read");
    fs::remove_dir_all(scratch.0.join("Zebra")).expect("remove it");
    assert!(run(&mut tree, job));
    assert!(tree.listing(Path::new("Zebra")).is_none());
    assert!(
        matches!(
            tree.error(Path::new("Zebra")),
            Some(WorkspaceError::NotBrowsable { .. })
        ),
        "{:?}",
        tree.error(Path::new("Zebra"))
    );
    // After a failure, opening it reads it again.
    scratch.file("Zebra/again.txt", b"");
    expand(&mut tree, "Zebra");
    assert!(tree.error(Path::new("Zebra")).is_none());
    assert_eq!(
        tree.listing(Path::new("Zebra"))
            .expect("read")
            .entries()
            .len(),
        1
    );
}

#[test]
fn paths_outside_the_tree_are_refused() {
    let scratch = project("outside-paths");
    let mut tree = FileTree::new(scratch.folder());
    for bad in ["..", "../elsewhere", "/etc", "src/../.."] {
        assert!(
            matches!(
                tree.expand(Path::new(bad)),
                Err(WorkspaceError::NotBrowsable { .. })
            ),
            "{bad}"
        );
    }
}

#[test]
fn an_ignore_file_that_cannot_be_used_is_reported_and_the_folder_still_listed() {
    let scratch = Scratch::new("bad-ignore");
    scratch.file(".gitignore", &vec![b'a'; 300 * 1024]);
    scratch.file("kept.txt", b"");
    let mut tree = FileTree::new(scratch.folder());
    expand(&mut tree, "");
    assert_eq!(tree.problems(Path::new("")).len(), 1);
    assert!(matches!(
        tree.problems(Path::new(""))[0],
        WorkspaceError::TooLarge { .. }
    ));
    assert_eq!(shown(&tree.rows()), [".gitignore", "kept.txt"]);
}

/// Makes `at` a link to `target`, or prints `SKIPPED:` for `test`.
fn linked(test: &str, target: &Path, at: &Path) -> bool {
    // `fs::soft_link` is deprecated in favor of one function per system,
    // which would need a platform `cfg` (`AGENTS.md` rule 5).
    #[allow(deprecated)]
    match fs::soft_link(target, at) {
        Ok(()) => true,
        Err(reason) => {
            let _ = writeln!(io::stderr(), "SKIPPED: {test}: no link: {reason}");
            false
        }
    }
}

#[test]
fn a_link_shows_as_a_link_and_is_not_followed() {
    let test = "a_link_shows_as_a_link_and_is_not_followed";
    let scratch = project("link");
    let outside = Scratch::new("outside");
    outside.file("private.txt", b"");
    if !linked(test, &outside.0, &scratch.0.join("linked")) {
        return;
    }
    let mut tree = FileTree::new(scratch.folder());
    expand(&mut tree, "");
    let rows = tree.rows();
    let row = rows
        .iter()
        .find(|row| row.path == Path::new("linked"))
        .expect("the link shows");
    assert_eq!(row.kind, EntryKind::Link);
    let job = tree
        .expand(Path::new("linked"))
        .expect("a name")
        .expect("a read");
    assert!(run(&mut tree, job));
    assert!(tree.listing(Path::new("linked")).is_none());
    assert!(matches!(
        tree.error(Path::new("linked")),
        Some(WorkspaceError::NotBrowsable { .. })
    ));
}
