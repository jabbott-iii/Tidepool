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

//! The demo page's HTML: `site/index.html` with its placeholders filled
//! from the build, the gate's run, the examples' output, the roadmap and
//! the features. Everything taken from them is escaped here. Also its
//! `llms.txt`, from `site/llms.txt`: the same, as plain text for language
//! models, with control characters escaped as `xtask` escapes its messages.
//!
//! Ported from nadir xtask `site/page.rs`, without its scenes, recordings
//! and download. Adapted for Tidepool (ADR-0030): the gate's run and the
//! two examples' output in their place; the commit's facts, given by the
//! workflow or read from `.git`, or unknown; the structured data of a page
//! about Tidepool's source; and the names and classes of Tidepool's
//! template (`tp-…`).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

use tidepool_codec::json::{self, Number, Value};

use crate::features::{Feature, Status};
use crate::walk;

/// The page's address, which its metadata gives every file it names under.
pub(crate) const PAGE_URL: &str = "https://jabbott.dev/Tidepool/";

/// The repository's web address, with no `/` at the end.
pub(crate) const REPOSITORY: &str = "https://github.com/jabbott-iii/Tidepool";

/// jabbott.dev's own nodes in its structured data, which this page's refer
/// to as that site's pages do.
const SITE_ID: &str = "https://jabbott.dev/#website";
const PERSON_ID: &str = "https://jabbott.dev/#person";

/// The page's title, as `<title>` gives it before the site's name.
const TITLE: &str = "Tidepool IDE demo";

/// What the page is, as its `<meta name="description">` says it.
const DESCRIPTION: &str = "Tidepool, a zero-dependency IDE written in Rust with no third-party \
                           crates: its gate and its libraries run on each commit, its \
                           architecture, and every feature with how far it has come.";

/// What Tidepool is, in the structured data.
const SOFTWARE: &str = "A zero-dependency IDE written in Rust from the ground up, with no \
                        third-party crates: its own OS bindings, rasterizer, font renderer, UI \
                        toolkit, text engine, parsers, protocols and terminal emulator.";

/// Where the facts of the commit the page was built from came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// The workflow gave them, in the `TIDEPOOL_SITE_*` variables.
    Live {
        /// The full commit ID, 40 lowercase hex digits.
        commit: String,
        /// The committer date, `YYYY-MM-DD`.
        date: String,
        /// The first line of its message, trimmed.
        subject: String,
    },
    /// A local build: HEAD's commit, if `.git` gave it.
    Local {
        /// The full commit ID, or `None` when it could not be read.
        commit: Option<String>,
    },
}

/// The commit the page was built from, and the workspace at it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Build {
    /// The commit's facts.
    pub(crate) source: Source,
    /// The pinned toolchain, such as `1.99.0`.
    pub(crate) toolchain: String,
    /// The workspace's version, such as `0.1.0`.
    pub(crate) version: String,
    /// The workspace's packages.
    pub(crate) crates: usize,
    /// The packages from outside the workspace.
    pub(crate) third_party: usize,
    /// The OS and architecture the gate and the examples ran on, such as
    /// `Linux x86_64`.
    pub(crate) platform: String,
    /// The year of the commit, or of the build when the commit's date was
    /// not given.
    pub(crate) year: String,
}

impl Build {
    /// The full commit ID, if known.
    fn commit(&self) -> Option<&str> {
        match &self.source {
            Source::Live { commit, .. } => Some(commit),
            Source::Local { commit } => commit.as_deref(),
        }
    }

    /// The commit's first 7 characters, or `unknown`.
    pub(crate) fn short(&self) -> &str {
        self.commit()
            .map_or("unknown", |commit| commit.get(..7).unwrap_or(commit))
    }

    /// What the repository's links name the commit by: its ID, or `HEAD`,
    /// the default branch, when it is unknown.
    fn reference(&self) -> &str {
        self.commit().unwrap_or("HEAD")
    }

    /// The commit's page on GitHub, or the repository's when it is unknown.
    fn commit_url(&self) -> String {
        match self.commit() {
            Some(commit) => format!("{REPOSITORY}/commit/{commit}"),
            None => REPOSITORY.to_string(),
        }
    }

    /// Whether the workflow gave the commit's facts.
    fn is_live(&self) -> bool {
        matches!(self.source, Source::Live { .. })
    }

    /// Where the ticks were read, for the sentence above the features: the
    /// commit, or in a local build its working tree, which may hold
    /// changes HEAD does not.
    fn at(&self) -> String {
        if self.is_live() {
            self.short().to_string()
        } else {
            "this build".to_string()
        }
    }
}

/// How the gate went, as the page shows it: never a line that cargo or a
/// test printed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GateRun {
    /// The lines of `ci`'s summary, from `== summary` on, without the last
    /// line break: this tool's own lines.
    pub(crate) summary: String,
    /// The steps that passed, and all of them.
    pub(crate) steps: (usize, usize),
    /// The tests that passed, less those that printed `SKIPPED:`.
    pub(crate) passed: usize,
    /// The tests that failed.
    pub(crate) failed: usize,
    /// The tests marked `#[ignore]`.
    pub(crate) ignored: usize,
    /// The tests that printed `SKIPPED:`.
    pub(crate) skipped: usize,
    /// How long the gate took.
    pub(crate) elapsed: Duration,
}

/// A milestone of the roadmap, and how many of its tasks are ticked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Milestone {
    /// Its ID, such as `M1`.
    pub(crate) id: String,
    /// Its title, such as `Text engine and formats (headless)`.
    pub(crate) title: String,
    pub(crate) done: usize,
    pub(crate) total: usize,
}

/// Everything the page shows.
pub(crate) struct Page<'a> {
    pub(crate) build: &'a Build,
    pub(crate) gate: &'a GateRun,
    /// What `tidepool-codec`'s example `formats` printed.
    pub(crate) formats: &'a str,
    /// What `tidepool-text`'s example `editing` printed.
    pub(crate) editing: &'a str,
    pub(crate) milestones: &'a [Milestone],
    /// The roadmap's ticked tasks, and all of them.
    pub(crate) tasks: (usize, usize),
    /// The features, each with how far it has come, in the file's order.
    pub(crate) features: &'a [(Feature, Status)],
}

/// The page: `template` with each `{{name}}` replaced.
///
/// # Errors
///
/// Names a placeholder the page has no value for, or one left open.
pub(crate) fn render(template: &str, page: &Page) -> Result<String, String> {
    fill(template, &values(page))
}

/// The page as plain text, for language models: `template` with each
/// `{{name}}` replaced, unescaped but for control characters.
///
/// # Errors
///
/// As [`render`].
pub(crate) fn render_text(template: &str, page: &Page) -> Result<String, String> {
    fill(template, &text_values(page))
}

/// `template` with each `{{name}}` replaced by its value.
fn fill(template: &str, values: &BTreeMap<&str, String>) -> Result<String, String> {
    let mut out = String::with_capacity(template.len() * 2);
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| "the template has a `{{` with no `}}`".to_string())?;
        let name = &after[..end];
        let value = values.get(name).ok_or_else(|| {
            format!(
                "the template's `{{{{{}}}}}` has no value",
                walk::escape_controls(name, |_| false)
            )
        })?;
        out.push_str(value);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The features by status.
struct Counts {
    shipped: usize,
    in_progress: usize,
    planned: usize,
}

impl Counts {
    fn of(page: &Page) -> Self {
        let count = |status| {
            page.features
                .iter()
                .filter(|(_, other)| *other == status)
                .count()
        };
        Self {
            shipped: count(Status::Shipped),
            in_progress: count(Status::InProgress),
            planned: count(Status::Planned),
        }
    }

    /// The sentence above the features, whose ticks were read `at`.
    fn features_lede(&self, at: &str) -> String {
        format!(
            "{} shipped, {} in progress and {} planned. Each feature is marked from the \
             roadmap's ticks at {at}, so the list moves as the tasks land.",
            self.shipped, self.in_progress, self.planned
        )
    }
}

/// The packages from outside the workspace, as the page says it.
fn third_party(build: &Build) -> String {
    match build.third_party {
        0 => "None".to_string(),
        n => thousands(n),
    }
}

/// What the page calls the build.
fn build_kind(build: &Build) -> &'static str {
    if build.is_live() {
        "Live build"
    } else {
        "Local build"
    }
}

/// Who ran the gate and the examples, as the template's sentences name it:
/// "Run on {{platform}} by {{runner}}, …" and "Its output, as {{runner}}
/// ran it on {{platform}}."
fn runner(build: &Build) -> &'static str {
    if build.is_live() {
        "the job that published this page"
    } else {
        "a local build of the working tree"
    }
}

/// Whether `subject`, a commit's, says nothing: `git commit
/// --allow-empty-message` makes one.
fn is_blank(subject: &str) -> bool {
    subject.trim().is_empty()
}

/// The commit's date, or that it was not given.
fn commit_date(build: &Build) -> &str {
    match &build.source {
        Source::Live { date, .. } => date,
        Source::Local { .. } => "Not given",
    }
}

/// The gate's steps, as `11 of 11`.
fn gate_steps(gate: &GateRun) -> String {
    format!("{} of {}", gate.steps.0, gate.steps.1)
}

/// The value of every placeholder, as HTML.
fn values(page: &Page) -> BTreeMap<&'static str, String> {
    let build = page.build;
    let gate = page.gate;
    let counts = Counts::of(page);
    let mut values = BTreeMap::new();
    let mut set = |name, value: String| {
        values.insert(name, value);
    };
    set("structured_data", structured_data(page));
    set("year", escape(&build.year));
    set("commit", escape(build.reference()));
    set("commit_short", escape(build.short()));
    set("commit_url", escape(&build.commit_url()));
    set("repository_url", escape(REPOSITORY));
    set("build_kind", escape(build_kind(build)));
    set("runner", escape(runner(build)));
    set("platform", escape(&build.platform));
    set("commit_date", escape(commit_date(build)));
    set("latest_change", latest_change(build));
    set("toolchain", escape(&build.toolchain));
    set("crates", thousands(build.crates));
    set("third_party", escape(&third_party(build)));
    set("tasks_done", thousands(page.tasks.0));
    set("tasks_total", thousands(page.tasks.1));
    set("milestones", milestones(page.milestones));
    set("gate_summary", escape(&lines(&gate.summary)));
    set("gate_steps", escape(&gate_steps(gate)));
    set("gate_tests", thousands(gate.passed));
    set("gate_skipped", thousands(gate.skipped));
    set("gate_time", escape(&duration(gate.elapsed)));
    set("formats_output", escape(&lines(page.formats)));
    set("editing_output", escape(&lines(page.editing)));
    set(
        "feature_filters",
        feature_filters(
            page.features.len(),
            counts.shipped,
            counts.in_progress,
            counts.planned,
        ),
    );
    set("features_lede", escape(&counts.features_lede(&build.at())));
    set("features", features(page.features));
    values
}

/// The value of every placeholder of `llms.txt`, as plain text: the
/// names the page has that are not HTML, with the features and milestones
/// as Markdown lists, and `build_line` and `gate_line`. Control characters
/// are escaped, but for the line breaks and tabs of the outputs.
fn text_values(page: &Page) -> BTreeMap<&'static str, String> {
    let build = page.build;
    let gate = page.gate;
    let counts = Counts::of(page);
    let values = BTreeMap::from([
        ("year", build.year.clone()),
        ("commit", build.reference().to_string()),
        ("commit_short", build.short().to_string()),
        ("commit_url", build.commit_url()),
        ("repository_url", REPOSITORY.to_string()),
        ("build_kind", build_kind(build).to_string()),
        ("runner", runner(build).to_string()),
        ("platform", build.platform.clone()),
        ("commit_date", commit_date(build).to_string()),
        ("toolchain", build.toolchain.clone()),
        ("crates", thousands(build.crates)),
        ("third_party", third_party(build)),
        ("tasks_done", thousands(page.tasks.0)),
        ("tasks_total", thousands(page.tasks.1)),
        ("milestones", milestones_text(page.milestones)),
        ("gate_summary", gate.summary.clone()),
        ("gate_steps", gate_steps(gate)),
        ("gate_tests", thousands(gate.passed)),
        ("gate_skipped", thousands(gate.skipped)),
        ("gate_time", duration(gate.elapsed)),
        ("formats_output", page.formats.to_string()),
        ("editing_output", page.editing.to_string()),
        ("features_lede", counts.features_lede(&build.at())),
        ("features", features_text(page.features)),
        ("build_line", build_line(page)),
        ("gate_line", gate_line(page)),
    ]);
    values
        .into_iter()
        .map(|(name, value)| (name, lines(&value).into_owned()))
        .collect()
}

/// `text` with its control characters escaped as `xtask` escapes them in
/// what it prints ([`walk::escape_controls`]), but for line breaks and
/// tabs: an output's lines stay lines.
fn lines(text: &str) -> Cow<'_, str> {
    walk::escape_controls(text, |c| matches!(c, '\n' | '\t'))
}

/// The paragraph under the build's facts: the commit's subject, or that it
/// has none; or, in a local build, that what the page shows is the working
/// tree's, which may hold changes HEAD does not, and that it was given no
/// date or subject.
fn latest_change(build: &Build) -> String {
    let text = match &build.source {
        Source::Live { subject, .. } if is_blank(subject) => {
            "Latest change: a commit with no subject".to_string()
        }
        Source::Live { subject, .. } => format!(
            "Latest change: <span class=\"tp-mono\">{}</span>",
            escape(subject)
        ),
        Source::Local { commit: Some(_) } => "A local build of the working tree at HEAD; it may \
                                              hold changes HEAD does not. It was given no commit \
                                              date or subject."
            .to_string(),
        Source::Local { commit: None } => "A local build of the working tree; its HEAD could \
                                           not be read from <code>.git</code>. It was given no \
                                           commit date or subject."
            .to_string(),
    };
    format!("        <p class=\"side-panel__text tp-subject\">{text}</p>")
}

/// The roadmap's count and the commit, as a sentence of `llms.txt`: with
/// its date and subject, or in a local build as [`latest_change`] says it.
fn build_line(page: &Page) -> String {
    let build = page.build;
    let (done, total) = page.tasks;
    let tasks = format!(
        "{} of {} roadmap tasks done",
        thousands(done),
        thousands(total)
    );
    match &build.source {
        Source::Live { date, subject, .. } if is_blank(subject) => format!(
            "{tasks} at commit {} of {date}, a commit with no subject.",
            build.short()
        ),
        Source::Live { date, subject, .. } => {
            format!("{tasks} at commit {} of {date} ({subject}).", build.short())
        }
        Source::Local { commit: Some(_) } => format!(
            "{tasks}, in a local build of the working tree at HEAD, commit {}; it may hold \
             changes HEAD does not. It was given no commit date or subject.",
            build.short()
        ),
        Source::Local { commit: None } => format!(
            "{tasks}, in a local build of the working tree; its HEAD could not be read from \
             .git. It was given no commit date or subject."
        ),
    }
}

/// The gate's steps, tests, skipped tests and time, and where it ran, as
/// a clause of `llms.txt`.
fn gate_line(page: &Page) -> String {
    let gate = page.gate;
    format!(
        "{} steps passed; {} tests passed, {} failed and {} ignored, and {} printed SKIPPED \
         (not passes); in {}, on {}, run by {}",
        gate_steps(gate),
        thousands(gate.passed),
        thousands(gate.failed),
        thousands(gate.ignored),
        thousands(gate.skipped),
        duration(gate.elapsed),
        page.build.platform,
        runner(page.build)
    )
}

/// The page's structured data: one JSON-LD graph of the page, its
/// breadcrumbs and Tidepool's source. Its site and author are
/// jabbott.dev's nodes, by their `@id`, with what this page's metadata
/// says of them.
fn structured_data(page: &Page) -> String {
    let build = page.build;
    let text = |text: &str| Value::String(text.to_string());
    let id = |fragment: &str| format!("{PAGE_URL}#{fragment}");
    let person = || reference(PERSON_ID);
    let modified = match &build.source {
        Source::Live { date, .. } => Some(text(date)),
        Source::Local { .. } => None,
    };
    let mut web_page = vec![
        ("@type", text("WebPage")),
        ("@id", text(&id("page"))),
        ("url", text(PAGE_URL)),
        ("name", text(TITLE)),
        ("description", text(DESCRIPTION)),
        ("inLanguage", text("en-US")),
        ("isPartOf", reference(SITE_ID)),
        ("about", reference(&id("software"))),
        ("mainEntity", reference(&id("software"))),
        ("author", person()),
        ("publisher", person()),
    ];
    if let Some(date) = &modified {
        web_page.push(("dateModified", date.clone()));
    }
    web_page.push(("breadcrumb", reference(&id("breadcrumb"))));
    let crumb = |position: i64, name: &str, url: &str| {
        object(vec![
            ("@type", text("ListItem")),
            ("position", Value::Number(Number::from(position))),
            ("name", text(name)),
            ("item", text(url)),
        ])
    };
    let mut software = vec![
        ("@type", text("SoftwareSourceCode")),
        ("@id", text(&id("software"))),
        ("name", text("Tidepool")),
        ("description", text(SOFTWARE)),
        ("url", text(PAGE_URL)),
        ("codeRepository", text(REPOSITORY)),
        ("programmingLanguage", text("Rust")),
        // Apache-2.0, as the workspace's `Cargo.toml` says.
        (
            "license",
            text("https://www.apache.org/licenses/LICENSE-2.0"),
        ),
        ("version", text(&build.version)),
        ("creativeWorkStatus", text("In development")),
    ];
    if let Some(date) = modified {
        software.push(("dateModified", date));
    }
    software.extend([
        (
            "keywords",
            Value::Array(
                [
                    "IDE",
                    "code editor",
                    "Rust",
                    "vim",
                    "terminal UI",
                    "text engine",
                    "no dependencies",
                ]
                .map(text)
                .to_vec(),
            ),
        ),
        ("author", person()),
    ]);
    let graph = vec![
        object(web_page),
        object(vec![
            ("@type", text("BreadcrumbList")),
            ("@id", text(&id("breadcrumb"))),
            (
                "itemListElement",
                Value::Array(vec![
                    crumb(1, "Home", "https://jabbott.dev/"),
                    crumb(2, "Deployments", "https://jabbott.dev/deployments/"),
                    crumb(3, "Tidepool", PAGE_URL),
                ]),
            ),
        ]),
        object(software),
        object(vec![
            ("@type", text("WebSite")),
            ("@id", text(SITE_ID)),
            ("url", text("https://jabbott.dev/")),
            ("name", text("Joseph Abbott III")),
        ]),
        object(vec![
            ("@type", text("Person")),
            ("@id", text(PERSON_ID)),
            ("name", text("Joseph Abbott III")),
            ("url", text("https://jabbott.dev/")),
            (
                "sameAs",
                Value::Array(
                    [
                        "https://github.com/jabbott-iii",
                        "https://www.linkedin.com/in/josephabbottiii/",
                        "https://x.com/jabbottiii",
                    ]
                    .map(text)
                    .to_vec(),
                ),
            ),
        ]),
    ];
    let data = object(vec![
        ("@context", text("https://schema.org")),
        ("@graph", Value::Array(graph)),
    ]);
    script_safe(&json::to_string_pretty(&data))
}

/// A JSON object of `members`, in their order.
fn object(members: Vec<(&str, Value)>) -> Value {
    Value::Object(
        members
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

/// A reference to the node `id`.
fn reference(id: &str) -> Value {
    object(vec![("@id", Value::String(id.to_string()))])
}

/// `json` as it may stand in a `<script>`: `<`, `>` and `&`, which can
/// stand only in its strings, written as escapes, so that no `</script>`
/// or `<!--` ends or changes the element early.
fn script_safe(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if matches!(c, '<' | '>' | '&') {
            out.push('\\');
            let _ = write!(out, "u{:04x}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

/// Each milestone, with a bar of its ticked tasks.
fn milestones(milestones: &[Milestone]) -> String {
    let mut html = String::new();
    for milestone in milestones {
        let state = if milestone.done == milestone.total {
            "done"
        } else if milestone.done > 0 {
            "active"
        } else {
            "todo"
        };
        let percent = (milestone.done * 100)
            .checked_div(milestone.total)
            .unwrap_or(100);
        let _ = writeln!(
            html,
            "          <li class=\"tp-milestone\" data-state=\"{state}\"><span \
             class=\"tp-milestone__id\">{}</span><span class=\"tp-milestone__name\">{}\
             </span><span class=\"tp-milestone__count\">{}/{}</span><span \
             class=\"tp-milestone__bar\" aria-hidden=\"true\"><span style=\"width: \
             {percent}%\"></span></span></li>",
            escape(&milestone.id),
            escape(&milestone.title),
            milestone.done,
            milestone.total,
        );
    }
    trim_line_end(html)
}

/// The status filters, with how many features each shows.
fn feature_filters(all: usize, shipped: usize, in_progress: usize, planned: usize) -> String {
    let filters = [
        ("all", "All", all),
        ("shipped", "Shipped", shipped),
        ("in-progress", "In progress", in_progress),
        ("planned", "Planned", planned),
    ];
    let mut html = String::new();
    for (key, label, count) in filters {
        let active = key == "all";
        let _ = writeln!(
            html,
            "          <button class=\"chip{}\" type=\"button\" data-status-filter=\"{key}\" \
             aria-pressed=\"{active}\">{label} <span class=\"chip__count\">{count}</span></button>",
            if active { " is-active" } else { "" },
        );
    }
    trim_line_end(html)
}

/// A card for each feature.
fn features(features: &[(Feature, Status)]) -> String {
    let mut html = String::new();
    for (feature, status) in features {
        let key = status_key(*status);
        let tasks: Vec<String> = feature
            .tasks
            .iter()
            .map(|id| format!("<li class=\"tag\">{}</li>", escape(id)))
            .collect();
        let _ = write!(
            html,
            "        <article class=\"tp-feature panel\" data-status=\"{key}\">\n\
             \x20         <p class=\"kicker kicker--sm\">{area} <span class=\"kicker__sep\">//</span> \
             <span class=\"tp-status tp-status--{key}\">{status}</span></p>\n\
             \x20         <h3 class=\"tp-feature__name\">{name}</h3>\n\
             \x20         <p class=\"tp-feature__summary\">{summary}</p>\n\
             \x20         <ul class=\"tag-list tp-feature__tasks\" aria-label=\"Roadmap tasks\">{tasks}</ul>\n\
             \x20       </article>\n",
            area = escape(&feature.area),
            status = status.name(),
            name = escape(&feature.name),
            summary = escape(&feature.summary),
            tasks = tasks.join(""),
        );
    }
    trim_line_end(html)
}

/// The features, as `llms.txt` lists them: under a heading for each status
/// that has any, in the file's order.
fn features_text(features: &[(Feature, Status)]) -> String {
    let mut text = String::new();
    for status in [Status::Shipped, Status::InProgress, Status::Planned] {
        let mut first = true;
        for (feature, _) in features.iter().filter(|(_, other)| *other == status) {
            if first {
                if !text.is_empty() {
                    text.push('\n');
                }
                let _ = writeln!(text, "### {}\n", status.name());
                first = false;
            }
            let _ = writeln!(
                text,
                "- {} ({}): {} Roadmap tasks: {}.",
                feature.name,
                feature.area,
                feature
                    .summary
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                feature.tasks.join(", "),
            );
        }
    }
    trim_line_end(text)
}

/// The milestones, as `llms.txt` lists them.
fn milestones_text(milestones: &[Milestone]) -> String {
    let mut text = String::new();
    for milestone in milestones {
        let _ = writeln!(
            text,
            "- {}, {}: {} of {} tasks done",
            milestone.id, milestone.title, milestone.done, milestone.total
        );
    }
    trim_line_end(text)
}

/// The status as the page's filters and classes name it.
fn status_key(status: Status) -> &'static str {
    match status {
        Status::Shipped => "shipped",
        Status::InProgress => "in-progress",
        Status::Planned => "planned",
    }
}

/// `text` with the characters HTML gives a meaning replaced.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// `n` with commas between its thousands.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// How long something took, as the page says it: tenths of a second under
/// a minute, then whole minutes and seconds.
fn duration(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64();
    if seconds < 59.95 {
        format!("{seconds:.1} s")
    } else {
        let whole = elapsed.as_secs() + u64::from(elapsed.subsec_millis() >= 500);
        format!("{} min {} s", whole / 60, whole % 60)
    }
}

/// `html` without the line break after its last line.
fn trim_line_end(mut html: String) -> String {
    while html.ends_with('\n') {
        html.pop();
    }
    html
}

#[cfg(test)]
mod tests {
    use tidepool_codec::Limits;
    use tidepool_codec::json::Dialect;

    use super::*;

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    fn build(source: Source) -> Build {
        Build {
            source,
            toolchain: "1.99.0".to_string(),
            version: "0.1.0".to_string(),
            crates: 26,
            third_party: 0,
            platform: "Linux x86_64".to_string(),
            year: "2026".to_string(),
        }
    }

    fn live() -> Source {
        Source::Live {
            commit: COMMIT.to_string(),
            date: "2026-11-08".to_string(),
            subject: "site: the demo page <M0.14> & its \"gate\"".to_string(),
        }
    }

    fn gate() -> GateRun {
        GateRun {
            summary: "== summary\ndeps          ok            0.4 s\ntest          ok (1 \
                      SKIPPED)  120.3 s\nci: ok (1 test SKIPPED): 11 of 11 steps passed; not \
                      yet implemented: tools, theme"
                .to_string(),
            steps: (11, 11),
            passed: 1_234,
            failed: 0,
            ignored: 0,
            skipped: 1,
            elapsed: Duration::from_millis(192_400),
        }
    }

    fn feature(name: &str, tasks: &[&str]) -> Feature {
        Feature {
            name: name.to_string(),
            area: "Text".to_string(),
            summary: "Reads \"things\" <fast>.".to_string(),
            tasks: tasks.iter().map(|id| (*id).to_string()).collect(),
            ticked: Vec::new(),
        }
    }

    /// Calls `check` with a page of `source`, the gate's run, the two
    /// outputs, two milestones and two features, one shipped and one
    /// planned.
    fn with_page(source: Source, check: impl FnOnce(&Page)) {
        let build = build(source);
        let gate = gate();
        let milestones = [
            Milestone {
                id: "M0".to_string(),
                title: "Foundation and guardrails".to_string(),
                done: 13,
                total: 14,
            },
            Milestone {
                id: "M1".to_string(),
                title: "Text engine & formats (headless)".to_string(),
                done: 6,
                total: 9,
            },
        ];
        let features = [
            (feature("The rope", &["M1.1"]), Status::Shipped),
            (feature("Regex", &["M4.4", "M4.5"]), Status::Planned),
        ];
        check(&Page {
            build: &build,
            gate: &gate,
            formats: "== JSONC → JSON ==\njson │ {\"a\":\"<b>\"}\n\tcafé 🌊",
            editing: "text  \"a\\r\\nb\"\n👩\u{200d}💻 \u{1b}[31mred",
            milestones: &milestones,
            tasks: (19, 23),
            features: &features,
        });
    }

    #[test]
    fn placeholders_are_filled_and_an_unknown_or_open_one_fails() {
        let values = BTreeMap::from([("a", "1".to_string()), ("b", "<b>".to_string())]);
        assert_eq!(
            fill("x{{a}}y{{b}}{{a}}", &values),
            Ok("x1y<b>1".to_string())
        );
        assert_eq!(
            fill("no placeholders", &values),
            Ok("no placeholders".to_string())
        );
        assert_eq!(
            fill("{{c}}", &values),
            Err("the template's `{{c}}` has no value".to_string())
        );
        assert_eq!(
            fill("{{a", &values),
            Err("the template has a `{{` with no `}}`".to_string())
        );
        // A name is quoted escaped, as `xtask` prints every message.
        assert_eq!(
            fill("{{a\u{1b}[2K}}", &values),
            Err("the template's `{{a\\u{1b}[2K}}` has no value".to_string())
        );
    }

    #[test]
    fn text_is_escaped_and_numbers_are_grouped() {
        assert_eq!(escape("a<b>&\"c'"), "a&lt;b&gt;&amp;&quot;c&#39;");
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(10_203), "10,203");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(duration(Duration::from_millis(340)), "0.3 s");
        assert_eq!(duration(Duration::from_millis(59_940)), "59.9 s");
        assert_eq!(duration(Duration::from_millis(59_960)), "1 min 0 s");
        assert_eq!(duration(Duration::from_millis(192_400)), "3 min 12 s");
        assert_eq!(duration(Duration::from_millis(192_500)), "3 min 13 s");
        assert_eq!(lines("a\n\tb\u{1b}[2K\r"), "a\n\tb\\u{1b}[2K\\r");
    }

    #[test]
    fn the_template_in_the_repository_fills_with_everything_escaped() {
        with_page(live(), |page| {
            let html = render(include_str!("../../../site/index.html"), page)
                .expect("every placeholder has a value");
            assert!(!html.contains("{{"), "a placeholder is left");
            // The runs become tab panels only when the script shows the tabs.
            assert!(!html.contains("role=\"tabpanel\""));
            for expected in [
                "<a class=\"tp-mono\" href=\"https://github.com/jabbott-iii/Tidepool/commit/\
                 0123456789abcdef0123456789abcdef01234567\">0123456</a>",
                "Live build <span class=\"kicker__sep\">//</span>",
                "<p class=\"side-panel__text tp-subject\">Latest change: <span \
                 class=\"tp-mono\">site: the demo page &lt;M0.14&gt; &amp; its \
                 &quot;gate&quot;</span></p>",
                "<dd>19 of 23 tasks</dd>",
                "<dd>None</dd>",
                "<dd>2026-11-08</dd>",
                "<dd>1.99.0</dd>",
                "<dd>26 in the workspace</dd>",
                "<dd>11 of 11 passed</dd>",
                "<dd>1,234 passed</dd>",
                "<div><dt>Skipped</dt><dd>1</dd></div>",
                "<dd>3 min 12 s</dd>",
                "Run on Linux x86_64 by the job that published this page, with Rust 1.99.0.",
                "<span>Linux x86_64</span>",
                // The gate's summary, after the elided step output.
                "…each step's output…</span>\n== summary\ndeps          ok            0.4 s\n",
                "not yet implemented: tools, theme</pre>",
                // The outputs, escaped, their lines kept, a control written out.
                "--example formats\n== JSONC → JSON ==\njson │ {&quot;a&quot;:&quot;&lt;b&gt;\
                 &quot;}\n\tcafé 🌊</pre>",
                "--example editing\ntext  &quot;a\\r\\nb&quot;\n👩\u{200d}💻 \\u{1b}[31mred</pre>",
                "tidepool.css?v=0123456",
                "tidepool.js?v=0123456",
                "data-state=\"active\"",
                "style=\"width: 66%\"",
                "<span class=\"tp-milestone__name\">Text engine &amp; formats (headless)</span>",
                "Reads &quot;things&quot; &lt;fast&gt;.",
                "<span class=\"tp-status tp-status--planned\">Planned</span>",
                "<span class=\"tp-status tp-status--shipped\">Shipped</span>",
                "<ul class=\"tag-list tp-feature__tasks\" aria-label=\"Roadmap tasks\"><li \
                 class=\"tag\">M4.4</li><li class=\"tag\">M4.5</li></ul>",
                "data-status-filter=\"shipped\" aria-pressed=\"false\">Shipped <span \
                 class=\"chip__count\">1</span>",
                "1 shipped, 0 in progress and 1 planned. Each feature is marked from the \
                 roadmap&#39;s ticks at 0123456,",
                "/blob/0123456789abcdef0123456789abcdef01234567/docs/roadmap.md",
                "/tree/0123456789abcdef0123456789abcdef01234567/diagrams",
                "<span>&copy; 2026 Joseph Abbott III</span>",
            ] {
                assert!(html.contains(expected), "missing {expected:?}");
            }
            // Its metadata describes the page as the structured data does.
            assert!(html.contains(&format!(
                "<meta name=\"description\" content=\"{}\">",
                escape(DESCRIPTION)
            )));
            assert!(html.contains(&format!("<link rel=\"canonical\" href=\"{PAGE_URL}\">")));
            assert!(html.contains(&format!("<title>{TITLE} | Joseph Abbott III</title>")));
            // The build comes before the architecture, and it before the
            // features.
            let at = |id: &str| html.find(&format!("id=\"{id}\"")).expect(id);
            assert!(at("build-title") < at("arch-title"));
            assert!(at("arch-title") < at("features-title"));
        });
    }

    #[test]
    fn a_local_build_names_head_or_says_its_commit_is_unknown() {
        with_page(
            Source::Local {
                commit: Some(COMMIT.to_string()),
            },
            |page| {
                let html = render(include_str!("../../../site/index.html"), page).unwrap();
                for expected in [
                    "Local build <span class=\"kicker__sep\">//</span> <a class=\"tp-mono\" \
                     href=\"https://github.com/jabbott-iii/Tidepool/commit/\
                     0123456789abcdef0123456789abcdef01234567\">0123456</a>",
                    "<dd>Not given</dd>",
                    // What the page shows is the working tree's, not HEAD's.
                    "<p class=\"side-panel__text tp-subject\">A local build of the working tree \
                     at HEAD; it may hold changes HEAD does not. It was given no commit date or \
                     subject.</p>",
                    "Run on Linux x86_64 by a local build of the working tree, with Rust 1.99.0.",
                    "Its output, as a local build of the working tree ran it on Linux x86_64.",
                    "roadmap&#39;s ticks at this build,",
                ] {
                    assert!(html.contains(expected), "missing {expected:?}");
                }
                assert!(!html.contains("Latest change"));
                assert!(!html.contains("dateModified"));
                assert!(!html.contains("ticks at 0123456"));
                let text = render_text(include_str!("../../../site/llms.txt"), page).unwrap();
                for expected in [
                    "19 of 23 roadmap tasks done, in a local build of the working tree at HEAD, \
                     commit 0123456; it may hold changes HEAD does not. It was given no commit \
                     date or subject.",
                    "on Linux x86_64, run by a local build of the working tree.",
                    "roadmap's ticks at this build,",
                ] {
                    assert!(text.contains(expected), "missing {expected:?} in\n{text}");
                }
            },
        );
        with_page(Source::Local { commit: None }, |page| {
            let html = render(include_str!("../../../site/index.html"), page).unwrap();
            assert!(!html.contains("{{"));
            for expected in [
                "Local build <span class=\"kicker__sep\">//</span> <a class=\"tp-mono\" \
                 href=\"https://github.com/jabbott-iii/Tidepool\">unknown</a>",
                "<p class=\"side-panel__text tp-subject\">A local build of the working tree; its \
                 HEAD could not be read from <code>.git</code>. It was given no commit date or \
                 subject.</p>",
                "Run on Linux x86_64 by a local build of the working tree, with Rust 1.99.0.",
                "/blob/HEAD/docs/roadmap.md",
                "roadmap&#39;s ticks at this build,",
            ] {
                assert!(html.contains(expected), "missing {expected:?}");
            }
            let text = render_text(include_str!("../../../site/llms.txt"), page).unwrap();
            assert!(
                text.contains(
                    "19 of 23 roadmap tasks done, in a local build of the working tree; its HEAD \
                     could not be read from .git. It was given no commit date or subject."
                ),
                "{text}"
            );
        });
    }

    #[test]
    fn a_commit_with_no_subject_is_said_to_have_none() {
        // `git commit --allow-empty-message`; `given` trims a blank one to
        // nothing.
        for subject in ["", "  "] {
            let source = Source::Live {
                commit: COMMIT.to_string(),
                date: "2026-11-08".to_string(),
                subject: subject.to_string(),
            };
            with_page(source, |page| {
                let html = render(include_str!("../../../site/index.html"), page).unwrap();
                assert!(
                    html.contains(
                        "<p class=\"side-panel__text tp-subject\">Latest change: a commit with \
                         no subject</p>"
                    ),
                    "{subject:?}"
                );
                assert!(
                    !html.contains("<span class=\"tp-mono\"></span>"),
                    "{subject:?}"
                );
                let text = render_text(include_str!("../../../site/llms.txt"), page).unwrap();
                assert!(
                    text.contains(
                        "19 of 23 roadmap tasks done at commit 0123456 of 2026-11-08, a commit \
                         with no subject.\n"
                    ),
                    "{subject:?}: {text}"
                );
                assert!(!text.contains("2026-11-08 ("), "{subject:?}: {text}");
            });
        }
    }

    /// The node of type `kind` in `graph`.
    fn node<'a>(graph: &'a [Value], kind: &str) -> &'a Value {
        graph
            .iter()
            .find(|node| node.get("@type").and_then(Value::as_str) == Some(kind))
            .unwrap_or_else(|| panic!("no {kind} node"))
    }

    /// The string `key` of the object `value`.
    fn string<'a>(value: &'a Value, key: &str) -> &'a str {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("no string `{key}`"))
    }

    /// The structured data of the page `html`, and the text it stands in.
    fn data_of(html: &str) -> (Value, &str) {
        let (_, rest) = html
            .split_once("<script type=\"application/ld+json\">")
            .expect("the structured data");
        let (data, _) = rest.split_once("</script>").expect("its end");
        let value = json::parse(data, Dialect::Json, Limits::DEFAULT).expect("JSON");
        (value, data)
    }

    #[test]
    fn the_structured_data_is_json_ld_of_the_page_and_tidepool_s_source() {
        with_page(live(), |page| {
            let html = render(include_str!("../../../site/index.html"), page).unwrap();
            let (data, text) = data_of(&html);
            assert!(!text.contains(['<', '>', '&']), "{text}");
            assert_eq!(string(&data, "@context"), "https://schema.org");
            let graph = data.get("@graph").and_then(Value::as_array).unwrap();

            let referred = |value: &Value| string(value, "@id").to_string();
            let web_page = node(graph, "WebPage");
            assert_eq!(string(web_page, "url"), "https://jabbott.dev/Tidepool/");
            assert_eq!(string(web_page, "name"), TITLE);
            assert_eq!(string(web_page, "description"), DESCRIPTION);
            assert_eq!(string(web_page, "dateModified"), "2026-11-08");
            assert_eq!(
                web_page.get("isPartOf").map(referred).as_deref(),
                Some("https://jabbott.dev/#website")
            );
            assert_eq!(
                web_page.get("author").map(referred).as_deref(),
                Some("https://jabbott.dev/#person")
            );
            assert_eq!(
                web_page.get("about").map(referred).as_deref(),
                Some("https://jabbott.dev/Tidepool/#software")
            );

            let crumbs = node(graph, "BreadcrumbList")
                .get("itemListElement")
                .and_then(Value::as_array)
                .unwrap();
            let names: Vec<&str> = crumbs.iter().map(|crumb| string(crumb, "name")).collect();
            assert_eq!(names, ["Home", "Deployments", "Tidepool"]);
            assert_eq!(crumbs[2].get("position").and_then(Value::as_i64), Some(3));

            let software = node(graph, "SoftwareSourceCode");
            assert_eq!(
                string(software, "@id"),
                "https://jabbott.dev/Tidepool/#software"
            );
            assert_eq!(string(software, "name"), "Tidepool");
            assert_eq!(
                string(software, "codeRepository"),
                "https://github.com/jabbott-iii/Tidepool"
            );
            assert_eq!(string(software, "programmingLanguage"), "Rust");
            assert_eq!(
                string(software, "license"),
                "https://www.apache.org/licenses/LICENSE-2.0"
            );
            assert_eq!(string(software, "version"), "0.1.0");
            assert_eq!(string(software, "creativeWorkStatus"), "In development");
            assert_eq!(string(software, "dateModified"), "2026-11-08");

            assert_eq!(
                string(node(graph, "WebSite"), "@id"),
                "https://jabbott.dev/#website"
            );
            assert_eq!(
                string(node(graph, "Person"), "@id"),
                "https://jabbott.dev/#person"
            );
        });
        // A local build gives no date.
        with_page(Source::Local { commit: None }, |page| {
            let html = render(include_str!("../../../site/index.html"), page).unwrap();
            let (data, _) = data_of(&html);
            let graph = data.get("@graph").and_then(Value::as_array).unwrap();
            assert!(node(graph, "WebPage").get("dateModified").is_none());
            assert!(
                node(graph, "SoftwareSourceCode")
                    .get("dateModified")
                    .is_none()
            );
        });
    }

    #[test]
    fn text_in_a_script_cannot_end_it() {
        let text = "</script><!-- & -->";
        let json = json::to_string_compact(&Value::String(text.to_string()));
        let safe = script_safe(&json);
        assert!(!safe.contains(['<', '>', '&']), "{safe}");
        assert_eq!(
            json::parse(&safe, Dialect::Json, Limits::DEFAULT),
            Ok(Value::String(text.to_string()))
        );
    }

    #[test]
    fn the_text_template_in_the_repository_fills_without_escaping() {
        with_page(live(), |page| {
            let text = render_text(include_str!("../../../site/llms.txt"), page)
                .expect("every placeholder has a value");
            assert!(!text.contains("{{"), "a placeholder is left");
            assert!(!text.contains("&amp;") && !text.contains("&lt;"), "{text}");
            for expected in [
                "# Tidepool\n\n> Tidepool is a zero-dependency IDE",
                "run on each commit of `main` that passes CI",
                "nothing opens a window yet. 19 of 23 roadmap tasks done at commit 0123456 of \
                 2026-11-08 (site: the demo page <M0.14> & its \"gate\").\n",
                "Rust 1.99.0, edition 2024; 26 crates in the workspace; third-party crates: None",
                "Here: 11 of 11 steps passed; 1,234 tests passed, 0 failed and 0 ignored, and 1 \
                 printed SKIPPED (not passes); in 3 min 12 s, on Linux x86_64, run by the job that \
                 published this page. Its summary:",
                "```\n== summary\ndeps          ok            0.4 s\n",
                "not yet implemented: tools, theme\n```",
                "```\n== JSONC → JSON ==\njson │ {\"a\":\"<b>\"}\n\tcafé 🌊\n```",
                "```\ntext  \"a\\r\\nb\"\n👩\u{200d}💻 \\u{1b}[31mred\n```",
                "https://github.com/jabbott-iii/Tidepool/blob/0123456789abcdef0123456789abcdef01234567/\
                 diagrams/c4-architecture.md",
                "1 shipped, 0 in progress and 1 planned.",
                "### Shipped\n\n- The rope (Text): Reads \"things\" <fast>. Roadmap tasks: M1.1.\n\n\
                 ### Planned\n\n- Regex (Text): Reads \"things\" <fast>. Roadmap tasks: M4.4, \
                 M4.5.\n\n## Roadmap",
                "- M0, Foundation and guardrails: 13 of 14 tasks done\n- M1, Text engine & formats \
                 (headless): 6 of 9 tasks done",
                "https://github.com/jabbott-iii/Tidepool/blob/0123456789abcdef0123456789abcdef01234567/\
                 docs/roadmap.md",
            ] {
                assert!(text.contains(expected), "missing {expected:?} in\n{text}");
            }
            assert!(!text.contains("### In progress"), "{text}");
            assert!(!text.contains('\u{1b}'), "{text}");
        });
    }

    #[test]
    fn every_name_the_text_template_may_use_is_one_the_page_has() {
        // `llms.txt` takes the page's names that are not HTML, and two of
        // its own.
        with_page(live(), |page| {
            let html: Vec<&str> = values(page).into_keys().collect();
            let text: Vec<&str> = text_values(page).into_keys().collect();
            let html_only = ["feature_filters", "latest_change", "structured_data"];
            let text_only = ["build_line", "gate_line"];
            for name in &html {
                assert_eq!(
                    text.contains(name),
                    !html_only.contains(name),
                    "{name} in llms.txt"
                );
            }
            for name in &text {
                assert_eq!(
                    html.contains(name),
                    !text_only.contains(name),
                    "{name} in the page"
                );
            }
        });
    }

    #[test]
    fn a_milestone_with_no_tasks_counts_as_done() {
        let html = milestones(&[Milestone {
            id: "M0".to_string(),
            title: "Empty".to_string(),
            done: 0,
            total: 0,
        }]);
        assert!(html.contains("data-state=\"done\""), "{html}");
        assert!(html.contains("width: 100%"), "{html}");
        let html = milestones(&[Milestone {
            id: "M9".to_string(),
            title: "Later".to_string(),
            done: 0,
            total: 4,
        }]);
        assert!(html.contains("data-state=\"todo\""), "{html}");
        assert!(html.contains("width: 0%"), "{html}");
    }
}
