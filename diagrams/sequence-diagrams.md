# UML sequence diagrams

These diagrams follow Tidepool's flows through time: who calls whom, in
what order, and what comes back. Sections 1 to 7 are read from the code:
`cargo xtask ci`, `cargo xtask fuzz-smoke` and its engine, and loading,
saving, editing and undoing text in `tidepool-text`. Sections 8 to 13 are
the flows the specs plan, from startup to signing an Android package; most
of their code does not exist yet, and what does is marked (built).

Drawn from the code and docs as of roadmap task M2.1 (2026-11-10, version 0.1). What the code has is drawn solid; what the roadmap plans is dashed and labelled with the task that builds it (ADR-0029).

## Legend

| Style | Means |
| :- | :- |
| A box at the top and bottom of a line | A participant: a process, a thread, a module or a person |
| Stick figure | A person, or a CI job standing in for one |
| Solid arrow, filled head (`->>`) | A call, a request or a message written |
| Dashed arrow, filled head (`-->>`) | Its answer: a return value or an exit status |
| Solid arrow, open head (`-)`) | Sent without waiting: through a channel, a pipe or a new thread |
| `alt` / `else` | One of several paths |
| `opt` | A step that happens only in some runs |
| `loop` | Steps that repeat |
| `par` / `and` | Steps that run at the same time, on different threads |
| Note | What a step means, or the rule or budget it keeps |
| "(planned)" in the section title, and a note at the top naming the tasks | The whole flow is planned: not built yet |
| A task ID in a participant's label, such as "(M2.2)" | That participant is planned, and that task builds it |
| "(built)" in a participant's label | That participant exists in the code now, inside a planned flow |

UML draws an answer as a dashed arrow, and Mermaid cannot dash a
participant's box, so a sequence diagram marks planned work by its title,
its note and its participants' labels instead of by dashes. Names in
messages are the code's (sections 1 to 7) or the specs' (sections 8 to
13). Paths are relative to the repository root. A planned flow too long
for one picture is drawn in numbered parts, read in order.

## 1. `cargo xtask ci`

`run_ci` runs its eleven steps in order and keeps going after a failure, so
one run reports every problem. `deps` is the gate: when it fails, `clippy`
and `test`, which compile the workspace and run its code, are not run, and
the summary lists them as "not run" (`xtask/src/check.rs`). `deps`,
`layers`, `crates` and `unsafe-audit` ask `cargo metadata
--format-version 1 --offline`: `deps` with `--locked`, once for each
workspace; `layers` and `crates` with `--no-deps`, for the root
workspace; and `unsafe-audit` with `--no-deps`, once for each workspace.
`crates` checks each member's manifest, and its README: that it starts
with `# <package name>`, has its sections (Status, Usage, an optional
Features, Documentation, Rules, Checks, License) once each and in that
order, and names only task IDs that `docs/roadmap.md` has. It also checks
that each member's copies of `LICENSE` and `NOTICE` are, byte for byte,
the root's (`xtask/src/crates.rs`). `features` checks that every ticked
task belongs to a feature in `docs/features.jsonc`, and that each
feature's `ticked` is what the roadmap ticks (`xtask/src/features.rs`).
`cargo xtask site` runs the same steps through the same runner,
`run_gate` (ADR-0030).

```mermaid
sequenceDiagram
    actor Dev as Developer or CI job
    participant Cli as xtask main
    participant Ci as check run_ci
    participant Own as xtask's own checks
    participant Cargo as cargo children
    participant Repo as Repository files

    Dev->>Cli: cargo xtask ci
    Cli->>Cli: Args::from_env,<br/>the root made<br/>absolute
    Cli->>Ci: run_ci(CI, LATER,<br/>root, stdout)
    Ci->>Dev: == deps: its heading, flushed<br/>before any child writes
    Ci->>Own: deps::run(root)
    Own->>Repo: find every Cargo.toml and Cargo.lock,<br/>read each lockfile as text
    Own->>Cargo: cargo metadata<br/>--locked, for each<br/>workspace
    Cargo-->>Own: the resolved graph,<br/>as JSON, read with<br/>codec json
    Own-->>Ci: a Report:<br/>notes and violations
    Ci->>Dev: the violation lines, then deps: ok,<br/>or deps: FAILED (N violations)
    Ci->>Cargo: cargo fmt --all -- --check, its<br/>output straight to the terminal
    Cargo-->>Ci: exit status
    alt deps passed
        Ci->>Cargo: cargo clippy --workspace<br/>--all-targets -- -D warnings
        Cargo-->>Ci: exit status
        Ci->>Cargo: cargo test --workspace, its output<br/>passed on and read as it comes
        Cargo-->>Ci: exit status, and the lines that<br/>start with SKIPPED (Skips)
        Ci->>Dev: a skipped line for each,<br/>then test: ok (N SKIPPED)
    else deps failed
        Ci->>Dev: clippy: not run, deps failed, and<br/>this step would compile and<br/>run the code it rejected
        Ci->>Dev: test: not run,<br/>for the same reason
    end
    Ci->>Own: layers::run: parse<br/>the dependency matrix<br/>in docs/01-architecture.md
    Own->>Cargo: cargo metadata --no-deps
    Cargo-->>Own: the members and<br/>the dependencies<br/>they declare
    Ci->>Own: crates::run: manifests,<br/>READMEs and LICENSE<br/>and NOTICE copies,<br/>the task IDs of<br/>docs/roadmap.md
    Own->>Cargo: cargo metadata --no-deps,<br/>the root workspace
    Cargo-->>Own: the members and<br/>their manifests
    Ci->>Own: features::run: docs/features.jsonc<br/>against the task lines<br/>of docs/roadmap.md
    Ci->>Own: unsafe_audit::run:<br/>every .rs file<br/>and Cargo.toml
    Own->>Cargo: cargo metadata --no-deps,<br/>for each workspace
    Cargo-->>Own: each member's<br/>crate roots
    Ci->>Own: headers::run: the<br/>Apache-2.0 header of<br/>every source file
    Ci->>Own: workflows::run: the<br/>files in .github/workflows
    Ci->>Own: ucd::check: the UCD<br/>files against their<br/>SHA-256 pins, the<br/>tables against what<br/>they generate
    Own->>Repo: walk and read,<br/>never following a link
    Own-->>Ci: a Report for each,<br/>printed as for deps
    Ci->>Dev: == summary: a row for each step<br/>with its time, not run (deps<br/>failed), or not yet implemented
    Note over Dev,Ci: The last line counts the failed steps and<br/>the skipped tests. A skipped test never<br/>fails ci, and is never a pass. tools (M5.1)<br/>and theme (M3.2) show as not yet implemented.
    Cli-->>Dev: exit status 0 if<br/>every step passed,<br/>1 if one failed<br/>or did not run
```

A run that passes ends with `ci: ok: 11 of 11 steps passed; not yet
implemented: tools, theme`; one whose gate failed, with `ci: FAILED: 1 of
11 steps failed: deps; not run: clippy, test; not yet implemented: tools,
theme`.

## 2. `cargo xtask fuzz-smoke`: `xtask` and its children

`fuzz-smoke` checks that `fuzz/` agrees with its table of targets, then
runs each target's test in its own `cargo test`, and reads the one result
line the test prints (`xtask/src/fuzz.rs`). The parsers run in those test
processes, never in `xtask`. The targets run in table order: `json` and
`toml` (`tidepool-codec`), then `encoding` (`tidepool-text`).

```mermaid
sequenceDiagram
    actor Dev as Developer or CI job
    participant Smoke as xtask fuzz run
    participant Fz as fuzz directory
    participant Cargo as cargo test,<br/>one per target
    participant Test as test process,<br/>the crate's tests/fuzz.rs

    Dev->>Smoke: cargo xtask fuzz-smoke<br/>--secs 60
    Smoke->>Smoke: Options::parse: the targets,<br/>the seconds, --replay, the seed<br/>from the clock unless --seed
    Smoke->>Fz: check_table: fuzz/,<br/>fuzz/corpus/ and fuzz/crashes/<br/>against TARGETS, links refused
    alt they disagree
        Smoke->>Dev: the violation lines,<br/>then fuzz-smoke: FAILED,<br/>nothing was run
    else they agree
        Smoke->>Dev: fuzz-smoke: 3 targets, 60 s<br/>each, the seed, and how<br/>to pass it again
        loop each target, in table order
            Smoke->>Dev: == json: the variables<br/>and the command line
            Smoke->>Cargo: cargo test --profile fuzz<br/>-p tidepool-codec --test fuzz<br/>-- --exact json --nocapture
            Note over Smoke,Cargo: An argv vector, stdin closed,<br/>CARGO_NET_OFFLINE=true,<br/>CARGO_TERM_COLOR=never, and<br/>TIDEPOOL_FUZZ_SECS, TIDEPOOL_FUZZ_SEED<br/>and TIDEPOOL_FUZZ_REPLAY
            Cargo->>Test: build in the fuzz profile,<br/>run the one test json
            Test->>Test: the engine runs the<br/>target (section 3)
            Test-)Smoke: fuzz-result: target=json seed=...<br/>inputs=... findings=... stopped=...,<br/>on the pipe xtask reads
            Test-->>Cargo: exit status, a failed test<br/>if anything was found
            Cargo-->>Smoke: exit status, and its own lines,<br/>passed on as they come
            Smoke->>Smoke: keep the KEPT lines,<br/>Summary::parse the result<br/>line, classify the run
            alt Clean
                Smoke->>Dev: json: ok: the inputs, inputs<br/>a second, both memory<br/>checks, the slowest input
            else Found
                Smoke->>Dev: json: FAILED: the findings,<br/>where they were saved, the<br/>command that makes the<br/>same inputs again
            else Died
                Smoke->>Dev: json: DIED: how it ended,<br/>and the replay command<br/>(section 4)
            else ExitedWithoutResult
                Smoke->>Dev: json: FAILED: exited with<br/>libtest's failure status<br/>without a result, and the<br/>replay command (section 4)
            else Failed, NotRun, NotBuilt or CargoFailed
                Smoke->>Dev: json: FAILED, and why
            end
        end
        Smoke->>Dev: the summary table
    end
    Smoke-->>Dev: exit status 0 if every<br/>target ran clean, else 1
```

## 3. One fuzz target's run: the engine

Inside the test process, `Target::run_with` reads the saved crashes and the
seeds, and `engine::run` feeds them to the target on a thread of its own
while the test's thread watches the clock (`crates/core/src/testing/fuzz.rs`,
`fuzz/engine.rs`). A plain `cargo test` runs the same test with no
`TIDEPOOL_FUZZ_SECS`: only the unchanged inputs, so every saved crash is a
regression test.

```mermaid
sequenceDiagram
    participant Test as the test json,<br/>crates/codec/tests/fuzz.rs
    participant Tgt as Target run_with
    participant Wd as engine run, the watchdog<br/>on the test's thread
    participant Th as thread fuzz-json,<br/>2 MiB stack
    participant Mem as memory probe,<br/>/proc/self/status
    participant Fz as fuzz directory

    Test->>Tgt: Target::new(ROOT,<br/>json, parse_json),<br/>seeds_in, dictionary,<br/>memory_cap, then run()
    Tgt->>Tgt: Config::from_env reads<br/>TIDEPOOL_FUZZ_SECS,<br/>TIDEPOOL_FUZZ_SEED and<br/>TIDEPOOL_FUZZ_REPLAY
    Tgt->>Tgt: take the lock RUN:<br/>one target of a<br/>process at a time
    Tgt->>Fz: read fuzz/crashes/json/, then<br/>fuzz/corpus/json/, then the seeds_in<br/>tree, in name order, links refused,<br/>at most 1 MiB a file
    Tgt->>Wd: engine::run(Plan)
    Wd-)Th: spawn with<br/>STACK_SIZE, whatever<br/>RUST_MIN_STACK says
    par the target thread
        Th->>Mem: Probe::start: the<br/>baselines of VmHWM,<br/>and of VmSize when<br/>TIDEPOOL_FUZZ_SECS<br/>is set
        loop each saved crash, then each seed, unchanged
            Th->>Th: run_target:<br/>catch_unwind around<br/>the target, a panic<br/>kept with its place
            Th->>Mem: read after every<br/>unchanged input
        end
        loop mutated inputs, until TIDEPOOL_FUZZ_SECS have passed
            Th->>Th: Pcg32 from the seed,<br/>the name and the input<br/>number: a seed with 1<br/>to 16 changes, at<br/>most 64 KiB
            Th->>Th: run_target
            Th->>Mem: read once enough of<br/>the target's time has<br/>passed, after every<br/>input in a replay
            opt a panic, a return after 5 s, or memory past a cap
                Th->>Fz: save a mutated input as panic-, hang-,<br/>memory- or reserve- and its FNV-1a<br/>hash, written whole through a hidden file
            end
        end
        Th-)Wd: done, through a channel
    and the watchdog
        loop every 50 ms until done, recv_timeout
            Wd->>Wd: has the input running<br/>taken more than<br/>HANG_LIMIT, 5 s?
        end
        opt it has
            Wd->>Fz: record the stuck input as a hang,<br/>the run stops with stopped=hang,<br/>the thread is left running
        end
    end
    Wd-->>Tgt: Ran: the inputs, the<br/>time, the findings, why<br/>it stopped, both<br/>memory readings
    Tgt-->>Test: Outcome
    Test->>Test: print outcome.summary(),<br/>the fuzz-result line
    Test->>Test: assert_clean: panic with<br/>the report and the reproduce<br/>command if anything was found
    Note over Test,Fz: An unchanged input that fails is named where<br/>it is, not saved again. Reserved memory read<br/>on the time budget names a range of inputs<br/>and saves none: the replay names the one.
```

## 4. A death, and the replay that keeps its input

A process that dies, from an allocation too big to make or a stack
overflow, prints no result line. `fuzz-smoke` then prints the replay
command; the replay writes each input to `.last-input` before it runs it,
so the input the process dies on is on disk, and `fuzz-smoke` keeps it as
`died-<hash>` (`xtask/src/fuzz.rs`, `crates/core/src/testing/fuzz/paths.rs`).
When a saved crash of that hash already holds the input (a `panic-`,
`hang-`, `memory-` or `reserve-` file, or an earlier `died-` one),
`keep_died` keeps no second copy, and `fuzz-smoke` says the process died on
that saved crash. A replay that ends without dying can still find
something: a saved crash that still fails, a finding the run it replays
died before reporting, or a reserved-memory input that the replay, reading
the memory after every input, names exactly and saves.

```mermaid
sequenceDiagram
    actor Dev as Developer or CI job
    participant Smoke as xtask fuzz run
    participant Cargo as cargo test
    participant Test as test process<br/>and its engine
    participant Fz as fuzz/crashes/json/

    Dev->>Smoke: cargo xtask fuzz-smoke
    Smoke->>Cargo: cargo test for json,<br/>TIDEPOOL_FUZZ_REPLAY=0
    Cargo->>Test: run the test json
    Test->>Test: an input aborts the<br/>process, or overflows<br/>the stack
    Cargo-->>Smoke: process didn't exit<br/>successfully, how it ended,<br/>and no result line
    Smoke->>Smoke: classify: Died
    Smoke->>Dev: json: DIED. The saved crashes<br/>run first, and a died- file<br/>among them may be the one
    Smoke->>Dev: cargo xtask fuzz-smoke<br/>--target json --seed 0x...<br/>--secs 60 --replay
    Dev->>Smoke: that command
    opt a .last-input left by an earlier replay
        Smoke->>Fz: read it, and print its size, its hash and<br/>the mv command that keeps it by hand
    end
    Smoke->>Cargo: cargo test for json,<br/>TIDEPOOL_FUZZ_REPLAY=1
    Cargo->>Test: run the test json
    Test->>Fz: write a leftover .last-input<br/>whole under a hidden name,<br/>rename it over the old one,<br/>run it first
    loop the same inputs as the<br/>run it replays, time<br/>counted only inside<br/>the target
        Test->>Fz: write the input<br/>to .last-input
        Test->>Test: run it, then<br/>read the memory
    end
    alt the process dies again, or exits with<br/>libtest's status 101 without a result
        Cargo-->>Smoke: died, or exited with status<br/>101 (json: FAILED), no result line
        Smoke->>Fz: the replay wrote .last-input,<br/>so keep_died reads it
        alt no saved crash holds its bytes
            Smoke->>Fz: rename it to died- and its hash
            Smoke->>Dev: the input it died on is<br/>kept, and runs with the<br/>saved crashes from now on
        else a saved crash of that hash holds them
            Smoke->>Fz: remove .last-input, or rename<br/>it over that died- file
            Smoke->>Dev: it died on that saved crash,<br/>which every run replays first:<br/>fix what it shows, or remove it
        end
    else a signal stopped it: SIGINT, SIGTERM or SIGHUP
        Smoke->>Dev: not kept, the next<br/>replay runs it first
    else it ends without dying
        Test->>Fz: remove its own .last-input,<br/>and the directories it<br/>made if empty
        alt it found nothing
            Smoke->>Dev: json: ok, it may not have<br/>reached that input,<br/>try more --secs
        else it found something
            Smoke->>Dev: json: FAILED: the findings,<br/>where they were saved, and<br/>the replay command that<br/>makes them again
        end
    end
```

## 5. Loading a file into `tidepool-text`

`text::file::load` reads a regular file in 64 KiB blocks into a streaming
`Decoder`, which picks the encoding from a byte-order mark alone
(`crates/text/src/file.rs`, `encoding.rs`). Nothing is written. A file
with bytes that are not text loads read-only, with those bytes kept beside
the text. Today the callers are tests; `workspace::buffers` runs loads on
workers from M4.3.

```mermaid
sequenceDiagram
    participant Caller as caller: a test today,<br/>workspace buffers on<br/>a worker (M4.3)
    participant Load as text file load
    participant Dec as text encoding Decoder
    participant Fs as file system

    Caller->>Load: load(path)
    Load->>Fs: fs::metadata(path),<br/>following links
    Fs-->>Load: metadata
    Load->>Load: check_regular, else<br/>TextError::NotRegularFile,<br/>so a FIFO never blocks the read
    Load->>Fs: File::open(path)
    loop 64 KiB blocks, until a read returns 0
        Load->>Fs: read
        Fs-->>Load: bytes
        Load->>Dec: push(bytes)
    end
    Note over Load,Dec: The first bytes pick the format: EF BB BF<br/>is UTF-8 with a BOM, FF FE UTF-16 LE,<br/>FE FF UTF-16 BE, and anything else UTF-8<br/>without one. There are no other heuristics.
    Load->>Dec: finish()
    Dec-->>Load: Decoded: the text as a Rope,<br/>the detected Format, the<br/>runs of replaced bytes
    Load->>Load: LineEndings::of(text),<br/>the trailing break,<br/>the bytes read
    Load-->>Caller: Loaded
    Caller->>Load: is_large()
    Load-->>Caller: true above LARGE_FILE_BYTES,<br/>16 MiB, for the editor's<br/>large-file mode (M4.3)
    Caller->>Load: format()
    alt bytes were replaced
        Load-->>Caller: None, and read_only() is true:<br/>the file is not to be<br/>written back
        Note over Caller,Load: decoded().original_bytes()<br/>still gives back the input exactly
    else the text decoded cleanly
        Load-->>Caller: the detected Format,<br/>to save in
    end
```

## 6. The atomic save

`text::file::save` replaces a file so that it holds either its old bytes
or exactly the new ones, even after a crash (ADR-0025). The caller seals
the revision first with `History::checkpoint`, and records success with
`mark_saved`; saves of one file are serialized by the caller. The
directory sync after the rename is skipped where the directory cannot be
opened (always on Windows, where `std` cannot open one; on Unix, one the
user may not read) or the file system refuses to sync one (`EINVAL`,
`ENOTSUP`); any other error syncing it fails the save, with the new text
already in place.

```mermaid
sequenceDiagram
    participant Caller as caller: a test today,<br/>workspace buffers on<br/>a worker (M4.3)
    participant Hist as text History
    participant Save as text file save
    participant Fs as file system

    Caller->>Hist: checkpoint()
    Hist-->>Caller: Checkpoint: the sealed<br/>revision and its exact text
    Caller->>Save: save(path, checkpoint.text(), format)
    loop at most 40 symbolic links
        Save->>Fs: symlink_metadata, read_link,<br/>where Windows' rules apply<br/>canonicalize the link's directory
    end
    Note over Save,Fs: The target is the file at the end<br/>of the links, each relative link read<br/>from its own directory as the OS<br/>reaches it (ADR-0034). It need not<br/>exist yet, unless it ends in a separator.
    opt the target exists
        Save->>Save: check_regular,<br/>else NotRegularFile
        Save->>Fs: open it for writing,<br/>without creating or truncating it
        Fs-->>Save: ok, or TextError::NotWritable
    end
    Save->>Fs: create_new a temp file named<br/>.tidepool-save- and a fixed-length<br/>suffix, in the target's directory
    Note over Save,Fs: Mode 0600 on Unix for an existing<br/>target, so no other process opens it<br/>first: the one cfg(unix) in the library<br/>of text. Up to 16 names are tried.
    Save->>Fs: encode(text, format) through a<br/>64 KiB BufWriter, the BOM first<br/>if the format has one
    Save->>Fs: the target's permissions, less<br/>setuid and setgid on Unix
    Save->>Fs: sync_all
    alt every step so far succeeded
        Save->>Fs: close the temp file, rename it over<br/>the target, so links stay links
        Save->>Fs: sync the directory, skipped where<br/>it cannot be opened or the file<br/>system refuses (EINVAL, ENOTSUP)
        alt the directory synced, or the sync was skipped
            Save-->>Caller: Ok
            Caller->>Hist: mark_saved(checkpoint)
            Note over Caller,Hist: is_modified() is now false exactly<br/>at this revision, on any branch
        else any other error syncing the directory
            Save-->>Caller: TextError::Io at FileStep::SyncDirectory:<br/>the new text is already in place,<br/>and the caller does not mark_saved
        end
    else a step failed before the rename
        Save->>Fs: remove the temp file
        Save-->>Caller: TextError::Io with its FileStep,<br/>the target as it was
    end
```

## 7. An edit, its undo and its redo

Every edit of a buffer goes through its `History` (ADR-0020): a
`Transaction` is built from the cursors, `commit` applies it to the rope
and keeps its inverse, and `undo` and `redo` apply one or the other. Each
call changes both the rope and the history or neither, and returns a
`Step`, whose change sets LSP and syntax are to read (planned M6.7,
M6.2) (`crates/text/src/transaction.rs`, `history.rs`). A commit joins the
current revision when that revision is open and the group matches: the
same `Command` id, or `Coalesce` with the same kind, inside the window,
with the cursors unmoved.

```mermaid
sequenceDiagram
    participant Caller as caller: a test today,<br/>editor commands (M4.2)
    participant Tx as Transaction
    participant Cs as ChangeSet
    participant Hist as History
    participant Buf as the buffer's Rope

    Caller->>Hist: History::new(text)
    Caller->>Tx: Transaction::change_each(text,<br/>selection, an edit per cursor)
    Tx->>Tx: sort the edits, collapse<br/>identical ones, merge<br/>overlapping deletions
    Tx->>Cs: the canonical ChangeSet,<br/>with each cursor<br/>mapped or placed
    Tx-->>Caller: a Transaction, its ChangeSet<br/>and the Selection after,<br/>or OverlappingEdits
    Caller->>Hist: commit(text, transaction,<br/>selection before, now, group)
    Hist->>Buf: same_tree: the rope must be<br/>the one the history last<br/>produced, else HistoryMismatch
    alt the change set changes nothing
        Hist-->>Caller: a Step with no change<br/>sets, nothing recorded
    else it changes the text
        Hist->>Cs: invert(text)
        Cs-->>Hist: the inverse
        alt the current revision is open and the group matches
            Hist->>Hist: join it: push the piece,<br/>merge the pieces like<br/>a binary counter
        else Group::New, or no match
            Hist->>Hist: a new revision, a child<br/>of the current one
        end
        Hist->>Cs: apply to a clone of the<br/>rope, then validate the<br/>selection after
        Hist->>Buf: replace the rope<br/>with the edited one
        Hist-->>Caller: a Step: the revision, the text<br/>before, the change sets,<br/>the selection
    end
    Caller->>Hist: undo(text)
    Hist->>Hist: check the rope, fold the<br/>open revision's pieces,<br/>close it
    Hist->>Cs: the inverse,<br/>applied to a clone
    Hist->>Buf: replace the rope
    Hist-->>Caller: Some(Step) at the parent, with<br/>the selection the edit was made from
    Caller->>Hist: redo(text)
    Hist->>Cs: the child's change set,<br/>applied to a clone
    Hist->>Buf: replace the rope
    Hist-->>Caller: Some(Step) at the child created<br/>or visited last, with its selection after
    Note over Caller,Buf: Undo at the root and redo with no child give None. A commit after an<br/>undo starts a branch and keeps the old one, which jump_to reaches<br/>through the lowest common ancestor.
```

## 8. Startup to the first frame (planned)

From `cargo tp` to the first frame, as `docs/01-architecture.md` → "The
main loop" and `docs/subsystems/frontends.md` plan it. M2.6 draws the first
frame (the backdrop and a line of each face); M3.4 adds the loop and the
shell, on ADR-0012, which is Proposed until then. The binary today is an
empty `main` (M0.1). The frontend draws with `gfx` (the framebuffer and the
backdrop, M2.3) and `font` (the bundled faces, M2.5), libraries it calls:
drawn as the frontend's own steps in 8.1, and as one participant in 8.2.

### 8.1 Opening the frontend

```mermaid
sequenceDiagram
    actor User
    participant Cli as tidepool main<br/>(M3.4)
    participant Cfg as Config load<br/>(M3.4, M4.7)
    participant Fe as gui frontend<br/>(M2.6)
    participant Plat as platform window<br/>and events (M2.2)
    participant Disp as X server,<br/>through XWayland

    Note over User,Disp: planned M2.2, M2.3, M2.5, M2.6, M3.4.<br/>Built: core's Args and InputEvent (M0.2),<br/>and sys's Xlib declarations (M2.1).
    User->>Cli: cargo tp, or tidepool
    Cli->>Cli: Args::from_env,<br/>core cli (built)
    Cli->>Cfg: Config::load(args)
    Cfg-->>Cli: settings.toml, a bad<br/>value replaced by its<br/>default, with a warning,<br/>read-only until M4.7
    alt --tui, or no display
        Cli->>Cli: Frontend::open gives a<br/>tui Terminal (M7.1),<br/>first frame within 150 ms
    else the GUI, by default
        Cli->>Fe: Frontend::open(config, mode)
        Fe->>Plat: Window::open(config)
        Plat->>Disp: libX11.so.6 through<br/>dlopen and sys's Xlib<br/>table (built),<br/>one Display,<br/>XCreateWindow
        Plat->>Plat: the scale<br/>from Xft.dpi
        Fe->>Fe: font (M2.5): load<br/>Saira and JetBrains Mono<br/>from assets/fonts/
        Fe->>Fe: gfx (M2.3): a Framebuffer<br/>at device resolution,<br/>the backdrop layers built
    end
```

### 8.2 The main loop, to the first frame

```mermaid
sequenceDiagram
    participant Shell as App, the shell<br/>(M3.4)
    participant Cli as tidepool main<br/>(M3.4)
    participant Fe as gui frontend<br/>(M2.6)
    participant Gfx as gfx and font<br/>(M2.3, M2.5)
    participant Plat as platform window<br/>and events (M2.2)
    participant Disp as X server,<br/>through XWayland

    Note over Shell,Disp: planned M2.2, M2.3, M2.5, M2.6, M3.4.<br/>core's InputEvent is built (M0.2).
    Cli->>Cli: an mpsc channel for<br/>workers, child readers<br/>and timers
    Cli->>Shell: App::new(config, tx,<br/>frontend.waker())
    loop every turn of the main loop
        Cli->>Fe: wait_events(deadline,<br/>events)
        Fe->>Plat: poll on the X connection<br/>and the Waker's self-pipe
        Plat-->>Fe: input, resize, focus, a<br/>wake, or the deadline
        Fe-->>Cli: InputEvent values,<br/>core input (built)
        Cli->>Shell: handle_input for each,<br/>then at most<br/>MAX_MSGS_PER_TURN<br/>messages, then fire_timers
        Cli->>Shell: build_frame(metrics)
        Shell-->>Cli: a Frame, its draw list<br/>and damage, or None<br/>when nothing is damaged
        opt a Frame
            Cli->>Fe: present(frame)
            Fe->>Gfx: rasterize the<br/>damaged parts
            Fe->>Plat: present(framebuffer, damage rects)
            Plat->>Disp: XPutImage of<br/>the damaged rects
        end
    end
    Note over Shell,Disp: Cold start to first frame within 300 ms for the GUI,<br/>checked with cargo tp -- --print-startup-time (M2.6).<br/>Idle: no redraw.
```

## 9. A keystroke to pixels, on the main thread (planned)

One key press, from the X server to the pixels on screen, all on the main
thread, which never waits on I/O, a child, a server or an agent
(`AGENTS.md` rule 6, `docs/01-architecture.md` → "Data flow"). Work that
could wait goes to other threads with a rope snapshot and the buffer's
version, and comes back as a message the loop reads on a later turn. The
whole flow is planned in M2.2, M2.6, M3.1, M3.4, M4.1, M4.2, M4.3 and
M4.6; `Transaction` and `History` are built (M1.2), and so is core's
`InputEvent` (M0.2).

### 9.1 The key, to the focused editor

```mermaid
sequenceDiagram
    actor User
    participant Disp as X server
    participant Plat as platform events<br/>(M2.2)
    participant Fe as gui frontend<br/>(M2.6)
    participant ML as app main loop<br/>(M3.4)
    participant Ui as ui input and<br/>focus (M3.1)

    Note over User,Ui: planned M2.2, M2.6, M3.1, M3.4.<br/>core's InputEvent is built (M0.2).
    ML->>Fe: wait_events(deadline,<br/>events)
    Fe->>Plat: wait_events: poll on<br/>the X connection<br/>and the self-pipe
    User->>Disp: presses a key
    Disp->>Plat: a key event on<br/>the X connection
    Plat-->>Fe: the event
    Fe-->>ML: an InputEvent,<br/>core input (built)
    ML->>Ui: route it to the focused<br/>node and its ancestors
    Ui-->>ML: the focused editor
```

### 9.2 The edit, and the work sent off

```mermaid
sequenceDiagram
    participant ML as app main loop<br/>(M3.4)
    participant Keys as editor keymap<br/>engine (M4.6)
    participant Cmd as editor command and<br/>buffer (M4.2, M4.3)
    participant Txt as text Transaction<br/>and History (built)
    participant Bg as syntax worker, lang,<br/>agent (M6.2, M6.7, M9.4)

    Note over ML,Bg: planned M3.4, M4.2, M4.3, M4.6.<br/>Transaction and History are built (M1.2).
    ML->>Keys: the key, in the current<br/>mode: the default keymap<br/>has one, vim mode adds<br/>the rest (M7.2)
    Keys->>Cmd: a named command, such<br/>as typing a character
    Cmd->>Txt: Transaction::change_each(text,<br/>selection, an edit per cursor)
    Txt-->>Cmd: a Transaction
    Cmd->>Txt: History::commit(text, transaction,<br/>selection before, now,<br/>Group::Coalesce)
    Txt-->>Cmd: a Step: the change<br/>sets and the selection
    Cmd->>Cmd: the buffer's version<br/>goes up by one
    Cmd-)Bg: a rope snapshot, a cheap clone,<br/>with the version: highlight,<br/>didChange, agent request
    Note over ML,Bg: The main thread sends, and never waits.<br/>A slow LSP server or agent adds 0 ms.
```

### 9.3 The frame, and a result on a later turn

```mermaid
sequenceDiagram
    participant ML as app main loop<br/>(M3.4)
    participant View as editor view and ui<br/>draw list (M4.1, M3.1)
    participant Fe as gui frontend<br/>(M2.6)
    participant Plat as platform events<br/>(M2.2)
    participant Disp as X server
    participant Bg as syntax worker, lang,<br/>agent (M6.2, M6.7, M9.4)

    Note over ML,Bg: planned M2.2, M2.6, M3.1, M3.4, M4.1
    ML->>View: build_frame(metrics)
    View-->>ML: a Frame: the draw<br/>list in logical units,<br/>and its damage
    ML->>Fe: present(frame)
    Fe->>Fe: rasterize only the<br/>damaged rects with gfx<br/>and font (M3.5, M2.3, M2.5)
    Fe->>Plat: present(framebuffer,<br/>damage rects)
    Plat->>Disp: XPutImage
    Note over ML,Disp: Keystroke to pixels: p99 within 16 ms<br/>at 1920x1080 on the reference machine
    Bg-)ML: a Msg with the version it was made for, through mpsc,<br/>and the Waker ends wait_events
    ML->>ML: handle_message on a<br/>later turn: a result for<br/>an older version is dropped
```

## 10. An LSP request and a stale result (planned)

A completion request to rust-analyzer while the user goes on typing. Every
request carries the buffer version it was made for, and the loop drops a
result for an older version (`docs/01-architecture.md` → "The main loop",
`docs/subsystems/languages.md`). The flow is planned in M3.4, M5.2, M6.1,
M6.3 and M6.7. Earlier, the session started rust-analyzer as a child of
`toolchain`'s sessions (M5.2) and sent `initialize` with
`positionEncoding`, `utf-8` preferred and `utf-16` by default.

### 10.1 The requests, and a cancellation

```mermaid
sequenceDiagram
    actor User
    participant Ed as editor and completion<br/>popup (M6.3)
    participant Lang as lang session<br/>(M6.7)
    participant Txt as text positions<br/>(built)
    participant Rpc as rpc JSON-RPC<br/>client (M6.1)
    participant Ra as rust-analyzer, a child of<br/>toolchain sessions (M5.2)

    Note over User,Ra: planned M5.2, M6.1, M6.3, M6.7.<br/>The session has already sent initialize.
    User->>Ed: types, and the buffer<br/>is at version 8
    Ed->>Lang: the Step's change<br/>sets, version 8
    Lang->>Txt: byte offsets to LSP<br/>positions: byte_to_line_col<br/>with Unit::Utf16, or<br/>Unit::Byte for utf-8
    Lang->>Rpc: textDocument/didChange,<br/>version 8, incremental
    Rpc->>Ra: Content-Length framed,<br/>on its stdin
    Ed->>Lang: completion at the<br/>cursor, for version 8
    Lang->>Rpc: textDocument/completion,<br/>request A, made for version 8
    Rpc->>Ra: request A
    User->>Ed: types again, and the<br/>buffer is at version 9
    Ed->>Lang: the change sets,<br/>version 9
    Lang->>Rpc: didChange for version 9,<br/>then $/cancelRequest for A
    Rpc->>Ra: didChange,<br/>$/cancelRequest
    Lang->>Rpc: textDocument/completion,<br/>request B, made for version 9
    Rpc->>Ra: request B
```

### 10.2 The answers, and the stale one dropped

```mermaid
sequenceDiagram
    actor User
    participant Ed as editor and completion<br/>popup (M6.3)
    participant ML as app main loop<br/>(M3.4)
    participant Rd as reader of the server's<br/>stdout (M6.1)
    participant Ra as rust-analyzer, a child of<br/>toolchain sessions (M5.2)

    Note over User,Ra: planned M3.4, M5.2, M6.1, M6.3.<br/>codec json is built (M1.4).
    Ra-->>Rd: the answer to A, or<br/>its cancellation error
    Rd->>Rd: framing, and parsing<br/>with codec json and<br/>its limits (built)
    Rd-)ML: a Msg: the result<br/>of A, for version 8
    ML->>ML: the buffer is at<br/>version 9, so it<br/>is dropped
    Ra-->>Rd: the answer to B
    Rd-)ML: a Msg: the result<br/>of B, for version 9
    ML->>Ed: the items, their ranges<br/>converted back to byte<br/>offsets through text
    Ed->>User: the completion popup
```

## 11. Installing a managed tool (planned)

Installing rustup from the Toolchains panel (ADR-0002,
`docs/subsystems/toolchains.md`): the plan and its licenses first, nothing
before the user confirms, the download through a `curl` child, and the
SHA-256 check before anything from it runs. `editor`'s row in the
dependency matrix does not list `toolchain`, so the panel reaches it
through `app`, which may use every crate. The flow is planned in M3.4,
M5.1, M5.2, M5.3, M5.4, M5.5 and M5.6; SHA-256 in `core::hash` is built
(M0.2, ADR-0016). The core binary opens no sockets: `curl` does the HTTPS
(`AGENTS.md` rule 8). rust-analyzer then comes from `rustup component add
rust-analyzer` (M5.5).

A step that needs `sudo` or an admin prompt, such as git's package-manager
plan (M5.5), is never elevated silently: `app` puts the command in the
integrated terminal (`editor`'s terminal panel, M5.4), where the user
reads it, presses Run and answers the prompt there.

### 11.1 The plan, and the user's answer

```mermaid
sequenceDiagram
    actor User
    participant Panel as editor Toolchains<br/>panel (M5.6)
    participant App as app wiring<br/>(M3.4)
    participant Plan as toolchain install<br/>(M5.3)
    participant Reg as toolchain descriptor,<br/>detect, registry (M5.1)

    Note over User,Reg: planned M3.4, M5.1, M5.3, M5.6
    User->>Panel: Install, on<br/>the rustup card
    Panel->>App: the action
    App->>Plan: a plan for rustup
    Plan->>Reg: rustup's descriptor in<br/>toolchains/: the source,<br/>its SHA-256 pin, where<br/>its license text is
    Reg-->>Plan: the descriptor
    Plan-->>App: the steps, sources,<br/>sizes and licenses
    App->>Panel: the plan
    Panel->>User: every step and the<br/>license text, before<br/>anything runs
    alt the user cancels
        Panel->>Panel: nothing runs
    else the user confirms
        Panel->>App: run the plan
        App->>Plan: run it, on a worker (11.2)
    end
```

### 11.2 The download, and its check

```mermaid
sequenceDiagram
    participant Plan as toolchain install<br/>(M5.3)
    participant Dl as toolchain download<br/>(M5.3)
    participant Hash as core hash<br/>Sha256 (built)
    participant Proc as toolchain process<br/>(M5.2)
    participant Curl as curl

    Note over Plan,Curl: planned M5.2, M5.3. SHA-256 in<br/>core hash is built (M0.2, ADR-0016).
    Plan->>Dl: download rustup-init<br/>from its official URL
    Dl->>Proc: Spawn curl: an argv vector,<br/>an explicit environment,<br/>a timeout
    Proc->>Curl: --proto =https --tlsv1.2<br/>--fail --location,<br/>to a temp file
    Curl-->>Proc: exit status
    Proc-->>Dl: the temp file
    Dl->>Hash: the file's SHA-256
    Hash-->>Dl: the digest
    alt the digest is not the descriptor's pin
        Dl->>Dl: delete the file
        Dl-->>Plan: a checksum mismatch, and<br/>nothing from the file runs
    else it matches
        Dl-->>Plan: the checked file (11.3)
    end
```

### 11.3 The install, and the result

```mermaid
sequenceDiagram
    participant Panel as editor Toolchains<br/>panel (M5.6)
    participant App as app wiring<br/>(M3.4)
    participant Plan as toolchain install<br/>(M5.3)
    participant Proc as toolchain process<br/>(M5.2)
    participant Inst as rustup-init
    participant Reg as toolchain descriptor,<br/>detect, registry (M5.1)

    Note over Panel,Reg: planned M3.4, M5.1, M5.2, M5.3, M5.6
    Plan->>Proc: run rustup-init as the<br/>user, never elevated
    Proc->>Inst: an argv vector, an<br/>explicit environment
    Inst-->>Proc: exit status
    Plan->>Reg: detect again: rustup<br/>--version, with a timeout
    Reg-->>Plan: Found, with its<br/>path and version
    Plan-)App: the result, as a message<br/>to the main loop
    App->>Panel: the card's new state
```

## 12. An agent's edit proposal (planned)

An edit proposal from a language agent over TAP (ADR-0009, Proposed until
M9.1; `docs/subsystems/agents.md`). Tidepool assembles every byte the agent
sees, the agent can only propose, and the user reviews the proposal as a
diff before it touches the buffer. The flow is planned in M5.2, M6.1,
M9.3, M9.5 and M9.6; the agent was spawned through `toolchain`'s sessions
and sent `initialize` with `tapVersion` 0.1.

### 12.1 Consent, and the context

```mermaid
sequenceDiagram
    actor User
    participant Chat as editor chat panel<br/>and proposals (M9.5)
    participant Host as agent host<br/>and tap (M9.3)
    participant Cons as agent consent<br/>(M9.3)
    participant Ctx as agent context<br/>(M9.3)

    Note over User,Ctx: planned M9.3, M9.5
    User->>Chat: an instruction, for<br/>a range of the buffer
    Chat->>Host: propose edits: the<br/>instruction, the uri,<br/>the range
    Host->>Cons: is proposeEdits granted<br/>to this agent in<br/>this workspace?
    alt no grant yet
        Cons-->>Host: not yet
        Host-->>Chat: consent needed
        Chat->>User: once, always<br/>or never?
        User-->>Chat: the answer
        Chat->>Cons: record it, for this<br/>workspace and agent
    end
    Host->>Ctx: build the request's<br/>context
    Ctx->>Ctx: prefix and suffix windows,<br/>open buffers and symbols<br/>only when granted, size caps
    Ctx->>Ctx: leave out what<br/>.tidepoolignore or<br/>the deny-list matches
    Note over Ctx: Deny-list: .env*, *.pem, *.key,<br/>*.jks, *.keystore, *.p12, *.p8,<br/>*.mobileprovision, id_*,<br/>keystore.properties, .git/, and<br/>files the user marks private.<br/>An untrusted workspace (ADR-0014)<br/>sends only the focused buffer,<br/>with consent.
    Ctx-->>Host: the context (12.2)
```

### 12.2 The request, and the user's review

```mermaid
sequenceDiagram
    actor User
    participant Chat as editor chat panel<br/>and proposals (M9.5)
    participant Host as agent host<br/>and tap (M9.3)
    participant Rpc as rpc JSON-RPC<br/>(M6.1)
    participant Agt as agent process, such<br/>as tp-agent-claude (M9.6)
    participant Hist as text History<br/>(built)

    Note over User,Hist: planned M5.2, M6.1, M9.3, M9.5, M9.6.<br/>History is built (M1.2).
    Host->>Rpc: tap/proposeEdits with<br/>instruction, uri and range
    Rpc->>Agt: Content-Length framed,<br/>on the agent's stdin
    Agt-->>Rpc: an LSP-shaped<br/>WorkspaceEdit
    Rpc-->>Host: the parsed result
    Host->>Host: validate its sizes, ranges<br/>and document version,<br/>never run or render its text
    alt invalid, or for an older version
        Host-->>Chat: dropped
    else valid
        Host-->>Chat: the proposal
        Chat->>User: shows it as a diff
        alt the user accepts
            Chat->>Hist: commit it as one Transaction,<br/>with Group::Command:<br/>one undo step
        else the user rejects
            Chat->>Chat: the buffer<br/>is unchanged
        end
    end
```

## 13. Building and signing an Android APK (planned)

A release APK from the Gradle-free pipeline, signed by `apksigner`
(ADR-0007, Proposed until M10.3; `docs/subsystems/mobile.md`). The
keystore password stays in memory as a `SecretArg` and reaches `apksigner`
on its standard input, never on argv. The flow is planned in M5.2, M10.1,
M10.2, M10.3, M10.5 and M10.6. An AAB uses `bundletool build-bundle`,
then `jarsigner` with `-storepass:env NAME`, then `jarsigner -verify`.

### 13.1 Building the APK

```mermaid
sequenceDiagram
    actor User
    participant Panel as editor build<br/>artifacts panel (M10.6)
    participant Direct as mobile android<br/>direct (M10.5)
    participant Sign as mobile android<br/>sign (M10.3)
    participant Proc as toolchain process<br/>(M5.2)
    participant Sdk as JDK and SDK<br/>build-tools,<br/>managed (M10.1)

    Note over User,Sdk: planned M5.2, M10.1, M10.2, M10.3, M10.5, M10.6
    User->>Panel: build a release APK
    Panel->>Direct: run the pipeline of<br/>tidepool-android.toml
    Direct->>Proc: aapt2 compile the resources,<br/>aapt2 link against android.jar
    Proc->>Sdk: argv vectors
    Direct->>Proc: javac, then d8. A Rust app:<br/>cargo build --release with<br/>the NDK's clang as linker
    Proc->>Sdk: argv vectors
    Direct->>Direct: assemble the APK zip,<br/>codec zip (M10.2)
    Direct->>Proc: zipalign -p 4
    Proc->>Sdk: zipalign
    Direct->>Sign: sign the aligned<br/>APK, with the keystore<br/>and key alias (13.2)
```

### 13.2 Signing it, and verifying the signature

```mermaid
sequenceDiagram
    actor User
    participant Panel as editor build<br/>artifacts panel (M10.6)
    participant Sign as mobile android<br/>sign (M10.3)
    participant Sec as toolchain secret<br/>(M5.2)
    participant Proc as toolchain process<br/>(M5.2)
    participant Sdk as JDK and SDK<br/>build-tools,<br/>managed (M10.1)

    Note over User,Sdk: planned M5.2, M10.1, M10.3, M10.6
    Sign-->>Panel: the keystore password<br/>is needed
    Panel->>User: asks for it,<br/>once a session
    User-->>Panel: the password
    Panel->>Sign: the password
    Sign->>Sec: hold it as a SecretArg,<br/>in memory only, redacted<br/>in logs and Debug
    Sign->>Proc: apksigner sign<br/>--ks-pass stdin
    Proc->>Sdk: start apksigner, with<br/>no secret on its argv
    Proc->>Sdk: the password,<br/>on its stdin
    Sdk-->>Proc: exit status
    Sign->>Proc: apksigner verify<br/>--verbose
    Proc->>Sdk: apksigner verify
    Sdk-->>Proc: the verifier's<br/>output and status
    Proc-->>Sign: the result
    Sign-->>Panel: the signed APK and<br/>its verification
    Note over User,Sdk: Tidepool never reads the keystore. A test checks<br/>that no constructed argv holds a secret (M10.3).
```

## Sources

- `xtask/src/main.rs`, `check.rs`, `cargo.rs`, `deps.rs`, `layers.rs`,
  `crates.rs`, `metadata.rs`, `ucd.rs`, `workflows.rs`, `fuzz.rs`
- `crates/core/src/testing/fuzz.rs`, `fuzz/engine.rs`, `fuzz/paths.rs`
- `crates/codec/tests/fuzz.rs`, `crates/text/tests/fuzz.rs`
- `crates/text/src/file.rs`, `encoding.rs`, `transaction.rs`,
  `history.rs`, `pos.rs`, `lib.rs`
- `crates/core/src/cli.rs`, `input.rs`, `hash.rs`
- `crates/app/src/main.rs`, `crates/app/README.md`
- `AGENTS.md` (rules 2, 6, 7 and 8)
- `docs/00-overview.md` → "Performance budgets"
- `docs/01-architecture.md` → "Dependency matrix", "The main loop", "Data
  flow", "Threading model"
- `docs/03-platforms.md` → "Linux"
- `docs/04-quality.md` → "Fuzz smoke", "`xtask` checks"
- `docs/subsystems/frontends.md`, `sys-and-platform.md`, `rendering.md`,
  `ui-and-theme.md`, `editor.md`, `languages.md`, `toolchains.md`,
  `agents.md`, `mobile.md`
- `docs/decisions.md`: ADR-0002, ADR-0007, ADR-0009, ADR-0012, ADR-0014,
  ADR-0016, ADR-0017, ADR-0020, ADR-0025, ADR-0027, ADR-0029
- `docs/roadmap.md`: the tasks named in each section
