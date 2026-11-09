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

//! Unit tests of `core::path`: the display, `to_utf8`, dropping the
//! verbatim prefix, `under` and the errors, and a file whose name is not
//! Unicode.
//!
//! The only `cfg(unix)` and `cfg(windows)` in `tidepool-core` are here
//! (ADR-0032). They build paths that are not UTF-8, which needs
//! `OsStrExt::from_bytes` on Unix and `OsStringExt::from_wide` with an
//! unpaired surrogate on Windows. Tests of Windows' prefixes ask at run
//! time whether `std` parses them instead (ADR-0033). The canonical forms
//! of real files are tested in `tests/path.rs`.

use std::ffi::OsStr;
use std::io::Write as _;
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;
use crate::testing::check;

/// A directory of its own for one test, under the repository's `target/`,
/// removed when dropped.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(test: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/core-unit-tmp")
            .join(format!(
                "{}-{}-path-{test}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        // It may be left over from an earlier run that was killed.
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Says on standard error, as the gate counts it, that `test` skipped.
fn skipped(test: &str, reason: &str) {
    let _ = writeln!(io::stderr(), "SKIPPED: {test}: {reason}");
}

/// `bytes` as the display writes them.
fn escaped(bytes: &[u8]) -> String {
    let mut out = String::new();
    write_escaped(bytes, &mut out).expect("writing to a String cannot fail");
    out
}

/// ADR-0033's escaped set, written out from the ADR rather than from
/// `char::is_control`: C0, DEL and C1, the bidirectional controls, and the
/// line and paragraph separators.
fn escaped_set() -> Vec<char> {
    let ranges: [RangeInclusive<u32>; 7] = [
        0x00..=0x1f,
        0x7f..=0x9f,
        0x061c..=0x061c,
        0x200e..=0x200f,
        0x202a..=0x202e,
        0x2066..=0x2069,
        0x2028..=0x2029,
    ];
    let mut set: Vec<char> = ranges
        .into_iter()
        .flatten()
        .filter_map(char::from_u32)
        .collect();
    set.sort_unstable();
    set
}

/// Whether `std` parses Windows' prefixes here: asked at run time, not
/// through `cfg` (ADR-0033).
fn windows_paths() -> bool {
    matches!(
        Path::new(r"C:\").components().next(),
        Some(Component::Prefix(_))
    )
}

#[test]
fn a_utf8_path_without_controls_displays_unchanged() {
    for path in [
        "/home/me/café/日本.rs",
        r"C:\Users\me",
        r"a\x41b",
        r#"say "hi" and 'bye'"#,
        "",
    ] {
        assert_eq!(display(path).to_string(), path);
        assert_eq!(escaped(path.as_bytes()), path);
    }
}

#[test]
fn control_and_bidirectional_characters_are_escaped_as_the_log_escapes_them() {
    let set = escaped_set();
    assert_eq!(set.len(), 65 + 1 + 2 + 5 + 4 + 2);
    // Exactly these, of every character there is.
    let found: Vec<char> = (char::MIN..=char::MAX).filter(|&c| is_escaped(c)).collect();
    assert_eq!(found, set);
    for &c in &set {
        assert_eq!(
            display(&format!("a{c}b")).to_string(),
            format!("a\\u{{{:x}}}b", u32::from(c)),
            "U+{:04X}",
            u32::from(c)
        );
    }
    assert_eq!(
        display("a\tb\nc\rd\0e\u{1b}[31m\u{7f}").to_string(),
        r"a\u{9}b\u{a}c\u{d}d\u{0}e\u{1b}[31m\u{7f}"
    );
    assert_eq!(
        display("\u{85}\u{9b}\u{9f}").to_string(),
        r"\u{85}\u{9b}\u{9f}"
    );
    assert_eq!(
        display("invoice\u{202e}txt.exe").to_string(),
        r"invoice\u{202e}txt.exe"
    );
    assert_eq!(
        display("a\u{2028}b\u{2029}\u{2066}c\u{2069}").to_string(),
        r"a\u{2028}b\u{2029}\u{2066}c\u{2069}"
    );
    // Kept: the no-break space, the soft hyphen, the zero-width space and
    // joiner, the byte order mark, and the neighbours of the escaped ranges.
    for kept in [
        ' ', '~', '\u{a0}', '\u{ad}', '\u{61b}', '\u{61d}', '\u{200b}', '\u{200d}', '\u{2010}',
        '\u{2027}', '\u{202f}', '\u{2065}', '\u{206a}', '\u{feff}',
    ] {
        let text = format!("a{kept}b");
        assert_eq!(
            display(&text).to_string(),
            text,
            "U+{:04X}",
            u32::from(kept)
        );
    }
}

#[test]
fn bytes_that_are_not_utf8_show_as_uppercase_hex() {
    let cases: [(&[u8], &str); 9] = [
        (b"caf\xe9.txt", r"caf\xE9.txt"),
        (b"\xff\xfe", r"\xFF\xFE"),
        // A sequence cut short, at the end and before more text.
        (b"\xf0\x9f\x98", r"\xF0\x9F\x98"),
        (b"\xf0\x9f\x98a", r"\xF0\x9F\x98a"),
        // A surrogate in WTF-8, and an overlong `/`.
        (b"\xed\xa0\x80", r"\xED\xA0\x80"),
        (b"\xc0\xaf", r"\xC0\xAF"),
        // A lone byte 0x9B is not UTF-8; U+009B (CSI) encoded is a control.
        (b"\x9b", r"\x9B"),
        (b"\xc2\x9b", r"\u{9b}"),
        (b"a\xe9\x1b\xf0\x9f\x98\x80", r"a\xE9\u{1b}😀"),
    ];
    for (bytes, shown) in cases {
        assert_eq!(escaped(bytes), shown, "{bytes:02X?}");
    }
}

/// Reads a display of bytes that hold no backslash back to the bytes:
/// `\xNN` (uppercase) is a byte, `\u{h}` (lowercase) a character of the
/// escaped set, and anything else itself.
fn read_back(shown: &str, set: &[char]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut rest = shown;
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix(r"\x") {
            let hex = after.get(..2).ok_or("a short `\\x` escape")?;
            if !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
            {
                return Err(format!("`\\x{hex}` is not two uppercase hex digits"));
            }
            bytes.push(u8::from_str_radix(hex, 16).map_err(|error| error.to_string())?);
            rest = after.get(2..).unwrap_or_default();
        } else if let Some(after) = rest.strip_prefix(r"\u{") {
            let (hex, after) = after.split_once('}').ok_or("an unclosed `\\u` escape")?;
            if hex.bytes().any(|b| b.is_ascii_uppercase()) {
                return Err(format!("`\\u{{{hex}}}` is not lowercase"));
            }
            let escaped = u32::from_str_radix(hex, 16)
                .ok()
                .and_then(char::from_u32)
                .filter(|c| set.contains(c))
                .ok_or_else(|| format!("`\\u{{{hex}}}` is not in the escaped set"))?;
            bytes.extend_from_slice(escaped.encode_utf8(&mut [0; 4]).as_bytes());
            rest = after;
        } else {
            bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            rest = rest.get(c.len_utf8()..).unwrap_or_default();
        }
    }
    Ok(bytes)
}

#[test]
fn the_display_hides_no_byte() {
    // Characters of each kind the display treats differently.
    const CHARS: [char; 21] = [
        'a', '/', '"', 'é', '日', '😀', '\t', '\n', '\u{1b}', '\u{7f}', '\u{85}', '\u{9b}',
        '\u{a0}', '\u{ad}', '\u{61c}', '\u{200b}', '\u{200e}', '\u{2028}', '\u{202e}', '\u{2066}',
        '\u{feff}',
    ];
    let set = escaped_set();
    check(
        "the display hides no byte",
        500,
        |rng| {
            let pieces = rng.range(0..40u32);
            let mut bytes = Vec::new();
            for _ in 0..pieces {
                match rng.range(0..3u32) {
                    // Any byte but the backslash, so every `\` shown starts
                    // an escape.
                    0 => {
                        let [byte, ..] = rng.next_u32().to_le_bytes();
                        if byte != b'\\' {
                            bytes.push(byte);
                        }
                    }
                    1 => {
                        let c = rng.choose(&CHARS).copied().unwrap_or('a');
                        bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
                    }
                    // A character cut short.
                    _ => {
                        let cut = rng.range(1..4u32) as usize;
                        bytes.extend_from_slice(&"😀".as_bytes()[..cut]);
                    }
                }
            }
            bytes
        },
        |bytes| {
            let shown = escaped(bytes);
            if let Some(c) = shown.chars().find(|c| set.contains(c)) {
                return Err(format!("{shown:?} holds U+{:04X}", u32::from(c)));
            }
            let back = read_back(&shown, &set)?;
            if back == *bytes {
                Ok(())
            } else {
                Err(format!("{shown:?} reads back as {back:02X?}"))
            }
        },
    );
}

#[test]
fn debug_is_the_display_in_quotes_and_flags_are_ignored() {
    let shown = display("notes\tdraft.md");
    assert_eq!(format!("{shown:?}"), r#""notes\u{9}draft.md""#);
    // Neither backslashes nor quotes are doubled.
    assert_eq!(
        format!("{:?}", display(r#"C:\Users\"me""#)),
        r#""C:\Users\"me"""#
    );
    let plain = shown.to_string();
    for formatted in [
        format!("{shown:>30}"),
        format!("{shown:<30}"),
        format!("{shown:^30}"),
        format!("{shown:*>30}"),
        format!("{shown:.2}"),
    ] {
        assert_eq!(formatted, plain);
    }
    assert_eq!(format!("{shown:>30?}"), format!("{shown:?}"));
}

#[cfg(unix)]
#[test]
fn a_unix_path_that_is_not_utf8_shows_its_bytes() {
    use std::os::unix::ffi::OsStrExt;

    let path = Path::new(OsStr::from_bytes(b"/tmp/caf\xe9\x1b.txt"));
    assert_eq!(display(path).to_string(), r"/tmp/caf\xE9\u{1b}.txt");
    assert_eq!(
        format!("{:?}", display(path)),
        r#""/tmp/caf\xE9\u{1b}.txt""#
    );
    // Two names a lossy display would show alike.
    let e8 = Path::new(OsStr::from_bytes(b"caf\xe8"));
    let e9 = Path::new(OsStr::from_bytes(b"caf\xe9"));
    assert_eq!(e8.to_string_lossy(), e9.to_string_lossy());
    assert_eq!(display(e8).to_string(), r"caf\xE8");
    assert_eq!(display(e9).to_string(), r"caf\xE9");
    let error = to_utf8(path).expect_err("the path is not UTF-8");
    assert_eq!(error.to_string(), r"`/tmp/caf\xE9\u{1b}.txt` is not UTF-8");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(matches!(&error, PathError::NotUtf8 { path: named } if named == path));
}

#[cfg(windows)]
#[test]
fn an_unpaired_surrogate_shows_as_its_wtf8_bytes() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let high = PathBuf::from(OsString::from_wide(&[0x61, 0xd800, 0x62]));
    assert_eq!(display(&high).to_string(), r"a\xED\xA0\x80b");
    let low = PathBuf::from(OsString::from_wide(&[0xdc00]));
    assert_eq!(display(&low).to_string(), r"\xED\xB0\x80");
    // A pair is one character.
    let pair = PathBuf::from(OsString::from_wide(&[0xd83d, 0xde00]));
    assert_eq!(display(&pair).to_string(), "😀");
    assert_eq!(to_utf8(&pair).ok(), Some("😀"));
    let error = to_utf8(&high).expect_err("an unpaired surrogate is not UTF-8");
    assert_eq!(error.to_string(), r"`a\xED\xA0\x80b` is not UTF-8");

    // Dropping `\\?\` does not need the path as text.
    let mut wide: Vec<u16> = r"\\?\C:\d\a".encode_utf16().collect();
    wide.push(0xd800);
    let verbatim = PathBuf::from(OsString::from_wide(&wide));
    let plain = PathBuf::from(OsString::from_wide(wide.get(4..).unwrap()));
    assert_eq!(simplify(verbatim).into_os_string(), plain.into_os_string());
}

#[test]
fn to_utf8_borrows_a_utf8_path() {
    let text = "src/main.rs";
    assert!(std::ptr::eq(to_utf8(text).unwrap(), text));
    let owned = PathBuf::from("src/lib.rs");
    let borrowed = to_utf8(&owned).unwrap();
    assert_eq!(borrowed, "src/lib.rs");
    assert!(std::ptr::eq(
        borrowed.as_ptr(),
        owned.as_os_str().as_encoded_bytes().as_ptr()
    ));
    let os = OsStr::new("a/b");
    assert!(std::ptr::eq(
        to_utf8(os).unwrap().as_ptr(),
        os.as_encoded_bytes().as_ptr()
    ));
}

#[test]
fn a_verbatim_prefix_is_dropped_only_where_win32_reads_the_plain_form_back() {
    let deep = vec!["d".repeat(49); 6].join(r"\");
    let long_verbatim = format!(r"\\?\C:\{deep}");
    let long_plain = format!(r"C:\{deep}");
    assert!(long_plain.encode_utf16().count() > 300);
    let dropped = [
        (r"\\?\C:\Users\me\x.rs", r"C:\Users\me\x.rs"),
        (r"\\?\C:\", r"C:\"),
        (r"\\?\UNC\srv\sh\d\f", r"\\srv\sh\d\f"),
        (long_verbatim.as_str(), long_plain.as_str()),
    ];
    let kept = [
        r"\\?\C:\d\trailing.",
        r"\\?\C:\d\space ",
        // Win32 drops one final dot from each name inside a path too.
        r"\\?\C:\d\dir.\a.txt",
        r"\\?\C:\a\..\b",
        r"\\?\C:",
        r"\\?\Volume{0b1f5e2a-0000-0000-0000-100000000000}\x",
        r"\\.\COM1",
    ];
    let simplified = |path: &str| simplify(PathBuf::from(path)).into_os_string();
    if windows_paths() {
        for (verbatim, plain) in dropped {
            assert_eq!(simplified(verbatim), OsStr::new(plain), "{verbatim}");
        }
        for path in kept {
            assert_eq!(simplified(path), OsStr::new(path));
        }
        // Windows 10 reads `C:\d\nul.txt` as the device `\\.\nul`;
        // Windows 11 does not.
        let nul = r"\\?\C:\d\nul.txt";
        let rewritten = !std::path::absolute(r"C:\d\nul.txt")
            .is_ok_and(|back| back.as_os_str() == OsStr::new(r"C:\d\nul.txt"));
        assert_eq!(simplified(nul) == OsStr::new(nul), rewritten);
    } else {
        // No path has a prefix here: each is one ordinary name.
        let nul = [r"\\?\C:\d\nul.txt"];
        for path in dropped
            .iter()
            .map(|&(verbatim, _)| verbatim)
            .chain(kept)
            .chain(nul)
        {
            assert_eq!(simplified(path), OsStr::new(path));
        }
        assert_eq!(simplified("/home/me/x.rs"), OsStr::new("/home/me/x.rs"));
    }
}

#[test]
fn under_compares_components_and_refuses_dots() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(
        under(&root.join("src").join("path.rs"), root),
        Some(Path::new("src/path.rs"))
    );
    assert_eq!(under(root, root), Some(Path::new("")));
    // A name that starts like the root's last is not inside it.
    assert_eq!(under(&root.join("wx").join("a"), &root.join("w")), None);
    assert_eq!(under(root, &root.join("src")), None);
    // `starts_with` is lexical; `under` refuses `..` on either side.
    let escape = root.join("w").join("..").join("etc");
    assert!(escape.starts_with(root.join("w")));
    assert_eq!(under(&escape, &root.join("w")), None);
    assert_eq!(under(&root.join("a"), &root.join("w").join("..")), None);
    // Relative paths.
    assert_eq!(under(Path::new("w/a"), Path::new("w")), None);
    assert_eq!(under(&root.join("a"), Path::new("w")), None);
    assert_eq!(under(Path::new("src/lib.rs"), root), None);

    let windows: [(&str, &str, Option<&str>); 13] = [
        // A plain and a verbatim prefix of one volume are the same.
        (r"\\?\C:\r\nul.txt", r"C:\r", Some("nul.txt")),
        (r"C:\r\x", r"\\?\C:\r", Some("x")),
        (r"\\?\C:\r", r"C:\r", Some("")),
        // `std` uppercases drive letters.
        (r"c:\r\x", r"C:\r", Some("x")),
        (r"D:\w", r"C:\w", None),
        (r"\\?\UNC\s\h\x", r"\\s\h", Some("x")),
        // Server and share keep their case.
        (r"\\S\h\x", r"\\s\h", None),
        (r"\\s\other\x", r"\\s\h", None),
        // A bare verbatim prefix is a volume or a share written without
        // its root directory: nothing is inside it.
        (r"C:\x", r"\\?\C:", None),
        (r"\\s\h\x", r"\\?\UNC\s\h", None),
        // A part that would read back as something else: `c:x` as a drive,
        // a verbatim path's `a/b` as two names.
        (r"D:\w\c:x", r"D:\w", None),
        (r"\\?\D:\w\c:x", r"D:\w", None),
        (r"\\?\D:\w\a/b", r"D:\w", None),
    ];
    for (path, within, inside) in windows {
        let want = inside.filter(|_| windows_paths()).map(Path::new);
        assert_eq!(
            under(Path::new(path), Path::new(within)),
            want,
            "{path} in {within}"
        );
    }
    // A verbatim path keeps `.`, which `under` refuses.
    if windows_paths() {
        assert_eq!(under(Path::new(r"\\?\C:\r\.\x"), Path::new(r"C:\r")), None);
    } else {
        // Elsewhere `c:x` is an ordinary name.
        assert_eq!(under(&root.join("c:x"), root), Some(Path::new("c:x")));
    }
}

#[test]
fn a_plain_windows_prefix_is_probed_in_its_verbatim_spelling() {
    let spelled = |path: &str| verbatim(PathBuf::from(path)).into_os_string();
    let turned = [
        // A trailing space inside the path, which Win32 would trim from a
        // leading part probed alone.
        (r"C:\w\space \b.txt", r"\\?\C:\w\space \b.txt"),
        (r"C:\", r"\\?\C:\"),
        (r"\\srv\sh\d\f", r"\\?\UNC\srv\sh\d\f"),
        (r"\\srv\sh", r"\\?\UNC\srv\sh"),
        // A device path, which Win32 normalizes too.
        (r"\\.\C:\w\space \b.txt", r"\\?\C:\w\space \b.txt"),
        (r"\\.\COM1", r"\\?\COM1"),
    ];
    let kept = [
        r"\\?\C:\w\x",
        r"\\?\UNC\srv\sh\x",
        r"\\?\Volume{0b1f5e2a-0000-0000-0000-100000000000}\x",
        "/home/me/x.rs",
    ];
    for (plain, verbatim) in turned {
        let want = if windows_paths() { verbatim } else { plain };
        assert_eq!(spelled(plain), OsStr::new(want), "{plain}");
    }
    for path in kept {
        assert_eq!(spelled(path), OsStr::new(path));
    }
}

#[test]
fn a_name_that_reads_back_as_something_else_is_not_one_name() {
    for name in [
        "x",
        "space ",
        "trailing.",
        "nul.txt",
        "ab:c",
        "file.txt:stream",
    ] {
        assert!(is_one_name(OsStr::new(name)), "{name}");
    }
    // On Windows `c:x` and `Q:` read as drives; elsewhere they are names.
    for name in ["c:x", "Q:"] {
        assert_eq!(is_one_name(OsStr::new(name)), !windows_paths(), "{name}");
    }
}

#[test]
fn a_final_separator_asks_for_a_directory() {
    let asks = |text: &str, windows: bool| asks_for_directory(OsStr::new(text), windows);
    for windows in [false, true] {
        assert!(asks("a/", windows));
        assert!(asks("a//", windows));
        assert!(asks("/", windows));
        assert!(!asks("a", windows));
        assert!(!asks("a.", windows));
        assert!(!asks(".", windows));
        assert!(!asks("a/..", windows));
    }
    // POSIX reads a final `/.` as a directory; Win32 drops it.
    assert!(asks("a/.", false));
    assert!(!asks("a/.", true));
    // `\` is a separator only where Windows' rules apply.
    assert_eq!(asks(r"a\", false), windows_paths());
}

#[test]
fn a_link_target_is_queued_as_the_os_reads_it() {
    let queued = |target: &str, windows: bool| {
        let mut queue = vec![Step::Name("after".into())];
        queue_target(Path::new(target), windows, &mut queue).map(|()| {
            queue.reverse();
            queue
        })
    };
    let name = |name: &str| Step::Name(name.into());
    let after = || name("after");
    // Each `..` in turn, as the kernel takes it.
    assert_eq!(
        queued("./a/../../b", false).unwrap(),
        [name("a"), Step::Up, Step::Up, name("b"), after()]
    );
    // By its text first, as Windows reads a link's target.
    assert_eq!(
        queued("./a/../../b", true).unwrap(),
        [Step::Up, name("b"), after()]
    );
    assert_eq!(queued("a/b/..", true).unwrap(), [name("a"), after()]);
    // A target with a root but not a prefix, on every OS.
    let rooted = queued("/x", false).unwrap_err();
    assert_eq!(rooted.kind(), io::ErrorKind::InvalidInput);
    // An empty target, which macOS and the BSDs allow, names nothing.
    for windows in [false, true] {
        let empty = queued("", windows).unwrap_err();
        assert_eq!(empty.kind(), io::ErrorKind::NotFound);
    }
    // On Windows a drive with no root, and a name that reads as a drive.
    for target in [r"c:x", r"a\c:x"] {
        assert_eq!(queued(target, true).is_err(), windows_paths(), "{target}");
    }
}

#[test]
fn the_walk_starts_at_the_whole_path_or_at_its_root() {
    let scratch = Scratch::new("enter");
    let deep = scratch.root.join("a").join("b");
    fs::create_dir_all(&deep).unwrap();
    let absolute = |path: &Path| std::path::absolute(path).unwrap();
    let at_path = |at: At| PathBuf::from(at.path);
    // The steps a path's names give, the first one first.
    let steps = |path: &Path| -> Vec<Step> {
        path.components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(Step::Name(name.to_owned())),
                Component::ParentDir => Some(Step::Up),
                _ => None,
            })
            .collect()
    };
    let root = |path: &Path| fs::canonicalize(path.ancestors().last().unwrap()).unwrap();

    // A path that exists, tried whole: its canonical path, nothing queued.
    let mut queue = Vec::new();
    let at = enter(absolute(&deep), true, &mut queue).unwrap();
    assert_eq!(at_path(at), fs::canonicalize(&deep).unwrap());
    assert!(queue.is_empty());

    // One that does not: its root, with every name queued.
    let missing = absolute(&deep.join("m").join("x"));
    let at = enter(missing.clone(), true, &mut queue).unwrap();
    assert_eq!(at_path(at), root(&missing));
    queue.reverse();
    assert_eq!(queue, steps(&missing));

    // A link's target is never tried whole, so the walk counts its links.
    let mut queue = Vec::new();
    let at = enter(absolute(&deep), false, &mut queue).unwrap();
    assert_eq!(at_path(at), root(&deep));
    queue.reverse();
    assert_eq!(queue, steps(&absolute(&deep)));

    // An error for the whole path that is not `NotFound` is the answer:
    // Unix says a name below a file is not a directory.
    fs::write(deep.join("f.txt"), "f").unwrap();
    let below_file = absolute(&deep.join("f.txt").join("x"));
    match enter(below_file, true, &mut Vec::new()) {
        Err(error) => assert_eq!(error.kind(), io::ErrorKind::NotADirectory),
        // Windows says the name is missing, and the walk finds the file.
        Ok(at) => assert!(windows_paths(), "{}", display(&at.path)),
    }
}

#[test]
fn errors_name_their_path() {
    let path = PathBuf::from("/srv/notes\n.md");
    let shown = r"/srv/notes\u{a}.md";

    let empty = PathError::Empty;
    assert_eq!(empty.to_string(), "the path is empty");
    assert_eq!(empty.kind(), io::ErrorKind::InvalidInput);
    assert!(std::error::Error::source(&empty).is_none());

    let not_utf8 = PathError::NotUtf8 { path: path.clone() };
    assert_eq!(not_utf8.to_string(), format!("`{shown}` is not UTF-8"));
    assert_eq!(not_utf8.kind(), io::ErrorKind::InvalidData);
    assert!(std::error::Error::source(&not_utf8).is_none());

    let source = io::Error::from(io::ErrorKind::NotFound);
    let message = format!("cannot resolve `{shown}`: {source}");
    let io_error = PathError::Io {
        path: path.clone(),
        source,
    };
    assert_eq!(io_error.to_string(), message);
    assert_eq!(io_error.kind(), io::ErrorKind::NotFound);
    let inner = std::error::Error::source(&io_error).expect("an Io error has a source");
    assert_eq!(
        inner.downcast_ref::<io::Error>().map(io::Error::kind),
        Some(io::ErrorKind::NotFound)
    );

    let links = PathError::TooManyLinks { path };
    assert_eq!(
        links.to_string(),
        format!("cannot resolve `{shown}`: it goes through more than 40 symbolic links")
    );
    assert_eq!(links.kind(), io::ErrorKind::Other);
    assert!(std::error::Error::source(&links).is_none());
}

#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_canonicalizes_to_its_bytes() {
    use std::os::unix::ffi::OsStrExt;

    let test = "a_name_that_is_not_utf8_canonicalizes_to_its_bytes";
    let scratch = Scratch::new("not-utf8");
    let name = OsStr::from_bytes(b"caf\xe9.txt");
    let file = scratch.root.join(name);
    if let Err(error) = fs::write(&file, "café") {
        // APFS, and ZFS with `utf8only`, refuse such names.
        return skipped(test, &format!("the file system refuses the name: {error}"));
    }
    let anchor = canonicalize(&scratch.root).unwrap();
    let canonical = canonicalize(&file).unwrap();
    assert_eq!(canonical.file_name(), Some(name));
    assert_eq!(under(&canonical, &anchor), Some(Path::new(name)));
    assert_eq!(fs::read_to_string(&canonical).unwrap(), "café");
    assert_eq!(resolve(&file).unwrap(), canonical);
    assert!(display(&canonical).to_string().ends_with(r"/caf\xE9.txt"));
    assert!(matches!(
        to_utf8(&canonical),
        Err(PathError::NotUtf8 { .. })
    ));
}

#[cfg(windows)]
#[test]
fn a_name_with_an_unpaired_surrogate_canonicalizes_to_it() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let test = "a_name_with_an_unpaired_surrogate_canonicalizes_to_it";
    let scratch = Scratch::new("surrogate");
    // `a`, U+D800 alone, `.txt`.
    let name = OsString::from_wide(&[0x61, 0xd800, 0x2e, 0x74, 0x78, 0x74]);
    let file = scratch.root.join(&name);
    if let Err(error) = fs::write(&file, "x") {
        return skipped(test, &format!("the file system refuses the name: {error}"));
    }
    let anchor = canonicalize(&scratch.root).unwrap();
    let canonical = canonicalize(&file).unwrap();
    // Win32 reads the plain form back unchanged, surrogate and all, so the
    // `\\?\` goes, as for any other name (ADR-0033).
    assert!(matches!(
        canonical.components().next(),
        Some(Component::Prefix(prefix)) if !prefix.kind().is_verbatim()
    ));
    assert_eq!(canonical.file_name(), Some(name.as_os_str()));
    assert_eq!(under(&canonical, &anchor), Some(Path::new(&name)));
    assert_eq!(fs::read_to_string(&canonical).unwrap(), "x");
    assert_eq!(resolve(&file).unwrap(), canonical);
    assert!(
        display(&canonical)
            .to_string()
            .ends_with(r"\a\xED\xA0\x80.txt")
    );
    assert!(matches!(
        to_utf8(&canonical),
        Err(PathError::NotUtf8 { .. })
    ));
}
