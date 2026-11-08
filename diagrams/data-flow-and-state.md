# Data flow and state diagrams

These diagrams show how data moves through Tidepool and which states its
parts go through. Sections 1 to 5 are data-flow diagrams: a file through
the text engine and the workspace around it, a managed tool's install, an
agent's context, and fuzz smoke. Sections 6 to 12 are state diagrams: the
fuzz engine's run, a text `History`, a saved file, workspace trust, vim
mode, a managed tool and a language session. The text engine, the fuzz
engine and fuzz smoke are built; the rest is planned.

Drawn from the code and docs as of roadmap task M0.13 (2026-11-08, version 0.1). What the code has is drawn solid; what the roadmap plans is dashed and labelled with the task that builds it (ADR-0029).

## Legend

| Style | Means |
| :- | :- |
| Dark blue box | A person: the user or a developer |
| Grey box | An external entity: a program or system outside Tidepool's code |
| Grey cylinder | A store outside Tidepool's code: files on disk, a tool's install directory |
| Blue rounded box | A process the code has: a function or a command |
| Blue cylinder | A data store the code has, in memory or on disk |
| White shape with a dashed border | Planned: not built yet. Its label names the task that builds it |
| Solid arrow | A data flow the code has, labelled with what moves |
| Dotted arrow | A planned data flow, labelled with what moves and its task |
| Blue state | A state the code has |
| White state with a dashed border | A planned state, labelled with its task |
| Box with states inside | A composite state: the part is in one of the states inside it |
| `[*]` | Where a state diagram starts or ends |
| Diamond | A choice: the next state depends on a check |

- Mermaid cannot dash a transition in a state diagram, so a planned
  transition says "planned M#.#" in its label. In a section whose title
  says "planned", every state and transition is planned, and the note
  names the tasks.
- Names in code font are the code's own. Planned parts use the names their
  spec gives, and are described in words where the spec gives none.
- A diagram split into numbered parts (3.1, 3.2) draws again, in short,
  the nodes or states the parts share.

## 1. Data flow: a file through `tidepool-text` (built)

A caller loads a file, edits its rope through its `History`, and saves a
revision atomically (ADR-0025). Today the caller is `tidepool-text`'s own
tests; `workspace::buffers` takes that place in M4.3 (section 2).

```mermaid
flowchart TB
    caller["Caller<br/>today the tests<br/>of tidepool-text"]
    disk[("File on disk<br/>the target, symbolic<br/>links followed")]
    temp[("Temporary file<br/>beside the target<br/>.tidepool-save-PID-<br/>COUNT-TIME.tmp")]
    rope[("The buffer's Rope<br/>held by the caller,<br/>O(1) snapshots")]
    hist[("History<br/>the revision tree:<br/>each change, its inverse,<br/>the selections")]
    load(["file::load<br/>regular files only,<br/>64 KiB blocks into<br/>encoding::Decoder"])
    edit(["History::commit, undo,<br/>redo, jump_to<br/>change the Rope and<br/>the History, both<br/>or neither"])
    ckpt(["History::checkpoint<br/>seals the current<br/>revision"])
    save(["file::save<br/>resolve links, temp file,<br/>encoding::encode, sync,<br/>rename, sync the<br/>directory"])
    mark(["History::mark_saved<br/>the checkpoint's<br/>revision is on disk"])

    disk -->|"bytes"| load
    load -->|"Loaded: Rope, Format<br/>or None, LineEndings,<br/>trailing break, is_large"| caller
    caller -->|"the loaded Rope"| rope
    caller -->|"History::new: the root<br/>revision, marked saved"| hist
    caller -->|"Transaction, selection<br/>before, Instant, Group"| edit
    rope -->|"the text before"| edit
    edit -->|"the text after"| rope
    hist -->|"the inverse or the<br/>change to apply"| edit
    edit -->|"a new or<br/>joined revision"| hist
    edit -->|"Step: text before,<br/>ChangeSets, Selection,<br/>RevisionId"| caller
    hist -->|"the current revision<br/>and its text"| ckpt
    ckpt -->|"Checkpoint::text,<br/>with the Format"| save
    save -->|"the encoded bytes,<br/>BOM first if the<br/>Format has one"| temp
    temp -->|"renamed over<br/>the target"| disk
    save -->|"Ok"| mark
    save -->|"TextError with<br/>its FileStep"| caller
    mark -->|"the saved revision"| hist

    classDef person fill:#08427b,stroke:#052e56,color:#ffffff
    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class caller,disk external
    class temp,rope,hist store
    class load,edit,ckpt,save,mark built
```

- Decoding never fails: bytes that are not text become U+FFFD and are kept
  beside the text, and such a file has no save `Format` (section 8).
- Line endings and the trailing newline are reported, never changed, so an
  unchanged file saves byte for byte (ADR-0019).
- An error before the rename removes the temporary file and leaves the
  target untouched; a crash before it leaves the temporary file behind.

## 2. Data flow: buffers, watching and session around the text engine (planned)

The workspace wraps section 1: buffers keyed by canonical path, loaded and
saved on workers, watched for outside changes, and remembered in the
session. None of it is built; the text engine it calls is.

```mermaid
flowchart TB
    user["User<br/>edits, saves,<br/>opens folders"]
    fsEvents["OS file events<br/>inotify on Linux"]
    disk[("Files on disk")]
    text(["tidepool-text<br/>file::load, History,<br/>file::save, as in<br/>section 1"])
    cmds(["editor commands<br/>edits as Transactions,<br/>one command one<br/>undo step<br/>planned M4.2"])
    buffers(["workspace::buffers<br/>keyed by canonical path,<br/>loads and saves on<br/>workers, large-file<br/>mode above 16 MB<br/>planned M4.3"])
    meta(["File metadata<br/>through platform<br/>link count, owner<br/>and group<br/>planned M4.3"])
    watcher(["platform file watching<br/>inotify<br/>planned M4.5"])
    watch(["workspace::watch<br/>debounced file events,<br/>polling fallback<br/>planned M4.5"])
    session(["workspace::session<br/>open editors, layout,<br/>cursors<br/>planned M4.3"])
    sessionStore[("Session state<br/>in the user data<br/>directory<br/>planned M4.3")]
    recovery[("recovery/<br/>unsaved buffers<br/>after a crash<br/>planned M4.9")]

    user -.->|"keys and commands,<br/>planned M4.2"| cmds
    cmds -.->|"Transactions,<br/>planned M4.2"| buffers
    buffers -.->|"load, commit, checkpoint,<br/>save, mark_saved,<br/>planned M4.3"| text
    disk -->|"bytes"| text
    text -->|"the atomic save"| disk
    disk -.->|"link count, owner,<br/>group, planned M4.3"| meta
    meta -.->|"before a save: a second<br/>hard link, or another<br/>owner or group,<br/>planned M4.3"| buffers
    buffers -.->|"a warning before such<br/>a save, planned M4.3"| user
    buffers -.->|"reload or merge prompts<br/>after an outside change,<br/>planned M4.5"| user
    fsEvents -.->|"changed paths,<br/>planned M4.5"| watcher
    watcher -.->|"file events,<br/>planned M4.5"| watch
    watch -.->|"a file changed on disk,<br/>planned M4.5"| buffers
    buffers -.->|"the open buffers,<br/>planned M4.3"| session
    session -.->|"saved, planned M4.3"| sessionStore
    sessionStore -.->|"restored on start,<br/>planned M4.3"| session
    buffers -.->|"the panic hook saves<br/>unsaved buffers,<br/>planned M4.9"| recovery
    recovery -.->|"offered on the next<br/>start, planned M4.9"| user

    classDef person fill:#08427b,stroke:#052e56,color:#ffffff
    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class user person
    class fsEvents,disk external
    class text built
    class cmds,buffers,meta,watcher,watch,session,sessionStore,recovery planned
```

- A save renames a new file over the old one, which splits a hard link
  and gives the file the saver's owner and group (ADR-0025). So buffers
  check the link count, owner and group first, and warn or write in place
  (M4.3).
- The reload or merge prompts after an outside change come with file
  watching (M4.5), whose acceptance checks them.
- Only the main thread touches buffers; other threads get immutable rope
  snapshots (`docs/01-architecture.md` → "Threading model").

## 3. Data flow: a managed tool's install (planned)

A tool is found from its descriptor, and an install or update runs only
after the user confirms its plan; a download is checked against its pinned
SHA-256 before anything from it runs (ADR-0002, ADR-0016). Only the SHA-256
function in `tidepool-core` is built. 3.1 draws how a tool is found, and
3.2 an install or update.

### 3.1 Detecting a tool

```mermaid
flowchart TB
    descs[("toolchains/*.toml<br/>detection, version rules,<br/>install methods,<br/>checksums, license text<br/>planned M5.1")]
    toolsCheck(["cargo xtask tools<br/>every descriptor has an<br/>HTTPS source, a SHA-256<br/>and a license entry<br/>planned M5.1"])
    toolDir[("The tool's standard<br/>location, where its own<br/>installer puts it")]
    detect(["toolchain::detect<br/>settings override,<br/>env vars, standard<br/>locations, PATH, then<br/>the version command<br/>with a timeout<br/>planned M5.1"])
    registry[("toolchain::registry<br/>the live state<br/>of every tool<br/>planned M5.1")]
    record[("toolchains.toml<br/>in user data<br/>what was found<br/>or installed<br/>planned M5.1, M5.3")]

    descs -.->|"every descriptor,<br/>planned M5.1"| toolsCheck
    descs -.->|"detection rules,<br/>planned M5.1"| detect
    toolDir -.->|"the tool and its<br/>version output,<br/>planned M5.1"| detect
    detect -.->|"Missing, Found,<br/>Outdated or Broken,<br/>planned M5.1"| registry
    registry -.->|"what was found<br/>or installed,<br/>planned M5.1, M5.3"| record

    classDef person fill:#08427b,stroke:#052e56,color:#ffffff
    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class toolDir external
    class descs,toolsCheck,detect,registry,record planned
```

### 3.2 An install or update

```mermaid
flowchart TB
    user["User"]
    source["Official HTTPS source<br/>the URL in the<br/>descriptor"]
    curl["curl<br/>a managed tool"]
    toolDir[("The tool's standard<br/>location, as in 3.1")]
    descs[("toolchains/*.toml<br/>as in 3.1<br/>planned M5.1")]
    registry[("toolchain::registry<br/>as in 3.1<br/>planned M5.1")]
    detect(["toolchain::detect<br/>as in 3.1<br/>planned M5.1"])
    plan(["toolchain::install<br/>the plan: download,<br/>verify, run installer,<br/>accept license, run<br/>package manager<br/>planned M5.3"])
    download(["toolchain::download<br/>curl --proto =https<br/>--tlsv1.2 --fail<br/>--location to a temp<br/>file, then the check<br/>planned M5.3"])
    tempFile[("Downloaded temp file<br/>planned M5.3")]
    sha(["core::hash::sha256<br/>FIPS 180-4,<br/>built in M0.2"])
    install(["Run the installer<br/>or package manager<br/>as the user, never<br/>elevated silently<br/>planned M5.5"])
    terminal(["Integrated terminal<br/>tidepool-term on<br/>a platform PTY<br/>planned M5.4"])

    registry -.->|"a Missing or Outdated<br/>tool, planned M5.3"| plan
    descs -.->|"install method, source,<br/>checksum, license,<br/>planned M5.3"| plan
    plan -.->|"steps, sources, sizes,<br/>licenses, planned M5.3"| user
    user -.->|"confirms, or nothing<br/>runs, planned M5.3"| plan
    plan -.->|"the download step,<br/>planned M5.3"| download
    download -.->|"an argv through<br/>toolchain::process,<br/>planned M5.2, M5.3"| curl
    source -.->|"HTTPS, planned M5.3"| curl
    curl -.->|"writes, planned M5.3"| tempFile
    tempFile -.->|"the bytes,<br/>planned M5.3"| sha
    sha -.->|"the digest,<br/>planned M5.3"| download
    descs -.->|"the pinned digest,<br/>planned M5.3"| download
    download -.->|"on a mismatch the file<br/>is deleted and reported,<br/>nothing runs,<br/>planned M5.3"| user
    download -.->|"a verified file,<br/>planned M5.3"| install
    install -.->|"commands that need<br/>admin, license prompts,<br/>planned M5.4, M5.5"| terminal
    user -.->|"reads, presses Run,<br/>answers, planned M5.5"| terminal
    install -.->|"installs, planned M5.5"| toolDir
    install -.->|"detect again,<br/>planned M5.5"| detect

    classDef person fill:#08427b,stroke:#052e56,color:#ffffff
    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class user person
    class source,curl,toolDir external
    class sha built
    class descs,registry,detect,plan,download,tempFile,install,terminal planned
```

- Descriptor versions and checksums change only by a reviewed change,
  never at runtime (`docs/subsystems/toolchains.md`).
- In an untrusted workspace (ADR-0014) tools get detection only: no
  project-owned executable runs.
- The plan is shown in the Toolchains panel and the first-run checklist
  (M5.6).

## 4. Data flow: an agent's context (planned)

Tidepool assembles every byte an agent receives, gated by consent and
workspace trust and filtered by the context rules; the agent answers over
TAP, and an edit comes back only as a proposal for the user to review
(ADR-0009, `docs/subsystems/agents.md`). Only `History`, where an accepted
proposal lands, is built.

```mermaid
flowchart TB
    user["User"]
    agentProc["Language agent<br/>a process speaking<br/>TAP on stdin<br/>and stdout"]
    files[("Workspace files<br/>on disk")]
    buffers[("Open buffers<br/>rope snapshots from<br/>workspace::buffers<br/>planned M4.3")]
    symbols(["Symbols from<br/>tidepool-lang<br/>document and<br/>workspace symbols<br/>planned M6.8"])
    manifest[("agent.toml<br/>in the user and<br/>workspace agent<br/>directories<br/>planned M9.3")]
    grants[("Consent grants<br/>in settings.toml<br/>planned M9.3")]
    trustStore[("Trust decisions<br/>in user data<br/>planned M4.7")]
    ignore(["workspace::folder<br/>.tidepoolignore<br/>matching<br/>planned M4.7"])
    consent(["agent::consent<br/>once, always or never,<br/>per workspace, agent<br/>and capability<br/>planned M9.3"])
    context(["agent::context<br/>prefix and suffix<br/>windows, open buffers<br/>and symbols when<br/>granted<br/>planned M9.3"])
    rules(["Context rules<br/>size caps,<br/>.tidepoolignore, the<br/>secret deny-list, files<br/>marked private<br/>planned M9.3"])
    host(["agent::host and<br/>agent::tap<br/>spawned through<br/>toolchain::sessions,<br/>JSON-RPC from<br/>tidepool-rpc<br/>planned M9.3"])
    agentLog[("Agent log<br/>the agent's stderr<br/>planned M9.3")]
    checks(["Output checks<br/>sizes, ranges, document<br/>versions, stale<br/>results dropped<br/>planned M9.3, M9.4"])
    ghost(["Completion popup<br/>and ghost text<br/>planned M9.4"])
    proposal(["Edit proposal<br/>shown as a diff<br/>planned M9.5"])
    hist[("History<br/>of the buffer the<br/>proposal edits")]

    buffers -.->|"the focused buffer,<br/>other open buffers<br/>when granted,<br/>planned M9.3"| context
    files -.->|"only with<br/>context.workspaceRead<br/>granted, planned M9.3"| context
    symbols -.->|"only with<br/>context.symbols<br/>granted, planned M9.3"| context
    manifest -.->|"capabilities and<br/>requests, planned M9.3"| consent
    grants -.->|"stored grants,<br/>planned M9.3"| consent
    trustStore -.->|"trusted or not,<br/>planned M4.7"| consent
    user -.->|"answers consent<br/>prompts, planned M9.3"| consent
    consent -.->|"what may be sent, only<br/>the focused buffer when<br/>untrusted, planned M9.3"| context
    context -.->|"the assembled context,<br/>planned M9.3"| rules
    ignore -.->|"ignored paths,<br/>planned M4.7"| rules
    rules -.->|"what passes,<br/>planned M9.3"| host
    host -.->|"didOpen, didChange,<br/>tap/inlineCompletion,<br/>tap/predict, tap/chat,<br/>tap/proposeEdits,<br/>planned M9.3"| agentProc
    agentProc -.->|"results and<br/>tap/chatDelta,<br/>planned M9.3"| host
    agentProc -.->|"stderr, planned M9.3"| agentLog
    host -.->|"untrusted output,<br/>planned M9.3"| checks
    checks -.->|"inline completions<br/>and predictions,<br/>planned M9.4"| ghost
    checks -.->|"a WorkspaceEdit,<br/>planned M9.5"| proposal
    user -.->|"reviews the diff,<br/>accepts or rejects,<br/>planned M9.5"| proposal
    proposal -.->|"on accept, one commit<br/>with Group::Command,<br/>one undo step,<br/>planned M9.5"| hist

    classDef person fill:#08427b,stroke:#052e56,color:#ffffff
    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class user person
    class agentProc,files external
    class hist store
    class buffers,symbols,manifest,grants,trustStore,ignore,consent,context,rules,host,agentLog,checks,ghost,proposal planned
```

- The deny-list applies always, beside `.tidepoolignore`: `.env*`,
  `*.pem`, `*.key`, `*.jks`, `*.keystore`, `*.p12`, `*.p8`,
  `*.mobileprovision`, `id_*`, `keystore.properties` and `.git/`.
- Agents never get paths to read for themselves, cannot run anything and
  cannot write files. Requests carry the buffer version, are cancelled on
  every keystroke and time out (`docs/subsystems/agents.md` → "Merging and
  display").
- The dependency matrix does not let `agent` depend on `lang`, so the
  symbols must reach `context` through a crate that may use both, such as
  `editor`. The spec does not say which.

## 5. Data flow: fuzz smoke (built)

`cargo xtask fuzz-smoke` runs each fuzz target's test in a `cargo test`
child, which replays the saved crashes and the seeds, then mutates the
seeds, and saves each finding's input (ADR-0027). A plain `cargo test`
runs the same tests with the unchanged inputs only.

```mermaid
flowchart TB
    dev["Developer"]
    ci["CI on Linux<br/>fuzz-smoke --secs 30<br/>after the gate"]
    procStatus["Linux<br/>/proc/self/status<br/>VmHWM and VmPeak"]
    smoke(["xtask fuzz-smoke<br/>checks TARGETS<br/>against fuzz/, then<br/>one cargo test child<br/>a target"])
    test(["A target's test<br/>in tests/fuzz.rs<br/>Target::run: Config<br/>from the environment,<br/>reads the inputs"])
    engine(["fuzz engine<br/>unchanged inputs, then<br/>mutated ones, a watchdog<br/>for hangs, the<br/>memory checks"])
    mutator(["mutate<br/>1 to 16 changes,<br/>from a Pcg32 stream<br/>for each input"])
    parser(["The code under test<br/>json, toml or encoding,<br/>on a thread with<br/>a 2 MiB stack"])
    corpus[("fuzz/corpus/TARGET/<br/>the seeds,<br/>in name order")]
    inPlace[("Seeds read in place<br/>crates/codec/tests/json/,<br/>crates/codec/tests/<br/>toml-test/")]
    crashes[("fuzz/crashes/TARGET/<br/>KIND-HASH files,<br/>died-HASH, the<br/>hidden .last-input")]
    report(["Outcome and Summary<br/>the fuzz-result: line,<br/>and assert_clean fails<br/>the test with the report"])

    dev -->|"cargo xtask fuzz-smoke<br/>--secs N --target T<br/>--seed S --replay"| smoke
    ci -->|"cargo xtask fuzz-smoke<br/>--secs 30"| smoke
    dev -->|"cargo test: the<br/>unchanged inputs only"| test
    corpus -->|"directory names,<br/>checked against<br/>TARGETS"| smoke
    smoke -->|"argv and<br/>TIDEPOOL_FUZZ_SECS,<br/>TIDEPOOL_FUZZ_SEED,<br/>TIDEPOOL_FUZZ_REPLAY"| test
    crashes -->|"saved crashes, and<br/>a replay's leftover<br/>.last-input"| test
    corpus -->|"seeds"| test
    inPlace -->|"seeds, in path order"| test
    test -->|"Plan: crashes, seeds,<br/>dictionary, caps,<br/>Config"| engine
    engine -->|"the seeds and<br/>the dictionary"| mutator
    mutator -->|"input n"| engine
    engine -->|"each input"| parser
    parser -->|"returns, panics<br/>or hangs"| engine
    procStatus -->|"the peaks, after<br/>an input or on<br/>a time budget"| engine
    engine -->|"a finding's input,<br/>written whole, and in<br/>a replay every input<br/>to .last-input"| crashes
    engine -->|"findings, why it<br/>stopped, memory, the<br/>slowest input"| report
    report -->|"the fuzz-result: line<br/>and the test's verdict,<br/>through cargo"| smoke
    smoke -->|"after a replay that died<br/>or exited without a result,<br/>keep_died renames<br/>.last-input to died-HASH,<br/>or removes it when a<br/>saved crash holds<br/>the same bytes"| crashes
    smoke -->|"a line for each target,<br/>the summary table,<br/>the exit status"| dev

    classDef person fill:#08427b,stroke:#052e56,color:#ffffff
    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef external fill:#8a8a8a,stroke:#6b6b6b,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    classDef store fill:#1168bd,stroke:#0b4884,color:#ffffff
    class dev person
    class ci,procStatus external
    class smoke,test,engine,mutator,parser,report built
    class corpus,inPlace,crashes store
```

- The targets are `json` and `toml` in `tidepool-codec` and `encoding` in
  `tidepool-text` (`TARGETS` in `xtask/src/fuzz.rs`).
- A file in `fuzz/crashes/TARGET/` is named after its `Kind` and the
  input's FNV-1a hash: `panic-`, `hang-`, `memory-` or `reserve-`, then 16
  hex digits. An unchanged input that fails is named where it is, not
  saved again. `fuzz/crashes/` is made when the first finding is saved.
- When a replay died on an input that a saved crash already holds
  (`Kept::Existing` from `keep_died`), fuzz-smoke says it died on that
  saved crash, which every run replays first: fix what it shows, or
  remove it.
- The memory checks read `/proc/self/status` on Linux only; elsewhere they
  are skipped, and the report says why.

## 6. State: the fuzz engine's run (built)

One run of a target, as `engine::run` and the fuzzing thread go through
it, ending in one of the seven `Stop` reasons that the result line shows
as `stopped=`, or in a `FuzzError` when it cannot start or finish.
Findings do not stop a run, except a hang.

Each input can add a `Finding`, and the run goes on: `Kind::Panic`, one
for each place a panic was raised, counted in `hits`; `Kind::Memory`, the
written peak `VmHWM` past the memory cap; `Kind::Reserved`, the reserved
peak `VmPeak` past its cap, or a `Range` of inputs; `Kind::Hang`, only at
`Stop::Hang` or `Stop::SlowInput`. In a replay each input is written to
`.last-input` before it runs, and the memory is read after every input.

```mermaid
stateDiagram-v2
    state "Reading the crashes<br/>and the seeds" as reading
    state "Could not run<br/>or finish" as cannotRun
    state "Unchanged inputs" as unchanged
    state checkNext <<choice>>
    state "Running mutated<br/>input n" as mutating
    state "Stopped, with one of the seven Stop reasons" as stopped {
        direction LR
        state "Stop Unchanged, unchanged<br/>the duration is zero,<br/>as in a plain cargo test" as stUnchanged
        state "Stop MaxInputs, max-inputs<br/>max_inputs mutated<br/>inputs ran" as stMaxInputs
        state "Stop Time, time<br/>a normal run,<br/>its duration is up" as stTime
        state "Stop TargetTime,<br/>target-time<br/>a replay, its time<br/>in the target is up" as stTargetTime
        state "Stop WallCap, wall-cap<br/>a replay, 20 times<br/>its duration in all" as stWallCap
        state "Stop Hang, hang<br/>the watchdog finds an<br/>input past the hang limit" as stHang
        state "Stop SlowInput,<br/>slow-input<br/>an input returned<br/>after the hang limit" as stSlow
    }
    state "Outcome" as outcome

    [*] --> reading: Target run, or<br/>run_with and a Config
    reading --> cannotRun: FuzzError, such as a link,<br/>a missing or empty seed<br/>directory, a seed over 1 MiB
    cannotRun --> [*]
    reading --> unchanged: engine run starts<br/>the target thread and<br/>the memory Probe
    unchanged --> unchanged: the next input, a replay's<br/>leftover first, then each<br/>crash, then each seed
    unchanged --> cannotRun: FuzzError, a finding<br/>or a replay's .last-input<br/>cannot be written
    unchanged --> stopped: Hang or SlowInput
    unchanged --> checkNext: every unchanged<br/>input ran
    checkNext --> stopped: Unchanged, MaxInputs,<br/>Time, TargetTime<br/>or WallCap
    checkNext --> mutating: time is left, mutate<br/>a seed into input n
    mutating --> checkNext: it returned within<br/>the hang limit
    mutating --> cannotRun: FuzzError, a finding<br/>or a replay's .last-input<br/>cannot be written
    mutating --> stopped: Hang or SlowInput
    stopped --> outcome
    outcome --> [*]

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class reading, cannotRun, unchanged, mutating, stUnchanged, stMaxInputs, stTime, stTargetTime, stWallCap, stHang, stSlow, outcome built
```

- `Target::run` panics when the run cannot start or finish; `run_with`
  returns the `FuzzError`. `Outcome::assert_clean` fails the test when
  there is a finding.
- A hang ends the run, since a thread stuck in a target cannot be stopped.
  While such a thread is stuck, later runs in the process skip the memory
  checks (`Skip` code `hung-thread`).
- A memory check that does not run says why with a `Skip` code:
  `unreadable`, `no-vmhwm`, `no-vmpeak`, `not-asked`, `hung-thread`,
  `read-failed` or `not-started`.

## 7. State: a text `History` (built)

The undo tree of one buffer (ADR-0020), seen from its current revision:
whether a commit can join it, and how undo, redo and jumps move through
the tree.

```mermaid
stateDiagram-v2
    state "At the root, nothing to undo" as atRoot
    state "Current revision open, a commit may join it" as revOpen
    state "Current revision sealed" as revSealed

    [*] --> atRoot: History new, the root marked saved and not open
    atRoot --> revOpen: commit, a new revision under the root
    revOpen --> revOpen: commit, joined, or a new revision under the current one
    revOpen --> revSealed: seal, checkpoint, undo or jump_to
    revOpen --> atRoot: undo or jump_to back to the root
    revSealed --> revOpen: commit, a new revision, after an undo a new branch
    revSealed --> revSealed: undo, redo or jump_to
    revSealed --> atRoot: undo or jump_to back to the root
    atRoot --> revSealed: redo or jump_to

    note right of revOpen
        A commit joins only an open revision.
        Group Command joins one made with the same GroupId.
        Group Coalesce joins one of the same kind, within its window, with the cursors unmoved.
        Group New never joins.
    end note
    note left of atRoot
        A commit that changes nothing records nothing.
        An error such as HistoryMismatch changes neither the rope nor the history.
        Undo at the root and redo without a child return Ok None.
    end note

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    class atRoot, revOpen, revSealed built
```

An example tree: revisions 1 and 2 were committed, 2 was undone, and the
next commit made 3, a branch. Undo goes to the parent; redo goes to the
child created or visited last; `jump_to(2)` goes up to 1, the lowest
common ancestor, then down to 2, and makes 2 the last child.

```mermaid
flowchart LR
    r0(("0<br/>root, saved"))
    r1(("1"))
    r2(("2"))
    r3(("3<br/>current, is_modified"))

    r0 -->|"last child"| r1
    r1 -->|"child, kept"| r2
    r1 -->|"last child, where redo goes"| r3

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    class r0,r1,r2,r3 built
```

- Nothing is pruned in v1, and deleted text stays in memory until the
  history is dropped.
- `is_modified` is false exactly at the saved revision, on any branch.

## 8. State: a saved file (built, and planned where the workspace adds to it)

What `tidepool-text`'s calls report about one file: `Loaded::read_only`,
`History::is_modified`, `checkpoint`, `file::save` and `mark_saved`. No
type holds these states yet; `workspace::buffers` does from M4.3, and adds
the dashed ones. 8.1 draws what the text engine reports, and 8.2 the
states the workspace adds.

### 8.1 What the text engine reports (built)

```mermaid
stateDiagram-v2
    state "Loading" as loading
    state "Load failed" as loadFailed
    state "Read-only<br/>no save Format" as readOnly
    state "Unmodified<br/>is_modified false" as clean
    state "Modified<br/>is_modified true" as dirty
    state "Saving the Checkpoint" as saving
    state "Save failed" as saveFailed

    [*] --> loading: file load, on a<br/>worker from M4.3
    loading --> loadFailed: NotRegularFile, or Io<br/>at Metadata, Open or Read
    loadFailed --> [*]
    loading --> readOnly: bytes were replaced,<br/>so the Loaded format<br/>is None
    loading --> clean: the Loaded has a Format,<br/>History new marks<br/>the root saved
    clean --> dirty: a commit that<br/>changes the text
    dirty --> clean: undo, redo or jump_to<br/>back to the saved revision
    dirty --> saving: checkpoint, then save<br/>of its text in the Format
    saving --> clean: Ok and mark_saved,<br/>no commit since<br/>the checkpoint
    saving --> dirty: Ok and mark_saved,<br/>with commits since<br/>the checkpoint
    saving --> saveFailed: TooManyLinks,<br/>NotRegularFile,<br/>NotWritable, or Io<br/>with its FileStep
    saveFailed --> dirty: nothing changed, and<br/>the target is untouched<br/>before the rename

    note right of readOnly
        Undecodable bytes became U+FFFD and are kept beside the text. Writing them back would change them, so a caller saves only by choosing a Format itself.
    end note

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    class loading, loadFailed, readOnly, clean, dirty, saving, saveFailed built
```

### 8.2 What the workspace adds

```mermaid
stateDiagram-v2
    state "Loading" as loading
    state "Unmodified" as clean
    state "Modified" as dirty
    state "Saving the Checkpoint" as saving
    state "Large-file mode<br/>planned M4.3" as large
    state "Save warning<br/>planned M4.3" as warn
    state "Changed on disk<br/>planned M4.5" as changed
    state "In recovery<br/>planned M4.9" as recovered

    [*] --> loading: file load, as in 8.1
    loading --> clean: a Format, as in 8.1
    clean --> dirty: a commit, as in 8.1
    loading --> large: is_large, more than<br/>16 MiB, planned M4.3
    dirty --> warn: a second hard link,<br/>or another owner or<br/>group, planned M4.3
    warn --> saving: the user goes on, or<br/>the buffer writes in<br/>place, planned M4.3
    clean --> changed: the file changed<br/>on disk, planned M4.5
    dirty --> changed: the file changed<br/>on disk, planned M4.5
    changed --> clean: reload, planned M4.5
    changed --> dirty: merge, clear_saved as<br/>no revision is on disk,<br/>planned M4.5
    dirty --> recovered: the app panics,<br/>planned M4.9
    recovered --> dirty: restored on the next<br/>start, planned M4.9

    note left of large
        Read-only by default, with no highlighting or LSP. The text engine only reports is_large. The mode is the editor's.
    end note

    classDef built fill:#1168bd,stroke:#0b4884,color:#ffffff
    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class loading, clean, dirty, saving built
    class large, warn, changed, recovered planned
```

- Saves of one buffer are serialized, so the last `mark_saved` matches the
  disk (`History::checkpoint`).
- An `Io` error at `FileStep::SyncDirectory` comes after the rename: the
  new text is in place, but the save reports an error.

## 9. State: workspace trust (planned)

A folder opens untrusted until the user trusts it, and the decision is
stored in user data (ADR-0014, which M4.7 accepts first). The prompt is
the editor's trust prompt (M4.3).

```mermaid
stateDiagram-v2
    state "Untrusted, planned M4.7" as untrusted
    state "Trusted, planned M4.7" as trusted

    [*] --> untrusted: a folder opens that user data does not record as trusted, the trust prompt asks
    [*] --> trusted: a folder opens that user data records as trusted
    untrusted --> trusted: the user trusts it in the prompt, the decision stored in user data

    note right of untrusted
        Planned M4.7 and M4.3. While untrusted
        no tasks run, planned M6.4,
        rust-analyzer runs without build scripts or proc macros, planned M6.7,
        no Gradle runs, planned M10.4,
        agents get only the focused buffer, with consent, planned M9.3,
        managed tools get detection only, planned M5.1.
    end note
    note right of trusted
        The spec does not say how trust is taken back.
    end note

    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class untrusted, trusted planned
```

## 10. State: vim modes (planned)

The modes `docs/subsystems/vim-mode.md` names, and the keys of its
supported subset that move between them. The keymap engine is modal from
M4.6; the default keymap is one Insert-like mode, and vim mode adds the
rest (ADR-0006). 10.1 draws Normal mode and the modes a key or an operator
enters from it, and 10.2 the visual modes.

### 10.1 Normal, Insert, Replace, operators, commands and search

```mermaid
stateDiagram-v2
    state "Normal<br/>planned M7.2" as normal
    state "Insert<br/>planned M7.2" as insert
    state "Replace<br/>planned M7.2" as replace
    state "OperatorPending<br/>planned M7.2" as opPending
    state "CommandLine<br/>planned M7.4" as cmdLine
    state "Search<br/>planned M7.4" as search

    [*] --> normal: editor.keymap<br/>set to vim
    normal --> insert: i a I A o O,<br/>s S or C
    insert --> normal: Esc
    normal --> replace: R
    replace --> normal: Esc
    normal --> opPending: an operator,<br/>such as d c y
    opPending --> normal: a motion or text<br/>object completes it,<br/>or Esc cancels
    opPending --> insert: c with its motion<br/>or text object
    normal --> cmdLine: the colon key
    cmdLine --> normal: Enter runs the<br/>ex command,<br/>Esc cancels
    normal --> search: the slash or<br/>question mark key
    search --> normal: Enter searches,<br/>Esc cancels

    note right of normal
        Planned M7.2, M7.3 and M7.4, on the engine of M4.6. Before an operator or action come an optional register and count. A parser state machine reads the keys and emits a whole command, or waits. Each Normal command and each Insert session is one undo step.
    end note

    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class normal, insert, replace, opPending, cmdLine, search planned
```

### 10.2 The visual modes

```mermaid
stateDiagram-v2
    state "Normal<br/>planned M7.2" as normal
    state "Visual<br/>planned M7.3" as visual
    state "VisualLine<br/>planned M7.3" as visualLine
    state "VisualBlock<br/>planned M7.3" as visualBlock
    state "Insert<br/>planned M7.2" as insert

    normal --> visual: v
    normal --> visualLine: V
    normal --> visualBlock: Ctrl-v
    visual --> normal: Esc, or an operator<br/>other than c on<br/>the selection
    visualLine --> normal: Esc, or an operator<br/>other than c on<br/>the selection
    visualBlock --> normal: Esc, or an operator<br/>other than c on<br/>the block
    visual --> insert: c on the selection
    visualLine --> insert: c on the selection
    visualBlock --> insert: c on the block,<br/>or block insert<br/>with I or A

    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class normal, visual, visualLine, visualBlock, insert planned
```

- The status bar shows the mode as a chip (`NORMAL`, `INSERT`,
  `VISUAL`…); the cursor is a block in Normal mode and a bar in Insert.
- In Insert mode `Ctrl-o {cmd}` runs one Normal command; completion and
  ghost text show only in Insert mode.
- A key outside the subset is a no-op with a status-bar message, never a
  silent wrong edit.

## 11. State: a managed tool (planned)

The live states of one tool in `toolchain::registry`, under the names the
spec gives: `Missing`, `Found { path, version }`, `Outdated`, `Installing`
and `Broken { reason }`. Detecting is the run of `toolchain::detect`, not
a registry state.

```mermaid
stateDiagram-v2
    state "Detecting, planned M5.1" as detecting
    state "Missing, planned M5.1" as missing
    state "Found, with path and version, planned M5.1" as found
    state "Outdated, planned M5.1" as outdated
    state "Broken, with a reason, planned M5.1" as broken
    state "Installing, planned M5.3" as installing

    [*] --> detecting: start, planned M5.1
    detecting --> missing: not in settings, env vars, standard locations or PATH
    detecting --> found: the version meets the descriptor
    detecting --> outdated: the version is below the descriptor's rules
    detecting --> broken: found, but not usable
    missing --> installing: the user confirms the install plan, planned M5.3
    outdated --> installing: the user confirms the update plan, planned M5.3
    installing --> detecting: the plan ended, detect again, planned M5.5
    missing --> detecting: Locate, planned M5.6
    broken --> detecting: Locate or Configure, planned M5.6

    note right of installing
        A download that fails its SHA-256 is deleted and reported, and nothing from it runs.
        The spec names no state for a failed install.
        Commands that need admin run in the integrated terminal when the user presses Run.
    end note
    note left of detecting
        Planned M5.1, M5.3, M5.5 and M5.6.
        In an untrusted workspace, detection only.
    end note

    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class detecting, missing, found, outdated, broken, installing planned
```

- The registry also records which features each tool unlocks; the
  Toolchains panel shows each tool's state as a status dot (M5.6).

## 12. State: a language session (planned)

One LSP session per server and workspace root, started through
`toolchain::sessions` and restarted after a crash (`docs/subsystems/languages.md`
→ `tidepool-lang`). The spec names no states; these are its steps.

```mermaid
stateDiagram-v2
    state "Starting, planned M6.7" as starting
    state "Initializing, planned M6.7" as initializing
    state "Ready, documents in sync, planned M6.7" as ready
    state "Requests in flight, planned M6.7" as busy
    state "Crashed, planned M6.7" as crashed
    state "Shutting down, planned M6.7" as shuttingDown
    state "Stopped, planned M6.7" as stopped

    [*] --> starting: the server from the registry, spawned through toolchain sessions
    starting --> initializing: initialize sent, positionEncoding utf-8 preferred, utf-16 by default
    initializing --> ready: its result, then initialized
    ready --> ready: didOpen, incremental didChange, didClose
    ready --> busy: a request that carries the buffer version
    busy --> ready: the responses, a cancelRequest or a timeout, stale results dropped
    initializing --> crashed: the server exits
    ready --> crashed: the server exits
    busy --> crashed: the server exits
    crashed --> starting: restart with backoff
    ready --> shuttingDown: Tidepool shuts down, shutdown then exit
    busy --> shuttingDown: Tidepool shuts down, shutdown then exit
    shuttingDown --> stopped: it exits, or is killed after 2 s
    stopped --> [*]

    note right of starting
        Planned M6.7, on M5.2 for toolchain sessions and M6.1 for the LSP client.
        In an untrusted workspace rust-analyzer gets no build scripts or proc macros.
        The restart policy and backoff are toolchain sessions'. The spec gives no limit.
    end note
    note left of busy
        Requests from M6.7, M6.8 and M6.3.
        Positions are converted through tidepool-text.
    end note

    classDef planned fill:#ffffff,stroke:#888888,stroke-dasharray: 5 5,color:#555555
    class starting, initializing, ready, busy, crashed, shuttingDown, stopped planned
```

- `rust-toolchain.toml` in the workspace is respected (M6.7).
- The main thread never waits on the server: results come back as
  versioned messages, and one for an older buffer version is dropped
  (`docs/01-architecture.md` → "The main loop").

## Sources

The docs and code these diagrams were drawn from:

- `docs/01-architecture.md`: "Dependency matrix", "The main loop", "Data
  flow", "Threading model", "Configuration and user data"
- `docs/subsystems/text-and-formats.md`: `history`, `encoding`, `file`
- `docs/subsystems/editor.md`: the `tidepool-workspace` modules, the
  editor view, "Dialogs that protect the user"
- `docs/subsystems/toolchains.md`: `tidepool-toolchain`
- `docs/subsystems/agents.md`
- `docs/subsystems/vim-mode.md`
- `docs/subsystems/languages.md`: `tidepool-rpc`, `tidepool-lang`
- `docs/04-quality.md`: "Fuzz smoke"
- `docs/roadmap.md`: M0.2, M4.2, M4.3, M4.5, M4.6, M4.7, M4.9, M5.1 to
  M5.6, M6.1, M6.3, M6.4, M6.7, M6.8, M7.2 to M7.4, M9.3 to M9.5, M10.4
- `docs/decisions.md`: ADR-0002, ADR-0006, ADR-0009, ADR-0014, ADR-0016,
  ADR-0019, ADR-0020, ADR-0025, ADR-0027, ADR-0029
- `crates/text/src/file.rs`, `encoding.rs`, `history.rs`,
  `transaction.rs`, `rope.rs`, `selection.rs`, `error.rs`, `lib.rs`
- `crates/core/src/testing/fuzz.rs`, `testing/fuzz/engine.rs`,
  `testing/fuzz/mutate.rs`, `testing/fuzz/paths.rs`,
  `testing/fuzz/memory.rs`, `rng.rs`, `hash/sha256.rs`
- `crates/codec/tests/fuzz.rs`, `crates/text/tests/fuzz.rs`
- `xtask/src/fuzz.rs`
- `.github/workflows/ci.yml`: the fuzz-smoke step
