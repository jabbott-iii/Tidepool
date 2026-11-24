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

//! A file's metadata and a new file's owner on Linux, for
//! [`file`](crate::file) (M4.10, ADR-0068).
//!
//! New in Tidepool. `std` reads the metadata (`stat(2)`, following links)
//! and gives the link count, owner, group and mode through
//! `std::os::unix::fs::MetadataExt`; `tidepool-sys` gives the process's
//! effective user and group, which `std` does not. The rule for a new
//! file's group is [`creator`], tested on its own.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use tidepool_sys::linux::libc::{ALLPERMS, S_ISGID, getegid, geteuid};

use crate::Error;
use crate::file::{Metadata, Owner};

/// Reads `path`'s metadata, following links.
fn stat(path: &Path) -> Result<fs::Metadata, Error> {
    fs::metadata(path).map_err(|source| Error::Io {
        call: "stat",
        source,
    })
}

/// The link count, owner, group and permission bits of the file at `path`.
pub(crate) fn file_metadata(path: &Path) -> Result<Metadata, Error> {
    let found = stat(path)?;
    Ok(Metadata {
        links: found.nlink(),
        owner: Owner {
            user: found.uid(),
            group: found.gid(),
        },
        permissions: found.mode() & ALLPERMS,
    })
}

/// The owner and group a file this process makes in `folder` gets.
pub(crate) fn new_file_owner(folder: &Path) -> Result<Owner, Error> {
    let found = stat(folder)?;
    if !found.is_dir() {
        return Err(Error::Io {
            call: "stat",
            source: io::Error::from(io::ErrorKind::NotADirectory),
        });
    }
    let process = Owner {
        user: geteuid(),
        group: getegid(),
    };
    Ok(creator(process, found.gid(), found.mode()))
}

/// The owner and group of a file that `process` (its effective user and
/// group) makes in a folder of group `folder_group` and mode `folder_mode`:
/// the process's user, and the folder's group if the folder has the setgid
/// bit, else the process's group, as open(2) of Linux man-pages 6.7 says:
/// "if that bit is set, then BSD semantics apply; otherwise, System V
/// semantics apply". Its caveat for the `bsdgroups` mount option is in
/// [`file::new_file_owner`](crate::file::new_file_owner).
fn creator(process: Owner, folder_group: u32, folder_mode: u32) -> Owner {
    let group = if folder_mode & S_ISGID == 0 {
        process.group
    } else {
        folder_group
    };
    Owner {
        user: process.user,
        group,
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::ops::Deref;
    use std::os::unix::fs::{PermissionsExt, chown, symlink};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    /// A test's scratch folder, made anew under the system's temporary
    /// folder with a name no other run has, and removed when the test
    /// passes; a failing test leaves it behind.
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            if !std::thread::panicking() {
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

    fn scratch(test: &str) -> Scratch {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970");
        let dir = std::env::temp_dir().join(format!(
            "tidepool-platform-file-{test}-{}-{}",
            std::process::id(),
            since.as_nanos()
        ));
        fs::create_dir(&dir).expect("create a fresh test folder");
        Scratch(dir)
    }

    fn write(path: &Path, text: &str) {
        File::create(path)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .expect("write a test file");
    }

    fn this_process() -> Owner {
        Owner {
            user: geteuid(),
            group: getegid(),
        }
    }

    /// Says on standard error, as the gate counts it, that `test` skipped.
    fn skipped(test: &str, reason: &str) {
        let _ = writeln!(io::stderr(), "SKIPPED: {test}: {reason}");
    }

    #[test]
    fn a_second_hard_link_is_counted_on_both_names() {
        let dir = scratch("links");
        let first = dir.join("first.txt");
        write(&first, "text\n");
        assert_eq!(file_metadata(&first).expect("read").links, 1);

        let second = dir.join("second.txt");
        fs::hard_link(&first, &second).expect("make a second name");
        assert_eq!(file_metadata(&first).expect("read").links, 2);
        assert_eq!(file_metadata(&second).expect("read").links, 2);

        fs::remove_file(&second).expect("remove the second name");
        assert_eq!(file_metadata(&first).expect("read").links, 1);
    }

    #[test]
    fn a_new_file_is_the_process_s_and_new_file_owner_says_so() {
        let dir = scratch("owner");
        let file = dir.join("mine.txt");
        write(&file, "text\n");
        let found = file_metadata(&file).expect("read");
        assert_eq!(found.owner.user, geteuid());
        assert_eq!(found.owner, new_file_owner(&dir).expect("the folder's"));
    }

    #[test]
    fn the_permission_bits_are_the_mode_s_without_its_type() {
        let dir = scratch("permissions");
        let file = dir.join("script.sh");
        write(&file, "#!/bin/sh\n");
        for mode in [0o600, 0o644, 0o755, 0o4755, 0o1777] {
            fs::set_permissions(&file, fs::Permissions::from_mode(mode)).expect("chmod");
            let found = file_metadata(&file).expect("read");
            assert_eq!(found.permissions, mode, "{mode:#o}");
        }
        fs::set_permissions(&*dir, fs::Permissions::from_mode(0o2750)).expect("chmod the folder");
        assert_eq!(file_metadata(&dir).expect("read").permissions, 0o2750);
    }

    #[test]
    fn links_are_followed_to_the_file_they_name() {
        let dir = scratch("symlinks");
        let target = dir.join("target.txt");
        write(&target, "text\n");
        fs::hard_link(&target, dir.join("other-name.txt")).expect("make a second name");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).expect("chmod");
        let link = dir.join("link.txt");
        symlink("target.txt", &link).expect("make a link");
        let chain = dir.join("chain.txt");
        symlink(&link, &chain).expect("make a link to the link");
        let through = file_metadata(&chain).expect("read through two links");
        assert_eq!(through, file_metadata(&target).expect("read the target"));
        assert_eq!(through.links, 2, "the target's, not the link's");
        assert_eq!(
            through.permissions, 0o640,
            "the target's, not the link's 0o777"
        );

        // A folder reached through a link answers for the folder.
        let folder_link = dir.join("here");
        symlink(&*dir, &folder_link).expect("link to the folder");
        assert_eq!(
            new_file_owner(&folder_link).expect("through the link"),
            new_file_owner(&dir).expect("the folder")
        );
    }

    #[test]
    fn a_missing_file_a_dangling_link_and_a_loop_are_errors_at_stat() {
        let dir = scratch("missing");
        symlink("nowhere", dir.join("dangling")).expect("make a dangling link");
        symlink("loop-b", dir.join("loop-a")).expect("make a loop");
        symlink("loop-a", dir.join("loop-b")).expect("close the loop");
        // `io::ErrorKind::FilesystemLoop` is not stable, so the loop's kind
        // is not checked.
        for (name, kind) in [
            ("absent.txt", Some(io::ErrorKind::NotFound)),
            ("dangling", Some(io::ErrorKind::NotFound)),
            ("loop-a", None),
        ] {
            let path = dir.join(name);
            match file_metadata(&path) {
                Err(Error::Io {
                    call: "stat",
                    source,
                }) => {
                    if let Some(kind) = kind {
                        assert_eq!(source.kind(), kind, "{name}: {source}");
                    }
                }
                other => panic!("{name}: {other:?}"),
            }
            assert!(
                matches!(new_file_owner(&path), Err(Error::Io { call: "stat", .. })),
                "{name}"
            );
        }
    }

    #[test]
    fn a_file_is_not_a_folder_to_make_files_in() {
        let dir = scratch("not-a-folder");
        let file = dir.join("plain.txt");
        write(&file, "text\n");
        match new_file_owner(&file) {
            Err(Error::Io {
                call: "stat",
                source,
            }) => {
                assert_eq!(source.kind(), io::ErrorKind::NotADirectory, "{source}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_setgid_bit_gives_the_folder_s_group_and_its_absence_the_process_s() {
        let process = Owner {
            user: 1000,
            group: 1000,
        };
        let without = creator(process, 4, 0o40755);
        assert_eq!(
            without,
            Owner {
                user: 1000,
                group: 1000
            }
        );
        let with = creator(process, 4, 0o42775);
        assert_eq!(
            with,
            Owner {
                user: 1000,
                group: 4
            }
        );
        // Only the setgid bit counts: setuid and sticky do not.
        assert_eq!(creator(process, 4, 0o47777), with);
        assert_eq!(creator(process, 4, 0o45777), without);
        assert_eq!(creator(process, 4, 0o41777), without);
        // Root's files are root's, whatever the folder.
        let root = Owner { user: 0, group: 0 };
        assert_eq!(creator(root, 4, 0o42775), Owner { user: 0, group: 4 });
    }

    /// The process's supplementary groups, from `/proc/self/status`, or
    /// `None` without `/proc`.
    fn groups() -> Option<Vec<u32>> {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        let line = status
            .lines()
            .find_map(|line| line.strip_prefix("Groups:"))?;
        line.split_whitespace().map(|id| id.parse().ok()).collect()
    }

    /// The overflow group, which a group that this user namespace does not
    /// map reads as: `/proc/sys/kernel/overflowgid`, or its default, 65534
    /// (user_namespaces(7)).
    fn overflow_group() -> u32 {
        fs::read_to_string("/proc/sys/kernel/overflowgid")
            .ok()
            .and_then(|id| id.trim().parse().ok())
            .unwrap_or(65534)
    }

    #[test]
    fn a_setgid_folder_of_another_group_gives_new_files_that_group() {
        let test = "a_setgid_folder_of_another_group_gives_new_files_that_group";
        let overflow = overflow_group();
        let Some(other) = groups()
            .unwrap_or_default()
            .into_iter()
            .find(|&group| group != getegid() && group != overflow)
        else {
            skipped(
                test,
                "the process is in no mapped group but its effective one, so no folder can be \
                 given another",
            );
            return;
        };
        let dir = scratch("setgid");
        // The owner of a folder may give it any group the owner is in,
        // unless something (a sandbox's policy, say) refuses it.
        if let Err(error) = chown(&*dir, None, Some(other)) {
            skipped(
                test,
                &format!("the folder cannot be given group {other}: {error}"),
            );
            return;
        }
        let plain = dir.join("plain");
        fs::create_dir(&plain).expect("make a folder without setgid");
        chown(&plain, None, Some(other)).expect("give it the other group too");
        fs::set_permissions(&plain, fs::Permissions::from_mode(0o755)).expect("chmod");
        fs::set_permissions(&*dir, fs::Permissions::from_mode(0o2755)).expect("set setgid");

        let in_setgid = new_file_owner(&dir).expect("the setgid folder's");
        assert_eq!(
            in_setgid,
            Owner {
                user: geteuid(),
                group: other
            }
        );
        let made = dir.join("made.txt");
        write(&made, "text\n");
        assert_eq!(file_metadata(&made).expect("read").owner, in_setgid);

        let in_plain = new_file_owner(&plain).expect("the plain folder's");
        assert_eq!(in_plain, this_process());
        let made = plain.join("made.txt");
        write(&made, "text\n");
        assert_eq!(file_metadata(&made).expect("read").owner, in_plain);
    }
}
