//! The parts of `cargo metadata` that the `deps` and `layers` checks read,
//! parsed with `tidepool_codec::json`.
//!
//! Ported from nadir xtask `metadata.rs`. Adapted for Tidepool: cargo runs
//! offline (and, when it resolves the graph, with `--locked`, so it never
//! rewrites a lockfile); manifest paths, dependency paths and requirements,
//! targets and the workspace root are read too, for the rules about path
//! packages, approved requirements and crate roots; and [`workspaces`]
//! finds every workspace in the repository once.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use tidepool_codec::Limits;
use tidepool_codec::json::{self, Dialect, Value};

use crate::cargo;
use crate::error::Error;
use crate::walk;

/// The source cargo reports for a package from crates.io, whichever
/// protocol fetched it (and also when a `[source]` replacement serves it).
pub(crate) const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// The resolved (or, with [`Resolve::No`], declared) package graph of one
/// workspace.
#[derive(Debug)]
pub(crate) struct Metadata {
    /// The workspace's root directory.
    pub(crate) workspace_root: PathBuf,
    /// Every package in the graph: workspace members and, when resolved,
    /// all they depend on.
    pub(crate) packages: Vec<Package>,
}

/// One package in the graph.
#[derive(Debug)]
pub(crate) struct Package {
    /// The package name, for example `tidepool-core`.
    pub(crate) name: String,
    /// The exact version.
    pub(crate) version: String,
    /// Where the package comes from: a registry or git URL, or `None` for a
    /// directory on disk.
    pub(crate) source: Option<String>,
    /// The package's `Cargo.toml`.
    pub(crate) manifest_path: PathBuf,
    /// Whether the package is a member of this workspace.
    pub(crate) is_member: bool,
    /// The dependencies the package's manifest declares, of every kind and
    /// for every target platform.
    pub(crate) dependencies: Vec<Dependency>,
    /// The package's targets: library, binaries, tests, build script...
    pub(crate) targets: Vec<Target>,
}

/// One target of a package: a crate that cargo compiles.
#[derive(Debug)]
pub(crate) struct Target {
    /// The target's name.
    pub(crate) name: String,
    /// Its kinds, such as `lib` or `custom-build`, joined by `, `.
    pub(crate) kind: String,
    /// Its crate root, the file the compiler starts from.
    pub(crate) src_path: PathBuf,
}

/// One dependency declared in a manifest.
#[derive(Debug)]
pub(crate) struct Dependency {
    /// The name of the package depended on (not a `rename`).
    pub(crate) name: String,
    /// Where it comes from, as for [`Package::source`]. This is where the
    /// manifest says it comes from: a `[patch]` or `[replace]` can serve it
    /// from somewhere else.
    pub(crate) source: Option<String>,
    /// The version requirement, such as `^1.0` or `=1.0.3`.
    pub(crate) req: String,
    /// For a path dependency, the directory it names.
    pub(crate) path: Option<PathBuf>,
    /// The manifest table it is declared in.
    pub(crate) kind: Kind,
}

/// The manifest table a dependency is declared in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    /// `[dependencies]`.
    Normal,
    /// `[dev-dependencies]`.
    Dev,
    /// `[build-dependencies]`.
    Build,
}

impl Kind {
    /// The kind in words, for messages.
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Self::Normal => "normal dependency",
            Self::Dev => "dev-dependency",
            Self::Build => "build-dependency",
        }
    }
}

/// Whether cargo resolves the whole dependency graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resolve {
    /// Every package the workspace uses, from the lockfile (`--locked`).
    Yes,
    /// Only the workspace members and what they declare (`--no-deps`).
    No,
}

/// The cargo arguments that describe the workspace of `manifest`.
pub(crate) fn args(manifest: &str, resolve: Resolve) -> Vec<&str> {
    let mut args = vec!["metadata", "--format-version", "1", "--offline"];
    match resolve {
        Resolve::Yes => args.push("--locked"),
        Resolve::No => args.push("--no-deps"),
    }
    args.extend(["--manifest-path", manifest]);
    args
}

/// Asks cargo, running in `root`, for the graph of the workspace that
/// `manifest` belongs to.
///
/// # Errors
///
/// [`Error::Spawn`] or [`Error::CommandFailed`] if cargo cannot describe the
/// workspace (for example because it cannot resolve it offline, or the
/// lockfile is out of date), [`Error::UnexpectedOutput`] if its output is
/// not what this module expects.
pub(crate) fn load(root: &Path, manifest: &Path, resolve: Resolve) -> Result<Metadata, Error> {
    let manifest = manifest.to_string_lossy();
    let args = args(&manifest, resolve);
    let output = cargo::capture(root, &args)?;
    parse(&output, &cargo::display(&args))
}

/// Reads the output of `cargo metadata --format-version 1`. `command` names
/// the command for messages.
///
/// # Errors
///
/// [`Error::UnexpectedOutput`] if `text` is not JSON, lacks a field this
/// module reads, or has no workspace member.
pub(crate) fn parse(text: &[u8], command: &str) -> Result<Metadata, Error> {
    let unexpected = |detail: String| Error::UnexpectedOutput {
        command: command.to_string(),
        detail,
    };
    let root = json::parse_bytes(text, Dialect::Json, Limits::DEFAULT)
        .map_err(|e| unexpected(e.to_string()))?;
    let members = array(&root, "workspace_members")
        .map_err(unexpected)?
        .iter()
        .map(|id| {
            id.as_str()
                .ok_or_else(|| unexpected("a workspace member id is not a string".to_string()))
        })
        .collect::<Result<Vec<&str>, Error>>()?;
    let workspace_root = PathBuf::from(string(&root, "workspace_root").map_err(unexpected)?);

    let mut packages = Vec::new();
    for package in array(&root, "packages").map_err(unexpected)? {
        packages.push(read_package(package, &members).map_err(unexpected)?);
    }

    // An empty graph would let every check pass without checking anything.
    if !packages.iter().any(|package| package.is_member) {
        return Err(unexpected("no package is a workspace member".to_string()));
    }
    Ok(Metadata {
        workspace_root,
        packages,
    })
}

fn read_package(package: &Value, members: &[&str]) -> Result<Package, String> {
    let id = string(package, "id")?;
    let name = string(package, "name")?.to_string();
    let version = string(package, "version")?.to_string();
    let source = optional_string(package, "source")?;
    let manifest_path = PathBuf::from(string(package, "manifest_path")?);
    let mut dependencies = Vec::new();
    for dependency in array(package, "dependencies")? {
        // Cargo leaves `path` out for a dependency that is not a path one.
        let path = match dependency.get("path") {
            None => None,
            Some(_) => optional_string(dependency, "path")?.map(PathBuf::from),
        };
        dependencies.push(Dependency {
            name: string(dependency, "name")?.to_string(),
            source: optional_string(dependency, "source")?,
            req: string(dependency, "req")?.to_string(),
            path,
            kind: kind(dependency)?,
        });
    }
    let mut targets = Vec::new();
    for target in array(package, "targets")? {
        let kinds = array(target, "kind")?
            .iter()
            .map(|kind| {
                kind.as_str()
                    .ok_or_else(|| "a target kind is not a string".to_string())
            })
            .collect::<Result<Vec<&str>, String>>()?;
        targets.push(Target {
            name: string(target, "name")?.to_string(),
            kind: kinds.join(", "),
            src_path: PathBuf::from(string(target, "src_path")?),
        });
    }
    Ok(Package {
        name,
        version,
        source,
        manifest_path,
        is_member: members.contains(&id),
        dependencies,
        targets,
    })
}

/// Whether a file named `name` is a cargo manifest. Case is ignored: on
/// macOS and Windows, which ignore it by default, cargo opens a file named
/// `cargo.toml` as a workspace's `Cargo.toml`. On Linux such a file is no
/// manifest to cargo, which then cannot describe it: a violation, which is
/// the safe way to be wrong.
pub(crate) fn is_manifest_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("Cargo.toml")
}

/// Whether a file named `name` is a cargo lockfile, in any case, for the
/// reason [`is_manifest_name`] gives.
pub(crate) fn is_lockfile_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("Cargo.lock")
}

/// `path` with symbolic links and `..` resolved, or as it is if it cannot
/// be (for example because it does not exist).
pub(crate) fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// One Cargo workspace in the repository.
#[derive(Debug)]
pub(crate) struct Workspace {
    /// The manifest it was found from, relative to the repository root.
    pub(crate) shown: String,
    /// Its graph, or why cargo could not describe it.
    pub(crate) graph: Result<Metadata, Error>,
    /// When the resolved graph could not be loaded, its members and what
    /// they declare (`--no-deps`), if cargo could describe that much.
    pub(crate) declared: Option<Metadata>,
}

/// Describes, once each, the workspaces that `manifests` (every
/// `Cargo.toml` in the repository) belong to.
///
/// A manifest that is a member, or the root, of a workspace already
/// described is not described again. Any other manifest is: cargo then
/// describes the workspace it belongs to, or fails (for a package that
/// belongs to no workspace, such as vendored code), and that failure is in
/// [`Workspace::graph`]. Which workspaces are found does not depend on the
/// order of `manifests`.
pub(crate) fn workspaces(root: &Path, manifests: &[PathBuf], resolve: Resolve) -> Vec<Workspace> {
    let mut covered = BTreeSet::new();
    let mut found = Vec::new();
    for manifest in manifests {
        if covered.contains(&canonical(manifest)) {
            continue;
        }
        let graph = load(root, manifest, resolve);
        // Learn the members without resolving anything, so that each of
        // them does not fail the same way again.
        let declared = match (&graph, resolve) {
            (Err(_), Resolve::Yes) => load(root, manifest, Resolve::No).ok(),
            _ => None,
        };
        if let Some(described) = graph.as_ref().ok().or(declared.as_ref()) {
            covered.insert(canonical(&described.workspace_root.join("Cargo.toml")));
            for package in described
                .packages
                .iter()
                .filter(|package| package.is_member)
            {
                covered.insert(canonical(&package.manifest_path));
            }
        }
        covered.insert(canonical(manifest));
        found.push(Workspace {
            shown: walk::relative(root, manifest),
            graph,
            declared,
        });
    }
    found
}

fn kind(dependency: &Value) -> Result<Kind, String> {
    match optional_string(dependency, "kind")?.as_deref() {
        None => Ok(Kind::Normal),
        Some("dev") => Ok(Kind::Dev),
        Some("build") => Ok(Kind::Build),
        Some(other) => Err(format!("unknown dependency kind `{other}`")),
    }
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value, String> {
    value.get(key).ok_or_else(|| format!("`{key}` is missing"))
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], String> {
    field(value, key)?
        .as_array()
        .ok_or_else(|| format!("`{key}` is not an array"))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    field(value, key)?
        .as_str()
        .ok_or_else(|| format!("`{key}` is not a string"))
}

/// A field that is a string or `null`.
fn optional_string(value: &Value, key: &str) -> Result<Option<String>, String> {
    match field(value, key)? {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(format!("`{key}` is not a string or null")),
    }
}

/// Builders for metadata in the tests of the checks that read it.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Where the packages these builders make live.
    pub(crate) const ROOT: &str = "/work/tidepool";

    /// A workspace member with no dependencies, in `crates/<short name>`.
    pub(crate) fn member(name: &str) -> Package {
        let dir = name.strip_prefix("tidepool-").unwrap_or(name);
        Package {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            source: None,
            manifest_path: PathBuf::from(format!("{ROOT}/crates/{dir}/Cargo.toml")),
            is_member: true,
            dependencies: Vec::new(),
            targets: Vec::new(),
        }
    }

    /// A package from crates.io.
    pub(crate) fn third_party(name: &str, version: &str) -> Package {
        Package {
            name: name.to_string(),
            version: version.to_string(),
            source: Some(CRATES_IO.to_string()),
            manifest_path: PathBuf::from(format!(
                "/home/u/.cargo/registry/src/index/{name}-{version}/Cargo.toml"
            )),
            is_member: false,
            dependencies: Vec::new(),
            targets: Vec::new(),
        }
    }

    /// A graph of `packages` in a workspace at [`ROOT`].
    pub(crate) fn graph(packages: Vec<Package>) -> Metadata {
        Metadata {
            workspace_root: PathBuf::from(ROOT),
            packages,
        }
    }

    impl Package {
        /// Adds a dependency on the directory of the workspace member
        /// `name` that [`member`] makes.
        pub(crate) fn path_dep(mut self, name: &str, kind: Kind) -> Self {
            let dir = name.strip_prefix("tidepool-").unwrap_or(name);
            self.dependencies.push(Dependency {
                name: name.to_string(),
                source: None,
                req: "*".to_string(),
                path: Some(PathBuf::from(format!("{ROOT}/crates/{dir}"))),
                kind,
            });
            self
        }

        /// Adds a normal dependency on a crates.io package, with the
        /// version requirement `req`.
        pub(crate) fn registry_dep(mut self, name: &str, req: &str) -> Self {
            self.dependencies.push(Dependency {
                name: name.to_string(),
                source: Some(CRATES_IO.to_string()),
                req: req.to_string(),
                path: None,
                kind: Kind::Normal,
            });
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cut-down copy of real `cargo metadata` output: two workspace
    /// members and one registry package, with most of the fields the
    /// parser ignores removed.
    const SAMPLE: &str = r#"{
        "packages": [
            {
                "name": "tidepool-core",
                "version": "0.1.0",
                "id": "path+file:///work/tidepool/crates/core#tidepool-core@0.1.0",
                "source": null,
                "dependencies": [],
                "targets": [
                    {"kind": ["lib"], "name": "tidepool_core",
                     "src_path": "/work/tidepool/crates/core/src/lib.rs"},
                    {"kind": ["custom-build"], "name": "build-script-build",
                     "src_path": "/work/tidepool/crates/core/build.rs"}
                ],
                "manifest_path": "/work/tidepool/crates/core/Cargo.toml"
            },
            {
                "name": "tidepool-codec",
                "version": "0.1.0",
                "id": "path+file:///work/tidepool/crates/codec#tidepool-codec@0.1.0",
                "source": null,
                "dependencies": [
                    {
                        "name": "tidepool-core",
                        "source": null,
                        "req": "*",
                        "kind": "dev",
                        "rename": null,
                        "optional": false,
                        "target": null,
                        "path": "/work/tidepool/crates/core"
                    },
                    {
                        "name": "cc",
                        "source": "registry+https://github.com/rust-lang/crates.io-index",
                        "req": "^1",
                        "kind": "build",
                        "optional": false
                    },
                    {
                        "name": "left-pad",
                        "source": "registry+https://github.com/rust-lang/crates.io-index",
                        "req": "^1",
                        "kind": null,
                        "rename": "pad",
                        "optional": false,
                        "target": "cfg(windows)"
                    }
                ],
                "targets": [],
                "manifest_path": "/work/tidepool/crates/codec/Cargo.toml"
            },
            {
                "name": "left-pad",
                "version": "1.0.3",
                "id": "registry+https://github.com/rust-lang/crates.io-index#left-pad@1.0.3",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
                "dependencies": [],
                "targets": [],
                "manifest_path": "/home/u/.cargo/registry/src/index/left-pad-1.0.3/Cargo.toml"
            }
        ],
        "workspace_members": [
            "path+file:///work/tidepool/crates/core#tidepool-core@0.1.0",
            "path+file:///work/tidepool/crates/codec#tidepool-codec@0.1.0"
        ],
        "workspace_root": "/work/tidepool",
        "version": 1
    }"#;

    fn sample() -> Metadata {
        parse(SAMPLE.as_bytes(), "cargo metadata").expect("the sample parses")
    }

    fn detail(text: &str) -> String {
        match parse(text.as_bytes(), "cargo metadata") {
            Err(Error::UnexpectedOutput { command, detail }) => {
                assert_eq!(command, "cargo metadata");
                detail
            }
            other => panic!("expected an unexpected-output error, got {other:?}"),
        }
    }

    #[test]
    fn args_are_offline_and_never_rewrite_the_lockfile() {
        assert_eq!(
            args("/r/Cargo.toml", Resolve::Yes),
            [
                "metadata",
                "--format-version",
                "1",
                "--offline",
                "--locked",
                "--manifest-path",
                "/r/Cargo.toml"
            ]
        );
        assert_eq!(
            args("/r/Cargo.toml", Resolve::No),
            [
                "metadata",
                "--format-version",
                "1",
                "--offline",
                "--no-deps",
                "--manifest-path",
                "/r/Cargo.toml"
            ]
        );
    }

    #[test]
    fn reads_packages_sources_paths_and_membership() {
        let metadata = sample();
        assert_eq!(metadata.workspace_root, Path::new("/work/tidepool"));
        let summary: Vec<_> = metadata
            .packages
            .iter()
            .map(|package| {
                (
                    package.name.as_str(),
                    package.version.as_str(),
                    package.source.as_deref(),
                    package.is_member,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("tidepool-core", "0.1.0", None, true),
                ("tidepool-codec", "0.1.0", None, true),
                ("left-pad", "1.0.3", Some(CRATES_IO), false),
            ]
        );
        assert_eq!(
            metadata.packages[1].manifest_path,
            Path::new("/work/tidepool/crates/codec/Cargo.toml")
        );
    }

    #[test]
    fn reads_declared_dependencies_of_every_kind_by_package_name() {
        let metadata = sample();
        let dependencies: Vec<_> = metadata.packages[1]
            .dependencies
            .iter()
            .map(|dependency| {
                (
                    dependency.name.as_str(),
                    dependency.source.as_deref(),
                    dependency.req.as_str(),
                    dependency.path.as_deref(),
                    dependency.kind,
                )
            })
            .collect();
        // `left-pad` is renamed `pad`; the package name is what counts.
        assert_eq!(
            dependencies,
            [
                (
                    "tidepool-core",
                    None,
                    "*",
                    Some(Path::new("/work/tidepool/crates/core")),
                    Kind::Dev
                ),
                ("cc", Some(CRATES_IO), "^1", None, Kind::Build),
                ("left-pad", Some(CRATES_IO), "^1", None, Kind::Normal),
            ]
        );
        assert!(metadata.packages[0].dependencies.is_empty());
    }

    #[test]
    fn reads_every_target_and_its_crate_root() {
        let metadata = sample();
        let targets: Vec<_> = metadata.packages[0]
            .targets
            .iter()
            .map(|target| {
                (
                    target.name.as_str(),
                    target.kind.as_str(),
                    target.src_path.as_path(),
                )
            })
            .collect();
        assert_eq!(
            targets,
            [
                (
                    "tidepool_core",
                    "lib",
                    Path::new("/work/tidepool/crates/core/src/lib.rs")
                ),
                (
                    "build-script-build",
                    "custom-build",
                    Path::new("/work/tidepool/crates/core/build.rs")
                ),
            ]
        );
        assert_eq!(
            detail(&SAMPLE.replace(r#""kind": ["lib"]"#, r#""kind": "lib""#)),
            "`kind` is not an array"
        );
        assert_eq!(
            detail(&SAMPLE.replace(r#""targets": [],"#, "")),
            "`targets` is missing"
        );
    }

    #[test]
    fn the_real_sample_in_codec_parses() {
        // The `cargo metadata` sample that `tidepool-codec`'s own tests use.
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/codec/tests/json/cargo_metadata.json");
        let text = std::fs::read(&path).expect("the codec sample exists");
        let metadata = parse(&text, "cargo metadata").expect("the codec sample parses");
        assert!(metadata.packages.iter().any(|package| package.is_member));
    }

    #[test]
    fn manifest_and_lockfile_names_match_in_any_case() {
        for name in ["Cargo.toml", "cargo.toml", "CARGO.TOML"] {
            assert!(is_manifest_name(name), "{name}");
        }
        for name in ["Cargo.lock", "cargo.lock", "CARGO.LOCK"] {
            assert!(is_lockfile_name(name), "{name}");
        }
        for name in ["Cargo.toml.orig", "xCargo.toml", "Cargo.tom", "Cargo.lock"] {
            assert!(!is_manifest_name(name), "{name}");
        }
        assert!(!is_lockfile_name("Cargo.toml"));
    }

    #[test]
    fn text_that_is_not_json_is_an_error() {
        let message = detail("error: could not find `Cargo.toml`");
        assert!(
            message.starts_with("JSON error at line 1, column 1"),
            "{message}"
        );
    }

    #[test]
    fn a_missing_or_mistyped_field_is_named() {
        assert_eq!(
            detail(r#"{"packages": []}"#),
            "`workspace_members` is missing"
        );
        assert_eq!(
            detail(r#"{"workspace_members": [], "packages": []}"#),
            "`workspace_root` is missing"
        );
        assert_eq!(
            detail(r#"{"workspace_members": [], "workspace_root": "/", "packages": {}}"#),
            "`packages` is not an array"
        );
        assert_eq!(
            detail(
                r#"{"workspace_members": [], "workspace_root": "/",
                    "packages": [{"id": "a", "dependencies": []}]}"#
            ),
            "`name` is missing"
        );
        assert_eq!(
            detail(&SAMPLE.replace(r#""version": "0.1.0""#, r#""version": 1"#)),
            "`version` is not a string"
        );
        assert_eq!(
            detail(&SAMPLE.replace(r#""source": null"#, r#""source": false"#)),
            "`source` is not a string or null"
        );
        assert_eq!(
            detail(&SAMPLE.replace(r#""path": "/work/tidepool/crates/core""#, r#""path": 7"#)),
            "`path` is not a string or null"
        );
        assert_eq!(
            detail(&SAMPLE.replace(r#""req": "*","#, "")),
            "`req` is missing"
        );
        assert_eq!(
            detail(r#"{"workspace_members": [7], "workspace_root": "/", "packages": []}"#),
            "a workspace member id is not a string"
        );
        assert_eq!(
            detail(&SAMPLE.replace(
                r#""manifest_path": "/work/tidepool/crates/core/Cargo.toml""#,
                r#""manifest_path": null"#
            )),
            "`manifest_path` is not a string"
        );
    }

    #[test]
    fn an_unknown_dependency_kind_is_an_error() {
        let text = SAMPLE.replace(r#""kind": "dev""#, r#""kind": "proc""#);
        assert_eq!(detail(&text), "unknown dependency kind `proc`");
    }

    #[test]
    fn a_graph_without_workspace_members_is_an_error() {
        assert_eq!(
            detail(r#"{"workspace_members": [], "workspace_root": "/", "packages": []}"#),
            "no package is a workspace member"
        );
        // Members are matched by id, so ids that match nothing count as none.
        let text = SAMPLE.replace(
            r#""id": "path+file:///work/"#,
            r#""id": "path+file:///other/"#,
        );
        assert_eq!(detail(&text), "no package is a workspace member");
    }
}
