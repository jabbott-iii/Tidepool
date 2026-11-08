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

//! Tests of `fuzz-smoke`: its options, its table, how it reads a target's
//! output, what it prints, and, end to end, runs on a generated workspace
//! whose fake targets are clean, find something, die, or print nothing.

use tidepool_core::testing::fuzz::{PeakMemory, Slowest};

use super::*;
use crate::walk::testing::{TempTree, symlink};

const FAKES: &[Target] = &[
    Target {
        name: "clean",
        package: "fake-fuzz",
    },
    Target {
        name: "found",
        package: "fake-fuzz",
    },
    Target {
        name: "dies",
        package: "fake-fuzz",
    },
    Target {
        name: "silent",
        package: "fake-fuzz",
    },
];

fn options<'a>(args: &[&str], table: &'a [Target]) -> Result<Options<'a>, Error> {
    let mut args = Args::new(args.iter().copied()).unwrap();
    let options = Options::parse(&mut args, table, || 0xc10c)?;
    assert_eq!(args.finish().unwrap(), Vec::<String>::new());
    Ok(options)
}

fn usage_error(args: &[&str]) -> String {
    match options(args, FAKES) {
        Err(Error::Usage(message)) => message,
        other => panic!("expected a usage error for {args:?}, got {other:?}"),
    }
}

fn names(options: &Options<'_>) -> Vec<&'static str> {
    options.targets.iter().map(|target| target.name).collect()
}

#[test]
fn options_default_to_every_target_for_a_minute_with_a_seed_from_the_clock() {
    let options = options(&[], FAKES).unwrap();
    assert_eq!(names(&options), ["clean", "found", "dies", "silent"]);
    assert_eq!(options.secs, 60);
    assert_eq!((options.seed, options.seed_from_clock), (0xc10c, true));
    assert!(!options.replay);
}

#[test]
fn options_take_the_time_a_target_a_seed_and_replay() {
    let options = options(
        &["--target", "dies", "--secs=5", "--seed", "0xAB", "--replay"],
        FAKES,
    )
    .unwrap();
    assert_eq!(names(&options), ["dies"]);
    assert_eq!(options.secs, 5);
    assert_eq!((options.seed, options.seed_from_clock), (0xab, false));
    assert!(options.replay);
    assert_eq!(self::options(&["--seed=12"], FAKES).unwrap().seed, 12);
    assert_eq!(self::options(&["--secs", "0"], FAKES).unwrap().secs, 0);
}

#[test]
fn bad_options_are_usage_errors() {
    assert_eq!(
        usage_error(&["--target", "yaml"]),
        "unknown fuzz target `yaml`; the targets are: clean, found, dies, silent"
    );
    assert_eq!(
        usage_error(&["--secs", "-1"]),
        "invalid value `-1` for option `--secs` (argument 1)"
    );
    assert_eq!(
        usage_error(&["--seed", "0x"]),
        "option `--seed` needs a u64, in decimal or as 0x-prefixed hex, not `0x`"
    );
    assert_eq!(
        usage_error(&["--target"]),
        "option `--target` (argument 1) needs a value"
    );
}

#[test]
fn clock_seeds_differ_from_run_to_run() {
    let first = clock_seed();
    std::thread::sleep(Duration::from_millis(2));
    assert_ne!(first, clock_seed());
}

#[test]
fn the_real_table_matches_the_repository() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    assert_eq!(check_table(root, TARGETS).unwrap(), Vec::<String>::new());
    let mut names: Vec<&str> = TARGETS.iter().map(|target| target.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), TARGETS.len());
    for target in TARGETS {
        // Each package's tests/fuzz.rs has a test of the target's name.
        let dir = target.package.strip_prefix("tidepool-").unwrap();
        let path = root.join("crates").join(dir).join("tests/fuzz.rs");
        let manifest =
            fs::read_to_string(root.join("crates").join(dir).join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains(&format!("name = \"{}\"", target.package)),
            "{}",
            target.package
        );
        let source = fs::read_to_string(&path).unwrap();
        assert!(
            source.contains(&format!("#[test]\nfn {}() {{", target.name)),
            "{} has no test `{}`",
            path.display(),
            target.name
        );
    }
    // The profile the targets are built in exists, and the notice beside
    // the seeds is not in the table's way.
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("\n[profile.fuzz]\n"), "{manifest}");
    assert!(root.join("fuzz/NOTICE.md").is_file());
}

#[test]
fn the_table_and_the_directories_must_agree() {
    let tree = TempTree::new("fuzz-table");
    for name in ["clean", "found"] {
        tree.write(&format!("fuzz/corpus/{name}/seed"), "x");
    }
    // A corpus of hidden files and directories only has no seeds.
    tree.write("fuzz/corpus/dies/.gitkeep", "");
    tree.write("fuzz/corpus/dies/sub/seed", "x");
    tree.write("fuzz/corpus/stray/seed", "x");
    tree.write("fuzz/corpus/a-file", "x");
    tree.write("fuzz/corpus/.hidden", "x");
    tree.write("fuzz/crashes/found/panic-0", "x");
    tree.write("fuzz/crashes/renamed/panic-0", "x");
    tree.write("fuzz/NOTICE.md", "a notice beside the directories");
    let violations = check_table(tree.path(), FAKES).unwrap();
    assert_eq!(
        violations,
        [
            "fuzz/corpus/a-file is not a directory: fuzz/corpus/ holds a directory of seeds for \
             each target (a symbolic link is never followed)",
            "fuzz/corpus/stray is the seeds of no fuzz target: the table in xtask/src/fuzz.rs \
             has no row `stray`",
            "fuzz/crashes/renamed is the saved crashes of no fuzz target: the table in \
             xtask/src/fuzz.rs has no row `renamed`",
            "the fuzz target `dies` has no seeds: fuzz/corpus/dies holds no regular file that is \
             not hidden",
            "the fuzz target `silent` has no seeds: fuzz/corpus/silent is not a directory",
        ]
    );

    // Nothing runs then.
    let options = options(&[], FAKES).unwrap();
    let mut out = Vec::new();
    let passed = run(&options, FAKES, tree.path(), &mut out).unwrap();
    assert!(!passed);
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.ends_with("\nfuzz-smoke: FAILED (5 violations); nothing was run\n"),
        "{text}"
    );
    assert!(!text.contains("== "), "{text}");
}

#[test]
fn links_at_fuzz_or_its_directories_are_refused() {
    let outside = TempTree::new("fuzz-links-outside");
    outside.write("corpus/clean/seed", "x");
    for linked in ["fuzz", "fuzz/corpus", "fuzz/crashes"] {
        // A real corpus, but for the directory that is a link.
        let tree = TempTree::new("fuzz-links");
        tree.write("fuzz/corpus/clean/seed", "x");
        let path = tree.path().join(linked);
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        if let Err(reason) = symlink(&tree, linked, &outside.path().join("corpus")) {
            let _ = writeln!(
                io::stderr(),
                "SKIPPED: links_at_fuzz_or_its_directories_are_refused: cannot make a link here: \
                 {reason}"
            );
            return;
        }
        let violations = check_table(tree.path(), &FAKES[..1]).unwrap();
        assert_eq!(
            violations,
            [format!(
                "{linked} is a symbolic link, which fuzz-smoke does not follow"
            )],
            "{linked}"
        );
    }
}

/// A result line for `target`.
fn line(target: &str, findings: usize, memory: bool) -> String {
    let reading = |cap: u64| {
        if memory {
            Reading::Measured(PeakMemory {
                baseline: 3 << 20,
                peak: 4 << 20,
                cap,
                stopped: None,
            })
        } else {
            Reading::Skipped("unreadable".to_string())
        }
    };
    Summary {
        target: target.to_string(),
        seed: 0xc10c,
        seeds: 3,
        crashes: 1,
        inputs: 120_000,
        elapsed: Duration::from_secs(60),
        findings,
        saved: findings,
        ranges: 0,
        hung: false,
        stopped: "time".to_string(),
        memory: reading(16 << 20),
        reserved: reading(256 << 20),
        slowest: Some(Slowest {
            took: Duration::from_micros(1250),
            len: 900,
        }),
    }
    .to_string()
}

fn lines(lines: &[&str]) -> Vec<String> {
    lines.iter().map(ToString::to_string).collect()
}

#[test]
fn a_target_s_output_is_read_for_its_verdict() {
    let options = options(&[], FAKES).unwrap();
    let classify = |passed, kept: &[&str]| classify("json", &options, passed, &lines(kept));
    let ok = "test result: ok. 1 passed; 0 failed";
    let failed = "test result: FAILED. 0 passed; 1 failed";
    let clean = line("json", 0, true);
    let summary = Summary::parse(&clean).unwrap();
    assert_eq!(
        classify(true, &[&clean, ok]),
        Verdict::Clean(summary.clone())
    );
    let found = line("json", 2, true);
    assert!(matches!(
        classify(false, &[&found, failed]),
        Verdict::Found(summary) if summary.findings == 2
    ));
    assert!(matches!(
        classify(true, &[&line("toml", 0, true), ok]),
        Verdict::Failed(Some(_), why) if why == "its test reported for the target `toml`"
    ));
    // Child text is escaped where it is quoted.
    let forged = line("json", 0, true).replace("target=json", "target=a\u{1b}[2Kb");
    assert!(matches!(
        classify(true, &[&forged, ok]),
        Verdict::Failed(Some(_), why) if why == "its test reported for the target `a\\u{1b}[2Kb`"
    ));
    assert!(matches!(
        classify(false, &[&clean, failed]),
        Verdict::Failed(Some(_), why) if why == "its test failed after reporting no findings"
    ));
    assert!(matches!(
        classify(false, &[&clean, &clean]),
        Verdict::Failed(None, why) if why == "its test reported 2 times"
    ));
    assert_eq!(
        classify(true, &["test result: ok. 0 passed"]),
        Verdict::NotRun
    );
    assert!(matches!(
        classify(false, &[failed]),
        Verdict::Failed(None, why) if why.starts_with("its test failed before it could report")
    ));
    assert_eq!(
        classify(
            false,
            &["error: could not compile `tidepool-codec` (test \"fuzz\")"]
        ),
        Verdict::NotBuilt
    );
    assert_eq!(
        classify(
            false,
            &[
                "process didn't exit successfully: `/t/fuzz-0123 --exact json --nocapture` \
               (signal: 6, SIGABRT: process abort signal)"
            ]
        ),
        Verdict::Died(Some(
            "(signal: 6, SIGABRT: process abort signal)".to_string()
        ))
    );
    // A death is the test binary's: another process cargo ran, or no
    // line at all, is cargo failing before the test ran.
    assert_eq!(
        classify(
            false,
            &["process didn't exit successfully: `/t/build-script-build` (exit status: 1)"]
        ),
        Verdict::CargoFailed
    );
    assert_eq!(
        classify(
            false,
            &[
                "process didn't exit successfully: `/t/fuzz-0123 --exact jsonx --nocapture` (signal: 9)"
            ]
        ),
        Verdict::CargoFailed
    );
    assert_eq!(classify(false, &[]), Verdict::CargoFailed);
    // Cargo says only that the test failed when the binary exits with
    // libtest's own status, 101, without a result.
    assert_eq!(
        classify(
            false,
            &["error: test failed, to rerun pass `-p tidepool-codec --test fuzz`"]
        ),
        Verdict::ExitedWithoutResult
    );
    // It says that too when it could not start the binary, or its runner,
    // and then why: that is cargo failing, and no input to look for. The
    // lines are cargo's, the second kept from where the match starts.
    for command in [
        "/t/fuzz-0123 --exact json --nocapture",
        "/nonexistent-runner /t/fuzz-0123 --exact json --nocapture",
    ] {
        assert_eq!(
            classify(
                false,
                &[
                    "error: test failed, to rerun pass `-p tidepool-codec --test fuzz`",
                    &format!("could not execute process `{command}` (never executed)"),
                ]
            ),
            Verdict::CargoFailed,
            "{command}"
        );
    }
}

#[test]
fn a_replay_stopped_by_a_signal_is_not_a_death_to_keep() {
    for (how, stopped) in [
        ("(signal: 2, SIGINT: interrupt)", Some("SIGINT")),
        ("(signal: 15, SIGTERM: termination signal)", Some("SIGTERM")),
        ("(signal: 1, SIGHUP: hangup)", Some("SIGHUP")),
        ("(signal: 6, SIGABRT: process abort signal)", None),
        ("(signal: 9, SIGKILL: kill)", None),
        ("(signal: 11, SIGSEGV: invalid memory reference)", None),
        ("(exit status: 1)", None),
    ] {
        assert_eq!(
            stopped_by(&Verdict::Died(Some(how.to_string()))),
            stopped,
            "{how}"
        );
    }
    assert_eq!(stopped_by(&Verdict::Died(None)), None);
    assert_eq!(stopped_by(&Verdict::CargoFailed), None);
}

#[test]
fn a_result_line_that_does_not_match_what_was_asked_fails() {
    let options = options(&["--secs", "60"], FAKES).unwrap();
    let ok = "test result: ok. 1 passed";
    let other_seed = line("json", 0, true).replace("seed=0x000000000000c10c", "seed=0x01");
    assert!(matches!(
        classify("json", &options, true, &lines(&[&other_seed, ok])),
        Verdict::Failed(Some(_), why)
            if why == "its test used the seed 0x0000000000000001, not the 0x000000000000c10c it \
                       was given"
    ));
    let short = line("json", 0, true).replace("micros=60000000", "micros=1000");
    assert!(matches!(
        classify("json", &options, true, &lines(&[&short, ok])),
        Verdict::Failed(Some(_), why) if why == "its test ran for 0.0 s, not the 60 s it was given"
    ));
    // A replay, which counts only the time in the target, may take less;
    // so may a run that hung.
    let replay = self::options(&["--secs", "60", "--replay"], FAKES).unwrap();
    assert!(matches!(
        classify("json", &replay, true, &lines(&[&short, ok])),
        Verdict::Clean(_)
    ));
    let hung = short.replace("hung=0", "hung=1");
    assert!(matches!(
        classify("json", &options, false, &lines(&[&hung])),
        Verdict::Found(_)
    ));
}

#[test]
fn each_target_s_line_says_what_it_did() {
    let options = options(&["--seed", "0xab", "--secs", "30"], FAKES).unwrap();
    let json = Target {
        name: "json",
        package: "tidepool-codec",
    };
    let summary = Summary::parse(&line("json", 0, true)).unwrap();
    assert_eq!(
        describe(&json, &options, &Verdict::Clean(summary.clone())),
        "json: ok: 120000 inputs in 60.0 s (2000 a second); written memory 4.0 MiB (+1.0 MiB \
         of a 16.0 MiB cap); reserved memory 4.0 MiB (+1.0 MiB of a 256.0 MiB cap); slowest \
         input 1.25 ms (900 bytes)"
    );
    let skipped = Summary::parse(&line("json", 1, false)).unwrap();
    assert_eq!(
        describe(&json, &options, &Verdict::Found(skipped.clone())),
        "json: FAILED: 1 finding (the report is above): 120000 inputs in 60.0 s (2000 a \
         second); written memory not checked (unreadable); reserved memory not checked \
         (unreadable); slowest input 1.25 ms (900 bytes)\n  The saved inputs are in \
         fuzz/crashes/json/. Where they are lost, as on a CI runner, the seed makes the same \
         inputs again:\n  cargo xtask fuzz-smoke --target json --seed 0x00000000000000ab --secs \
         30"
    );
    // Nothing saved: the report names the files; and a range asks for a
    // replay.
    let unsaved = Summary {
        saved: 0,
        ..skipped.clone()
    };
    assert!(
        describe(&json, &options, &Verdict::Found(unsaved))
            .ends_with("\n  The failing inputs are the files named in the report above.")
    );
    let range = Summary {
        saved: 0,
        ranges: 1,
        ..skipped
    };
    assert!(describe(&json, &options, &Verdict::Found(range)).ends_with(
        "\n  1 finding names a range of inputs, none of them saved. A replay reads the memory \
             after every input, and names and saves the one:\n  cargo xtask fuzz-smoke --target \
             json --seed 0x00000000000000ab --secs 30 --replay"
    ));
    assert_eq!(
        describe(&json, &options, &Verdict::NotRun),
        "json: FAILED: no test named `json` ran in tidepool-codec's tests/fuzz.rs; the table \
         in xtask/src/fuzz.rs names it"
    );
    assert_eq!(
        describe(&json, &options, &Verdict::CargoFailed),
        "json: FAILED: cargo failed before its test ran (see its output above)"
    );
    let died = describe(
        &json,
        &options,
        &Verdict::Died(Some(
            "(signal: 11, SIGSEGV: invalid memory reference)".to_string(),
        )),
    );
    assert!(
        died.starts_with(
            "json: DIED: its test process ended without a result (signal: 11, SIGSEGV: invalid \
             memory reference): an allocation too big to make, a stack overflow, a kill, or a \
             call to exit"
        ),
        "{died}"
    );
    assert!(died.contains("fuzz/crashes/json/.last-input"), "{died}");
    assert!(died.contains("fuzz/crashes/json/died-<hash>"), "{died}");
    assert!(
        died.contains(
            "\n  The saved crashes in fuzz/crashes/json/ run first; if it holds died-* files, it \
             may have died on one of them."
        ),
        "{died}"
    );
    let exited = describe(&json, &options, &Verdict::ExitedWithoutResult);
    assert!(
        exited.starts_with(
            "json: FAILED: its test binary exited with libtest's failure status without a \
             result (see its output above).\n  The saved crashes"
        ),
        "{exited}"
    );
    assert!(exited.ends_with(" --replay"), "{exited}");
    assert!(
        died.ends_with(
            "\n  cargo xtask fuzz-smoke --target json --seed 0x00000000000000ab --secs 30 --replay"
        ),
        "{died}"
    );

    // A replay that did not die again says why it stopped.
    let replay = self::options(&["--seed", "0xab", "--secs", "30", "--replay"], FAKES).unwrap();
    let stopped = Summary {
        stopped: "wall-cap".to_string(),
        ..summary.clone()
    };
    let text = describe(&json, &replay, &Verdict::Clean(stopped));
    assert!(
        text.ends_with(
            "\n  The replay found nothing in 120000 inputs: it stopped because the replay \
             reached 20 times its seconds in all. It may not have reached the input the run it \
             replays failed on; try more --secs."
        ),
        "{text}"
    );
    // A replay that saved a finding gives the replay to make it again.
    let saved = Summary::parse(&line("json", 1, true)).unwrap();
    assert!(describe(&json, &replay, &Verdict::Found(saved)).ends_with(
        "the seed makes the same inputs again:\n  cargo xtask fuzz-smoke --target json \
             --seed 0x00000000000000ab --secs 30 --replay"
    ));
    // A replay that died again has no replay to suggest.
    let died = describe(&json, &replay, &Verdict::Died(None));
    assert!(!died.contains("--replay"), "{died}");
}

#[test]
fn the_summary_lines_up_and_gives_the_verdict() {
    let options = options(&["--seed", "0xab", "--secs", "30"], FAKES).unwrap();
    let clean = Summary::parse(&line("json", 0, true)).unwrap();
    let skipped = Summary::parse(&line("encoding", 0, false)).unwrap();
    let table = summary(
        &options,
        &[
            ("json", Verdict::Clean(clean.clone())),
            ("encoding", Verdict::Clean(skipped)),
        ],
    );
    assert_eq!(
        table,
        "== summary\n\
         target    result       inputs    a second      written     reserved     slowest\n\
         json      ok           120000        2000     +1.0 MiB     +1.0 MiB     1.25 ms\n\
         encoding  ok           120000        2000  not checked  not checked     1.25 ms\n\
         fuzz-smoke: ok: 2 of 2 targets ran clean for 30 s each (seed 0x00000000000000ab)\n"
    );
    let table = summary(
        &options,
        &[
            ("json", Verdict::Clean(clean)),
            ("toml", Verdict::Died(None)),
            ("encoding", Verdict::CargoFailed),
        ],
    );
    assert!(
        table.ends_with(
            "toml      DIED\n\
             encoding  FAILED\n\
             fuzz-smoke: FAILED: 2 of 3 targets failed: toml, encoding (seed 0x00000000000000ab)\n"
        ),
        "{table}"
    );
}

#[test]
fn the_child_s_command_line_is_shown_with_its_variables() {
    let json = &TARGETS[0];
    let args = test_args(json);
    assert_eq!(
        cargo::display(&args),
        "cargo test --profile fuzz -p tidepool-codec --test fuzz -- --exact json --nocapture"
    );
    let env = [
        (SECS_VAR, "60".to_string()),
        (SEED_VAR, "0x0000000000000001".to_string()),
        (REPLAY_VAR, "0".to_string()),
    ];
    assert_eq!(
        shown_command(&env, &args),
        "TIDEPOOL_FUZZ_SECS=60 TIDEPOOL_FUZZ_SEED=0x0000000000000001 TIDEPOOL_FUZZ_REPLAY=0 \
         cargo test --profile fuzz -p tidepool-codec --test fuzz -- --exact json --nocapture"
    );
}

/// The fake targets' tests: one clean, one that reports a finding and
/// fails, one that exits with libtest's failure status and one that aborts
/// its process (each leaving, in a replay, the input it ended on, as the
/// engine would, unless `FAKE_WRITES_NOTHING` is set), and one that prints
/// nothing. The clean one checks the variables it was given.
const FAKE_TESTS: &str = r#"
use std::env::var;

fn line(target: &str, findings: u32) -> String {
    let secs: u64 = var("TIDEPOOL_FUZZ_SECS").unwrap().parse().unwrap();
    format!(
        "fuzz-result: target={target} seed={} seeds=1 crashes=0 inputs=5 micros={} \
         findings={findings} saved={findings} ranges=0 hung=0 stopped=time memory=skipped \
         memory_reason=fake reserved=skipped reserved_reason=fake",
        var("TIDEPOOL_FUZZ_SEED").unwrap(),
        secs * 1_000_000
    )
}

#[test]
fn clean() {
    assert_eq!(var("CARGO_NET_OFFLINE").unwrap(), "true");
    assert_eq!(var("CARGO_TERM_COLOR").unwrap(), "never");
    println!("{}", line("clean", 0));
}

#[test]
fn found() {
    println!("{}", line("found", 1));
    panic!("fuzz target `found`: FAILED: 1 finding");
}

/// In a replay, writes `input` as the engine writes an input before it
/// runs it.
fn save(target: &str, input: &[u8]) {
    if var("TIDEPOOL_FUZZ_REPLAY").unwrap() == "1" && var("FAKE_WRITES_NOTHING").is_err() {
        let dir = format!("{}/../fuzz/crashes/{target}", env!("CARGO_MANIFEST_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/.last-input"), input).unwrap();
    }
}

#[test]
fn exits() {
    save("exits", b"it exited here");
    std::process::exit(101);
}

#[test]
fn dies() {
    save("dies", b"the culprit");
    std::process::abort();
}

#[test]
fn silent() {}
"#;

/// A generated workspace with the fake targets.
fn fake_workspace(name: &str) -> TempTree {
    let tree = TempTree::new(name);
    tree.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"fake\"]\nresolver = \"3\"\n\n[profile.fuzz]\ninherits = \
         \"release\"\n",
    );
    tree.write(
        "fake/Cargo.toml",
        "[package]\nname = \"fake-fuzz\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = \
         false\n",
    );
    tree.write("fake/src/lib.rs", "");
    tree.write("fake/tests/fuzz.rs", FAKE_TESTS);
    for target in FAKES {
        tree.write(&format!("fuzz/corpus/{}/seed", target.name), "x");
    }
    tree
}

/// Runs `fuzz-smoke` with `args` on `tree`; returns whether it passed, what
/// it printed, and what its children printed.
fn run_on(tree: &TempTree, args: &[&str], threads: Option<&str>) -> (bool, String, String) {
    let options = options(args, FAKES).unwrap();
    let (mut out, mut child_out, mut child_err) = (Vec::new(), Vec::new(), Vec::new());
    // A build directory of its own, so as not to wait for the one that the
    // outer `cargo test` holds; and coloured output asked for, as CI asks.
    let mut env = vec![
        (
            "CARGO_TARGET_DIR",
            tree.path().join("target").to_string_lossy().into_owned(),
        ),
        ("CARGO_TERM_COLOR", "always".to_string()),
    ];
    if let Some(threads) = threads {
        env.push(("RUST_TEST_THREADS", threads.to_string()));
    }
    let children = Children {
        out: &mut child_out,
        err: &mut child_err,
        env: &env,
    };
    let passed = run_with(&options, FAKES, tree.path(), &mut out, children).unwrap();
    let child = format!(
        "{}{}",
        String::from_utf8_lossy(&child_out),
        String::from_utf8_lossy(&child_err)
    );
    (passed, String::from_utf8(out).unwrap(), child)
}

#[test]
fn a_run_spawns_cargo_test_for_each_target_and_reads_what_it_did() {
    let tree = fake_workspace("fuzz-end-to-end");
    // Once with libtest's threads as they come, once with one, when it
    // writes `test clean ... ` before the result line.
    for threads in [None, Some("1")] {
        let (passed, text, child) = run_on(&tree, &["--secs", "1", "--seed", "0xab"], threads);
        assert!(!passed, "{text}\n{child}");
        assert!(
            text.starts_with(
                "fuzz-smoke: 4 targets, 1 s each, seed 0x00000000000000ab (from --seed)\n\
                 == clean: TIDEPOOL_FUZZ_SECS=1 TIDEPOOL_FUZZ_SEED=0x00000000000000ab \
                 TIDEPOOL_FUZZ_REPLAY=0 cargo test --profile fuzz -p fake-fuzz --test fuzz -- \
                 --exact clean --nocapture\n\
                 clean: ok: 5 inputs in 1.0 s (5 a second); written memory not checked (fake); \
                 reserved memory not checked (fake)\n"
            ),
            "{threads:?}\n{text}\n{child}"
        );
        assert!(
            text.contains("\nfound: FAILED: 1 finding (the report is above): 5 inputs in 1.0 s"),
            "{text}"
        );
        assert!(
            text.contains("\ndies: DIED: its test process ended without a result"),
            "{text}\n{child}"
        );
        assert!(
            text.contains(
                "\n  cargo xtask fuzz-smoke --target dies --seed 0x00000000000000ab --secs 1 \
                 --replay\n"
            ),
            "{text}"
        );
        assert!(
            text.contains("\nsilent: FAILED: no test named `silent` ran in fake-fuzz's"),
            "{text}"
        );
        assert!(
            text.ends_with(
                "fuzz-smoke: FAILED: 3 of 4 targets failed: found, dies, silent (seed \
                 0x00000000000000ab)\n"
            ),
            "{text}"
        );
        if threads.is_some() {
            assert!(child.contains("test clean ... fuzz-result: "), "{child}");
        }
        // The children's output was passed on: the clean target's line,
        // and libtest's report of the failing one.
        assert!(child.contains("fuzz-result: target=clean "), "{child}");
        assert!(
            child.contains("fuzz target `found`: FAILED: 1 finding"),
            "{child}"
        );
    }
}

#[test]
fn any_change_of_the_time_counts_as_written() {
    let now = SystemTime::now();
    let later = now + Duration::from_secs(3600);
    assert!(written_since(None, Some(now)), "made by the replay");
    assert!(written_since(Some(now), Some(later)));
    // A leftover whose time was in the future, rewritten now.
    assert!(written_since(Some(later), Some(now)));
    assert!(!written_since(Some(now), Some(now)), "not touched");
    assert!(!written_since(Some(now), None), "gone");
    assert!(!written_since(None, None));
}

#[test]
fn a_replay_that_dies_again_keeps_the_input_it_died_on() {
    let tree = fake_workspace("fuzz-replay-died");
    let (passed, text, child) = run_on(
        &tree,
        &[
            "--target", "dies", "--secs", "1", "--seed", "0xab", "--replay",
        ],
        None,
    );
    assert!(!passed, "{text}\n{child}");
    let kept = format!("fuzz/crashes/dies/died-{:016x}", fnv1a64(b"the culprit"));
    assert!(
        text.contains(&format!(
            "\n  The input it died on is kept as {kept}, where it runs with the saved crashes \
             from now on.\n"
        )),
        "{text}\n{child}"
    );
    assert_eq!(fs::read(tree.path().join(&kept)).unwrap(), b"the culprit");
    assert!(!tree.path().join("fuzz/crashes/dies/.last-input").exists());

    // A leftover from an earlier replay is noted before the replay starts;
    // dying again on an input already kept says so.
    tree.write("fuzz/crashes/dies/.last-input", "left over");
    let (passed, text, child) = run_on(
        &tree,
        &[
            "--target", "dies", "--secs", "1", "--seed", "0xab", "--replay",
        ],
        None,
    );
    assert!(!passed, "{text}\n{child}");
    let leftover = format!("{:016x}", fnv1a64(b"left over"));
    assert!(
        text.contains(&format!(
            "\nnote: fuzz/crashes/dies/.last-input (9 bytes, hash {leftover}), left by an \
             earlier replay, runs first. If that replay ended with fuzz-smoke itself killed \
             (systemd stops every process of the unit after an out-of-memory kill unless \
             OOMPolicy=continue, and systemd-oomd kills them all), it is likely the input it \
             died on; to keep it:\n  mv \
             fuzz/crashes/dies/.last-input fuzz/crashes/dies/died-{leftover}\n"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "\n  It died on the saved crash {kept}, which every run replays first: fix what it \
             shows, or remove it.\n"
        )),
        "{text}\n{child}"
    );
}

#[test]
fn a_test_binary_that_exits_with_libtest_s_status_is_not_a_cargo_failure() {
    const EXITS: &[Target] = &[Target {
        name: "exits",
        package: "fake-fuzz",
    }];
    let tree = fake_workspace("fuzz-exits");
    tree.write("fuzz/corpus/exits/seed", "x");
    // The table check reads every corpus directory: the others are the
    // fake workspace's, which this table does not name.
    for target in FAKES {
        fs::remove_dir_all(tree.path().join("fuzz/corpus").join(target.name)).unwrap();
    }
    let run = |args: &[&str], writes: bool| {
        let mut args = Args::new(args.iter().copied()).unwrap();
        let options = Options::parse(&mut args, EXITS, || 0).unwrap();
        let (mut out, mut child_out, mut child_err) = (Vec::new(), Vec::new(), Vec::new());
        let mut env = vec![(
            "CARGO_TARGET_DIR",
            tree.path().join("target").to_string_lossy().into_owned(),
        )];
        if !writes {
            env.push(("FAKE_WRITES_NOTHING", "1".to_string()));
        }
        let children = Children {
            out: &mut child_out,
            err: &mut child_err,
            env: &env,
        };
        let passed = run_with(&options, EXITS, tree.path(), &mut out, children).unwrap();
        let text = String::from_utf8(out).unwrap();
        (
            passed,
            format!("{text}\n{}", String::from_utf8_lossy(&child_err)),
        )
    };
    let failed = "\nexits: FAILED: its test binary exited with libtest's failure status without \
                  a result (see its output above).";
    let (passed, text) = run(&["--secs", "1", "--seed", "0xab"], true);
    assert!(!passed);
    assert!(text.contains(&format!("{failed}\n")), "{text}");
    assert!(
        text.contains(
            "cargo xtask fuzz-smoke --target exits --seed 0x00000000000000ab --secs 1 --replay"
        ),
        "{text}"
    );

    // A replay that exits so again keeps the input it wrote last.
    let replay = ["--secs", "1", "--seed", "0xab", "--replay"];
    let (passed, text) = run(&replay, true);
    assert!(!passed);
    let kept = format!(
        "fuzz/crashes/exits/died-{:016x}",
        fnv1a64(b"it exited here")
    );
    assert!(
        text.contains(&format!(
            "{failed}\n  The input it died on is kept as {kept}, where it runs with the saved \
             crashes from now on.\n"
        )),
        "{text}"
    );
    assert_eq!(
        fs::read(tree.path().join(&kept)).unwrap(),
        b"it exited here"
    );
    fs::remove_file(tree.path().join(&kept)).unwrap();

    // One that exits so before writing its file leaves an older leftover
    // as it was: it is not what this replay ended on.
    tree.write("fuzz/crashes/exits/.last-input", "older");
    let (passed, text) = run(&replay, false);
    assert!(!passed);
    assert!(
        text.contains(&format!(
            "{failed}\n  Its fuzz/crashes/exits/.last-input was not written by this replay, \
             which so ended before its first input: it is not kept, and the next replay runs it \
             first.\n"
        )),
        "{text}"
    );
    assert_eq!(
        fs::read(tree.path().join("fuzz/crashes/exits/.last-input")).unwrap(),
        b"older"
    );
    assert_eq!(
        walk_names(&tree.path().join("fuzz/crashes/exits")),
        [".last-input"]
    );
}

/// The names in `dir`, sorted.
fn walk_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_cargo_failure_before_the_test_ran_is_not_a_death() {
    let tree = TempTree::new("fuzz-cargo-failed");
    // No such package: cargo fails before any test binary runs.
    tree.write(
        "Cargo.toml",
        "[workspace]\nmembers = []\nresolver = \"3\"\n\n[profile.fuzz]\ninherits = \"release\"\n",
    );
    tree.write("fuzz/corpus/clean/seed", "x");
    let options = options(&["--secs", "1"], &FAKES[..1]).unwrap();
    let (mut out, mut child_out, mut child_err) = (Vec::new(), Vec::new(), Vec::new());
    let env = [(
        "CARGO_TARGET_DIR",
        tree.path().join("target").to_string_lossy().into_owned(),
    )];
    let children = Children {
        out: &mut child_out,
        err: &mut child_err,
        env: &env,
    };
    let passed = run_with(&options, &FAKES[..1], tree.path(), &mut out, children).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(!passed);
    assert!(
        text.contains("\nclean: FAILED: cargo failed before its test ran (see its output above)\n"),
        "{text}\n{}",
        String::from_utf8_lossy(&child_err)
    );
    assert!(!text.contains("--replay"), "{text}");
}
