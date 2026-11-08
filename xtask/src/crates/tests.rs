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

//! Tests of `crates`: each rule for a member's manifest, the root edition,
//! the README and the roadmap, on text; the reading of each file, on a
//! scratch tree; and the real workspace.

use std::io::Write as _;

use super::*;
use crate::metadata::Package;
use crate::walk::testing::{TempTree, symlink};

/// Where the manifest in these tests is, as a violation shows it.
const SHOWN: &str = "crates/core/Cargo.toml";

/// The violations of a member's manifest `text`: one if it does not parse,
/// else those of [`member_rules`].
fn manifest_violations(shown: &str, text: &str) -> Vec<String> {
    match parse(shown, text) {
        Ok(document) => member_rules(shown, &document),
        Err(problem) => vec![problem],
    }
}

/// The violations of the root manifest `text`, as [`manifest_violations`]
/// gives a member's.
fn workspace_violations(shown: &str, text: &str) -> Vec<String> {
    match parse(shown, text) {
        Ok(document) => workspace_rules(shown, &document),
        Err(problem) => vec![problem],
    }
}

/// A member's manifest that keeps every rule.
const MANIFEST_OK: &str = "[package]\n\
    name = \"tidepool-core\"\n\
    edition.workspace = true\n\
    version.workspace = true\n\
    license.workspace = true\n\
    publish = false\n\
    \n\
    [dependencies]\n\
    \n\
    [lints]\n\
    workspace = true\n";

/// The violations of [`MANIFEST_OK`] with `from` replaced by `to`.
fn manifest(from: &str, to: &str) -> Vec<String> {
    assert!(
        MANIFEST_OK.contains(from),
        "`{from}` is not in the manifest"
    );
    manifest_violations(SHOWN, &MANIFEST_OK.replacen(from, to, 1))
}

fn not_inherited(key: &str) -> String {
    format!(
        "{SHOWN} does not inherit `package.{key}` from the workspace: write `{key}.workspace = \
         true` in `[package]`, and nothing else for `{key}`"
    )
}

fn no_key(key: &str) -> String {
    format!(
        "{SHOWN} has no `package.{key}`: write `{key}.workspace = true` in `[package]`, to \
         inherit it from the workspace"
    )
}

const NO_PUBLISH: &str = "crates/core/Cargo.toml has no `package.publish`: write `publish = \
                          false` in `[package]`, since no member is published";
const WRONG_PUBLISH: &str = "crates/core/Cargo.toml sets `package.publish` to something other \
                             than `false`: write `publish = false` in `[package]`, since no \
                             member is published";
const NO_LINTS: &str = "crates/core/Cargo.toml has no `[lints]` table: write `[lints]` with \
                        `workspace = true` and nothing else in it";
const WRONG_LINTS: &str = "crates/core/Cargo.toml does not take its lints only from the \
                           workspace: write `[lints]` with `workspace = true` and nothing else \
                           in it";

#[test]
fn a_manifest_that_inherits_everything_passes_in_any_toml_form() {
    assert_eq!(
        manifest_violations(SHOWN, MANIFEST_OK),
        Vec::<String>::new()
    );
    // An inline table, a table of its own, and a dotted `lints` key are the
    // same TOML.
    assert_eq!(
        manifest("edition.workspace = true", "edition = { workspace = true }"),
        Vec::<String>::new()
    );
    let tables = format!(
        "lints.workspace = true\n{}",
        MANIFEST_OK
            .replace("license.workspace = true\n", "")
            .replace("[lints]\nworkspace = true\n", "")
            .replace("[dependencies]\n", "[package.license]\nworkspace = true\n")
    );
    assert_eq!(manifest_violations(SHOWN, &tables), Vec::<String>::new());
    // CR LF line endings and a byte-order mark change nothing.
    let crlf = format!("\u{feff}{}", MANIFEST_OK.replace('\n', "\r\n"));
    assert_eq!(manifest_violations(SHOWN, &crlf), Vec::<String>::new());
}

#[test]
fn edition_version_and_license_must_each_be_inherited() {
    // A literal value.
    assert_eq!(
        manifest("edition.workspace = true", "edition = \"2024\""),
        [not_inherited("edition")]
    );
    // A missing key.
    assert_eq!(
        manifest("version.workspace = true\n", ""),
        [no_key("version")]
    );
    // `workspace = false`.
    assert_eq!(
        manifest(
            "license.workspace = true",
            "license = { workspace = false }"
        ),
        [not_inherited("license")]
    );
    // An extra key beside `workspace`.
    assert_eq!(
        manifest(
            "edition.workspace = true",
            "edition = { workspace = true, path = \"x\" }"
        ),
        [not_inherited("edition")]
    );
    assert_eq!(
        manifest(
            "version.workspace = true",
            "version.workspace = true\nversion.extra = 1"
        ),
        [not_inherited("version")]
    );
    // `true` as a string is not `true`.
    assert_eq!(
        manifest("license.workspace = true", "license.workspace = \"true\""),
        [not_inherited("license")]
    );
    // An empty table.
    assert_eq!(
        manifest("edition.workspace = true", "edition = {}"),
        [not_inherited("edition")]
    );
}

#[test]
fn publish_must_be_the_boolean_false() {
    assert_eq!(manifest("publish = false\n", ""), [NO_PUBLISH]);
    for publish in [
        "publish = true",
        "publish = []",
        "publish = [\"crates-io\"]",
        "publish.workspace = true",
        "publish = \"false\"",
        "publish = 0",
    ] {
        assert_eq!(
            manifest("publish = false", publish),
            [WRONG_PUBLISH],
            "{publish}"
        );
    }
}

#[test]
fn lints_must_be_only_workspace_true() {
    assert_eq!(manifest("[lints]\nworkspace = true\n", ""), [NO_LINTS]);
    assert_eq!(
        manifest("[lints]\nworkspace = true", "[lints]\nworkspace = false"),
        [WRONG_LINTS]
    );
    // An extra key, here a table of lints of its own.
    assert_eq!(
        manifest(
            "[lints]\nworkspace = true",
            "[lints]\nworkspace = true\n\n[lints.rust]\nunsafe_code = \"allow\""
        ),
        [WRONG_LINTS]
    );
    assert_eq!(
        manifest(
            "[lints]\nworkspace = true",
            "[lints.rust]\nunsafe_code = \"allow\""
        ),
        [WRONG_LINTS]
    );
    // `lints` inside `[package]` is not the `[lints]` table.
    assert_eq!(
        manifest(
            "publish = false\n",
            "publish = false\nlints.workspace = true\n"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        manifest(
            "publish = false\n\n[dependencies]\n\n[lints]\nworkspace = true\n",
            "publish = false\nlints.workspace = true\n"
        ),
        [NO_LINTS]
    );
}

/// The violation for a target that sets its own edition: `which` is how
/// the message names it.
fn target_edition(which: &str) -> String {
    format!(
        "{SHOWN}: {which} sets an `edition` of its own: remove it, so that the target uses the \
         package's edition, which it inherits from the workspace"
    )
}

#[test]
fn a_target_with_an_edition_of_its_own_fails() {
    // `[lib]`, without and with a name.
    assert_eq!(
        manifest_violations(
            SHOWN,
            &format!("{MANIFEST_OK}\n[lib]\nedition = \"2021\"\n")
        ),
        [target_edition("the `[lib]` target")]
    );
    assert_eq!(
        manifest_violations(
            SHOWN,
            &format!("{MANIFEST_OK}\n[lib]\nname = \"core\"\nedition = \"2021\"\n")
        ),
        [target_edition("the `[lib]` target `core`")]
    );
    // One violation per target of an array, in the order they are written;
    // a target without one passes.
    let targets = format!(
        "{MANIFEST_OK}\n\
         [[bin]]\nname = \"tool\"\nedition = \"2021\"\n\n\
         [[bin]]\nname = \"fine\"\n\n\
         [[test]]\nedition = \"2018\"\n\n\
         [[bench]]\nname = \"speed\"\nedition = \"2024\"\n\n\
         [[example]]\nname = \"demo\"\nedition.workspace = true\n"
    );
    assert_eq!(
        manifest_violations(SHOWN, &targets),
        [
            target_edition("the `[[bin]]` target `tool`"),
            target_edition("a `[[test]]` target with no `name`"),
            target_edition("the `[[bench]]` target `speed`"),
            target_edition("the `[[example]]` target `demo`"),
        ]
    );
    // An array written as a value is the same TOML, and the value is never
    // quoted.
    let inline =
        format!("bin = [{{ name = \"tool\", edition = \"value-from-the-file\" }}]\n{MANIFEST_OK}");
    let violations = manifest_violations(SHOWN, &inline);
    assert_eq!(violations, [target_edition("the `[[bin]]` target `tool`")]);
    assert!(
        violations
            .iter()
            .all(|violation| !violation.contains("value-from-the-file")),
        "{violations:?}"
    );
    // The package's own edition, or `edition` in another table, is not a
    // target's.
    assert_eq!(
        manifest_violations(
            SHOWN,
            &format!("{MANIFEST_OK}\n[package.metadata.x]\nedition = \"2021\"\n")
        ),
        Vec::<String>::new()
    );
}

#[test]
fn each_broken_rule_is_one_violation_in_the_order_of_the_rules() {
    let text = "[package]\nname = \"tidepool-core\"\nedition = \"2024\"\nversion = \"0.1.0\"\n";
    assert_eq!(
        manifest_violations(SHOWN, text),
        [
            not_inherited("edition"),
            not_inherited("version"),
            no_key("license"),
            NO_PUBLISH.to_string(),
            NO_LINTS.to_string(),
        ]
    );
}

#[test]
fn a_manifest_that_does_not_parse_or_has_no_package_is_named() {
    let violations = manifest_violations(SHOWN, "[package\nname = \"a\"\n");
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("crates/core/Cargo.toml cannot be parsed: TOML error at line 1"),
        "{violations:?}"
    );
    // The limits are this check's own: a manifest nested deeper than any
    // real one is not parsed.
    let deep = format!("a{} = 1\n{MANIFEST_OK}", ".b".repeat(40));
    let violations = manifest_violations(SHOWN, &deep);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("crates/core/Cargo.toml cannot be parsed: "),
        "{violations:?}"
    );
    assert_eq!(
        manifest_violations(SHOWN, "[lints]\nworkspace = true\n"),
        [
            "crates/core/Cargo.toml has no `[package]` table, so it cannot inherit `edition`, \
          `version` and `license` or set `publish = false`"
        ]
    );
    assert_eq!(
        manifest_violations(SHOWN, "package = 1\n[lints]\nworkspace = true\n").len(),
        1
    );
}

#[test]
fn a_violation_never_quotes_a_value_from_the_manifest() {
    let text = MANIFEST_OK
        .replace(
            "edition.workspace = true",
            "edition = \"value-from-the-file\"",
        )
        .replace("publish = false", "publish = \"value-from-the-file\"");
    let violations = manifest_violations(SHOWN, &text);
    assert_eq!(violations.len(), 2, "{violations:?}");
    assert!(
        violations
            .iter()
            .all(|violation| !violation.contains("value-from-the-file")),
        "{violations:?}"
    );
}

/// The root manifest's violations with `edition` written as given.
fn root(package: &str) -> Vec<String> {
    workspace_violations(
        "Cargo.toml",
        &format!("[workspace]\nresolver = \"3\"\nmembers = [\"crates/core\"]\n\n{package}"),
    )
}

#[test]
fn the_root_workspace_must_set_edition_2024() {
    let advice = "write `edition = \"2024\"` in `[workspace.package]`, the edition every member \
                  inherits";
    assert_eq!(
        root("[workspace.package]\nedition = \"2024\"\nversion = \"0.1.0\"\n"),
        Vec::<String>::new()
    );
    for wrong in ["\"2021\"", "2024", "\"2024 \"", "{ workspace = true }"] {
        assert_eq!(
            root(&format!("[workspace.package]\nedition = {wrong}\n")),
            [format!(
                "Cargo.toml sets `workspace.package.edition` to something other than \"2024\": \
                 {advice}"
            )],
            "{wrong}"
        );
    }
    let missing = [format!(
        "Cargo.toml has no `edition` in `[workspace.package]`: {advice}"
    )];
    assert_eq!(root("[workspace.package]\nversion = \"0.1.0\"\n"), missing);
    assert_eq!(root(""), missing);
    // The member's own `[package] edition` is not the workspace's.
    assert_eq!(root("[package]\nedition = \"2024\"\n"), missing);
    assert_eq!(
        workspace_violations("Cargo.toml", "[workspace\n").len(),
        1,
        "a root manifest that does not parse is one violation"
    );
}

/// A README of `tidepool-core` in ADR-0023's shape.
const README_OK: &str = "# tidepool-core\n\
    \n\
    Errors, logging and the rest of the foundations.\n\
    \n\
    ## Status\n\
    \n\
    Built: the errors — M0.2. Not yet: paths — M1.8.\n\
    \n\
    ## Design\n\
    \n\
    `docs/subsystems/core.md`.\n";

/// The roadmap's tasks in these tests.
fn tasks() -> BTreeSet<String> {
    ["M0.2", "M1.2", "M1.8", "M1.10"]
        .into_iter()
        .map(String::from)
        .collect()
}

fn readme(text: &str) -> Vec<String> {
    readme_violations("crates/core/README.md", "tidepool-core", text, &tasks())
}

const FIRST_LINE: &str = "crates/core/README.md: its first line is not `# tidepool-core`, the \
                          package name as a heading";
const NO_STATUS: &str = "crates/core/README.md has no `## Status` line outside code blocks, HTML \
                         blocks and comments";
const NO_DESIGN: &str = "crates/core/README.md has no `## Design` line outside code blocks, HTML \
                         blocks and comments";

#[test]
fn a_readme_in_the_shape_passes_with_a_bom_or_crlf() {
    assert_eq!(readme(README_OK), Vec::<String>::new());
    assert_eq!(
        readme(&format!("\u{feff}{README_OK}")),
        Vec::<String>::new()
    );
    assert_eq!(
        readme(&README_OK.replace('\n', "\r\n")),
        Vec::<String>::new()
    );
    // Without a final newline too.
    assert_eq!(
        readme(README_OK.trim_end_matches('\n')),
        Vec::<String>::new()
    );
}

#[test]
fn the_first_line_must_be_the_package_name_as_a_heading() {
    for first in [
        "# Tidepool core",
        "# tidepool-core ",
        "#  tidepool-core",
        "## tidepool-core",
        " # tidepool-core",
        "# tidepool-cor",
        "",
    ] {
        let text = README_OK.replacen("# tidepool-core", first, 1);
        let violations = readme(&text);
        assert_eq!(violations, [FIRST_LINE], "{first:?}");
    }
    // The heading on the second line is not the first line.
    assert_eq!(readme(&format!("\n{README_OK}")), [FIRST_LINE]);
    // One byte-order mark and one CR are skipped, no more.
    assert_eq!(
        readme(&format!("\u{feff}\u{feff}{README_OK}")),
        [FIRST_LINE]
    );
    assert_eq!(
        readme(&README_OK.replacen("# tidepool-core\n", "# tidepool-core\r\r\n", 1)),
        [FIRST_LINE]
    );
    // What the line holds is never quoted.
    let violations = readme(&README_OK.replacen("tidepool-core", "Text-from-the-file", 1));
    assert!(
        violations
            .iter()
            .all(|violation| !violation.contains("Text-from-the-file")),
        "{violations:?}"
    );
    // An empty README breaks every rule of its shape.
    assert_eq!(readme(""), [FIRST_LINE, NO_STATUS, NO_DESIGN]);
}

#[test]
fn a_section_counts_only_as_its_exact_line_outside_fenced_code() {
    // Only inside a fence: backticks, tildes, a fence indented three
    // spaces, a longer closing fence.
    for (open, close) in [
        ("```", "```"),
        ("~~~", "~~~"),
        ("   ```text", "```"),
        ("~~~", "~~~~"),
    ] {
        let text = README_OK.replace("## Status\n", &format!("{open}\n## Status\n{close}\n"));
        assert_eq!(readme(&text), [NO_STATUS], "{open:?} {close:?}");
    }
    // A fence that never closes hides everything after it.
    let text = README_OK.replace("## Status\n", "````\n## Status\n```\n");
    assert_eq!(readme(&text), [NO_STATUS, NO_DESIGN]);
    // A closing fence of the other character, or indented four spaces,
    // does not close it.
    let text = README_OK.replace("## Status\n", "```\n~~~\n    ```\n## Status\n");
    assert_eq!(readme(&text), [NO_STATUS, NO_DESIGN]);
    // Not fences: indented four spaces or a tab (indented code), and
    // backticks with a backtick in the info string (inline code). The
    // section after each counts.
    for not_a_fence in ["    ```", "\t```", "``` `x`"] {
        let text = README_OK.replace("## Status\n", &format!("{not_a_fence}\n## Status\n"));
        assert_eq!(readme(&text), Vec::<String>::new(), "{not_a_fence:?}");
    }
    // After the fence closes, a section counts again.
    let text = README_OK.replace("## Status\n", "```\n## Status\n```\n\n## Status\n");
    assert_eq!(readme(&text), Vec::<String>::new());
    // Only the exact line counts.
    for near in [
        "## Designs",
        "## Design ",
        "##  Design",
        " ## Design",
        "### Design",
        "## design",
        "Design\n------",
    ] {
        let text = README_OK.replace("## Design", near);
        assert_eq!(readme(&text), [NO_DESIGN], "{near:?}");
    }
    // With a CR before the LF it is still exact.
    let text = README_OK.replace("## Design\n", "## Design\r\n");
    assert_eq!(readme(&text), Vec::<String>::new());
}

#[test]
fn a_section_counts_only_where_a_reader_sees_it_as_text() {
    // A fence opener inside an HTML comment opens nothing, so the sections
    // after it count: over several lines, or on one.
    let text = README_OK.replace("## Status\n", "<!--\n```\n-->\n\n## Status\n");
    assert_eq!(readme(&text), Vec::<String>::new());
    let text = README_OK.replace("## Status\n", "<!-- ``` -->\n## Status\n");
    assert_eq!(readme(&text), Vec::<String>::new());
    // Nor does one inside an HTML block.
    let text = README_OK.replace("## Status\n", "<div>\n~~~\n</div>\n\n## Status\n");
    assert_eq!(readme(&text), Vec::<String>::new());
    // A section only inside a comment, or an HTML block, is not seen.
    let text = README_OK.replace("## Status\n", "<!--\n## Status\n-->\n");
    assert_eq!(readme(&text), [NO_STATUS]);
    let text = README_OK.replace("## Design\n", "<details>\n## Design\n</details>\n");
    assert_eq!(readme(&text), [NO_DESIGN]);
    // A comment on the line makes it another line than the section.
    let text = README_OK.replace("## Status\n", "## Status<!-- -->\n");
    assert_eq!(readme(&text), [NO_STATUS]);
}

#[test]
fn unknown_task_ids_are_reported_once_each_in_numeric_order() {
    let text = format!("{README_OK}\nAlso M9.10, M9.9, M0.2, M9.9 again and M10.1.\n");
    let unknown = |id: &str| {
        format!("crates/core/README.md names the task {id}, which docs/roadmap.md does not have")
    };
    assert_eq!(
        readme(&text),
        [unknown("M9.9"), unknown("M9.10"), unknown("M10.1")]
    );
    // An ID is checked wherever it is: in a code block too.
    let text = format!("{README_OK}\n```\nM7.7\n```\n");
    assert_eq!(readme(&text), [unknown("M7.7")]);
}

#[test]
fn task_ids_follow_the_boundary_rule() {
    // `M1.2` inside `M1.23` is not named: the digits run on. `xM1.2`,
    // `_M1.2` and `1M1.2` name nothing; a non-ASCII letter before the `M`
    // does not join it.
    assert_eq!(
        task_ids("xM1.2 _M1.2 1M1.2 M1.23 (M1.2) M1.2.3 ÄM1.2 M1. M.1 M1x.2 MM1.2 M01.2"),
        ["M1.23", "M1.2", "M1.2", "M1.2", "M01.2"]
    );
    assert_eq!(task_ids("M1.2M3.4"), ["M1.2"]);
    assert_eq!(task_ids("**M0.9 —"), ["M0.9"]);
    assert_eq!(task_ids(""), Vec::<&str>::new());
    assert_eq!(task_ids("M"), Vec::<&str>::new());
    assert_eq!(task_ids("M12"), Vec::<&str>::new());
    // So with `M1.2` a task, `M1.23` is still unknown, and the other forms
    // name nothing to check.
    let mut only = BTreeSet::new();
    only.insert("M1.2".to_string());
    assert_eq!(unknown_ids("xM1.23 M1.23 M1.2 M1.2", &only), ["M1.23"]);
    // Numeric order, and the same number written another way is another ID.
    assert_eq!(
        unknown_ids("M1.10 M1.9 M10.0 M2.0 M01.9", &only),
        ["M01.9", "M1.9", "M1.10", "M2.0", "M10.0"]
    );
}

#[test]
fn the_roadmap_s_tasks_are_its_task_lines() {
    let text = "# Roadmap\n\
        \n\
        - [ ] **M0.1 — area: open**\n\
        - [x] **M0.10 — area: done**\n\
        - [x] **M2.3 — area: CR LF**\r\n\
        Mentions M3.3 in a sentence.\n\
        \x20 - [ ] **M5.5 — indented**\n\
        - [X] **M6.6 — capital X**\n\
        - [ ] **M7.7 - hyphen**\n\
        - [ ] **M8.8—no spaces**\n\
        - [ ] M9.9 — not bold\n\
        * [ ] **M4.4 — a star**\n\
        - [ ] **M1.2.3 — three parts**\n\
        - [ ] **M1.5**\n";
    let tasks = roadmap_tasks(text).expect("the roadmap has tasks");
    assert_eq!(
        tasks.iter().map(String::as_str).collect::<Vec<_>>(),
        ["M0.1", "M0.10", "M2.3"]
    );
}

#[test]
fn a_roadmap_without_a_task_is_an_error() {
    for text in [
        "",
        "# Roadmap\n\nM1.1 is mentioned, but no line is a task.\n",
    ] {
        let error = roadmap_tasks(text).expect_err("no task");
        assert!(matches!(error, Error::Doc { line: 0, .. }), "{error:?}");
        assert_eq!(
            error.to_string(),
            "docs/roadmap.md: no line starts `- [ ] **M<n>.<m> — ` or `- [x] **M<n>.<m> — `, \
             so there is no task for a README to name; is the root right?"
        );
    }
}

/// A member of a workspace whose manifest is at `manifest`.
fn member(name: &str, manifest: PathBuf) -> Package {
    Package {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        source: None,
        manifest_path: manifest,
        is_member: true,
        dependencies: Vec::new(),
        targets: Vec::new(),
    }
}

/// A manifest of `name` that keeps every rule.
fn manifest_of(name: &str) -> String {
    MANIFEST_OK.replace("tidepool-core", name)
}

/// A README of `name` in the shape.
fn readme_of(name: &str) -> String {
    README_OK.replace("tidepool-core", name)
}

/// Writes the member `crates/<dir>`, named `tidepool-<dir>`, with a manifest
/// and README that keep every rule, and returns it.
fn add_member(tree: &TempTree, dir: &str) -> Package {
    let name = format!("tidepool-{dir}");
    tree.write(&format!("crates/{dir}/Cargo.toml"), manifest_of(&name));
    tree.write(&format!("crates/{dir}/README.md"), readme_of(&name));
    member(&name, tree.path().join(format!("crates/{dir}/Cargo.toml")))
}

/// A scratch repository: a root manifest and `count` members that keep
/// every rule.
fn repository(name: &str, dirs: &[&str]) -> (TempTree, Vec<Package>) {
    let tree = TempTree::new(name);
    tree.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"3\"\n\n[workspace.package]\nedition = \"2024\"\n",
    );
    let packages = dirs.iter().map(|dir| add_member(&tree, dir)).collect();
    (tree, packages)
}

fn graph(tree: &TempTree, packages: Vec<Package>) -> Metadata {
    Metadata {
        workspace_root: tree.path().to_path_buf(),
        packages,
    }
}

#[test]
fn every_member_is_checked_and_counted() {
    let (tree, mut packages) = repository("crates-pass", &["core", "text"]);
    // A package that is not a member is not checked.
    packages.push(Package {
        is_member: false,
        ..member("left-pad", tree.path().join("vendor/left-pad/Cargo.toml"))
    });
    let report = check(tree.path(), &graph(&tree, packages), &tasks());
    assert_eq!(report.violations, Vec::<String>::new());
    assert_eq!(
        report.notes,
        ["members checked: 2, task IDs in docs/roadmap.md: 4"]
    );
}

#[test]
fn a_root_given_with_dot_dot_finds_the_members_cargo_names() {
    // Cargo writes the members' paths with `..` taken out; the root keeps
    // it, as `std::path::absolute` leaves it.
    let (tree, packages) = repository("crates-dotdot", &["core", "text"]);
    let root = tree.path().join("crates/..");
    let report = check(&root, &graph(&tree, packages), &tasks());
    assert_eq!(report.violations, Vec::<String>::new());
    assert_eq!(
        report.notes,
        ["members checked: 2, task IDs in docs/roadmap.md: 4"]
    );
}

#[test]
fn each_file_that_cannot_be_read_is_a_violation_and_the_check_goes_on() {
    let (tree, mut packages) = repository(
        "crates-unread",
        &["core", "sys", "text", "gfx", "font", "ui"],
    );
    // Missing.
    fs::remove_file(tree.path().join("crates/sys/README.md")).unwrap();
    // Not a regular file.
    fs::remove_file(tree.path().join("crates/text/README.md")).unwrap();
    fs::create_dir(tree.path().join("crates/text/README.md")).unwrap();
    // Not UTF-8.
    tree.write("crates/gfx/README.md", b"# tidepool-gfx\n\xff\n");
    // Over 1 MiB.
    tree.write(
        "crates/font/README.md",
        format!("{}{}", readme_of("tidepool-font"), "x".repeat(1024 * 1024)),
    );
    // A manifest that is missing (cargo would have said so; the check
    // does not crash).
    fs::remove_file(tree.path().join("crates/ui/Cargo.toml")).unwrap();
    // A member whose manifest is not under the root.
    packages.push(member(
        "tidepool-far",
        PathBuf::from("/elsewhere/far/Cargo.toml"),
    ));

    let report = check(tree.path(), &graph(&tree, packages), &tasks());
    let far = walk::shown(Path::new("/elsewhere/far/Cargo.toml"));
    assert_eq!(
        report.violations,
        [
            format!(
                "workspace member `tidepool-far` has its manifest at {far}, which is not under \
                 the root, so it was not checked"
            ),
            "crates/font/README.md cannot be read: the file is larger than the 1048576-byte \
             limit"
                .to_string(),
            "crates/gfx/README.md cannot be read: the file is not UTF-8".to_string(),
            "crates/sys/README.md is missing".to_string(),
            "crates/text/README.md cannot be read: it is not a regular file".to_string(),
            "crates/ui/Cargo.toml is missing".to_string(),
        ]
    );
    assert_eq!(
        report.notes,
        [
            "members checked: 6, task IDs in docs/roadmap.md: 4",
            "ADR-0023 (docs/decisions.md) gives the shape of a member's manifest and README, and \
             docs/04-quality.md (`crates`) the rules this check applies.",
        ]
    );
}

#[test]
fn a_linked_readme_manifest_or_member_directory_is_never_read() {
    let (tree, packages) = repository("crates-linked", &["core", "sys", "text"]);
    // Each link leads to a file that keeps the rules, so only the link can
    // fail.
    let outside = TempTree::new("crates-linked-target");
    outside.write("README.md", readme_of("tidepool-core"));
    outside.write("Cargo.toml", manifest_of("tidepool-sys"));
    outside.write("text/Cargo.toml", manifest_of("tidepool-text"));
    outside.write("text/README.md", readme_of("tidepool-text"));
    fs::remove_file(tree.path().join("crates/core/README.md")).unwrap();
    fs::remove_file(tree.path().join("crates/sys/Cargo.toml")).unwrap();
    fs::remove_dir_all(tree.path().join("crates/text")).unwrap();
    for (link, target) in [
        ("crates/core/README.md", outside.path().join("README.md")),
        ("crates/sys/Cargo.toml", outside.path().join("Cargo.toml")),
        ("crates/text", outside.path().join("text")),
    ] {
        if let Err(reason) = symlink(&tree, link, &target) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: a_linked_readme_manifest_or_member_directory_is_never_read: {reason}"
            );
            return;
        }
    }
    let report = check(tree.path(), &graph(&tree, packages), &tasks());
    let link = |rel: &str, path: &str| {
        format!("{rel} cannot be read: {path} is a symbolic link, which xtask does not follow")
    };
    assert_eq!(
        report.violations,
        [
            link("crates/core/README.md", "crates/core/README.md"),
            link("crates/sys/Cargo.toml", "crates/sys/Cargo.toml"),
            link("crates/text/Cargo.toml", "crates/text"),
            link("crates/text/README.md", "crates/text"),
        ]
    );
}

#[test]
fn the_root_manifest_and_each_member_are_checked_in_path_order() {
    let (tree, mut packages) = repository("crates-order", &["text", "core"]);
    tree.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"3\"\n\n[workspace.package]\nedition = \"2021\"\n",
    );
    tree.write(
        "crates/text/README.md",
        readme_of("tidepool-text").replace("## Design", "## Designs"),
    );
    tree.write(
        "crates/core/Cargo.toml",
        manifest_of("tidepool-core").replace("publish = false\n", ""),
    );
    packages.reverse();
    let report = check(tree.path(), &graph(&tree, packages), &tasks());
    assert_eq!(
        report.violations,
        [
            "Cargo.toml sets `workspace.package.edition` to something other than \"2024\": write \
             `edition = \"2024\"` in `[workspace.package]`, the edition every member inherits",
            "crates/core/Cargo.toml has no `package.publish`: write `publish = false` in \
             `[package]`, since no member is published",
            "crates/text/README.md has no `## Design` line outside code blocks, HTML blocks and \
             comments",
        ]
    );
}

#[test]
fn a_member_at_the_root_reads_the_root_s_files() {
    let tree = TempTree::new("crates-root-member");
    tree.write(
        "Cargo.toml",
        format!(
            "{}\n[workspace]\n\n[workspace.package]\nedition = \"2024\"\n",
            manifest_of("solo")
        ),
    );
    tree.write("README.md", readme_of("solo"));
    let packages = vec![member("solo", tree.path().join("Cargo.toml"))];
    let report = check(tree.path(), &graph(&tree, packages), &tasks());
    assert_eq!(report.violations, Vec::<String>::new());
    assert_eq!(
        report.notes,
        ["members checked: 1, task IDs in docs/roadmap.md: 4"]
    );
}

/// Reports a part of a test that cannot run here: never a pass.
fn skipped(test: &str, reason: &str) {
    let _ = writeln!(io::stderr(), "SKIPPED: {test}: {reason}");
}

/// The violation for a root manifest that is a symbolic link.
const LINKED_ROOT: &str =
    "Cargo.toml cannot be read: Cargo.toml is a symbolic link, which xtask does not follow";

#[test]
fn a_root_that_is_a_member_has_its_manifest_read_and_reported_once() {
    let tree = TempTree::new("crates-root-once");
    tree.write("README.md", readme_of("solo"));
    let packages = || vec![member("solo", tree.path().join("Cargo.toml"))];
    // A manifest that does not parse: one violation, not one as the
    // workspace's and one as the member's.
    tree.write("Cargo.toml", "[package\nname = \"solo\"\n");
    let report = check(tree.path(), &graph(&tree, packages()), &tasks());
    assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    assert!(
        report.violations[0].starts_with("Cargo.toml cannot be parsed: TOML error at line 1"),
        "{:?}",
        report.violations
    );
    // A linked one, which would pass if it were read.
    let outside = TempTree::new("crates-root-once-target");
    outside.write(
        "Cargo.toml",
        format!(
            "{}\n[workspace]\n\n[workspace.package]\nedition = \"2024\"\n",
            manifest_of("solo")
        ),
    );
    fs::remove_file(tree.path().join("Cargo.toml")).unwrap();
    if let Err(reason) = symlink(&tree, "Cargo.toml", &outside.path().join("Cargo.toml")) {
        skipped(
            "a_root_that_is_a_member_has_its_manifest_read_and_reported_once (link)",
            &reason,
        );
        return;
    }
    let report = check(tree.path(), &graph(&tree, packages()), &tasks());
    assert_eq!(report.violations, [LINKED_ROOT]);
    assert_eq!(
        report.notes[0],
        "members checked: 1, task IDs in docs/roadmap.md: 4"
    );
}

#[test]
fn a_linked_root_manifest_is_a_violation_and_the_members_are_still_checked() {
    let (tree, mut packages) = repository("crates-root-linked", &["core", "text"]);
    let outside = TempTree::new("crates-root-linked-target");
    outside.write(
        "Cargo.toml",
        "[workspace]\nresolver = \"3\"\n\n[workspace.package]\nedition = \"2024\"\n",
    );
    fs::remove_file(tree.path().join("Cargo.toml")).unwrap();
    if let Err(reason) = symlink(&tree, "Cargo.toml", &outside.path().join("Cargo.toml")) {
        skipped(
            "a_linked_root_manifest_is_a_violation_and_the_members_are_still_checked",
            &reason,
        );
        return;
    }
    tree.write(
        "crates/text/Cargo.toml",
        manifest_of("tidepool-text").replace("publish = false\n", ""),
    );
    packages.reverse();
    let report = check(tree.path(), &graph(&tree, packages), &tasks());
    assert_eq!(
        report.violations,
        [
            LINKED_ROOT,
            "crates/text/Cargo.toml has no `package.publish`: write `publish = false` in \
             `[package]`, since no member is published",
        ]
    );
}

#[test]
fn the_check_cannot_run_without_a_readable_roadmap() {
    // The roadmap is read before cargo is asked anything.
    let tree = TempTree::new("crates-no-roadmap");
    let error = run(tree.path()).expect_err("there is no roadmap");
    assert!(
        matches!(&error, Error::Read { source, .. } if source.kind() == io::ErrorKind::NotFound),
        "{error:?}"
    );
    assert!(error.to_string().starts_with("cannot read `"), "{error}");

    tree.write("docs/roadmap.md", "# Roadmap\n\nNothing planned.\n");
    let error = run(tree.path()).expect_err("there is no task");
    assert!(
        error
            .to_string()
            .starts_with("docs/roadmap.md: no line starts "),
        "{error}"
    );

    fs::remove_file(tree.path().join("docs/roadmap.md")).unwrap();
    fs::create_dir(tree.path().join("docs/roadmap.md")).unwrap();
    let error = run(tree.path()).expect_err("the roadmap is a directory");
    assert!(
        error.to_string().ends_with(": it is not a regular file"),
        "{error}"
    );

    // A linked roadmap is named, and not read.
    let outside = TempTree::new("crates-roadmap-target");
    outside.write(
        "roadmap.md",
        "- [ ] **M0.1 — this line must never be printed**\n",
    );
    fs::remove_dir(tree.path().join("docs/roadmap.md")).unwrap();
    if let Err(reason) = symlink(&tree, "docs/roadmap.md", &outside.path().join("roadmap.md")) {
        let _ = writeln!(
            io::stderr(),
            "SKIPPED: the_check_cannot_run_without_a_readable_roadmap (link): {reason}"
        );
        return;
    }
    let error = run(tree.path()).expect_err("the roadmap is a link");
    let message = error.to_string();
    assert!(
        message.ends_with(": docs/roadmap.md is a symbolic link, which xtask does not follow"),
        "{message}"
    );
    assert!(!message.contains("must never be printed"), "{message}");
}

#[test]
fn lexical_takes_out_dot_and_dot_dot_without_the_file_system() {
    assert_eq!(lexical(Path::new("/a/./b/../c/")), PathBuf::from("/a/c"));
    assert_eq!(lexical(Path::new("/a/b/../../..")), PathBuf::from("/"));
    assert_eq!(
        member_dir(Path::new("/r"), Path::new("/r/crates/x/../y/Cargo.toml")),
        Some("crates/y".to_string())
    );
    assert_eq!(
        member_dir(Path::new("/r"), Path::new("/r/Cargo.toml")),
        Some(String::new())
    );
    assert_eq!(
        member_dir(Path::new("/r"), Path::new("/r/../s/Cargo.toml")),
        None
    );
    assert_eq!(
        member_dir(Path::new("/r"), Path::new("/rx/Cargo.toml")),
        None
    );
}

#[test]
fn the_real_workspace_passes() {
    // `..`, as in the root `layers` is tested on: the members are found
    // all the same.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let report = run(&root).expect("the check runs on the real workspace");
    assert_eq!(report.violations, Vec::<String>::new());
    assert!(
        report.notes[0].starts_with("members checked: 26, task IDs in docs/roadmap.md: "),
        "{:?}",
        report.notes
    );
}

/// Whether a task ID starts at byte `at` of `text`, by the definition
/// written out again (not through [`leading_id`]): `M` with no ASCII letter,
/// digit or `_` before it, then digits, `.` and digits, each run as long as
/// it goes. Returns its length.
fn model_id_at(text: &[u8], at: usize) -> Option<usize> {
    if text.get(at) != Some(&b'M') {
        return None;
    }
    if at > 0 && matches!(text.get(at - 1), Some(b) if b.is_ascii_alphanumeric() || *b == b'_') {
        return None;
    }
    let mut end = at + 1;
    while text.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == at + 1 || text.get(end) != Some(&b'.') {
        return None;
    }
    end += 1;
    let minor = end;
    while text.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    (end > minor).then_some(end - at)
}

#[test]
fn the_scanners_keep_their_rules_on_generated_text() {
    // Pieces that make task IDs, near misses, fences, sections, task lines,
    // line endings and characters that no message may hold.
    const PIECES: &[&str] = &[
        "M",
        "1",
        "23",
        ".",
        "x",
        "_",
        "Ä",
        " ",
        "\n",
        "\r\n",
        "\r",
        "\u{feff}",
        "```",
        "~~~",
        "    ",
        "\t",
        "## Status",
        "## Design",
        "# tidepool-core",
        "- [ ] **",
        "- [x] **",
        " — ",
        "—",
        "`",
        "M1.2",
        "M0.10",
    ];
    tidepool_core::testing::check_sized(
        "crates_scanners",
        2000,
        |rng, size| {
            let mut text = String::new();
            for _ in 0..rng.range(0..size + 1) {
                text.push_str(rng.choose(PIECES).copied().unwrap_or(""));
            }
            text
        },
        |text| {
            // Every ID the model finds, and nothing else, in order.
            let bytes = text.as_bytes();
            let expected: Vec<&str> = (0..bytes.len())
                .filter_map(|at| Some(&text[at..at + model_id_at(bytes, at)?]))
                .collect();
            let found = task_ids(text);
            if found != expected {
                return Err(format!("task_ids gave {found:?}, the model {expected:?}"));
            }
            // Unknown IDs: each once, strictly in numeric order.
            let unknown = unknown_ids(text, &tasks());
            if let Some(pair) = unknown
                .windows(2)
                .find(|pair| numeric_order(pair[0]) >= numeric_order(pair[1]))
            {
                return Err(format!("unknown IDs out of order or repeated: {pair:?}"));
            }
            // Each roadmap task is an ID on a task line.
            if let Ok(found) = roadmap_tasks(text) {
                for task in &found {
                    let on_a_line = text.split('\n').any(|line| {
                        ["- [ ] **", "- [x] **"]
                            .iter()
                            .any(|prefix| line.starts_with(&format!("{prefix}{task} — ")))
                    });
                    if leading_id(task) != Some(task.as_str()) || !on_a_line {
                        return Err(format!("{task} is not a task line's ID"));
                    }
                }
            }
            // A README's violations quote nothing of it: none of these
            // characters is in a message, a path, the name or an ID.
            let violations = readme(text);
            if let Some(violation) = violations
                .iter()
                .find(|violation| violation.contains(['Ä', '~', '_', '\r', '\n', '\u{feff}', '\t']))
            {
                return Err(format!("a violation quotes the text: {violation:?}"));
            }
            let first = text.strip_prefix('\u{feff}').unwrap_or(text);
            let first = first.split('\n').next().unwrap_or("");
            let first_ok = first.strip_suffix('\r').unwrap_or(first) == "# tidepool-core";
            if violations.contains(&FIRST_LINE.to_string()) == first_ok {
                return Err(format!("the first-line rule is wrong for {first:?}"));
            }
            Ok(())
        },
    );
}
