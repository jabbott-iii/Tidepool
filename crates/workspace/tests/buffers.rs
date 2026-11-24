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

//! `workspace::buffers`, headless, with real files: opening, editing and
//! saving round-trip, jobs run on other threads, one buffer a file however
//! it is spelled, large and binary files open read-only, a save over a
//! file with a second hard link stops for the answer and each answer does
//! what it says, an answer for a file the path no longer reaches saves as
//! a checked save, and stale results are dropped (M4.11, M4.12, ADR-0068,
//! ADR-0069, ADR-0070).

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

use tidepool_text::{Group, Place, Selection, Transaction};
use tidepool_workspace::WorkspaceError;
use tidepool_workspace::buffers::{
    BufferId, Buffers, LARGE_FILE_BYTES, ReadOnly, SaveMode, SaveQuestion, SaveReport,
};

/// A fresh scratch folder for one test, removed when the test passes.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("tidepool-workspace-buffers")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("make the scratch folder");
        Scratch(dir)
    }

    /// `name` in the folder, holding `bytes`.
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).expect("write the file");
        path
    }

    /// The names in the folder, sorted.
    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .expect("list")
            .map(|entry| {
                let entry = entry.expect("an entry");
                entry.file_name().to_string_lossy().into_owned()
            })
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !thread::panicking() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

/// Opens `path`, running the load on another thread, as a worker would.
fn open(buffers: &mut Buffers, path: &Path) -> Result<BufferId, WorkspaceError> {
    let job = buffers.open(path);
    let done = thread::spawn(move || job.run()).join().expect("the worker");
    buffers.finish_load(done).map(|opened| opened.id)
}

/// Saves `id` in `mode`, running the save on another thread.
fn save(buffers: &mut Buffers, id: BufferId, mode: SaveMode) -> SaveReport {
    let job = buffers.save(id, mode).expect("a save starts");
    let done = thread::spawn(move || job.run()).join().expect("the worker");
    buffers.finish_save(done)
}

/// Types `typed` at the end of the buffer's text, through its history.
fn append(buffers: &mut Buffers, id: BufferId, typed: &str) {
    let buffer = buffers.get_mut(id).expect("open");
    let (text, history) = buffer.edit().expect("editable");
    let selection = Selection::caret(text.len());
    let transaction = Transaction::change_each(text, &selection, |cursor| {
        Some((cursor.head..cursor.head, typed, Place::End))
    })
    .expect("a transaction");
    history
        .commit(text, transaction, &selection, Instant::now(), Group::New)
        .expect("a commit");
}

/// Whether `platform` reads a file's links, owner and group on this
/// system (Linux now; macOS from M11.8, Windows from M12.2), without which
/// every save over a file asks.
fn checks_here(path: &Path) -> bool {
    tidepool_platform::file::metadata(path).is_ok()
}

/// A mode that saves `id` atomically without a question: a checked save
/// where `platform` can tell that an ordinary file needs none, else the
/// answer "save as a new file" to the question a checked save asks, which
/// knows the file no better.
fn atomic(buffers: &mut Buffers, id: BufferId, path: &Path) -> SaveMode {
    if checks_here(path) {
        return SaveMode::Check;
    }
    match save(buffers, id, SaveMode::Check) {
        SaveReport::Asks { question, .. } => question.save_as_new_file(),
        report => panic!("where nothing can be told, a save asks: {report:?}"),
    }
}

/// The question in `report`, which must ask.
fn asks(report: SaveReport) -> SaveQuestion {
    match report {
        SaveReport::Asks { question, .. } => question,
        other => panic!("the save must ask: {other:?}"),
    }
}

/// Says on standard error, as the gate counts it, that `test` skipped.
fn skipped(test: &str, reason: &str) {
    let _ = writeln!(io::stderr(), "SKIPPED: {test}: {reason}");
}

#[test]
fn opening_editing_and_saving_round_trips() {
    let scratch = Scratch::new("round-trip");
    let bytes = "\u{feff}fn main() {\r\n    println!(\"é\");\r\n}".as_bytes();
    let path = scratch.file("main.rs", bytes);
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    let buffer = buffers.get(id).expect("open");
    assert_eq!(buffer.path(), canonical(&path));
    assert_eq!(
        buffer.text().to_string(),
        "fn main() {\r\n    println!(\"é\");\r\n}"
    );
    assert_eq!(buffer.line_endings().crlf, 2);
    assert_eq!(buffer.trailing_break(), None);
    assert_eq!(buffer.read_only(), None);
    assert!(!buffer.is_modified());
    assert_eq!(buffer.version().get(), 0);

    // An unchanged buffer saves byte for byte, BOM and all.
    let report = save(&mut buffers, id, SaveMode::Check);
    assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
    assert_eq!(fs::read(&path).expect("read"), bytes);

    append(&mut buffers, id, "\r\n// more");
    let buffer = buffers.get(id).expect("open");
    assert!(buffer.is_modified());
    assert_eq!(buffer.version().get(), 1);
    let report = save(&mut buffers, id, SaveMode::Check);
    let SaveReport::Saved { version, .. } = report else {
        panic!("saved: {report:?}");
    };
    assert_eq!(version.get(), 1);
    assert!(!buffers.get(id).expect("open").is_modified());
    assert_eq!(scratch.names(), ["main.rs"], "no temporary file is left");

    // A second Tidepool reads the same text back.
    let mut again = Buffers::new();
    let reopened = open(&mut again, &path).expect("open");
    let text = again.get(reopened).expect("open").text().to_string();
    assert_eq!(text, "fn main() {\r\n    println!(\"é\");\r\n}\r\n// more");
    assert!(fs::read(&path).expect("read").starts_with(b"\xef\xbb\xbf"));
}

/// The canonical path a buffer is keyed by.
fn canonical(path: &Path) -> PathBuf {
    tidepool_core::path::canonicalize(path).expect("canonical")
}

#[test]
fn a_file_is_one_buffer_however_it_is_spelled() {
    let scratch = Scratch::new("one-buffer");
    let path = scratch.file("a.txt", b"x");
    fs::create_dir(scratch.0.join("sub")).expect("mkdir");
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    let other = scratch.0.join("sub").join("..").join("a.txt");
    let job = buffers.open(&other);
    let opened = buffers.finish_load(job.run()).expect("open");
    assert_eq!(opened.id, id);
    assert!(opened.already_open);
    assert_eq!(buffers.len(), 1);
    assert_eq!(buffers.find(&canonical(&path)), Some(id));

    // A buffer closed and opened again has a new id.
    let closed = buffers.close(id).expect("was open");
    assert_eq!(closed.id(), id);
    assert!(buffers.is_empty());
    let id_again = open(&mut buffers, &path).expect("open");
    assert_ne!(id_again, id);
}

#[test]
fn a_missing_file_does_not_open() {
    let scratch = Scratch::new("missing");
    let mut buffers = Buffers::new();
    let missing = scratch.0.join("nope.txt");
    let error = open(&mut buffers, &missing).expect_err("missing");
    assert!(matches!(error, WorkspaceError::Path(_)), "{error:?}");
    let error = open(&mut buffers, &scratch.0).expect_err("a folder");
    assert!(matches!(error, WorkspaceError::Load { .. }), "{error:?}");
    assert!(buffers.is_empty());
}

#[test]
fn a_large_file_opens_read_only_until_made_editable() {
    let scratch = Scratch::new("large");
    let line = b"0123456789abcdef0123456789abcde\n";
    let mut bytes = line.repeat(LARGE_FILE_BYTES as usize / line.len());
    bytes.push(b'!');
    let path = scratch.file("big.log", &bytes);
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    let buffer = buffers.get_mut(id).expect("open");
    assert_eq!(buffer.read_only(), Some(ReadOnly::Large));
    assert_eq!(buffer.file_len(), LARGE_FILE_BYTES + 1);
    assert!(matches!(
        buffer.edit(),
        Err(WorkspaceError::ReadOnlyBuffer {
            why: ReadOnly::Large,
            ..
        })
    ));
    assert_eq!(buffer.version().get(), 0, "a refused edit is no version");
    assert!(matches!(
        buffers.save(id, SaveMode::Check),
        Err(WorkspaceError::ReadOnlyBuffer { .. })
    ));

    buffers
        .get_mut(id)
        .expect("open")
        .make_editable()
        .expect("editable");
    assert_eq!(buffers.get(id).expect("open").read_only(), None);
    append(&mut buffers, id, "?");
    let report = save(&mut buffers, id, SaveMode::Check);
    assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
    let saved = fs::read(&path).expect("read");
    assert_eq!(saved.len(), bytes.len() + 1);
    assert!(saved.ends_with(b"!?"));
}

#[test]
fn a_file_that_is_not_text_stays_read_only() {
    let scratch = Scratch::new("not-text");
    let path = scratch.file("blob.bin", b"ok \xff\xfe not utf-8");
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    let buffer = buffers.get_mut(id).expect("open");
    assert_eq!(buffer.read_only(), Some(ReadOnly::NotText));
    assert_eq!(buffer.format(), None);
    assert!(matches!(
        buffer.make_editable(),
        Err(WorkspaceError::ReadOnlyBuffer {
            why: ReadOnly::NotText,
            ..
        })
    ));
    assert!(buffer.edit().is_err());
    let error = buffers
        .save(id, SaveMode::Check)
        .expect_err("never written");
    assert!(error.to_string().contains("not text"), "{error}");
    assert_eq!(fs::read(&path).expect("read"), b"ok \xff\xfe not utf-8");
}

/// ADR-0068 and ADR-0069: a save over a file with a second name stops and
/// asks, writing nothing; "write in place" keeps the link, and "save as a
/// new file" splits it.
#[test]
fn a_second_hard_link_stops_the_save_for_the_answer() {
    let scratch = Scratch::new("hard-link");
    let path = scratch.file("one.txt", b"old");
    let other = scratch.0.join("two.txt");
    fs::hard_link(&path, &other).expect("a second name");
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    append(&mut buffers, id, " and new");

    let report = save(&mut buffers, id, SaveMode::Check);
    let SaveReport::Asks {
        id: asked,
        question,
    } = report
    else {
        panic!("the save must ask: {report:?}");
    };
    assert_eq!(asked, id);
    if checks_here(&path) {
        assert!(question.splits_link());
        assert_eq!(question.links(), Some(2));
        assert_eq!(question.changes_user(), Some(false));
        assert_eq!(question.changes_group(), Some(false));
    } else {
        assert_eq!(question.links(), None, "unknown here, so it asks");
    }
    assert_eq!(question.target(), canonical(&path));
    assert_eq!(fs::read(&path).expect("read"), b"old");
    assert_eq!(fs::read(&other).expect("read"), b"old");
    assert_eq!(
        scratch.names(),
        ["one.txt", "two.txt"],
        "nothing left behind"
    );
    let buffer = buffers.get(id).expect("open");
    assert!(buffer.is_modified());
    assert!(!buffer.is_saving(), "an answer may start the next save");

    // Write in place: both names hold the new text.
    let report = save(&mut buffers, id, question.write_in_place());
    assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
    assert_eq!(fs::read(&path).expect("read"), b"old and new");
    assert_eq!(fs::read(&other).expect("read"), b"old and new");
    assert!(!buffers.get(id).expect("open").is_modified());

    // Save as a new file: the other name keeps the text it had. The file
    // still has two names, so the save asks again first.
    append(&mut buffers, id, ", newer");
    let report = save(&mut buffers, id, SaveMode::Check);
    let SaveReport::Asks { question, .. } = report else {
        panic!("the save must ask again: {report:?}");
    };
    let report = save(&mut buffers, id, question.save_as_new_file());
    assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
    assert_eq!(fs::read(&path).expect("read"), b"old and new, newer");
    assert_eq!(fs::read(&other).expect("read"), b"old and new");
    assert_eq!(scratch.names(), ["one.txt", "two.txt"]);

    // Now one name each, so the default save asks nothing where it can
    // tell.
    append(&mut buffers, id, "!");
    let report = save(&mut buffers, id, SaveMode::Check);
    if checks_here(&path) {
        assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
        assert_eq!(fs::read(&path).expect("read"), b"old and new, newer!");
    } else {
        assert!(matches!(report, SaveReport::Asks { .. }), "{report:?}");
    }
}

/// M4.12, ADR-0070: the path was renamed over while the user decided, by
/// a file with two names of its own, and again after the next question.
/// Neither answer writes into the file there: each saves as a checked
/// save, which asks again, about that file; the answer to the last
/// question writes in place into it.
#[test]
fn an_answer_for_a_file_renamed_over_asks_again() {
    let test = "an_answer_for_a_file_renamed_over_asks_again";
    let scratch = Scratch::new("renamed-over");
    let path = scratch.file("one.txt", b"old");
    fs::hard_link(&path, scratch.0.join("two.txt")).expect("a second name");
    if !checks_here(&path) {
        skipped(test, "`platform` reads no file identity on this system");
        return;
    }
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    append(&mut buffers, id, " and new");
    let first = asks(save(&mut buffers, id, SaveMode::Check));

    // Another program saves its own file, which has a second name too.
    let theirs = scratch.file("three.txt", b"theirs");
    fs::hard_link(&theirs, scratch.0.join("four.txt")).expect("its second name");
    fs::rename(&theirs, &path).expect("renamed over");
    let second = asks(save(&mut buffers, id, first.write_in_place()));
    assert_ne!(second.file(), first.file(), "about the file there now");
    assert!(second.splits_link());
    assert_eq!(fs::read(&path).expect("read"), b"theirs");
    assert_eq!(
        fs::read(scratch.0.join("four.txt")).expect("read"),
        b"theirs"
    );

    // And again, before the user answers the second question.
    let later = scratch.file("five.txt", b"later");
    fs::hard_link(&later, scratch.0.join("six.txt")).expect("its second name");
    fs::rename(&later, &path).expect("renamed over");
    let third = asks(save(&mut buffers, id, second.save_as_new_file()));
    assert_ne!(third.file(), second.file());
    assert_eq!(fs::read(&path).expect("read"), b"later");
    assert_eq!(fs::read(scratch.0.join("six.txt")).expect("read"), b"later");
    assert_eq!(
        fs::read(scratch.0.join("four.txt")).expect("read"),
        b"theirs"
    );
    assert_eq!(fs::read(scratch.0.join("two.txt")).expect("read"), b"old");
    assert_eq!(
        scratch.names(),
        ["four.txt", "one.txt", "six.txt", "two.txt"],
        "no temporary file left"
    );

    // The answer to the last question is for the file there now.
    let report = save(&mut buffers, id, third.write_in_place());
    assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
    assert_eq!(fs::read(&path).expect("read"), b"old and new");
    assert_eq!(
        fs::read(scratch.0.join("six.txt")).expect("read"),
        b"old and new"
    );
    assert_eq!(
        fs::read(scratch.0.join("four.txt")).expect("read"),
        b"theirs"
    );
    assert_eq!(fs::read(scratch.0.join("two.txt")).expect("read"), b"old");
}

/// M4.12, ADR-0070: the file was deleted, both its names, and made again
/// while the user decided, so the new file may get the old one's inode
/// number, which ext4 gives it. "Write in place" writes nothing into the
/// new file: the save runs as a checked save, which renames its own file
/// over the path, as for any ordinary file.
#[test]
fn an_answer_for_a_file_deleted_and_made_again_saves_as_a_checked_save() {
    let test = "an_answer_for_a_file_deleted_and_made_again_saves_as_a_checked_save";
    let scratch = Scratch::new("made-again");
    let path = scratch.file("one.txt", b"old");
    fs::hard_link(&path, scratch.0.join("two.txt")).expect("a second name");
    if !checks_here(&path) {
        skipped(test, "`platform` reads no file identity on this system");
        return;
    }
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    append(&mut buffers, id, " and new");
    let question = asks(save(&mut buffers, id, SaveMode::Check));

    fs::remove_file(&path).expect("delete");
    fs::remove_file(scratch.0.join("two.txt")).expect("delete its other name");
    fs::write(&path, b"theirs").expect("made again");
    let made = tidepool_platform::file::metadata(&path).expect("read").id;
    if Some(made) == question.file() {
        skipped(
            test,
            "this file system gave the new file the old one's identity, so the two \
             cannot be told apart",
        );
        return;
    }
    // A handle on the new file, which a rename over the path leaves as it
    // is, and a write in place would not.
    let mut handle = fs::File::open(&path).expect("open theirs");

    let report = save(&mut buffers, id, question.write_in_place());
    assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
    assert_eq!(fs::read(&path).expect("read"), b"old and new");
    let mut kept = Vec::new();
    io::Read::read_to_end(&mut handle, &mut kept).expect("read theirs");
    assert_eq!(kept, b"theirs", "nothing written into the new file");
    assert_eq!(scratch.names(), ["one.txt"]);
}

/// M4.12: "write in place" for a file deleted while the user decided, and
/// not made again, saves as a checked save of a deleted file does: it
/// makes the file anew, with a new file's permissions, and the other name
/// keeps the old text.
#[test]
fn an_answer_for_a_deleted_file_makes_it_again() {
    let test = "an_answer_for_a_deleted_file_makes_it_again";
    let scratch = Scratch::new("deleted-answer");
    let path = scratch.file("one.txt", b"old");
    fs::hard_link(&path, scratch.0.join("two.txt")).expect("a second name");
    if !checks_here(&path) {
        skipped(test, "`platform` reads no file identity on this system");
        return;
    }
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    append(&mut buffers, id, " and new");
    let question = asks(save(&mut buffers, id, SaveMode::Check));
    fs::remove_file(&path).expect("delete");

    let report = save(&mut buffers, id, question.write_in_place());
    assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
    assert_eq!(fs::read(&path).expect("read"), b"old and new");
    assert_eq!(fs::read(scratch.0.join("two.txt")).expect("read"), b"old");
}

/// M4.12: "save as a new file" when the path now reaches another program's
/// ordinary file, with one name and this user's owner and group: the
/// question asked again finds nothing to ask, so the save renames, as a
/// checked save would, and the replaced file keeps its bytes.
#[test]
fn a_new_file_answer_over_an_ordinary_file_renames_as_a_checked_save() {
    let test = "a_new_file_answer_over_an_ordinary_file_renames_as_a_checked_save";
    let scratch = Scratch::new("new-file-ordinary");
    let path = scratch.file("one.txt", b"old");
    fs::hard_link(&path, scratch.0.join("two.txt")).expect("a second name");
    if !checks_here(&path) {
        skipped(test, "`platform` reads no file identity on this system");
        return;
    }
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    append(&mut buffers, id, " and new");
    let question = asks(save(&mut buffers, id, SaveMode::Check));

    let theirs = scratch.file("three.txt", b"theirs");
    fs::rename(&theirs, &path).expect("renamed over");
    let mut handle = fs::File::open(&path).expect("open theirs");
    let report = save(&mut buffers, id, question.save_as_new_file());
    assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
    assert_eq!(fs::read(&path).expect("read"), b"old and new");
    let mut kept = Vec::new();
    io::Read::read_to_end(&mut handle, &mut kept).expect("read theirs");
    assert_eq!(kept, b"theirs");
    assert_eq!(fs::read(scratch.0.join("two.txt")).expect("read"), b"old");
    assert_eq!(scratch.names(), ["one.txt", "two.txt"]);
}

/// A file of this user's, with one name, in a folder of this user's, saves
/// without a question where `platform` can tell; elsewhere every save over
/// a file asks.
#[test]
fn an_ordinary_file_saves_without_a_question_where_it_can_be_told() {
    let scratch = Scratch::new("ordinary");
    let path = scratch.file("plain.txt", b"a");
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    append(&mut buffers, id, "b");
    let report = save(&mut buffers, id, SaveMode::Check);
    if checks_here(&path) {
        assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
        assert_eq!(fs::read(&path).expect("read"), b"ab");
    } else {
        let _ = writeln!(
            io::stderr(),
            "SKIPPED: an_ordinary_file_saves_without_a_question_where_it_can_be_told: \
             `platform` reads no file metadata on this system, so the save asks"
        );
        assert!(matches!(report, SaveReport::Asks { .. }), "{report:?}");
    }
}

/// A file deleted after it was opened is saved again as a new file, with
/// nothing to compare, so no question.
#[test]
fn a_file_deleted_since_it_opened_is_made_again() {
    let scratch = Scratch::new("deleted");
    let path = scratch.file("gone.txt", b"text");
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    fs::remove_file(&path).expect("delete");
    let report = save(&mut buffers, id, SaveMode::Check);
    assert!(matches!(report, SaveReport::Saved { .. }), "{report:?}");
    assert_eq!(fs::read(&path).expect("read"), b"text");
}

#[test]
fn saves_are_serialized_and_stale_results_dropped() {
    let scratch = Scratch::new("stale");
    let path = scratch.file("s.txt", b"one");
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    append(&mut buffers, id, " two");

    let mode = atomic(&mut buffers, id, &path);
    let first = buffers.save(id, mode).expect("a save");
    assert_eq!(first.version().get(), 1);
    assert!(buffers.get(id).expect("open").is_saving());
    assert!(matches!(
        buffers.save(id, SaveMode::Check),
        Err(WorkspaceError::SaveInFlight { .. })
    ));

    // Typing while the save runs: the save writes the text it started with,
    // and the buffer stays modified.
    append(&mut buffers, id, " three");
    assert!(!buffers.is_current(id, first.version()));
    let report = buffers.finish_save(first.run());
    let SaveReport::Saved { version, .. } = report else {
        panic!("saved: {report:?}");
    };
    assert_eq!(version.get(), 1);
    assert_eq!(fs::read(&path).expect("read"), b"one two");
    let buffer = buffers.get(id).expect("open");
    assert!(buffer.is_modified());
    assert!(buffers.is_current(id, buffer.version()));

    // A result for a buffer closed meanwhile is stale.
    let job = buffers.save(id, SaveMode::Check).expect("a save");
    let done = job.run();
    buffers.close(id).expect("open");
    assert!(matches!(buffers.finish_save(done), SaveReport::Stale));
    assert!(matches!(
        buffers.save(id, SaveMode::Check),
        Err(WorkspaceError::NoBuffer { .. })
    ));
}

#[test]
fn a_failed_save_leaves_the_buffer_modified() {
    let scratch = Scratch::new("failed");
    fs::create_dir(scratch.0.join("dir")).expect("mkdir");
    let path = scratch.file("dir/f.txt", b"kept");
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    append(&mut buffers, id, "!");
    fs::remove_dir_all(scratch.0.join("dir")).expect("remove the folder");
    let report = save(&mut buffers, id, SaveMode::Check);
    let SaveReport::Failed { error, .. } = report else {
        panic!("failed: {report:?}");
    };
    assert!(matches!(error, WorkspaceError::Save { .. }), "{error:?}");
    let buffer = buffers.get(id).expect("open");
    assert!(buffer.is_modified() && !buffer.is_saving());
}

#[test]
fn debug_output_never_shows_the_text() {
    let scratch = Scratch::new("debug");
    let path = scratch.file("notes.txt", b"words kept out of the output");
    let mut buffers = Buffers::new();
    let id = open(&mut buffers, &path).expect("open");
    let job = buffers.save(id, SaveMode::Check).expect("a save");
    for shown in [
        format!("{:?}", buffers.get(id).expect("open")),
        format!("{job:?}"),
        format!("{buffers:?}"),
    ] {
        assert!(!shown.contains("words kept"), "{shown}");
    }
    drop(job);
}
