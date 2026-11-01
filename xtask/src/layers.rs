//! `cargo xtask layers`: workspace crates depend only on what the
//! dependency matrix in `docs/01-architecture.md` allows.
//!
//! Ported from nadir xtask `layers.rs`. Adapted for Tidepool: the matrix is
//! parsed from the document on every run ([`crate::matrix`]) instead of
//! being kept in code, rows for crates that are not workspace members are
//! errors, and the "used only as a dev-dependency" note is enforced.
//!
//! The document says "A crate may depend on the workspace crates listed in
//! its row and nothing else". It makes no exception for dev- or
//! build-dependencies, so every declared dependency counts, of every kind
//! and for every target platform. The check fails when:
//!
//! - a workspace member has no row, or a row names a crate that is not a
//!   workspace member;
//! - a member declares a dependency on a workspace crate its row does not
//!   list (`tidepool-core -> tidepool-text not allowed`), or on a path
//!   crate that is not a workspace member at all;
//! - a member uses a crate whose row says it is "used only as a
//!   dev-dependency" from `[dependencies]` or `[build-dependencies]`.
//!
//! A path dependency is matched to a member by its directory, not by its
//! name, so a vendored crate that takes a member's name is not mistaken for
//! that member. A message names that directory as resolved, relative to the
//! resolved workspace root (`vendor/tidepool-core`), so that it reads the
//! same whatever links or OS path forms lead there. A registry or git dependency whose name is a member's is
//! checked as an edge to that member, since a `[patch]` can serve the
//! member under it; other registry and git dependencies are the `deps`
//! check's business.
//!
//! Only the root workspace is checked: the matrix has no rows for the
//! crates of other workspaces (`agents/`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::check::Report;
use crate::error::Error;
use crate::matrix::{self, Matrix};
use crate::metadata::{self, Dependency, Kind, Metadata, Resolve};
use crate::walk;

/// The largest architecture document read, in bytes.
const MAX_DOC_BYTES: u64 = 4 * 1024 * 1024;

/// Runs the check on the repository at `root`: its root workspace against
/// the matrix in its [`matrix::DOC`].
///
/// # Errors
///
/// [`Error::Read`] or [`Error::Doc`] if the document cannot be read or
/// parsed, and the errors of [`metadata::load`].
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let text = walk::read_required(&root.join(matrix::DOC), MAX_DOC_BYTES)?;
    let matrix = matrix::parse(&text)?;
    let metadata = metadata::load(root, &root.join("Cargo.toml"), Resolve::No)?;
    Ok(check(&metadata, &matrix, metadata::canonical))
}

/// The workspace member that `dependency` names: for a path dependency,
/// the member in the directory it names (`locate` resolves directories);
/// for a registry or git one, the member with its name. `Err` explains why
/// a path dependency is no member at all, naming its resolved directory
/// relative to `root`, the resolved workspace root; `Ok(None)` is a
/// registry or git dependency on something else.
fn target<'m>(
    dependency: &Dependency,
    by_dir: &BTreeMap<PathBuf, &'m str>,
    by_name: &BTreeSet<&'m str>,
    root: &Path,
    locate: fn(&Path) -> PathBuf,
) -> Result<Option<&'m str>, String> {
    let name = dependency.name.as_str();
    if dependency.source.is_some() {
        return Ok(by_name.get(name).copied());
    }
    let Some(dir) = &dependency.path else {
        return Err(format!(
            "cargo reports no directory for the path dependency `{name}`, so it cannot be checked"
        ));
    };
    let dir = locate(dir);
    by_dir.get(&dir).copied().map(Some).ok_or_else(|| {
        format!(
            "`{name}` is a path crate that is not a workspace member ({})",
            walk::relative(root, &dir)
        )
    })
}

/// Compares the dependencies each workspace member declares with its row of
/// `matrix`. `locate` resolves a directory's real path.
fn check(metadata: &Metadata, matrix: &Matrix, locate: fn(&Path) -> PathBuf) -> Report {
    let member_packages = || metadata.packages.iter().filter(|package| package.is_member);
    let root = locate(&metadata.workspace_root);
    let members: BTreeSet<&str> = member_packages()
        .map(|package| package.name.as_str())
        .collect();
    let by_dir: BTreeMap<PathBuf, &str> = member_packages()
        .filter_map(|package| {
            let dir = package.manifest_path.parent()?;
            Some((locate(dir), package.name.as_str()))
        })
        .collect();
    // A set: the same edge can be declared once per target platform.
    let mut violations = BTreeSet::new();

    for row in &matrix.rows {
        if !members.contains(row.name.as_str()) {
            violations.insert(format!(
                "{}:{}: the matrix has a row for `{}`, which is not a workspace member",
                matrix::DOC,
                row.line,
                row.name
            ));
        }
    }

    let mut edges = 0;
    for package in metadata.packages.iter().filter(|package| package.is_member) {
        let from = package.name.as_str();
        let Some(row) = matrix.row(from) else {
            violations.insert(format!(
                "workspace member `{from}` has no row in the dependency matrix ({})",
                matrix::DOC
            ));
            continue;
        };
        for dependency in &package.dependencies {
            let kind = dependency.kind.describe();
            let to = match target(dependency, &by_dir, &members, &root, locate) {
                Ok(Some(to)) => to,
                Ok(None) => continue,
                Err(problem) => {
                    edges += 1;
                    violations.insert(format!(
                        "{from} -> {} not allowed ({kind}): {problem}",
                        dependency.name
                    ));
                    continue;
                }
            };
            edges += 1;
            if !row.allows(to) {
                let allowed = match row.written.as_str() {
                    "—" | "-" => "no workspace crate",
                    written => written,
                };
                violations.insert(format!(
                    "{from} -> {to} not allowed ({kind}): the matrix row for `{from}` \
                     ({}:{}) allows {allowed}",
                    matrix::DOC,
                    row.line
                ));
            }
            if dependency.kind != Kind::Dev && matrix.row(to).is_some_and(|target| target.dev_only)
            {
                violations.insert(format!(
                    "{from} -> {to} not allowed ({kind}): the matrix says `{to}` is used only \
                     as a dev-dependency"
                ));
            }
        }
    }

    let mut report = Report {
        notes: vec![format!(
            "workspace crates: {}, matrix rows: {}, workspace edges checked: {edges}",
            members.len(),
            matrix.rows.len()
        )],
        violations: violations.into_iter().collect(),
        ..Report::default()
    };
    if !report.violations.is_empty() {
        report.notes.push(format!(
            "The matrix is in {}. A new edge is a design change: ask before adding one, and \
             never edit the matrix to make code compile.",
            matrix::DOC
        ));
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::Package;
    use crate::metadata::testing::{ROOT, graph, member, third_party};

    /// Takes a directory as already resolved.
    fn as_given(path: &Path) -> PathBuf {
        path.to_path_buf()
    }

    const DOC: &str = "## Dependency matrix\n\n\
        | Crate | Allowed workspace dependencies |\n\
        | :- | :- |\n\
        | `tidepool-core` | — |\n\
        | `tidepool-sys` | — |\n\
        | `tidepool-platform` | `core`, `sys` |\n\
        | `tidepool-text` | `core` |\n\
        | `tidepool` (app) | all crates above |\n\
        | `tidepool-bench` | any workspace crate |\n\
        | `tidepool-testkit` | `core` (used only as a dev-dependency) |\n";

    fn all_members() -> Vec<Package> {
        [
            "tidepool-core",
            "tidepool-sys",
            "tidepool-platform",
            "tidepool-text",
            "tidepool",
            "tidepool-bench",
            "tidepool-testkit",
        ]
        .into_iter()
        .map(member)
        .collect()
    }

    /// The violations for a workspace of every crate in [`DOC`], with
    /// `change` applied to its members.
    fn violations(change: impl FnOnce(&mut Vec<Package>)) -> Vec<String> {
        let mut packages = all_members();
        change(&mut packages);
        let matrix = matrix::parse(DOC).expect("the test matrix parses");
        check(&graph(packages), &matrix, as_given).violations
    }

    /// Adds a path dependency of `from` on the directory of the member
    /// `to`.
    fn add(packages: &mut [Package], from: &str, to: &str, kind: Kind) {
        let package = packages
            .iter_mut()
            .find(|package| package.name == from)
            .expect("the member exists");
        let with = std::mem::replace(package, member("placeholder")).path_dep(to, kind);
        *package = with;
    }

    #[test]
    fn dependencies_inside_the_matrix_pass() {
        let matrix = matrix::parse(DOC).unwrap();
        let mut packages = all_members();
        add(
            &mut packages,
            "tidepool-platform",
            "tidepool-core",
            Kind::Normal,
        );
        add(
            &mut packages,
            "tidepool-platform",
            "tidepool-sys",
            Kind::Build,
        );
        add(&mut packages, "tidepool-text", "tidepool-core", Kind::Dev);
        add(&mut packages, "tidepool", "tidepool-platform", Kind::Normal);
        add(&mut packages, "tidepool-bench", "tidepool", Kind::Normal);
        add(
            &mut packages,
            "tidepool-bench",
            "tidepool-testkit",
            Kind::Dev,
        );
        add(
            &mut packages,
            "tidepool-testkit",
            "tidepool-core",
            Kind::Normal,
        );
        let report = check(&graph(packages), &matrix, as_given);
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes,
            ["workspace crates: 7, matrix rows: 7, workspace edges checked: 7"]
        );
    }

    #[test]
    fn an_edge_outside_the_row_names_the_exact_edge() {
        assert_eq!(
            violations(|packages| add(packages, "tidepool-core", "tidepool-text", Kind::Normal)),
            [
                "tidepool-core -> tidepool-text not allowed (normal dependency): the matrix row \
              for `tidepool-core` (docs/01-architecture.md:5) allows no workspace crate"
            ]
        );
        assert_eq!(
            violations(|packages| add(packages, "tidepool-text", "tidepool-sys", Kind::Normal)),
            [
                "tidepool-text -> tidepool-sys not allowed (normal dependency): the matrix row \
              for `tidepool-text` (docs/01-architecture.md:8) allows `core`"
            ]
        );
    }

    #[test]
    fn dev_and_build_dependencies_count() {
        assert_eq!(
            violations(|packages| {
                add(packages, "tidepool-core", "tidepool-text", Kind::Dev);
                add(packages, "tidepool-sys", "tidepool-core", Kind::Build);
            }),
            [
                "tidepool-core -> tidepool-text not allowed (dev-dependency): the matrix row \
                 for `tidepool-core` (docs/01-architecture.md:5) allows no workspace crate",
                "tidepool-sys -> tidepool-core not allowed (build-dependency): the matrix row \
                 for `tidepool-sys` (docs/01-architecture.md:6) allows no workspace crate",
            ]
        );
    }

    #[test]
    fn testkit_is_not_an_implicit_exception() {
        // No row but bench's lists testkit, so nothing else may use it,
        // even as a dev-dependency: the document makes no such exception.
        assert_eq!(
            violations(|packages| add(packages, "tidepool-text", "tidepool-testkit", Kind::Dev)),
            [
                "tidepool-text -> tidepool-testkit not allowed (dev-dependency): the matrix row \
              for `tidepool-text` (docs/01-architecture.md:8) allows `core`"
            ]
        );
    }

    #[test]
    fn a_dev_only_crate_may_not_be_a_normal_or_build_dependency() {
        assert_eq!(
            violations(|packages| {
                add(packages, "tidepool-bench", "tidepool-testkit", Kind::Normal);
                add(packages, "tidepool-bench", "tidepool-testkit", Kind::Build);
            }),
            [
                "tidepool-bench -> tidepool-testkit not allowed (build-dependency): the matrix \
                 says `tidepool-testkit` is used only as a dev-dependency",
                "tidepool-bench -> tidepool-testkit not allowed (normal dependency): the matrix \
                 says `tidepool-testkit` is used only as a dev-dependency",
            ]
        );
    }

    #[test]
    fn all_crates_above_means_the_rows_above() {
        assert_eq!(
            violations(|packages| {
                add(packages, "tidepool", "tidepool-text", Kind::Normal);
                add(packages, "tidepool", "tidepool-bench", Kind::Normal);
            }),
            [
                "tidepool -> tidepool-bench not allowed (normal dependency): the matrix row for \
              `tidepool` (docs/01-architecture.md:9) allows all crates above"
            ]
        );
    }

    #[test]
    fn members_and_rows_must_match() {
        assert_eq!(
            violations(|packages| {
                packages.retain(|package| package.name != "tidepool-sys");
                packages.push(member("tidepool-net"));
            }),
            [
                "docs/01-architecture.md:6: the matrix has a row for `tidepool-sys`, which is not \
                 a workspace member",
                "workspace member `tidepool-net` has no row in the dependency matrix \
                 (docs/01-architecture.md)",
            ]
        );
    }

    #[test]
    fn a_path_crate_outside_the_workspace_is_not_allowed() {
        let mut vendored = third_party("left-pad", "1.0.3");
        vendored.source = None;
        assert_eq!(
            violations(|packages| {
                add(packages, "tidepool-bench", "left-pad", Kind::Normal);
                packages.push(vendored);
            }),
            [
                "tidepool-bench -> left-pad not allowed (normal dependency): `left-pad` is a path \
              crate that is not a workspace member (crates/left-pad)"
            ]
        );
    }

    #[test]
    fn a_path_crate_is_matched_by_its_directory_not_its_name() {
        // A vendored crate named like a member, in another directory.
        assert_eq!(
            violations(|packages| {
                if let Some(text) = packages.iter_mut().find(|p| p.name == "tidepool-text") {
                    let mut dependency = member("x")
                        .path_dep("tidepool-core", Kind::Normal)
                        .dependencies
                        .remove(0);
                    dependency.path = Some(PathBuf::from(format!("{ROOT}/vendor/core")));
                    text.dependencies.push(dependency);
                }
            }),
            [
                "tidepool-text -> tidepool-core not allowed (normal dependency): `tidepool-core` \
                 is a path crate that is not a workspace member (vendor/core)"
            ]
        );
        // A path dependency cargo gives no directory for cannot be checked.
        assert_eq!(
            violations(|packages| {
                add(packages, "tidepool-text", "tidepool-core", Kind::Normal);
                if let Some(text) = packages.iter_mut().find(|p| p.name == "tidepool-text") {
                    text.dependencies[0].path = None;
                }
            }),
            [
                "tidepool-text -> tidepool-core not allowed (normal dependency): cargo reports no \
                 directory for the path dependency `tidepool-core`, so it cannot be checked"
            ]
        );
    }

    #[test]
    fn a_path_crate_is_named_by_its_resolved_directory() {
        // Cargo reports the directory through a link (`/link/tidepool` to
        // `/work/tidepool`), or, on Windows, without the `\\?\` prefix
        // that resolving adds: the message names the resolved directory,
        // relative to the resolved workspace root, either way.
        fn through_link(path: &Path) -> PathBuf {
            match path.strip_prefix("/link/tidepool") {
                Ok(rest) => Path::new(ROOT).join(rest),
                Err(_) => path.to_path_buf(),
            }
        }
        let mut packages = all_members();
        if let Some(text) = packages.iter_mut().find(|p| p.name == "tidepool-text") {
            let mut dependency = member("x")
                .path_dep("tidepool-core", Kind::Normal)
                .dependencies
                .remove(0);
            dependency.path = Some(PathBuf::from("/link/tidepool/vendor/tidepool-core"));
            text.dependencies.push(dependency);
        }
        let mut metadata = graph(packages);
        metadata.workspace_root = PathBuf::from("/link/tidepool");
        let matrix = matrix::parse(DOC).unwrap();
        assert_eq!(
            check(&metadata, &matrix, through_link).violations,
            [
                "tidepool-text -> tidepool-core not allowed (normal dependency): `tidepool-core` \
                 is a path crate that is not a workspace member (vendor/tidepool-core)"
            ]
        );
        // One outside the workspace root is shown whole.
        if let Some(text) = metadata
            .packages
            .iter_mut()
            .find(|p| p.name == "tidepool-text")
        {
            text.dependencies[0].path = Some(PathBuf::from("/elsewhere/tidepool-core"));
        }
        assert_eq!(
            check(&metadata, &matrix, through_link).violations,
            [
                "tidepool-text -> tidepool-core not allowed (normal dependency): `tidepool-core` \
                 is a path crate that is not a workspace member (/elsewhere/tidepool-core)"
            ]
        );
    }

    #[test]
    fn a_member_declared_as_a_registry_dependency_is_still_an_edge() {
        // `tidepool-text = "0.1"` with `[patch.crates-io] tidepool-text =
        // { path = "crates/text" }`.
        assert_eq!(
            violations(|packages| {
                if let Some(core) = packages.first_mut() {
                    core.dependencies = member("x")
                        .registry_dep("tidepool-text", "^0.1")
                        .dependencies;
                }
            }),
            [
                "tidepool-core -> tidepool-text not allowed (normal dependency): the matrix row \
                 for `tidepool-core` (docs/01-architecture.md:5) allows no workspace crate"
            ]
        );
    }

    #[test]
    fn registry_dependencies_and_non_members_are_left_alone() {
        assert_eq!(
            violations(|packages| {
                if let Some(core) = packages.first_mut() {
                    core.dependencies = member("x").registry_dep("left-pad", "^1").dependencies;
                }
                // A package outside the workspace may depend on whatever
                // it likes, and needs no row.
                packages
                    .push(third_party("left-pad", "1.0.3").path_dep("tidepool-text", Kind::Normal));
            }),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_same_edge_for_two_targets_is_reported_once() {
        assert_eq!(
            violations(|packages| {
                add(packages, "tidepool-core", "tidepool-text", Kind::Normal);
                add(packages, "tidepool-core", "tidepool-text", Kind::Normal);
            })
            .len(),
            1
        );
    }

    #[test]
    fn the_real_workspace_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let report = run(&root).expect("the check runs on the real workspace");
        assert_eq!(report.violations, Vec::<String>::new());
        assert!(
            report.notes[0].starts_with("workspace crates: 26, matrix rows: 26"),
            "{:?}",
            report.notes
        );
    }
}
