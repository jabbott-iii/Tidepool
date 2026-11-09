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

//! `cargo xtask headers`: every source file starts with the Apache-2.0
//! header of `LICENSE`'s appendix, in its comment syntax (ADR-0021).
//!
//! Ported from nadir xtask `headers.rs`. Adapted for Tidepool: the kinds of
//! source Tidepool has (Rust, shell scripts, workflows, and the demo page's
//! HTML, CSS and JavaScript, ADR-0030), files exempt by path, the walk that
//! `deps` and `unsafe-audit` share ([`walk::scan`]), symbolic links
//! reported instead of skipped, a size bound, and a leading byte-order mark
//! skipped.
//!
//! The check is specified in `docs/04-quality.md` → "`headers`". A source
//! file ([`kind`]) is a Rust file (`.rs`), a shell script (`.sh`), a
//! workflow (`.yml` or `.yaml` directly in `.github/workflows/`, where
//! GitHub reads them), an HTML file (`.html`), a stylesheet (`.css`) or a
//! script (`.js`), with the extension in any case. Its header is
//! [`HEADER`], then a blank line unless the file ends there: `// ` or `# `
//! before each line (`//` or `#` alone on an empty one), or, in CSS and
//! HTML, one `/* */` or `<!-- -->` comment whose opening and closing are on
//! lines of their own and whose lines are indented by two spaces (an empty
//! one left empty), as Nadir's are. It starts on line 1, or on line 2 after
//! a shell script's `#!` line or an HTML file's `<!doctype …>` line, if the
//! file has one. `site/llms.txt`, the page as plain text, is not source and
//! carries none. A violation is reported for each of these:
//!
//! - **A header missing, altered or out of place**: the first line that
//!   differs, with what it should be and which line of the appendix that
//!   is.
//! - **No blank line after the header.**
//! - **A file larger than the size bound, or not UTF-8.**
//! - **A symbolic link**, any: links are neither followed nor resolved
//!   ([`walk::Tree::links`]), so what one leads to was not checked.
//!
//! [`EXEMPT`] lists, by path, the source files the check skips.

use std::path::Path;

use crate::check::Report;
use crate::error::Error;
use crate::{ucd, walk};

/// The header's text, a line each, as `LICENSE`'s appendix gives it.
const HEADER: [&str; 13] = [
    "Copyright 2026 Joseph Anthony Abbott III",
    "",
    "Licensed under the Apache License, Version 2.0 (the \"License\");",
    "you may not use this file except in compliance with the License.",
    "You may obtain a copy of the License at",
    "",
    "    http://www.apache.org/licenses/LICENSE-2.0",
    "",
    "Unless required by applicable law or agreed to in writing, software",
    "distributed under the License is distributed on an \"AS IS\" BASIS,",
    "WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.",
    "See the License for the specific language governing permissions and",
    "limitations under the License.",
];

/// The source files the check skips, each with the reason, relative to the
/// root with `/` between components (ADR-0021): the generated tables keep
/// the Unicode data's notice instead.
const EXEMPT: [(&str, &str); 1] = [(
    ucd::OUTPUT,
    "generated from the Unicode data, and keeps its notice (ADR-0018)",
)];

/// The largest file read, in bytes, as for `unsafe-audit`.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// How a kind of file writes the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    /// `//` before each line.
    Slashes,
    /// `#` before each line.
    Hash,
    /// One `/* */` comment, its lines indented by two spaces.
    Block,
    /// One `<!-- -->` comment, its lines indented by two spaces.
    Markup,
}

/// What may come before the header, on line 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum First {
    /// Nothing.
    Nothing,
    /// A `#!` line, if the file has one.
    Shebang,
    /// An HTML doctype, if the file has one.
    Doctype,
}

/// Runs the check on the source files under `root`.
///
/// # Errors
///
/// [`Error::Read`] if a directory or a file cannot be read.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    check_tree(root, MAX_FILE_BYTES)
}

/// [`run`], with files larger than `limit` bytes reported unread.
fn check_tree(root: &Path, limit: u64) -> Result<Report, Error> {
    let tree = walk::scan(root, is_source_name)?;
    let mut report = Report::default();
    report
        .notes
        .extend(walk::worktrees_note(root, &tree.worktrees));
    report.notes.extend(walk::build_dir_note(root, &tree));
    for link in &tree.links {
        report.violations.push(format!(
            "{}: a symbolic link, which `headers` does not follow: what it leads to was not \
             checked",
            walk::relative(root, link)
        ));
    }
    let mut checked = 0;
    let mut exempt = Vec::new();
    for path in &tree.files {
        let shown = walk::relative(root, path);
        let Some((style, first)) = kind(&shown) else {
            continue;
        };
        if let Some((_, why)) = EXEMPT.iter().find(|(exempt, _)| *exempt == shown) {
            exempt.push(format!("{shown}, {why}"));
            continue;
        }
        match walk::read_bounded(path, limit) {
            Ok(text) => {
                checked += 1;
                if let Some(problem) = problem(&text, style, first) {
                    report.violations.push(format!("{shown}:{problem}"));
                }
            }
            Err(walk::ReadError::TooLarge { limit }) => report.violations.push(format!(
                "{shown}: larger than {limit} bytes, so its header was not checked"
            )),
            Err(walk::ReadError::NotUtf8) => report
                .violations
                .push(format!("{shown}: not UTF-8, so its header was not checked")),
            Err(walk::ReadError::Io(source)) => {
                return Err(Error::Read {
                    path: path.clone(),
                    source,
                });
            }
        }
    }
    report
        .notes
        .push(format!("source files checked: {checked}"));
    if !exempt.is_empty() {
        report
            .notes
            .push(format!("exempt by path: {}", exempt.join("; ")));
    }
    Ok(report)
}

/// Whether a file named `name` may be a source file: the walk's predicate,
/// which sees only the name. [`kind`] decides from the whole path.
fn is_source_name(name: &str) -> bool {
    extension(name).is_some_and(|extension| {
        ["rs", "sh", "yml", "yaml", "html", "css", "js"]
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    })
}

/// The part of `name` after its last `.`, if it has one after its first
/// character.
fn extension(name: &str) -> Option<&str> {
    name.rsplit_once('.')
        .and_then(|(stem, extension)| (!stem.is_empty()).then_some(extension))
}

/// How the file at `path`, relative to the root with `/` between
/// components, writes the header and what may come before it, or `None`
/// for a file that is not source.
fn kind(path: &str) -> Option<(Style, First)> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = extension(name)?.to_ascii_lowercase();
    match extension.as_str() {
        "rs" | "js" => Some((Style::Slashes, First::Nothing)),
        "sh" => Some((Style::Hash, First::Shebang)),
        "css" => Some((Style::Block, First::Nothing)),
        "html" => Some((Style::Markup, First::Doctype)),
        "yml" | "yaml"
            if path
                .strip_prefix(".github/workflows/")
                .is_some_and(|rest| !rest.contains('/')) =>
        {
            Some((Style::Hash, First::Nothing))
        }
        _ => None,
    }
}

/// The header's lines as `style` writes them.
fn header(style: Style) -> Vec<String> {
    let prefixed = |prefix: &str| -> Vec<String> {
        HEADER
            .iter()
            .map(|line| {
                if line.is_empty() {
                    prefix.to_string()
                } else {
                    format!("{prefix} {line}")
                }
            })
            .collect()
    };
    let enclosed = |open: &str, close: &str| -> Vec<String> {
        let mut lines = vec![open.to_string()];
        lines.extend(HEADER.iter().map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("  {line}")
            }
        }));
        lines.push(close.to_string());
        lines
    };
    match style {
        Style::Slashes => prefixed("//"),
        Style::Hash => prefixed("#"),
        Style::Block => enclosed("/*", "*/"),
        Style::Markup => enclosed("<!--", "-->"),
    }
}

/// Where `text`'s header is wrong, as `line: what`, or `None` if it is
/// right.
fn problem(text: &str, style: Style, first: First) -> Option<String> {
    // A byte-order mark, which some editors write (on Windows above all)
    // and rustc skips, as `deps` does for the files it reads: it would
    // otherwise fail line 1 with nothing visible to show for it.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let expected = header(style);
    // The `#!` or doctype line, the header and the blank line after it are
    // all it reads, whatever the length of the file.
    let lines: Vec<&str> = text.lines().take(expected.len() + 2).collect();
    let line_1 = lines.first().copied().unwrap_or_default();
    let start = match first {
        First::Nothing => 0,
        First::Shebang => usize::from(line_1.starts_with("#!")),
        First::Doctype => usize::from(line_1.to_ascii_lowercase().starts_with("<!doctype")),
    };
    // CSS and HTML open and close their comment on lines of their own.
    let opening = usize::from(matches!(style, Style::Block | Style::Markup));
    for (index, wanted) in expected.iter().enumerate() {
        let line = start + index;
        if lines.get(line) != Some(&wanted.as_str()) {
            let which = match index.checked_sub(opening) {
                None => "the comment that holds the Apache-2.0 header".to_string(),
                Some(appendix) if appendix < HEADER.len() => format!(
                    "line {} of the Apache-2.0 header in `LICENSE`'s appendix",
                    appendix + 1
                ),
                Some(_) => "the end of the comment that holds the Apache-2.0 header".to_string(),
            };
            return Some(format!("{}: expected `{wanted}`, {which}", line + 1));
        }
    }
    let after = start + expected.len();
    match lines.get(after) {
        Some(line) if !line.is_empty() => Some(format!(
            "{}: expected a blank line after the Apache-2.0 header",
            after + 1
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use crate::walk::testing::{TempTree, symlink};

    /// `body` after the header in `style`, a blank line, and `first`
    /// before it if given.
    fn with_header(style: Style, first: Option<&str>, body: &str) -> String {
        let mut lines: Vec<String> = first.map(str::to_string).into_iter().collect();
        lines.extend(header(style));
        lines.push(String::new());
        lines.push(body.to_string());
        lines.join("\n")
    }

    #[test]
    fn the_header_is_the_one_in_license_s_appendix() {
        let license = include_str!("../../LICENSE");
        let (_, appendix) = license
            .split_once("APPENDIX: How to apply the Apache License to your work.")
            .expect("LICENSE has its appendix");
        let lines: Vec<&str> = appendix.lines().collect();
        let start = lines
            .iter()
            .position(|line| line.trim_start().starts_with("Copyright "))
            .expect("the appendix has a copyright line");
        let boilerplate: Vec<&str> = lines[start..start + HEADER.len()]
            .iter()
            .map(|line| line.strip_prefix("   ").unwrap_or(line))
            .collect();
        assert_eq!(boilerplate, HEADER);
        // The appendix ends there.
        assert!(
            lines[start + HEADER.len()..]
                .iter()
                .all(|line| line.trim().is_empty())
        );
    }

    #[test]
    fn each_kind_of_source_has_its_comments_and_place() {
        for (path, expected) in [
            (
                "crates/core/src/lib.rs",
                Some((Style::Slashes, First::Nothing)),
            ),
            ("xtask/src/main.RS", Some((Style::Slashes, First::Nothing))),
            ("site/tidepool.js", Some((Style::Slashes, First::Nothing))),
            ("site/tidepool.css", Some((Style::Block, First::Nothing))),
            ("site/index.html", Some((Style::Markup, First::Doctype))),
            ("site/PAGE.HTML", Some((Style::Markup, First::Doctype))),
            ("scripts/check-deps.sh", Some((Style::Hash, First::Shebang))),
            (
                ".claude/hooks/deps-guard.sh",
                Some((Style::Hash, First::Shebang)),
            ),
            (
                ".github/workflows/ci.yml",
                Some((Style::Hash, First::Nothing)),
            ),
            (
                ".github/workflows/release.yaml",
                Some((Style::Hash, First::Nothing)),
            ),
            (
                ".github/workflows/NIGHTLY.YML",
                Some((Style::Hash, First::Nothing)),
            ),
            // Configuration and data carry none: GitHub reads workflows
            // only directly in `.github/workflows/`.
            (".github/workflows/data/matrix.yml", None),
            (".github/ISSUE_TEMPLATE/issue.yml", None),
            (".github/dependabot.yml", None),
            ("docs/example.yml", None),
            ("Cargo.toml", None),
            ("README.md", None),
            ("crates/codec/tests/json/settings.jsonc", None),
            ("crates/text/ucd/17.0.0/GraphemeBreakTest.txt", None),
            ("tests/sample.rs.txt", None),
            // The page as plain text, served as it is, and the features.
            ("site/llms.txt", None),
            ("docs/features.jsonc", None),
            ("site/index.htm", None),
            ("site/tidepool.mjs", None),
            ("LICENSE", None),
            ("NOTICE", None),
            (".gitignore", None),
            // A name that is only an extension is not one.
            ("crates/core/.rs", None),
        ] {
            assert_eq!(kind(path), expected, "{path}");
            if expected.is_some() {
                let name = path.rsplit('/').next().unwrap();
                assert!(is_source_name(name), "{path}");
            }
        }
    }

    #[test]
    fn each_style_writes_the_header_in_its_comments() {
        assert_eq!(
            header(Style::Slashes)[..3],
            [
                "// Copyright 2026 Joseph Anthony Abbott III",
                "//",
                "// Licensed under the Apache License, Version 2.0 (the \"License\");",
            ]
        );
        assert_eq!(
            header(Style::Hash)[6],
            "#     http://www.apache.org/licenses/LICENSE-2.0"
        );
        assert_eq!(header(Style::Hash).len(), HEADER.len());
        let block = header(Style::Block);
        assert_eq!(
            [block[0].as_str(), block[1].as_str(), block[2].as_str()],
            ["/*", "  Copyright 2026 Joseph Anthony Abbott III", ""]
        );
        assert_eq!(block.last().map(String::as_str), Some("*/"));
        let markup = header(Style::Markup);
        assert_eq!(markup.first().map(String::as_str), Some("<!--"));
        assert_eq!(
            markup[7].as_str(),
            "      http://www.apache.org/licenses/LICENSE-2.0"
        );
        assert_eq!(markup.last().map(String::as_str), Some("-->"));
        assert_eq!(markup.len(), HEADER.len() + 2);
    }

    #[test]
    fn a_file_with_its_header_in_place_passes() {
        for (style, first, line_1) in [
            (Style::Slashes, First::Nothing, None),
            (Style::Hash, First::Shebang, Some("#!/bin/sh")),
            (Style::Hash, First::Shebang, None),
            (Style::Hash, First::Nothing, None),
            (Style::Block, First::Nothing, None),
            (Style::Markup, First::Doctype, Some("<!doctype html>")),
            (Style::Markup, First::Doctype, Some("<!DOCTYPE html>")),
            (Style::Markup, First::Doctype, None),
        ] {
            let text = with_header(style, line_1, "rest");
            assert_eq!(problem(&text, style, first), None, "{text}");
            // As a Windows checkout may have it.
            let crlf = text.replace('\n', "\r\n");
            assert_eq!(problem(&crlf, style, first), None, "{crlf}");
        }
        // Nothing after it is fine too.
        let text = header(Style::Slashes).join("\n");
        assert_eq!(problem(&text, Style::Slashes, First::Nothing), None);
        // A byte-order mark before it is skipped; one anywhere else is not.
        let rust = with_header(Style::Slashes, None, "fn f() {}");
        let marked = format!("\u{feff}{rust}");
        assert_eq!(problem(&marked, Style::Slashes, First::Nothing), None);
        let twice = format!("\u{feff}\u{feff}{rust}");
        assert!(
            problem(&twice, Style::Slashes, First::Nothing)
                .is_some_and(|problem| problem.starts_with("1: expected `// Copyright"))
        );
        // Only the start of a long file is read.
        let long = format!("{rust}\n{}", "fn g() {}\n".repeat(100_000));
        assert_eq!(problem(&long, Style::Slashes, First::Nothing), None);
    }

    #[test]
    fn a_file_without_its_header_fails_on_the_line_it_should_start() {
        assert_eq!(
            problem("//! A crate.\n", Style::Slashes, First::Nothing).as_deref(),
            Some(
                "1: expected `// Copyright 2026 Joseph Anthony Abbott III`, line 1 of the \
                 Apache-2.0 header in `LICENSE`'s appendix"
            )
        );
        assert_eq!(
            problem("", Style::Hash, First::Nothing).as_deref(),
            Some(
                "1: expected `# Copyright 2026 Joseph Anthony Abbott III`, line 1 of the \
                 Apache-2.0 header in `LICENSE`'s appendix"
            )
        );
        // After the `#!` line.
        assert!(
            problem("#!/bin/sh\necho\n", Style::Hash, First::Shebang)
                .is_some_and(|problem| problem.starts_with("2: expected `# Copyright"))
        );
        // After the doctype, a page needs the comment.
        assert_eq!(
            problem("<!doctype html>\n<html>\n", Style::Markup, First::Doctype).as_deref(),
            Some("2: expected `<!--`, the comment that holds the Apache-2.0 header")
        );
        assert_eq!(
            problem("body { margin: 0; }\n", Style::Block, First::Nothing).as_deref(),
            Some("1: expected `/*`, the comment that holds the Apache-2.0 header")
        );
        assert!(
            problem("'use strict';\n", Style::Slashes, First::Nothing)
                .is_some_and(|problem| problem.starts_with("1: expected `// Copyright"))
        );
        // A stylesheet has no doctype: one there is not skipped.
        let css = format!(
            "<!doctype html>\n{}",
            with_header(Style::Block, None, "a {}")
        );
        assert!(
            problem(&css, Style::Block, First::Nothing)
                .is_some_and(|problem| problem.starts_with("1: expected `/*`"))
        );
        // A Rust file's `#!` is not a script's: `#![...]` is an attribute.
        let attribute = format!("#![allow(x)]\n{}", with_header(Style::Slashes, None, ""));
        assert!(
            problem(&attribute, Style::Slashes, First::Nothing)
                .is_some_and(|problem| problem.starts_with("1: expected `// Copyright"))
        );
    }

    #[test]
    fn a_header_out_of_place_or_altered_fails() {
        // Below other text.
        let below = format!("//! A crate.\n\n{}", with_header(Style::Slashes, None, ""));
        assert!(
            problem(&below, Style::Slashes, First::Nothing)
                .is_some_and(|problem| problem.starts_with("1: expected `// Copyright"))
        );
        // Another year or holder, a reworded line, the wrong comments.
        let altered = with_header(Style::Slashes, None, "fn f() {}")
            .replace("Copyright 2026", "Copyright 2027");
        assert!(
            problem(&altered, Style::Slashes, First::Nothing)
                .is_some_and(|problem| problem.starts_with("1: expected"))
        );
        let reworded = with_header(Style::Hash, None, "x").replace("AS IS", "as is");
        assert_eq!(
            problem(&reworded, Style::Hash, First::Nothing).as_deref(),
            Some(
                "10: expected `# distributed under the License is distributed on an \"AS IS\" \
                 BASIS,`, line 10 of the Apache-2.0 header in `LICENSE`'s appendix"
            )
        );
        let hashes = with_header(Style::Hash, None, "fn f() {}");
        assert!(problem(&hashes, Style::Slashes, First::Nothing).is_some());
        // A trailing space is a change too.
        let spaced = with_header(Style::Slashes, None, "").replacen("//\n", "// \n", 1);
        assert!(
            problem(&spaced, Style::Slashes, First::Nothing)
                .is_some_and(|problem| problem.starts_with("2: expected `//`"))
        );
        // Code right after it.
        let crowded = format!("{}\nfn f() {{}}\n", header(Style::Slashes).join("\n"));
        assert_eq!(
            problem(&crowded, Style::Slashes, First::Nothing).as_deref(),
            Some("14: expected a blank line after the Apache-2.0 header")
        );
        // After a `#!` line, the line numbers are the file's.
        let script = format!("#!/bin/sh\n{}\necho\n", header(Style::Hash).join("\n"));
        assert_eq!(
            problem(&script, Style::Hash, First::Shebang).as_deref(),
            Some("15: expected a blank line after the Apache-2.0 header")
        );
        // In CSS and HTML, the appendix's lines are counted inside the
        // comment, and its end is a line of its own.
        let css = with_header(Style::Block, None, "body {}").replace("2026", "2027");
        assert_eq!(
            problem(&css, Style::Block, First::Nothing).as_deref(),
            Some(
                "2: expected `  Copyright 2026 Joseph Anthony Abbott III`, line 1 of the \
                 Apache-2.0 header in `LICENSE`'s appendix"
            )
        );
        let unclosed =
            with_header(Style::Markup, Some("<!doctype html>"), "<html>").replace("-->\n", "");
        assert_eq!(
            problem(&unclosed, Style::Markup, First::Doctype).as_deref(),
            Some("16: expected `-->`, the end of the comment that holds the Apache-2.0 header")
        );
        let page = format!(
            "<!doctype html>\n{}\n<html>\n",
            header(Style::Markup).join("\n")
        );
        assert_eq!(
            problem(&page, Style::Markup, First::Doctype).as_deref(),
            Some("17: expected a blank line after the Apache-2.0 header")
        );
        // The comment's lines are indented by two spaces, not a tab.
        let tabbed =
            with_header(Style::Block, None, "a {}").replacen("  Copyright", "\tCopyright", 1);
        assert!(problem(&tabbed, Style::Block, First::Nothing).is_some());
    }

    #[test]
    fn the_walk_finds_source_skips_the_exempt_and_reports_what_it_cannot_read() {
        let tree = TempTree::new("headers-walk");
        let good_rust = with_header(Style::Slashes, None, "fn f() {}\n");
        let good_hash = with_header(Style::Hash, None, "on: push\n");
        tree.write("Cargo.toml", "[workspace]\nresolver = \"3\"\n");
        tree.write("src/lib.rs", &good_rust);
        tree.write("src/bad.rs", "fn f() {}\n");
        tree.write("src/LOUD.RS", "fn f() {}\n");
        tree.write("src/notes.txt", "no header\n");
        tree.write("src/latin1.rs", [0xE9, b'\n']);
        tree.write("src/big.rs", format!("{good_rust}{}", "x".repeat(4096)));
        tree.write(
            "scripts/run.sh",
            with_header(Style::Hash, Some("#!/bin/sh"), "echo\n"),
        );
        tree.write(".github/workflows/ci.yml", "on: push\n");
        tree.write(".github/workflows/lint.yaml", &good_hash);
        tree.write(".github/workflows/data/matrix.yml", "os: [linux]\n");
        tree.write(".github/dependabot.yml", "version: 2\n");
        // The demo page's files: the template after its doctype, the
        // stylesheet and the script with theirs, and plain text without.
        tree.write(
            "site/index.html",
            with_header(Style::Markup, Some("<!doctype html>"), "<html></html>\n"),
        );
        tree.write("site/page.css", with_header(Style::Block, None, "a {}\n"));
        tree.write(
            "site/page.js",
            with_header(Style::Slashes, None, "'use strict';\n"),
        );
        tree.write("site/llms.txt", "# Tidepool\n");
        tree.write("site/bad.html", "<!doctype html>\n<html></html>\n");
        tree.write("site/bad.css", "a {}\n");
        tree.write("site/bad.js", "'use strict';\n");
        // Hidden directories are searched; the root's `.git` is not.
        tree.write(".claude/hooks/hook.sh", "#!/bin/sh\necho\n");
        tree.write(".git/hooks/pre-commit.sh", "echo\n");
        // A workspace root's build output is skipped.
        tree.write("target/debug/build/out.rs", "fn f() {}\n");
        // Exempt by path: it keeps its own notice.
        tree.write(ucd::OUTPUT, "// Generated from the Unicode data.\n");
        // No longer exempt: roadmap M0.8 rewrote GitHub's CodeQL template.
        tree.write(".github/workflows/codeql.yml", "name: CodeQL\n");

        let report = check_tree(tree.path(), 4096).unwrap();
        assert_eq!(
            report.violations,
            [
                ".claude/hooks/hook.sh:2: expected `# Copyright 2026 Joseph Anthony Abbott III`, \
                 line 1 of the Apache-2.0 header in `LICENSE`'s appendix",
                ".github/workflows/ci.yml:1: expected `# Copyright 2026 Joseph Anthony Abbott \
                 III`, line 1 of the Apache-2.0 header in `LICENSE`'s appendix",
                ".github/workflows/codeql.yml:1: expected `# Copyright 2026 Joseph Anthony \
                 Abbott III`, line 1 of the Apache-2.0 header in `LICENSE`'s appendix",
                "site/bad.css:1: expected `/*`, the comment that holds the Apache-2.0 header",
                "site/bad.html:2: expected `<!--`, the comment that holds the Apache-2.0 header",
                "site/bad.js:1: expected `// Copyright 2026 Joseph Anthony Abbott III`, line 1 \
                 of the Apache-2.0 header in `LICENSE`'s appendix",
                "src/LOUD.RS:1: expected `// Copyright 2026 Joseph Anthony Abbott III`, line 1 \
                 of the Apache-2.0 header in `LICENSE`'s appendix",
                "src/bad.rs:1: expected `// Copyright 2026 Joseph Anthony Abbott III`, line 1 of \
                 the Apache-2.0 header in `LICENSE`'s appendix",
                "src/big.rs: larger than 4096 bytes, so its header was not checked",
                "src/latin1.rs: not UTF-8, so its header was not checked",
            ]
        );
        // The two files it could not read are not counted as checked.
        assert_eq!(
            report.notes,
            [
                "source files checked: 14".to_string(),
                format!(
                    "exempt by path: {}, generated from the Unicode data, and keeps its notice \
                     (ADR-0018)",
                    ucd::OUTPUT
                ),
            ]
        );
    }

    #[test]
    fn every_symbolic_link_fails() {
        let tree = TempTree::new("headers-links");
        let outside = TempTree::new("headers-links-outside");
        tree.write("Cargo.toml", "[workspace]\nresolver = \"3\"\n");
        tree.write(
            "src/lib.rs",
            with_header(Style::Slashes, None, "fn f() {}\n"),
        );
        outside.write("evil.rs", "fn f() {}\n");
        outside.write("dir/evil.rs", "fn f() {}\n");
        outside.write("notes.txt", "notes\n");
        outside.write("config.yml", "key: value\n");
        if let Err(reason) = symlink(&tree, "src/linked.rs", &outside.path().join("evil.rs")) {
            let _ = writeln!(
                std::io::stderr(),
                "SKIPPED: every_symbolic_link_fails: cannot make a link here: {reason}"
            );
            return;
        }
        symlink(&tree, "src/m", &outside.path().join("dir")).expect("a second link");
        symlink(&tree, "src/notes.txt", &outside.path().join("notes.txt")).expect("a third link");
        symlink(&tree, "src/dangling.sh", &tree.path().join("missing")).expect("a fourth link");
        symlink(&tree, "docs/config.yml", &outside.path().join("config.yml"))
            .expect("a fifth link");

        let report = run(tree.path()).unwrap();
        // Every link fails, whatever its name and wherever it leads, a
        // text file or a YAML file outside the workflows included: telling
        // which is which would mean resolving it.
        assert_eq!(
            report.violations,
            [
                "docs/config.yml: a symbolic link, which `headers` does not follow: what it \
                 leads to was not checked",
                "src/dangling.sh: a symbolic link, which `headers` does not follow: what it \
                 leads to was not checked",
                "src/linked.rs: a symbolic link, which `headers` does not follow: what it leads \
                 to was not checked",
                "src/m: a symbolic link, which `headers` does not follow: what it leads to was \
                 not checked",
                "src/notes.txt: a symbolic link, which `headers` does not follow: what it leads \
                 to was not checked",
            ]
        );
        assert_eq!(report.notes, ["source files checked: 1"]);
    }
}
