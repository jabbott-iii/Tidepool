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

//! `workspace::trust`: untrusted by default, grants that persist and cover
//! the folders under them, and a file that is never overwritten when it
//! cannot be used (M4.7, ADR-0014, ADR-0067).

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use tidepool_workspace::WorkspaceError;
use tidepool_workspace::folder::Folder;
use tidepool_workspace::trust::{self, TrustStore};

/// A fresh scratch folder for one test, with `data/` for the store and
/// `work/` for folders, removed when it is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("tidepool-trust-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("work")).expect("make the work folder");
        Scratch(dir)
    }

    /// The user data folder, which does not exist until a grant is saved.
    fn data(&self) -> PathBuf {
        self.0.join("data")
    }

    /// The folder `work/<path>`, made and opened.
    fn folder(&self, path: &str) -> Folder {
        let dir = self.0.join("work").join(path);
        fs::create_dir_all(&dir).expect("make the folder");
        Folder::open(dir).expect("open the folder")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_folder_is_untrusted_until_it_is_trusted() {
    let scratch = Scratch::new("default");
    let store = TrustStore::load(&scratch.data());
    assert!(store.problem().is_none());
    assert!(store.grants().is_empty());
    assert!(!store.is_trusted(&scratch.folder("repo")));
    assert_eq!(store.file(), scratch.data().join(trust::FILE_NAME));
}

#[test]
fn a_grant_persists_and_covers_the_folders_under_it() {
    let scratch = Scratch::new("persist");
    let repo = scratch.folder("repo");
    let mut store = TrustStore::load(&scratch.data());
    store.trust(&repo).unwrap();
    assert!(store.is_trusted(&repo));

    // A later run reads it back.
    let store = TrustStore::load(&scratch.data());
    assert!(store.problem().is_none(), "{:?}", store.problem());
    assert_eq!(store.grants(), [repo.root().to_path_buf()]);
    assert!(store.is_trusted(&repo));
    let inner = scratch.folder("repo/crates/core");
    assert!(store.is_trusted(&inner), "a folder inside a grant");
    assert_eq!(store.grant_for(&inner), Some(repo.root()));
    assert!(
        !store.is_trusted(&scratch.folder("repo-other")),
        "a sibling sharing a prefix"
    );
    assert!(!store.is_trusted(&scratch.folder("")), "the folder above");

    let text = fs::read_to_string(store.file()).unwrap();
    assert!(
        text.starts_with("# The folders the user trusts (ADR-0014)."),
        "{text}"
    );
    assert!(text.contains("version = 1\n"), "{text}");
}

#[test]
fn trusting_a_trusted_folder_writes_nothing_and_the_nearest_grant_wins() {
    let scratch = Scratch::new("nearest");
    let mut store = TrustStore::load(&scratch.data());
    let outer = scratch.folder("a");
    let inner = scratch.folder("a/b");
    store.trust(&inner).unwrap();
    store.trust(&outer).unwrap();
    let written = fs::read(store.file()).unwrap();
    store.trust(&scratch.folder("a/b/c")).unwrap();
    assert_eq!(fs::read(store.file()).unwrap(), written, "already trusted");
    assert_eq!(store.grants().len(), 2);
    assert_eq!(
        store.grant_for(&scratch.folder("a/b/c")),
        Some(inner.root())
    );
}

#[test]
fn revoking_removes_one_grant_exactly() {
    let scratch = Scratch::new("revoke");
    let mut store = TrustStore::load(&scratch.data());
    let outer = scratch.folder("a");
    let inner = scratch.folder("a/b");
    store.trust(&inner).unwrap();
    store.trust(&outer).unwrap();
    assert!(store.revoke(outer.root()).unwrap());
    assert!(!store.is_trusted(&outer));
    assert!(store.is_trusted(&inner), "its own grant stays");
    assert!(
        !store.revoke(outer.root()).unwrap(),
        "no such grant any more"
    );
    assert!(!store.revoke(&scratch.0.join("never")).unwrap());
    let reread = TrustStore::load(&scratch.data());
    assert_eq!(reread.grants(), [inner.root().to_path_buf()]);
}

#[test]
fn a_grant_another_store_saved_since_is_kept() {
    let scratch = Scratch::new("merge");
    let mut first = TrustStore::load(&scratch.data());
    let mut second = TrustStore::load(&scratch.data());
    first.trust(&scratch.folder("one")).unwrap();
    second.trust(&scratch.folder("two")).unwrap();
    let reread = TrustStore::load(&scratch.data());
    assert_eq!(reread.grants().len(), 2, "{:?}", reread.grants());
    assert_eq!(second.grants().len(), 2, "the store holds what is on disk");
}

#[test]
fn a_file_that_cannot_be_used_grants_nothing_and_is_never_overwritten() {
    for (name, text) in [
        ("newer", "version = 2\nfolders = [\"/x\"]\n"),
        ("broken", "version = 1\nfolders = [\"/x\"\n"),
        ("relative", "version = 1\nfolders = [\"x\"]\n"),
        ("noversion", "folders = [\"/x\"]\n"),
    ] {
        let scratch = Scratch::new(name);
        fs::create_dir_all(scratch.data()).unwrap();
        let file = scratch.data().join(trust::FILE_NAME);
        fs::write(&file, text).unwrap();
        let mut store = TrustStore::load(&scratch.data());
        assert!(store.problem().is_some(), "{name}");
        assert!(store.grants().is_empty(), "{name}");
        let error = store.trust(&scratch.folder("repo")).unwrap_err();
        assert!(
            matches!(error, WorkspaceError::Unwritable { .. }),
            "{name}: {error}"
        );
        assert!(error.to_string().contains("trust.toml"), "{error}");
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            text,
            "{name}: unchanged"
        );
        assert!(!store.is_trusted(&scratch.folder("repo")), "{name}");
        // Once the user removes it, trusting works.
        fs::remove_file(&file).unwrap();
        store.trust(&scratch.folder("repo")).unwrap();
        assert!(store.problem().is_none());
    }
}

#[test]
fn a_store_past_its_limits_is_refused() {
    let scratch = Scratch::new("limits");
    fs::create_dir_all(scratch.data()).unwrap();
    let file = scratch.data().join(trust::FILE_NAME);
    let mut text = String::from("version = 1\nfolders = [");
    for n in 0..trust::MAX_GRANTS {
        text.push_str(&format!("\"/g/{n}\", "));
    }
    text.push_str("]\n");
    fs::write(&file, &text).unwrap();
    let mut store = TrustStore::load(&scratch.data());
    assert_eq!(store.grants().len(), trust::MAX_GRANTS);
    assert!(matches!(
        store.trust(&scratch.folder("repo")),
        Err(WorkspaceError::TooManyGrants { limit: 4096 })
    ));
    assert_eq!(fs::read_to_string(&file).unwrap(), text);

    let big = vec![b' '; usize::try_from(trust::MAX_FILE_BYTES).unwrap() + 1];
    assert!(matches!(
        trust::parse(&big, &file),
        Err(WorkspaceError::TooLarge { .. })
    ));
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
fn a_link_out_of_a_trusted_folder_is_not_trusted() {
    let scratch = Scratch::new("link");
    let mut store = TrustStore::load(&scratch.data());
    let trusted = scratch.folder("trusted");
    let outside = scratch.folder("outside");
    store.trust(&trusted).unwrap();
    let at = trusted.root().join("escape");
    if let Err(reason) = link(outside.root(), &at) {
        let _ = writeln!(
            io::stderr(),
            "SKIPPED: a_link_out_of_a_trusted_folder_is_not_trusted: no link: {reason}"
        );
        return;
    }
    // Opened through the link, the folder is the link's target.
    let through = Folder::open(&at).unwrap();
    assert_eq!(through.root(), outside.root());
    assert!(!store.is_trusted(&through));
}
