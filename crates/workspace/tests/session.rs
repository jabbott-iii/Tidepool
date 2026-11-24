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

//! `workspace::session`: a session round-trips through its file in the
//! user data folder, one that names another folder is not used, files
//! past the limits or that are not sessions are refused, and a selection
//! is checked against the file as it is (M4.11, ADR-0068).

use std::fs;
use std::path::{Path, PathBuf};

use tidepool_text::{Cursor, Rope, Selection};
use tidepool_workspace::WorkspaceError;
use tidepool_workspace::folder::Folder;
use tidepool_workspace::session::{
    self, Axis, EditorState, Group, Layout, MAX_CURSORS, MAX_DEPTH, MAX_GROUPS, Pane, Scroll,
    Session, Split,
};

/// A fresh scratch folder for one test, with `data/` for the user data
/// folder and `work/` for folders, removed when it is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("tidepool-workspace-session")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("work")).expect("make the work folder");
        Scratch(dir)
    }

    fn data(&self) -> PathBuf {
        self.0.join("data")
    }

    fn folder(&self, name: &str) -> Folder {
        let dir = self.0.join("work").join(name);
        fs::create_dir_all(&dir).expect("make the folder");
        Folder::open(dir).expect("open the folder")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn editor(folder: &Folder, name: &str, selection: Selection, scroll: Scroll) -> EditorState {
    EditorState {
        path: folder.root().join(name),
        selection,
        scroll,
    }
}

/// Two panes side by side: a group of two editors, and two groups stacked.
fn sample(folder: &Folder) -> Session {
    let left = Group {
        editors: vec![
            editor(
                folder,
                "src/main.rs",
                Selection::new(Cursor::new(10, 4), [Cursor::caret(0), Cursor::new(20, 25)]),
                Scroll {
                    line: 120,
                    row: 0,
                    y: 7.25,
                    x: 33.5,
                },
            ),
            editor(folder, "README.md", Selection::caret(0), Scroll::default()),
        ],
        active: 1,
    };
    let top = Group {
        editors: vec![editor(
            folder,
            "Cargo.toml",
            Selection::caret(3),
            Scroll {
                line: 2,
                row: 1,
                y: 0.0,
                x: 0.0,
            },
        )],
        active: 0,
    };
    Session {
        folder: folder.root().to_path_buf(),
        layout: Layout::Split(Split {
            axis: Axis::Row,
            panes: vec![
                Pane {
                    weight: 2.0,
                    layout: Layout::Group(left),
                },
                Pane {
                    weight: 1.0,
                    layout: Layout::Split(Split {
                        axis: Axis::Column,
                        panes: vec![
                            Pane {
                                weight: 0.5,
                                layout: Layout::Group(top),
                            },
                            Pane {
                                weight: 0.5,
                                layout: Layout::Group(Group::default()),
                            },
                        ],
                    }),
                },
            ],
        }),
        active_group: 2,
    }
}

#[test]
fn a_session_round_trips_through_the_user_data_folder() {
    let scratch = Scratch::new("round-trip");
    let folder = scratch.folder("repo");
    assert!(Session::load(&scratch.data(), &folder).unwrap().is_none());

    let saved = sample(&folder);
    assert_eq!(saved.groups().len(), 3);
    saved.save(&scratch.data()).unwrap();
    let file = session::path_in(&scratch.data(), folder.root());
    assert!(file.starts_with(scratch.data().join(session::DIR_NAME)));
    assert!(file.is_file());
    assert_eq!(
        fs::read_dir(folder.root()).unwrap().count(),
        0,
        "nothing is written into the project"
    );
    let loaded = Session::load(&scratch.data(), &folder)
        .unwrap()
        .expect("a session");
    assert_eq!(loaded, saved);
    assert_eq!(loaded.groups()[loaded.active_group].editors.len(), 0);

    // Saving again replaces the file.
    let mut changed = loaded;
    changed.active_group = 0;
    changed.save(&scratch.data()).unwrap();
    let loaded = Session::load(&scratch.data(), &folder)
        .unwrap()
        .expect("a session");
    assert_eq!(loaded.active_group, 0);
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.starts_with("# The editors a folder had open"),
        "{text}"
    );
    assert!(text.contains("version = 1\n"), "{text}");
}

#[test]
fn a_new_session_has_one_empty_group() {
    let scratch = Scratch::new("new");
    let folder = scratch.folder("repo");
    let session = Session::new(&folder);
    assert_eq!(session.groups().len(), 1);
    session.save(&scratch.data()).unwrap();
    assert_eq!(
        Session::load(&scratch.data(), &folder).unwrap(),
        Some(session)
    );
}

#[test]
fn a_session_that_names_another_folder_is_not_used() {
    let scratch = Scratch::new("other");
    let first = scratch.folder("first");
    let second = scratch.folder("second");
    sample(&first).save(&scratch.data()).unwrap();
    // The first folder's file, under the second's name, as a collision of
    // names would leave it.
    fs::copy(
        session::path_in(&scratch.data(), first.root()),
        session::path_in(&scratch.data(), second.root()),
    )
    .unwrap();
    let error = Session::load(&scratch.data(), &second).expect_err("another folder's");
    assert!(
        matches!(&error, WorkspaceError::OtherFolder { folder, .. } if folder == second.root()),
        "{error:?}"
    );
    assert!(Session::load(&scratch.data(), &first).unwrap().is_some());
}

/// The module documentation's example, as written there, reads.
#[test]
fn the_documented_example_reads() {
    let text = r#"# The editors a folder had open (ADR-0068). Tidepool writes this file.
version = 1
folder = "/home/me/src/tidepool"
active_group = 1
layout = { split = "row", panes = [
    { weight = 1.0, layout = { active = 0, editors = [
        { path = "/home/me/src/tidepool/README.md", primary = 0,
          cursors = [[0, 0]], scroll = { line = 0, row = 0, y = 0.0, x = 0.0 } },
    ] } },
    { weight = 1.0, layout = { active = 0, editors = [] } },
] }
"#;
    let session = session::parse(text.as_bytes(), Path::new("example.toml")).unwrap();
    assert_eq!(session.folder, Path::new("/home/me/src/tidepool"));
    assert_eq!(session.groups().len(), 2);
    assert_eq!(session.active_group, 1);
}

#[test]
fn files_that_are_not_sessions_are_refused() {
    let head = "version = 1\nfolder = \"/w\"\n";
    let group = "layout = { active = 0, editors = [] }\n";
    let cases: &[(String, &str)] = &[
        ("version = 2\n".into(), "Version"),
        (
            format!("folder = \"/w\"\nactive_group = 0\n{group}"),
            "Version",
        ),
        (format!("version = 1\nactive_group = 0\n{group}"), "folder"),
        (
            format!("version = 1\nfolder = \"w\"\nactive_group = 0\n{group}"),
            "folder",
        ),
        (
            format!("version = 1\nfolder = \"/w/../x\"\nactive_group = 0\n{group}"),
            "folder",
        ),
        (format!("{head}active_group = 0\n"), "layout"),
        (format!("{head}active_group = 1\n{group}"), "active_group"),
        (format!("{head}active_group = -1\n{group}"), "active_group"),
        (
            format!("{head}active_group = 0\nlayout = {{ split = \"diagonal\", panes = [] }}\n"),
            "neither",
        ),
        (
            format!(
                "{head}active_group = 0\nlayout = {{ split = \"row\", panes = [{{ weight = 1.0, \
                 layout = {{ active = 0, editors = [] }} }}] }}\n"
            ),
            "fewer than 2",
        ),
        (
            format!(
                "{head}active_group = 0\nlayout = {{ split = \"row\", panes = [\
                 {{ weight = 0.0, layout = {{ active = 0, editors = [] }} }}, \
                 {{ weight = 1.0, layout = {{ active = 0, editors = [] }} }}] }}\n"
            ),
            "weight",
        ),
        (
            format!(
                "{head}active_group = 0\nlayout = {{ active = 0, editors = [{{ path = \"/w/a\", \
                 primary = 0, cursors = [], scroll = {{ line = 0, row = 0, y = 0.0, x = 0.0 }} \
                 }}] }}\n"
            ),
            "no cursor",
        ),
        (
            format!(
                "{head}active_group = 0\nlayout = {{ active = 0, editors = [{{ path = \"/w/a\", \
                 primary = 1, cursors = [[0, 0]], scroll = {{ line = 0, row = 0, y = 0.0, x = \
                 0.0 }} }}] }}\n"
            ),
            "primary",
        ),
        (
            format!(
                "{head}active_group = 0\nlayout = {{ active = 0, editors = [{{ path = \"/w/a\", \
                 primary = 0, cursors = [[0, -1]], scroll = {{ line = 0, row = 0, y = 0.0, x = \
                 0.0 }} }}] }}\n"
            ),
            "byte offsets",
        ),
        (
            format!(
                "{head}active_group = 0\nlayout = {{ active = 0, editors = [{{ path = \"/w/a\", \
                 primary = 0, cursors = [[0, 0]], scroll = {{ line = 0, row = 0, y = nan, x = \
                 0.0 }} }}] }}\n"
            ),
            "scroll",
        ),
        (
            format!(
                "{head}active_group = 0\nlayout = {{ active = 1, editors = [{{ path = \"/w/a\", \
                 primary = 0, cursors = [[0, 0]], scroll = {{ line = 0, row = 0, y = 0.0, x = \
                 0.0 }} }}] }}\n"
            ),
            "active",
        ),
        ("not = toml = at all".into(), "TOML"),
    ];
    for (text, says) in cases {
        let error = session::parse(text.as_bytes(), Path::new("s.toml"))
            .expect_err(&format!("refused: {text}"));
        let message = match &error {
            WorkspaceError::Version { .. } => "Version".to_string(),
            WorkspaceError::Toml { .. } => "TOML".to_string(),
            WorkspaceError::BadSessionFile { reason, .. } => (*reason).to_string(),
            other => panic!("{text}: {other:?}"),
        };
        assert!(message.contains(says), "{text}: {message}");
    }
    let big = vec![b' '; session::MAX_FILE_BYTES as usize + 1];
    assert!(matches!(
        session::parse(&big, Path::new("s.toml")),
        Err(WorkspaceError::TooLarge { .. })
    ));
}

/// A layout of `depth` nested splits, each of two empty groups but the
/// last.
fn nested(depth: usize) -> Layout {
    let mut layout = Layout::Group(Group::default());
    for _ in 0..depth {
        layout = Layout::Split(Split {
            axis: Axis::Row,
            panes: vec![
                Pane {
                    weight: 1.0,
                    layout,
                },
                Pane {
                    weight: 1.0,
                    layout: Layout::Group(Group::default()),
                },
            ],
        });
    }
    layout
}

#[test]
fn a_session_past_a_limit_is_not_written() {
    let scratch = Scratch::new("limits");
    let folder = scratch.folder("repo");
    let mut session = Session::new(&folder);

    session.layout = nested(MAX_DEPTH);
    session.to_text().expect("8 splits deep");
    session.layout = nested(MAX_DEPTH + 1);
    assert!(matches!(
        session.to_text(),
        Err(WorkspaceError::BadSessionFile { .. })
    ));

    // 64 groups, in a split of 16 panes of splits of 4.
    let groups = |count: usize| {
        Layout::Split(Split {
            axis: Axis::Column,
            panes: (0..count)
                .map(|_| Pane {
                    weight: 1.0,
                    layout: Layout::Group(Group::default()),
                })
                .collect(),
        })
    };
    let many = |inner: usize| {
        Layout::Split(Split {
            axis: Axis::Row,
            panes: (0..16)
                .map(|_| Pane {
                    weight: 1.0,
                    layout: groups(inner),
                })
                .collect(),
        })
    };
    session.layout = many(MAX_GROUPS / 16);
    assert_eq!(session.groups().len(), MAX_GROUPS);
    session.to_text().expect("64 groups");
    session.layout = many(MAX_GROUPS / 16 + 1);
    assert!(session.to_text().is_err(), "more than 64 groups");

    session.layout = Layout::Split(Split {
        axis: Axis::Row,
        panes: vec![
            Pane {
                weight: f32::INFINITY,
                layout: Layout::Group(Group::default()),
            },
            Pane {
                weight: 1.0,
                layout: Layout::Group(Group::default()),
            },
        ],
    });
    assert!(session.to_text().is_err(), "a weight that is not finite");

    session.layout = Layout::Group(Group::default());
    session.active_group = 1;
    assert!(session.to_text().is_err(), "no second group");
}

#[test]
fn a_selection_past_the_limit_is_stored_with_its_primary() {
    let scratch = Scratch::new("cursors");
    let folder = scratch.folder("repo");
    let others = (0..3000).map(|at| Cursor::caret(at * 2));
    let selection = Selection::new(Cursor::caret(5999), others);
    let mut session = Session::new(&folder);
    session.layout = Layout::Group(Group {
        editors: vec![editor(&folder, "a.rs", selection, Scroll::default())],
        active: 0,
    });
    session.save(&scratch.data()).unwrap();
    let loaded = Session::load(&scratch.data(), &folder)
        .unwrap()
        .expect("a session");
    let restored = &loaded.groups()[0].editors[0].selection;
    assert_eq!(restored.cursors().len(), MAX_CURSORS);
    assert_eq!(restored.primary(), Cursor::caret(5999));
}

#[test]
fn a_selection_is_restored_only_where_it_fits_the_file() {
    let state = EditorState {
        path: PathBuf::from("/w/a.txt"),
        selection: Selection::new(Cursor::new(1, 3), [Cursor::caret(5)]),
        scroll: Scroll::default(),
    };
    let fits = Rope::from("abcdef");
    assert_eq!(state.selection_in(&fits), state.selection);
    // Shorter now, or an offset inside a character.
    for text in ["abc", "ééé"] {
        assert_eq!(
            state.selection_in(&Rope::from(text)),
            Selection::caret(0),
            "{text}"
        );
    }
}

#[test]
fn messages_hold_no_unescaped_path() {
    let error = WorkspaceError::OtherFolder {
        path: PathBuf::from("/data/sessions/x.toml"),
        folder: PathBuf::from("/w/\u{202e}evil"),
    };
    assert!(error.to_string().contains(r"\u{202e}"), "{error}");
}
