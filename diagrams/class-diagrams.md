# UML class diagrams

These diagrams show the types the built crates have: `tidepool-core`,
`tidepool-codec`, `tidepool-text` and `xtask`. Each shows what a type owns,
what it refers to, which traits it implements, and the few fields and
methods the design rests on, under their names in the code. An overview of
how the crates' types connect comes first, then one or more diagrams per
area, then a list in prose of the types the specs name for crates that are
still empty.

Drawn from the code and docs as of roadmap task M1.8 (2026-11-09, version 0.1). What the code has is drawn solid; what the roadmap plans is dashed and labelled with the task that builds it (ADR-0029).

## Legend

A class diagram here shows only types that exist in the code (ADR-0029).
None of its boxes or lines is planned work. The dashed lines below are
UML's own: realization and dependency. Planned types are listed in prose in
section 23, and drawn in the other files of `diagrams/` under the names
their specs give.

| Notation | Means |
| :- | :- |
| `A <\|.. B`, a dashed line with a hollow triangle | B implements the trait A |
| `A *-- B`, a filled diamond | A owns B: as a field, a `Vec`, an `Option`, a `Box` or an enum variant's value |
| `A o-- B`, a hollow diamond | A holds B without being its only owner: an `Arc`, a borrow or a `static` slice |
| `A --> B`, a solid arrow | A refers to B by an id or an index |
| `A ..> B`, a dashed arrow | A uses B in a method, or makes or returns one |
| `"1"`, `"0..*"`, `"1..*"` at a line's ends | How many of each side there are |
| `+` | Public. In `xtask`, a binary, `+` marks `pub(crate)`: what its other modules use |
| `-` | Private, `pub(crate)` or `pub(super)` (outside `xtask`) |
| `<<enumeration>>` | An enum; the box lists its variants, and some of its methods |
| `<<trait>>` | A trait |
| `<<alias>>` | A type alias |
| `<<module>>` | A module's free functions and constants, in one box |
| A name such as `core::cli::Args` | A type from another crate or module, shown for context |

Generic parameters show as `Vec~T~`, which GitHub draws as `Vec<T>`.
References, lifetimes and the error type of a `Result` are left out; a
bare `Result` returns nothing on success. A fixed-size array shows as, for
example, `32 bytes`. Each box shows only some of its fields and methods.

## 1. Overview: types across crates

`tidepool-codec` and `tidepool-text` use nothing from `tidepool-core` in
their libraries; their `tests/fuzz.rs` use `core::testing::fuzz` as a
dev-dependency, and make one `core::testing::fuzz::Target` per fuzz target.
`xtask` uses `core` and `codec`, as the dependency matrix allows: it reads
`cargo metadata` with `codec::json`, manifests with `codec::toml`, and the
fuzz targets' result lines with `core::testing::fuzz::Summary`.

### 1.1 core and codec, and what xtask uses of them

```mermaid
classDiagram
    direction LR
    class CoreError["core::CoreError"]
    class Args["core::cli::Args"]
    class Pcg32["core::Pcg32"]
    class Sha256["core::hash::Sha256"]
    class FuzzTarget["core::testing::fuzz::Target"]
    class FuzzOutcome["core::testing::fuzz::Outcome"]
    class FuzzSummary["core::testing::fuzz::Summary"]
    class Limits["codec::Limits"]
    class CodecError["codec::CodecError"]
    class JsonValue["codec::json::Value"]
    class TomlTable["codec::toml::Table"]
    class Check["xtask::check::Check"]
    class Report["xtask::check::Report"]
    class Metadata["xtask::metadata::Metadata"]
    class SmokeTarget["xtask::fuzz::Target"]
    class XtaskError["xtask::error::Error"]
    Args ..> CoreError : fails with
    FuzzTarget ..> Pcg32 : mutates inputs with
    FuzzTarget ..> FuzzOutcome : run returns
    FuzzOutcome ..> FuzzSummary : summary
    JsonValue ..> Limits : parse takes
    TomlTable ..> Limits : parse takes
    JsonValue ..> CodecError : parse fails with
    TomlTable ..> CodecError : parse fails with
    Check ..> Report : a step returns
    Check ..> XtaskError : a step fails with
    Check ..> Sha256 : ucd checks<br/>the data with
    Check ..> TomlTable : crates reads<br/>manifests into
    Metadata ..> JsonValue : read from
    SmokeTarget ..> FuzzSummary : parses the<br/>result line into
    XtaskError ..> CoreError : Usage holds<br/>its text
```

### 1.2 text

The main types of `tidepool-text`, whose library uses no other crate's
types:

```mermaid
classDiagram
    direction TB
    class Rope["text::Rope"]
    class Selection["text::Selection"]
    class ChangeSet["text::ChangeSet"]
    class Transaction["text::Transaction"]
    class History["text::History"]
    class Decoded["text::Decoded"]
    class Loaded["text::Loaded"]
    class TextError["text::TextError"]
    Transaction *-- ChangeSet
    Transaction *-- Selection
    ChangeSet ..> Rope : applies to
    Selection ..> ChangeSet : maps through
    History *-- Rope : text
    History ..> Transaction : commit takes
    Loaded *-- Decoded
    Decoded *-- Rope
    Rope ..> TextError : fails with
```

## 2. core: errors, the command line, cancellation and paths

`CoreError` shows the pattern every crate follows: one error enum that
implements `Display` and `std::error::Error`. `ErrorContext` adds
`context` and `with_context` to every `Result`, wrapping the error in a
`WithContext`; `Args` takes a command line apart piece by piece, and an
error about an argument carries its 1-based position (too many arguments
carries the limit).

```mermaid
classDiagram
    direction LR
    class CoreError {
        <<enumeration>>
        InvalidColor
        MissingOptionValue
        InvalidOptionValue
        UnexpectedArgument
        TooManyArguments
        ArgumentTooLong
    }
    class ErrorContext {
        <<trait>>
        +context(what) Result~WithContext~
        +with_context(what) Result~WithContext~
    }
    class WithContext~E~ {
        -what: Cow~str~
        -source: E
        +new(what, source) WithContext
        +what() str
        +inner() E
        +into_inner() E
    }
    class StdResult["std::result::Result"]
    class Display["std::fmt::Display"] {
        <<trait>>
    }
    class StdError["std::error::Error"] {
        <<trait>>
    }
    class Args {
        -items: Vec~Arg~
        -trailing: Vec~Arg~
        +new(args) Result~Args~
        +with_limits(args, limits) Result~Args~
        +from_env() Result~Args~
        +subcommand() Option~String~
        +flag(name) bool
        +value(name) Result~Option~String~~
        +parsed(name) Result~Option~T~~
        +finish() Result~Vec~String~~
    }
    class Arg {
        -position: usize
        -text: String
    }
    class CliLimits["cli::Limits"] {
        +max_args: usize
        +max_arg_len: usize
        +DEFAULT: Limits
    }
    class CancelToken {
        -0: Arc~AtomicBool~
        +new() CancelToken
        +cancel()
        +is_cancelled() bool
    }
    class Deadline {
        -at: Option~Instant~
        +NEVER: Deadline
        +after(timeout) Deadline
        +at(instant) Deadline
        +instant() Option~Instant~
        +remaining() Duration
        +expired() bool
    }
    ErrorContext <|.. StdResult
    ErrorContext ..> WithContext : makes
    Display <|.. CoreError
    StdError <|.. CoreError
    Display <|.. WithContext
    StdError <|.. WithContext
    Args "1" *-- "0..*" Arg
    Args ..> CliLimits : with_limits takes
    Args ..> CoreError : fails with
    note for CancelToken "Clones share one flag, across threads"
    note for Deadline "None means never, and so does<br/>a deadline too far away to represent"
```

`core::path` (M1.8) shows a path to people and logs without hiding a byte
or letting a name drive a terminal (ADR-0032, ADR-0033), gives a path as
text, and canonicalizes it. `resolve` does so for a path whose end does not
exist yet, and `under` checks containment component by component. Its
error keeps the `std::io::Error` that caused it.

```mermaid
classDiagram
    direction LR
    class PathFns["core::path"] {
        <<module>>
        +display(path) PathDisplay
        +to_utf8(path) Result~str~
        +canonicalize(path) Result~PathBuf~
        +resolve(path) Result~PathBuf~
        +under(path, root) Option~Path~
    }
    class PathDisplay {
        -bytes: u8 slice
    }
    class PathError {
        <<enumeration>>
        Empty
        NotUtf8
        Io
        TooManyLinks
        +kind() ErrorKind
    }
    class Display["std::fmt::Display"] {
        <<trait>>
    }
    class StdError["std::error::Error"] {
        <<trait>>
    }
    class IoError["std::io::Error"]
    PathFns ..> PathDisplay : display returns
    PathFns ..> PathError : fails with
    Display <|.. PathDisplay
    Display <|.. PathError
    StdError <|.. PathError
    PathError *-- IoError : Io
    note for PathDisplay "Borrows the path's bytes<br/>and writes without allocating"
```

## 3. core: random numbers, hashes and time

`Pcg32` is the one random number generator; it is not `Copy`, so a
generator cannot be copied by accident and replay its numbers. `hash` has
FNV-1a (stable, for files on disk), CRC-32, SHA-256 for downloads, and a
fast hasher for in-memory maps that is not HashDoS-resistant.

```mermaid
classDiagram
    direction LR
    class Pcg32 {
        -state: u64
        -increment: u64
        +new(seed, stream) Pcg32
        +next_u32() u32
        +next_f32() f32
        +range(range) Output
        +shuffle(slice)
        +choose(slice) Option~T~
    }
    class SampleRange {
        <<trait>>
        +type Output
        +sample(rng) Output
    }
    class Sha256 {
        -state: 8 u32 words
        -block: 64 bytes
        -filled: usize
        -bit_length: u64
        +new() Sha256
        +update(data)
        +finalize() 32 bytes
    }
    class HashFns["hash"] {
        <<module>>
        +SHA256_LEN: usize
        +fnv1a64(bytes) u64
        +crc32(bytes) u32
        +sha256(data) 32 bytes
        +to_hex(bytes) String
    }
    class FastHasher {
        -hash: u64
    }
    class Hasher["std::hash::Hasher"] {
        <<trait>>
    }
    class FastBuildHasher {
        <<alias>>
    }
    class FastHashMap {
        <<alias>>
    }
    class FastHashSet {
        <<alias>>
    }
    class DateTime {
        +year: i64
        +month: u8
        +day: u8
        +hour: u8
        +minute: u8
        +second: u8
        +millisecond: u16
        +now() DateTime
        +from_system_time(time) DateTime
        +from_unix_seconds(seconds) DateTime
        +file_stamp() String
    }
    class Display["std::fmt::Display"] {
        <<trait>>
    }
    Pcg32 ..> SampleRange : range takes
    HashFns ..> Sha256 : sha256 uses
    Hasher <|.. FastHasher
    FastBuildHasher ..> FastHasher : builds
    FastHashMap ..> FastBuildHasher
    FastHashSet ..> FastBuildHasher
    Display <|.. DateTime
    note for SampleRange "Implemented for Range<br/>of u32, i32 and f32"
    note for DateTime "Display is RFC 3339 in UTC,<br/>with milliseconds"
```

## 4. core: geometry and color

Logical units are `f32` and device pixels `i32`; `Scale` converts between
them through the `ToDevice` and `ToLogical` traits. `Color` is straight
alpha and `PremultipliedColor` premultiplied, which is what blending uses.

### 4.1 Geometry

```mermaid
classDiagram
    direction TB
    class Point {
        +x: f32
        +y: f32
        +ZERO: Point
        +new(x, y) Point
    }
    class Size {
        +width: f32
        +height: f32
        +ZERO: Size
        +new(width, height) Size
    }
    class Rect {
        +x: f32
        +y: f32
        +width: f32
        +height: f32
        +from_origin_size(origin, size) Rect
        +origin() Point
        +size() Size
        +right() f32
        +bottom() f32
        +is_empty() bool
        +contains(point) bool
    }
    class DevicePoint {
        +x: i32
        +y: i32
    }
    class DeviceSize {
        +width: i32
        +height: i32
    }
    class DeviceRect {
        +x: i32
        +y: i32
        +width: i32
        +height: i32
        +right() i32
        +bottom() i32
        +is_empty() bool
        +contains(point) bool
    }
    class Scale {
        -0: f32
        +ONE: Scale
        +new(factor) Option~Scale~
        +factor() f32
        +to_device(value) Device
        +to_logical(value) Logical
    }
    class ToDevice {
        <<trait>>
        +type Device
        +to_device(scale) Device
    }
    class ToLogical {
        <<trait>>
        +type Logical
        +to_logical(scale) Logical
    }
    ToDevice <|.. Point
    ToDevice <|.. Size
    ToDevice <|.. Rect
    ToLogical <|.. DevicePoint
    ToLogical <|.. DeviceSize
    ToLogical <|.. DeviceRect
    Scale ..> ToDevice : to_device
    Scale ..> ToLogical : to_logical
    Rect ..> Point : origin
    Rect ..> Size : size
    DeviceRect ..> DevicePoint : contains
    note for Scale "Points round to nearest,<br/>sizes up, rects outward"
```

### 4.2 Color

```mermaid
classDiagram
    direction LR
    class Color {
        +r: u8
        +g: u8
        +b: u8
        +a: u8
        +WHITE: Color
        +BLACK: Color
        +TRANSPARENT: Color
        +rgb(r, g, b) Color
        +rgba(r, g, b, a) Color
        +parse(text) Result~Color~
        +premultiply() PremultipliedColor
    }
    class PremultipliedColor {
        +r: u8
        +g: u8
        +b: u8
        +a: u8
        +TRANSPARENT: PremultipliedColor
        +unpremultiply() Color
        +over(dst) PremultipliedColor
    }
    class CoreError["core::CoreError"]
    Color ..> PremultipliedColor : premultiply
    Color ..> CoreError : parse fails with InvalidColor
```

## 5. core: input

`InputEvent` is the one input type every frontend will produce. `Key`
names keys by their USB HID usage ids, and `Modifiers` is a small bit set.

```mermaid
classDiagram
    direction LR
    class InputEvent {
        <<enumeration>>
        KeyDown
        KeyUp
        Text
        MouseMove
        MouseDown
        MouseUp
        Wheel
        FocusGained
        FocusLost
        +modifiers() Modifiers
    }
    class Key {
        <<enumeration>>
        +ALL: Key slice
        +usage() u8
    }
    class MouseButton {
        <<enumeration>>
        Left
        Middle
        Right
        Back
        Forward
    }
    class Modifiers {
        -0: u8
        +NONE: Modifiers
        +SHIFT: Modifiers
        +CTRL: Modifiers
        +ALT: Modifiers
        +SUPER: Modifiers
        +bits() u8
        +from_bits(bits) Option~Modifiers~
        +contains(other) bool
        +union(other) Modifiers
        +insert(other)
        +remove(other)
        +set(other, held)
    }
    class ScrollDelta {
        <<enumeration>>
        Lines
        Logical
    }
    class Point["geom::Point"]
    InputEvent *-- Key : KeyDown, KeyUp
    InputEvent *-- Modifiers
    InputEvent *-- MouseButton : MouseDown, MouseUp
    InputEvent *-- ScrollDelta : Wheel
    InputEvent *-- Point : position
    note for Key "One variant per key, valued by<br/>its USB HID usage id on the<br/>keyboard page and named<br/>after the US layout"
```

## 6. core: logging

The `log` functions and macros (`error`, `warn`, `info`, `debug`, `trace`)
filter a record, render it with every registered secret redacted, keep it
in the ring buffer, and queue it for the `tidepool-log` thread, which
writes stderr and the rotating file. The `Logger` lives in the static
`LOGGER`, and the `Queue` in the static `SHARED`; a full queue drops lines
and counts them, and never makes a caller wait.

### 6.1 The log functions and the Logger

```mermaid
classDiagram
    direction TB
    class LogFns["log"] {
        <<module>>
        +RING_CAPACITY: usize
        +QUEUE_CAPACITY: usize
        +REDACTED: str
        +set_level(level)
        +set_target_level(target, level)
        +enabled(level, target) bool
        +set_stderr(enabled)
        +open_file(path, config) io Result
        +close_file()
        +tail(max_lines) Vec~String~
        +register_secret(secret)
        +flush() bool
        +flush_timeout(timeout) bool
        +dropped_lines() u64
        +withheld_lines() u64
    }
    class Level {
        <<enumeration>>
        Error
        Warn
        Info
        Debug
        Trace
        +as_str() str
    }
    class FileConfig {
        +max_bytes: u64
        +keep: u32
        +rotate_on_open: bool
    }
    class Logger {
        -filter: Filter
        -stderr: bool
        -capture: bool
        -file_open: bool
        -ring: Ring
        -secrets: Option~Arc~Secrets~~
        -withheld: u64
    }
    class Filter {
        -default: Level
        -targets: Vec of target and Level pairs
    }
    class Ring {
        -lines: VecDeque~RingLine~
    }
    class RingLine {
        -text: String
        -cut: Option~Cut~
    }
    class Cut {
        -kept: usize
        -removed: usize
    }
    class Secrets {
        -patterns: Vec~String~
        -groups: Vec~Group~
        -redact(text) Redaction
    }
    class Redaction {
        <<enumeration>>
        Clean
        Redacted
        Withheld
    }
    class Queue
    LogFns ..> Level
    LogFns ..> FileConfig : open_file takes
    LogFns ..> Logger : locks LOGGER
    LogFns ..> Queue : sends lines to
    Logger *-- Filter
    Logger *-- Ring
    Logger o-- Secrets : Arc
    Filter *-- Level
    Ring "1" *-- "0..*" RingLine
    RingLine *-- Cut
    Secrets ..> Redaction : redact returns
    note for Secrets "Its Debug never shows a value.<br/>A line it cannot make safe<br/>is withheld from every sink"
```

### 6.2 The queue and the log thread

```mermaid
classDiagram
    direction TB
    class Queue {
        -items: VecDeque~Item~
        -lines: usize
        -bytes: usize
        -dropped: u64
        -held: bool
        -thread: ThreadState
    }
    class Item {
        <<enumeration>>
        Line
        File
        Flush
    }
    class Line {
        -text: String
        -stderr: Stderr
    }
    class Stderr {
        <<enumeration>>
        Off
        Real
        Capture
    }
    class RotatingFile {
        -path: PathBuf
        -config: FileConfig
        -file: File
        -size: u64
        -rotate_at: u64
    }
    class ThreadState {
        <<enumeration>>
        NotStarted
        Running
        Failed
    }
    class FileConfig
    Queue "1" *-- "0..*" Item
    Queue *-- ThreadState
    Item *-- Line : Line
    Item *-- RotatingFile : File
    Line *-- Stderr
    RotatingFile *-- FileConfig
```

## 7. core: property tests

`check` and `check_sized` run a property on generated cases, from the seed
in `TIDEPOOL_SEED` or the default one. A failure report names the case's
input and ends with the command that reproduces it.

```mermaid
classDiagram
    direction LR
    class TestingFns["testing"] {
        <<module>>
        +SEED_VAR: str
        +DEFAULT_SEED: u64
        +MAX_SIZE: u32
        +check(name, cases, generate, property)
        +check_sized(name, cases, generate, property)
    }
    class Approx {
        <<trait>>
    }
    class F32["f32"]
    class F64["f64"]
    class Pcg32["core::Pcg32"]
    class Run {
        -name: str
        -seed: u64
        -cases: u32
        -reproduce: Reproduce
    }
    class Failure {
        -name: String
        -seed: u64
        -case: u32
        -cases: u32
        -sizes: Option~Sizes~
        -input: String
        -message: String
        -reproduce: Reproduce
    }
    class Sizes {
        -failed_at: u32
        -shrunk_to: u32
    }
    class Reproduce {
        -package: String
        -filter: String
    }
    TestingFns ..> Pcg32 : generate draws from
    TestingFns ..> Run
    Run *-- Reproduce
    Run ..> Failure : reports
    Failure *-- Sizes
    Failure *-- Reproduce
    Approx <|.. F32
    Approx <|.. F64
    note for Approx "The assert_approx macro compares<br/>two values of a type that implements it"
```

## 8. core: the fuzz engine's API

`testing::fuzz` is the engine behind each parser crate's `tests/fuzz.rs`
(ADR-0027). A `Target` names the function to fuzz and where its seeds and
saved crashes are; `run` replays them on every `cargo test`, and mutates
them when `cargo xtask fuzz-smoke` sets `TIDEPOOL_FUZZ_SECS`. The
`Outcome` it returns prints a report, and its `Summary` is the
`fuzz-result:` line that `fuzz-smoke` reads back.

### 8.1 Targets, runs and findings

```mermaid
classDiagram
    direction TB
    class Target {
        -name: String
        -run: fn of bytes
        -root: PathBuf
        -extra: Vec~SeedDir~
        -memory_cap: u64
        -reserved_cap: u64
        +new(root, name, run) Target
        +seeds_in(dir, extensions) Target
        +dictionary(tokens) Target
        +memory_cap(bytes) Target
        +reserved_cap(bytes) Target
        +corpus() PathBuf
        +crashes() PathBuf
        +run() Outcome
        +run_with(config) Result~Outcome~
    }
    class SeedDir {
        -dir: PathBuf
        -extensions: Vec~String~
    }
    class Config {
        +seed: u64
        +duration: Duration
        +max_inputs: u64
        +hang_limit: Duration
        +max_len: usize
        +replay: bool
        +check_reserved: bool
        +from_env() Result~Config~
    }
    class Outcome {
        +target: String
        +config: Config
        +inputs: u64
        +findings: Vec~Finding~
        +stopped: Stop
        +slowest: Option~Slowest~
        +memory: Memory
        +reserved: Memory
        +reproduce: String
        +is_clean() bool
        +hung() bool
        +summary() Summary
        +assert_clean()
    }
    class Finding {
        +kind: Kind
        +site: Option~String~
        +earlier: Option~String~
        +message: String
        +input: Found
        +hits: u64
        +path() Option~Path~
        +site_in_std() bool
    }
    class Kind {
        <<enumeration>>
        Panic
        Hang
        Memory
        Reserved
        +file_prefix() str
    }
    class Found {
        <<enumeration>>
        File
        Range
    }
    class Slowest {
        +took: Duration
        +len: usize
    }
    class Stop {
        <<enumeration>>
        Unchanged
        Time
        TargetTime
        WallCap
        MaxInputs
        Hang
        SlowInput
        +code() str
        +describe(code) Option~str~
    }
    class FuzzError {
        <<enumeration>>
        Var
        Read
        Link
        NotAFile
        NotUnderRoot
        TooLarge
        NoSeeds
        Write
        Thread
    }
    Target "1" *-- "0..*" SeedDir : extra
    Target ..> Config : run_with takes
    Target ..> Outcome : run returns
    Target ..> FuzzError : run_with fails with
    Outcome *-- Config
    Outcome "1" *-- "0..*" Finding
    Outcome *-- Stop
    Outcome *-- Slowest
    Finding *-- Kind
    Finding *-- Found
```

### 8.2 Memory, the result line and kept inputs

What an `Outcome` measured of memory, its `Summary`, written as the
`fuzz-result:` line, and `Kept`:

```mermaid
classDiagram
    direction TB
    class Outcome
    class Memory {
        <<enumeration>>
        Measured
        Skipped
    }
    class Skip {
        +code: str
        +reason: String
    }
    class Summary {
        +target: String
        +seed: u64
        +inputs: u64
        +findings: usize
        +hung: bool
        +stopped: String
        +memory: Reading
        +reserved: Reading
        +parse(line) Option~Summary~
    }
    class Reading {
        <<enumeration>>
        Measured
        Skipped
    }
    class PeakMemory {
        +baseline: u64
        +peak: u64
        +cap: u64
        +stopped: Option~String~
    }
    class Kept {
        <<enumeration>>
        New
        Existing
        +path() Path
    }
    Outcome *-- Memory : memory, reserved
    Memory *-- Skip
    Outcome ..> Summary : summary
    Summary *-- Reading : memory, reserved
    Reading *-- PeakMemory : Measured
    note for Kept "keep_died returns one: what<br/>fuzz-smoke keeps of an input<br/>on which a replay died"
```

## 9. core: inside the fuzz engine

`Target::run_with` reads the saved crashes and the seeds into a `Plan`,
and `engine::run` runs it. The target runs on a thread of its own, and the
calling thread is the watchdog: the two share a `Shared`, so a hang is
noticed and its input saved. Saved crashes and seeds run unchanged; only
seeds are mutated, each mutated input from its own random stream.

```mermaid
classDiagram
    direction LR
    class Target["fuzz::Target"]
    class Outcome["fuzz::Outcome"]
    class Finding["fuzz::Finding"]
    class Pcg32["core::Pcg32"]
    class Plan {
        -name: String
        -run: fn of bytes
        -root: PathBuf
        -crashes: Vec of path and bytes pairs
        -seeds: Vec of path and bytes pairs
        -leftover: Option~Vec~u8~~
        -memory_cap: u64
        -reserved_cap: u64
        -config: Config
    }
    class Ran {
        -inputs: u64
        -elapsed: Duration
        -findings: Vec~Finding~
        -stopped: Stop
        -memory: Memory
        -reserved: Memory
        -saved: usize
    }
    class Shared {
        -input: Vec~u8~
        -started: Option~Instant~
        -hang_claimed: bool
        -inputs: u64
        -findings: Vec~Finding~
        -stopped: Option~Stop~
    }
    class EngineFns["fuzz::engine"] {
        <<module>>
        -run(plan) Result~Ran~
        -time_is_up(config, elapsed, in_target) Option~Stop~
    }
    class MutateFns["fuzz::mutate"] {
        <<module>>
        -mutate(rng, input, corpus, dictionary, max_len)
    }
    class Mutation {
        <<enumeration>>
        FlipBit
        SetByte
        Insert
        Delete
        Duplicate
        Splice
        Interesting
        Arithmetic
        Token
        -apply(rng, input, corpus, dictionary)
    }
    Target ..> Plan : run_with builds
    EngineFns ..> Plan : run takes
    EngineFns ..> Ran : run returns
    EngineFns ..> Shared : fuzzing thread and watchdog share
    EngineFns ..> MutateFns : mutates seeds with
    MutateFns ..> Mutation : applies 1 to 16
    MutateFns ..> Pcg32 : draws from
    Ran "1" *-- "0..*" Finding
    Shared "1" *-- "0..*" Finding
    Target ..> Outcome : run_with turns Ran into
```

## 10. codec: limits, errors and JSON

Every parser takes a `Limits` and returns a `CodecError` with an offset, a
line and a column; neither parser panics on input. `json::parse` reads
strict JSON or, with `Dialect::Jsonc`, comments and one trailing comma. An
object keeps its members in document order, duplicates included.

```mermaid
classDiagram
    direction TB
    class Limits {
        +max_input_bytes: usize
        +max_depth: usize
        +max_string_bytes: usize
        +max_number_bytes: usize
        +max_entries: usize
        +DEFAULT: Limits
    }
    class CodecError {
        <<enumeration>>
        Json
        Toml
        TomlEdit
    }
    class JsonFns["json"] {
        <<module>>
        +parse(text, dialect, limits) Result~Value~
        +parse_bytes(bytes, dialect, limits) Result~Value~
        +to_string_compact(value) String
        +to_string_pretty(value) String
    }
    class Dialect {
        <<enumeration>>
        Json
        Jsonc
    }
    class Value {
        <<enumeration>>
        Null
        Bool
        Number
        String
        Array
        Object
        +is_null() bool
        +as_bool() Option~bool~
        +as_number() Option~Number~
        +as_i64() Option~i64~
        +as_f64() Option~f64~
        +as_str() Option~str~
        +as_array() Option of Value slice
        +as_object() Option of String and Value pairs
        +get(key) Option~Value~
    }
    class Number {
        -0: Repr
        +from_f64(value) Option~Number~
        +is_integer() bool
        +as_i64() Option~i64~
        +as_u64() Option~u64~
        +as_f64() f64
    }
    class Repr {
        <<enumeration>>
        PosInt
        NegInt
        Float
    }
    class Parser {
        -text: str
        -pos: usize
        -jsonc: bool
        -limits: Limits
    }
    class ErrorKind["json::ErrorKind"] {
        <<enumeration>>
        UnexpectedEnd
        UnexpectedCharacter
        InvalidUtf8
        InvalidLiteral
        InvalidEscape
        LoneSurrogate
        ControlCharacter
        LeadingZero
        NumberOutOfRange
        CommentNotAllowed
        TrailingCommaNotAllowed
        InputTooLarge
        TooDeep
        StringTooLong
        NumberTooLong
        TooManyEntries
    }
    JsonFns ..> Dialect : takes
    JsonFns ..> Limits : takes
    JsonFns ..> Parser : parse runs
    JsonFns ..> Value : parse returns
    JsonFns ..> CodecError : parse fails with
    Parser *-- Limits
    Value *-- Number : Number
    Number *-- Repr
    CodecError *-- ErrorKind : Json
    note for Value "Array holds a Vec of Value, and<br/>Object a Vec of String and Value<br/>pairs in document order"
    note for CodecError "Json and Toml carry offset,<br/>line, column and kind;<br/>TomlEdit a step and a kind"
```

## 11. codec: TOML

`toml::parse` reads TOML 1.1 into a tree whose keys and values carry their
byte spans in the source. A `Table` keeps its entries in order and builds a
hash index only once it has many. `Document` (M1.5, drawn below) keeps a
document's text beside its tree and writes edits back into that text.

```mermaid
classDiagram
    direction TB
    class TomlFns["toml"] {
        <<module>>
        +parse(text, limits) Result~Table~
        +parse_bytes(bytes, limits) Result~Table~
    }
    class Table {
        -entries: Vec of Key and Value pairs
        -index: Option~Box~Index~~
        -kind: TableKind
        +kind() TableKind
        +len() usize
        +is_empty() bool
        +entries() Key and Value pair slice
        +get(name) Option~Value~
        +get_key_value(name) Option of Key and Value
        +contains_key(name) bool
        +new_inline()$ Table
        +insert(name, value) Option~Value~
    }
    class Index {
        -hasher: RandomState
        -slots: Box of u32 slice
    }
    class TableKind {
        <<enumeration>>
        Root
        Header
        ArrayHeader
        Implicit
        Dotted
        Inline
    }
    class Key {
        +name: String
        +span: Span
    }
    class Value {
        +kind: ValueKind
        +span: Span
        +as_str() Option~str~
        +as_integer() Option~i64~
        +as_float() Option~f64~
        +as_bool() Option~bool~
        +as_datetime() Option~Datetime~
        +as_array() Option~Array~
        +as_table() Option~Table~
        +get(name) Option~Value~
    }
    class ValueKind {
        <<enumeration>>
        String
        Integer
        Float
        Boolean
        Datetime
        Array
        Table
    }
    class Array {
        -items: Vec~Value~
        -of_tables: bool
        +new(items)$ Array
        +items() Value slice
        +get(index) Option~Value~
        +len() usize
        +is_array_of_tables() bool
    }
    class Span {
        +start: usize
        +end: usize
        +new(start, end) Span
        +range() Range~usize~
    }
    class Datetime {
        <<enumeration>>
        OffsetDatetime
        LocalDatetime
        LocalDate
        LocalTime
    }
    class Date {
        +year: u16
        +month: u8
        +day: u8
    }
    class Time {
        +hour: u8
        +minute: u8
        +second: u8
        +nanosecond: u32
    }
    class Offset {
        <<enumeration>>
        Utc
        Minutes
    }
    class Limits["codec::Limits"]
    class CodecError["codec::CodecError"]
    TomlFns ..> Table : parse returns the root
    TomlFns ..> Limits : takes
    TomlFns ..> CodecError : parse fails with
    Table "1" *-- "0..*" Key : entries
    Table "1" *-- "0..*" Value : entries
    Table *-- Index : index
    Table *-- TableKind
    Key *-- Span
    Value *-- Span
    Value *-- ValueKind
    ValueKind *-- Array : Array
    ValueKind *-- Table : Table
    ValueKind *-- Datetime : Datetime
    Array "1" *-- "0..*" Value : items
    Datetime *-- Date
    Datetime *-- Time
    Datetime *-- Offset : OffsetDatetime
```

The errors of `toml::parse` are `CodecError::Toml`, with a
`toml::ErrorKind`:

```mermaid
classDiagram
    direction LR
    class CodecError["codec::CodecError"] {
        <<enumeration>>
        Json
        Toml
        TomlEdit
    }
    class ErrorKind["toml::ErrorKind"] {
        <<enumeration>>
        UnexpectedEnd
        UnexpectedCharacter
        InvalidUtf8
        ControlCharacter
        BareCarriageReturn
        InvalidLiteral
        InvalidEscape
        InvalidUnicodeEscape
        MultilineKey
        LeadingZero
        MisplacedUnderscore
        IntegerOutOfRange
        FloatOutOfRange
        InvalidDatetime
        DuplicateKey
        DuplicateTable
        NotATable
        NotAnArrayOfTables
        ExtendsInlineTable
        ExtendsDefinedTable
        InputTooLarge
        TooDeep
        StringTooLong
        NumberTooLong
        TooManyEntries
    }
    CodecError *-- ErrorKind : Toml
```

`Document` keeps the text it parsed, its root `Table` and its `Limits`.
`set` plans one splice, parses the new text again and checks it against
the old tree along the path before keeping either; a refused edit is
`CodecError::TomlEdit`, with the step of the path it concerns and an
`EditErrorKind`, and leaves the document as it was. What `set` writes is
TOML 1.0 (ADR-0031).

```mermaid
classDiagram
    direction LR
    class Document["toml::Document"] {
        -text: String
        -root: Table
        -limits: Limits
        +parse(text, limits)$ Result~Document~
        +parse_bytes(bytes, limits)$ Result~Document~
        +as_str() str
        +into_string() String
        +root() Table
        +get(path) Option~Value~
        +set(path, value) Result
    }
    class Step["toml::Step"] {
        <<enumeration>>
        Key
        Index
    }
    class EditErrorKind["toml::EditErrorKind"] {
        <<enumeration>>
        EmptyPath
        NotATable
        NotAnArray
        IndexOutOfRange
        MissingArray
        NotReplaceable
        InvalidDatetime
        TooLarge
        Limit
        Unverified
    }
    class CodecError["codec::CodecError"]
    class Table["toml::Table"]
    class ValueKind["toml::ValueKind"]
    class Limits["codec::Limits"]
    Document *-- Table : root
    Document *-- Limits
    Document ..> Step : paths of
    Document ..> ValueKind : set writes
    Document ..> CodecError : set fails with
    CodecError *-- EditErrorKind : TomlEdit
```

## 12. text: the rope

A `Rope` is a persistent B-tree of UTF-8 leaves of 1 to 4 KiB. Each child
carries a `Summary` of its bytes, chars, UTF-16 units and line breaks, so
edits and position lookups are O(log n), and a clone shares every node.

```mermaid
classDiagram
    direction TB
    class Rope {
        -root: Arc~Node~
        -summary: Summary
        +new() Rope
        +len() usize
        +is_empty() bool
        +insert(at, text) Result
        +remove(range) Result
        +replace(range, text) Result
        +split_off(at) Result~Rope~
        +append(other)
        +slice(range) Result~Rope~
        +is_char_boundary(byte) bool
        +chunks() Chunks
        +chunks_in(range) Result~Chunks~
        +chars_in(range) Result~Chars~
        +char_indices_in(range) Result~CharIndices~
    }
    class Node {
        <<enumeration>>
        Leaf
        Internal
        -height() u8
    }
    class Child {
        -summary: Summary
        -node: Arc~Node~
    }
    class Summary {
        -bytes: usize
        -chars: usize
        -utf16: usize
        -breaks: usize
        -of(text) Summary
        -get(metric) usize
    }
    class Metric {
        <<enumeration>>
        Bytes
        Chars
        Utf16
        Breaks
    }
    class RopeBuilder {
        -leaves: Vec~Child~
        -pending: String
        +new() RopeBuilder
        +push_str(text)
        +finish() Rope
    }
    class Chunks {
        -front: iter Cursor
        -back: iter Cursor
        -start: usize
        -end: usize
    }
    class CharIndices {
        -chunks: Chunks
    }
    class Chars {
        -inner: CharIndices
    }
    class Iterator["std::iter::Iterator"] {
        <<trait>>
    }
    Rope o-- Node : Arc root
    Rope *-- Summary
    Node "1" *-- "2..16" Child : Internal
    Child o-- Node : Arc
    Child *-- Summary
    Summary ..> Metric : get
    RopeBuilder "1" *-- "0..*" Child : leaves
    RopeBuilder ..> Rope : finish
    Rope ..> Chunks : chunks
    CharIndices *-- Chunks
    Chars *-- CharIndices
    Iterator <|.. Chunks
    Iterator <|.. CharIndices
    Iterator <|.. Chars
    note for Node "All leaves are at one depth.<br/>Internal nodes have 8 to 16<br/>children, the root 2 to 16"
```

## 13. text: positions and graphemes

Positions are byte offsets (ADR-0019). The `pos` module adds conversions to
`Rope` between bytes, chars, UTF-16 units, lines and line/column pairs in
any `Unit`, and `pos::grapheme` adds extended grapheme cluster boundaries
(UAX #29) from tables `cargo xtask ucd` generates. A bad position is a
`TextError`, never a panic.

```mermaid
classDiagram
    direction LR
    class Rope {
        +len_in(unit) usize
        +len_lines() usize
        +byte_to_offset(byte, unit) Result~usize~
        +offset_to_byte(offset, unit) Result~usize~
        +byte_to_line(byte) Result~usize~
        +line_to_byte(line) Result~usize~
        +line_span(line) Result~LineSpan~
        +byte_to_line_col(byte, unit) Result~LineCol~
        +line_col_to_byte(pos, unit) Result~usize~
        +is_grapheme_boundary(byte) bool
        +next_grapheme_boundary(byte) Option~usize~
        +prev_grapheme_boundary(byte) Option~usize~
        +graphemes_in(range) Result~Graphemes~
    }
    class Unit {
        <<enumeration>>
        Byte
        Utf16
        Char
    }
    class LineCol {
        +line: usize
        +col: usize
        +new(line, col) LineCol
    }
    class LineBreak {
        <<enumeration>>
        Lf
        CrLf
        Cr
        +byte_len() usize
        +as_str() str
    }
    class LineSpan {
        +start: usize
        +content_end: usize
        +line_break: Option~LineBreak~
        +end() usize
        +content() Range~usize~
        +full() Range~usize~
    }
    class GraphemeFns["pos::grapheme"] {
        <<module>>
        +UNICODE_VERSION: u8 triple
        +is_boundary(text, byte) bool
        +next_boundary(text, byte) Option~usize~
        +prev_boundary(text, byte) Option~usize~
        +graphemes(text) StrGraphemes
    }
    class Graphemes {
        -rope: Rope
        -inner: Segmenter~CharIndices~
    }
    class StrGraphemes {
        -text: str
        -inner: Segmenter
    }
    class Segmenter~I~ {
        -chars: I
        -end: usize
    }
    class TextError["text::TextError"]
    Rope ..> Unit : conversions take
    Rope ..> LineCol : byte_to_line_col
    Rope ..> LineSpan : line_span
    LineSpan *-- LineBreak
    Rope ..> Graphemes : graphemes_in
    GraphemeFns ..> StrGraphemes : graphemes
    Graphemes *-- Segmenter
    StrGraphemes *-- Segmenter
    Rope ..> TextError : a bad position fails with
    note for Rope "Only the methods pos and<br/>pos::grapheme add are shown"
```

## 14. text: selections and transactions

A `Selection` is one or more `Cursor`s, sorted and merged by one rule, with
a primary. A `ChangeSet` is a list of byte-range replacements in the old
text, each holding its new text as a `Rope`; it applies, maps positions,
composes and inverts. A `Transaction` is a change set and the selection
after it.

```mermaid
classDiagram
    direction TB
    class Cursor {
        +anchor: usize
        +head: usize
        +new(anchor, head) Cursor
        +caret(at) Cursor
        +start() usize
        +end() usize
        +range() Range~usize~
        +is_caret() bool
        +is_backward() bool
    }
    class Selection {
        -cursors: Vec~Cursor~
        -primary: usize
        +caret(at) Selection
        +single(cursor) Selection
        +new(primary, others) Selection
        +cursors() Cursor slice
        +primary() Cursor
        +primary_index() usize
        +transform(f) Selection
        +validate(text) Result
        +map(changes, caret) Result~Selection~
    }
    class Assoc {
        <<enumeration>>
        Before
        After
    }
    class ChangeSet {
        -replacements: Arc of Replacement slice
        -len_before: usize
        -len_after: usize
        +identity(len) ChangeSet
        +new(text, edits) Result~ChangeSet~
        +len_before() usize
        +len_after() usize
        +is_identity() bool
        +changes() Changes
        +apply(text) Result
        +invert(before) Result~ChangeSet~
        +compose(next) Result~ChangeSet~
        +map(pos, assoc) Result~usize~
    }
    class Replacement {
        -start: usize
        -end: usize
        -new_start: usize
        -text: Option~Rope~
    }
    class Changes {
        -inner: slice Iter of Replacement
    }
    class Change {
        +old: Range~usize~
        +new: Range~usize~
        +text: Option~Rope~
    }
    class Transaction {
        -changes: ChangeSet
        -selection: Selection
        +new(changes, selection) Transaction
        +changes() ChangeSet
        +selection() Selection
        +into_parts() ChangeSet and Selection
        +change_each(text, selection, edit) Result~Transaction~
    }
    class Place {
        <<enumeration>>
        Map
        Start
        End
        Select
    }
    class Rope["text::Rope"]
    Selection "1" *-- "1..*" Cursor
    Selection ..> ChangeSet : map through
    Selection ..> Assoc : map takes
    Selection ..> Rope : validate against
    ChangeSet "1" o-- "0..*" Replacement : Arc
    Replacement *-- Rope : text
    ChangeSet ..> Changes : changes
    Changes ..> Change : yields
    ChangeSet ..> Rope : apply, invert
    ChangeSet ..> Assoc : map takes
    Transaction *-- ChangeSet
    Transaction *-- Selection
    Transaction ..> Place : change_each edit returns
    note for Place "Where the cursor goes after<br/>change_each edits it: mapped,<br/>at the start or end of the<br/>new text, or selecting it"
```

## 15. text: history, the undo tree

`History` applies every step to the buffer's rope (ADR-0020). It keeps a
tree of revisions: undo moves to the parent, redo to the child created or
visited last, and `jump_to` to any revision, so no branch is lost. Commits group by command
(`GroupId`) or coalesce by time, and checkpoints mark the saved revision.

```mermaid
classDiagram
    direction LR
    class History {
        -revisions: Vec~Revision~
        -current: usize
        -pending: Vec~Piece~
        -saved: Option~usize~
        -open: bool
        -text: Rope
        +new(text) History
        +text() Rope
        +commit(text, transaction, selection_before, now, group) Result~Step~
        +undo(text) Result~Option~Step~~
        +redo(text) Result~Option~Step~~
        +jump_to(text, target) Result~Option~Step~~
        +seal()
        +checkpoint() Checkpoint
        +mark_saved(checkpoint) Result
        +is_modified() bool
        +current() RevisionId
        +root() RevisionId
        +can_undo() bool
        +can_redo() bool
    }
    class Revision {
        -parent: usize
        -last_child: Option~usize~
        -changes: ChangeSet
        -inverse: ChangeSet
        -selection_before: Selection
        -selection_after: Selection
        -time: Option~Instant~
        -tag: Tag
    }
    class Piece {
        -changes: ChangeSet
        -inverse: ChangeSet
    }
    class Tag {
        <<enumeration>>
        Root
        New
        Command
        Coalesce
    }
    class Group {
        <<enumeration>>
        New
        Command
        Coalesce
    }
    class GroupId {
        -0: u64
        +fresh() GroupId
    }
    class RevisionId {
        -0: u64
        +seq() u64
    }
    class Step {
        +revision: RevisionId
        +before: Rope
        +changes: Vec~ChangeSet~
        +selection: Selection
    }
    class Checkpoint {
        -revision: RevisionId
        -text: Rope
        +revision() RevisionId
        +text() Rope
    }
    class Transaction["text::Transaction"]
    class ChangeSet["text::ChangeSet"]
    class Selection["text::Selection"]
    class Rope["text::Rope"]
    History "1" *-- "1..*" Revision : revisions
    History "1" *-- "0..*" Piece : pending
    History *-- Rope : text
    History ..> Transaction : commit takes
    History ..> Group : commit takes
    History ..> Step : commit, undo, redo return
    History ..> Checkpoint : checkpoint
    History ..> RevisionId : current, root
    Revision *-- ChangeSet : changes, inverse
    Revision *-- Selection : before, after
    Revision *-- Tag
    Piece *-- ChangeSet : changes, inverse
    Tag --> GroupId : Command
    Group --> GroupId : Command
    Step *-- RevisionId
    Checkpoint *-- RevisionId
    note for Revision "parent and last_child are indexes<br/>into the revisions of its History:<br/>the undo tree"
```

## 16. text: encodings

`Decoder` detects UTF-8 or UTF-16 by the byte-order mark only, and decodes
bytes pushed in pieces. Invalid input decodes losslessly: each invalid
sequence becomes one U+FFFD in the text, the replaced bytes are kept in
runs, and the result is read-only; `write_original` gives back the input
byte for byte.

```mermaid
classDiagram
    direction TB
    class EncodingFns["encoding"] {
        <<module>>
        +decode(bytes) Decoded
        +encode(text, format, out) io Result
    }
    class Encoding {
        <<enumeration>>
        Utf8
        Utf16Le
        Utf16Be
        +bom() u8 slice
        +name() str
    }
    class Format {
        +encoding: Encoding
        +bom: bool
        +UTF8: Format
        +new(encoding, bom) Format
    }
    class Decoder {
        -mode: Mode
        -format: Format
        -text: Sink
        -runs: Runs
        +new() Decoder
        +push(bytes)
        +finish() Decoded
    }
    class Mode {
        <<enumeration>>
        Detecting
        Utf8
        Utf16
    }
    class Decoded {
        -text: Rope
        -detected: Format
        -runs: Runs
        +text() Rope
        +into_text() Rope
        +detected() Format
        +format() Option~Format~
        +read_only() bool
        +replaced() ReplacedRuns
        +write_original(out) io Result
        +original_bytes() Vec~u8~
    }
    class Runs {
        -heads: Vec~u8~
        -bytes: Vec~u8~
        -count: usize
        -end: usize
        -open: Option~OpenRun~
    }
    class ReplacedRuns {
        -heads: u8 slice
        -bytes: u8 slice
        -end: usize
    }
    class Replaced {
        +range: Range~usize~
        +bytes: u8 slice
    }
    class Rope["text::Rope"]
    EncodingFns ..> Decoder : decode runs
    EncodingFns ..> Format : encode takes
    Format *-- Encoding
    Decoder *-- Mode
    Decoder *-- Format
    Decoder ..> Decoded : finish
    Decoded *-- Rope : text
    Decoded *-- Format : detected
    Decoded *-- Runs
    Decoded ..> ReplacedRuns : replaced
    ReplacedRuns ..> Replaced : yields
    note for Decoded "format is None when the input<br/>was invalid: then it is read-only"
```

## 17. text: files

`file::load` reads a file and decodes it, and records its line endings and
whether its last line ends with a break. `file::save` writes the text in
the given `Format` through a temp file in the same directory, syncs it and
renames it over the file, keeping its permissions and following symbolic
links. Each I/O error names the `FileStep` that failed.

```mermaid
classDiagram
    direction LR
    class FileFns["file"] {
        <<module>>
        +LARGE_FILE_BYTES: u64
        +TEMP_PREFIX: str
        +load(path) Result~Loaded~
        +save(path, text, format) Result
    }
    class Loaded {
        -decoded: Decoded
        -line_endings: LineEndings
        -trailing_break: Option~LineBreak~
        -file_len: u64
        +text() Rope
        +decoded() Decoded
        +format() Option~Format~
        +read_only() bool
        +line_endings() LineEndings
        +trailing_break() Option~LineBreak~
        +file_len() u64
        +is_large() bool
    }
    class LineEndings {
        +lf: usize
        +crlf: usize
        +cr: usize
        +of(text) LineEndings
        +total() usize
        +is_mixed() bool
        +dominant() LineBreak
    }
    class Temp {
        -file: File
        -removal: Removal
        -target: PathBuf
        -dir: PathBuf
        -permissions: Option~Permissions~
        -create(path) Result~Temp~
        -write(write) Result~Temp~
        -commit() Result
    }
    class Removal {
        -path: PathBuf
        -armed: bool
    }
    class Drop["std::ops::Drop"] {
        <<trait>>
    }
    class FileStep {
        <<enumeration>>
        Metadata
        Open
        Read
        ReadLink
        CheckWritable
        CreateTemp
        Write
        SetPermissions
        Sync
        Rename
        SyncDirectory
    }
    class Decoded["encoding::Decoded"]
    class Format["encoding::Format"]
    class LineBreak["pos::LineBreak"]
    class TextError["text::TextError"]
    FileFns ..> Loaded : load returns
    FileFns ..> Format : save takes
    FileFns ..> Temp : save writes through
    FileFns ..> TextError : fails with
    Loaded *-- Decoded
    Loaded *-- LineEndings
    Loaded *-- LineBreak : trailing_break
    LineEndings ..> LineBreak : dominant
    Temp *-- Removal
    Drop <|.. Removal
    TextError *-- FileStep : Io
    note for Removal "Removes the temp file on drop<br/>unless the rename disarmed it"
    note for Loaded "is_large is true above<br/>LARGE_FILE_BYTES, 16 MiB,<br/>for the large-file mode"
```

## 18. text: the error enum

`TextError` is the one error type of `tidepool-text`: bad positions and
ranges, edits that do not fit their text, revisions that are not in a
history, and the file errors.

```mermaid
classDiagram
    direction LR
    class TextError {
        <<enumeration>>
        ByteOutOfBounds
        NotCharBoundary
        InvertedRange
        OffsetOutOfBounds
        OffsetInsideChar
        LineOutOfBounds
        ColumnOutOfBounds
        ColumnInsideChar
        LengthMismatch
        OverlappingEdits
        TooLong
        RevisionNotFound
        HistoryMismatch
        Io
        NotRegularFile
        NotWritable
        TooManyLinks
    }
    class Unit["pos::Unit"]
    class LineCol["pos::LineCol"]
    class FileStep["file::FileStep"]
    class IoError["std::io::Error"]
    class Display["std::fmt::Display"] {
        <<trait>>
    }
    class StdError["std::error::Error"] {
        <<trait>>
    }
    TextError *-- Unit : Offset and Column variants
    TextError *-- LineCol : Column variants
    TextError *-- FileStep : Io
    TextError *-- IoError : Io, NotWritable
    Display <|.. TextError
    StdError <|.. TextError
```

## 19. xtask: checks and reports

Each `cargo xtask` check is a `Check`: a name, a step and a role. A step
runs cargo with fixed arguments (`Cargo` and `Tests`), or one of `xtask`'s
own checks (`Own`: `deps::run`, `layers::run`, `crates::run`,
`features::run`, `unsafe_audit::run`, `headers::run`, `workflows::run`,
`ucd::check` or `ucd::write`), which returns a `Report`. A `Compiles` step does not run
once a `Gate` step has failed; `deps` is the gate. `ci` runs the checks in
order and ends with a summary table that also lists the `Later` commands
it will run once they exist (those with `in_ci`: `tools` and `theme`).
Every `Later` command exits 1 with "not yet implemented" until its task
lands. `run_gate` runs them and returns a `Gate`, each step's outcome and
the tests counted; `run_ci` is its `passed`, and `site` keeps the rest
(ADR-0030).

```mermaid
classDiagram
    direction TB
    class CheckFns["check"] {
        <<module>>
        +COMMANDS: Check slice
        +CI: Check slice
        +LATER: Later slice
        +run_one(check, root, out) io Result~bool~
        +run_ci(checks, later, root, out) io Result~bool~
        +run_gate(checks, later, root, out) io Result~Gate~
    }
    class Gate {
        -outcomes: Vec~Outcome~
        -tests: TestCounts
        +passed() bool
        +steps() usize, usize
        +skipped() usize
        +tests() TestCounts
    }
    class TestCounts["cargo::TestCounts"] {
        +results: usize
        +unreadable: usize
        +passed: usize
        +failed: usize
        +ignored: usize
    }
    class Check {
        +name: str
        +about: str
        +step: Step
        +role: Role
    }
    class Step {
        <<enumeration>>
        Cargo
        Tests
        Own
    }
    class Role {
        <<enumeration>>
        Independent
        Gate
        Compiles
    }
    class Report {
        +notes: Vec~String~
        +violations: Vec~String~
        +stale_lockfiles: usize
        +skips: Skips
        +tests: TestCounts
        +push_stale_lockfile(violation)
    }
    class Skips["cargo::Skips"] {
        +reasons: Vec~String~
        +count: usize
    }
    class Later {
        +name: str
        +task: str
        +in_ci: bool
    }
    class CiOutcome["check::Outcome"] {
        -name: str
        -status: Status
        -elapsed: Duration
        -skipped: usize
    }
    class Status {
        <<enumeration>>
        Passed
        Failed
        NotRun
    }
    class XtaskError["error::Error"] {
        <<enumeration>>
        Usage
        NotImplemented
        Read
        Spawn
        CommandFailed
        UnexpectedOutput
        Doc
        Output
        Write
        Env
    }
    CheckFns "1" o-- "0..*" Check : COMMANDS, CI
    CheckFns "1" o-- "0..*" Later : LATER
    Check *-- Step
    Check *-- Role
    Step ..> Report : Own returns
    Step ..> XtaskError : Own fails with
    Report *-- Skips
    Report *-- TestCounts
    CheckFns ..> Gate : run_gate returns
    Gate o-- CiOutcome : one per step
    Gate *-- TestCounts
    CheckFns ..> CiOutcome : one per ci step
    CiOutcome *-- Status
    note for Skips "Tests that printed SKIPPED:<br/>did not run, and are never<br/>counted as passes"
```

## 20. xtask: fuzz-smoke's table

`cargo xtask fuzz-smoke` runs each fuzz target in `TARGETS` as a
`cargo test` of its crate's `tests/fuzz.rs` in the `fuzz` profile, and
reads the `fuzz-result:` line the test prints back into a
`core::testing::fuzz::Summary`. It turns each run into a `Verdict`.

```mermaid
classDiagram
    direction TB
    class SmokeFns["fuzz"] {
        <<module>>
        +TARGETS: Target slice
        +DEFAULT_SECS: u64
        +PROFILE: str
        +clock_seed() u64
        +run(options, table, root, out) Result~bool~
    }
    class SmokeTarget["fuzz::Target"] {
        +name: str
        +package: str
    }
    class Options {
        +targets: Vec~Target~
        +secs: u64
        +seed: u64
        +seed_from_clock: bool
        +replay: bool
        +parse(args, table, clock) Result~Options~
    }
    class Verdict {
        <<enumeration>>
        Clean
        Found
        Failed
        NotRun
        NotBuilt
        CargoFailed
        Died
        ExitedWithoutResult
    }
    class Args["core::cli::Args"]
    class Summary["core::testing::fuzz::Summary"]
    class Reading["core::testing::fuzz::Reading"]
    class Kept["core::testing::fuzz::Kept"]
    class XtaskError["error::Error"]
    SmokeFns "1" o-- "3" SmokeTarget : TARGETS
    Options "1" o-- "1..*" SmokeTarget : borrows
    Options ..> Args : parse reads
    Options ..> XtaskError : parse fails with Usage
    SmokeFns ..> Options : run takes
    SmokeFns ..> Verdict : one per target
    Verdict *-- Summary : Clean, Found, Failed
    Summary *-- Reading : memory, reserved
    SmokeFns ..> Kept : keep_died
    note for SmokeTarget "TARGETS holds json and toml<br/>in tidepool-codec, and<br/>encoding in tidepool-text"
```

## 21. xtask: cargo metadata, the dependency matrix and the crates check

`metadata::load` runs `cargo metadata` and reads its JSON with
`codec::json` into a `Metadata`. The `layers` check reads the dependency
matrix from `docs/01-architecture.md` into a `Matrix` and checks every
member's dependencies against its row; `deps` checks each package against
the `Pin`s of its allowlist.

### 21.1 cargo metadata

```mermaid
classDiagram
    direction TB
    class MetadataFns["metadata"] {
        <<module>>
        +load(root, manifest, resolve) Result~Metadata~
        +parse(text, command) Result~Metadata~
        +workspaces(root, manifests, resolve) Vec~Workspace~
    }
    class Metadata {
        +workspace_root: PathBuf
        +packages: Vec~Package~
    }
    class Package {
        +name: String
        +version: String
        +source: Option~String~
        +manifest_path: PathBuf
        +is_member: bool
        +dependencies: Vec~Dependency~
        +targets: Vec~Target~
    }
    class Target {
        +name: String
        +kind: String
        +src_path: PathBuf
    }
    class Dependency {
        +name: String
        +source: Option~String~
        +req: String
        +path: Option~PathBuf~
        +kind: Kind
    }
    class Kind {
        <<enumeration>>
        Normal
        Dev
        Build
        +describe() str
    }
    class Resolve {
        <<enumeration>>
        Yes
        No
    }
    class Workspace {
        +shown: String
        +graph: Result~Metadata~
        +declared: Option~Metadata~
    }
    class JsonValue["codec::json::Value"]
    MetadataFns ..> Metadata : load, parse
    MetadataFns ..> Workspace : workspaces
    MetadataFns ..> Resolve : takes
    MetadataFns ..> JsonValue : parses the<br/>output into
    Workspace *-- Metadata : graph, declared
    Metadata "1" *-- "0..*" Package
    Package "1" *-- "0..*" Dependency
    Package "1" *-- "0..*" Target
    Dependency *-- Kind
```

### 21.2 The dependency matrix, the allowlist and `crates`

`crates::run` loads the workspace's `Metadata` without resolving it, and
reads the root manifest and each member's into a `codec::toml::Table`. A
member's README must start with `# <name>`, name only the roadmap's
tasks, and have each of `SECTIONS` once and in that order, and `FEATURES`
at most once, between Usage and Documentation. `crates` finds those lines
as `layers` reads its document, with `matrix::classify`, so a heading in a
fenced code block, an HTML block or an HTML comment does not count. It
also checks that each member keeps byte-for-byte copies of the root's
`LICENSE_FILES`, `LICENSE` and `NOTICE`.

```mermaid
classDiagram
    direction TB
    class MatrixFns["matrix"] {
        <<module>>
        +DOC: str
        +parse(text) Result~Matrix~
        +classify(lines) Vec of Kind and String pairs
    }
    class Matrix {
        +rows: Vec~Row~
        +row(name) Option~Row~
    }
    class Row {
        +name: String
        +line: usize
        +allowed: Allowed
        +written: String
        +dev_only: bool
        +allows(name) bool
    }
    class Allowed {
        <<enumeration>>
        Only
        AnyWorkspaceCrate
    }
    class LineKind["matrix::Kind"] {
        <<enumeration>>
        Shown
        Fenced
        Comment
        Html
    }
    class Pin["deps::Pin"] {
        +name: String
        +version: String
    }
    class CratesFns["crates"] {
        <<module>>
        -SECTIONS: 6 str
        -FEATURES: str
        -LICENSE_FILES: 2 str
        +run(root) Result~Report~
    }
    class Metadata["metadata::Metadata"]
    class TomlTable["codec::toml::Table"]
    class Report["check::Report"]
    MatrixFns ..> Matrix : parse
    Matrix "1" *-- "1..*" Row
    Row *-- Allowed
    MatrixFns ..> LineKind : classify
    CratesFns ..> Metadata : takes the<br/>members from
    CratesFns ..> TomlTable : reads<br/>manifests into
    CratesFns ..> MatrixFns : finds README<br/>sections with classify
    CratesFns ..> Report : run returns
    note for MatrixFns "DOC is docs/01-architecture.md"
    note for Pin "deps reads one per line<br/>of xtask/deps-allowlist.txt"
```

## 22. Error enums across crates

Each crate has its own error enum that implements `Display` and
`std::error::Error`; those of code that does I/O keep the `std::io::Error`
that caused the failure. No error wraps another crate's: `xtask` turns a `CoreError` into the text
of `Error::Usage`. The boxes list only some variants.

```mermaid
classDiagram
    direction LR
    class Display["std::fmt::Display"] {
        <<trait>>
    }
    class StdError["std::error::Error"] {
        <<trait>>
    }
    class IoError["std::io::Error"]
    class CoreError["core::CoreError"] {
        <<enumeration>>
        InvalidColor
        UnexpectedArgument
    }
    class WithContext["core::error::WithContext"]
    class PathError["core::path::PathError"] {
        <<enumeration>>
        Empty
        NotUtf8
        Io
        TooManyLinks
    }
    class FuzzError["core::testing::fuzz::FuzzError"] {
        <<enumeration>>
        Var
        Read
        Write
        Thread
    }
    class CodecError["codec::CodecError"] {
        <<enumeration>>
        Json
        Toml
    }
    class JsonErrorKind["codec::json::ErrorKind"]
    class TomlErrorKind["codec::toml::ErrorKind"]
    class TextError["text::TextError"] {
        <<enumeration>>
        ByteOutOfBounds
        OverlappingEdits
        Io
        NotWritable
    }
    class XtaskError["xtask::error::Error"] {
        <<enumeration>>
        Usage
        Read
        Spawn
        Output
    }
    Display <|.. CoreError
    StdError <|.. CoreError
    Display <|.. WithContext
    StdError <|.. WithContext
    Display <|.. PathError
    StdError <|.. PathError
    Display <|.. FuzzError
    StdError <|.. FuzzError
    Display <|.. CodecError
    StdError <|.. CodecError
    Display <|.. TextError
    StdError <|.. TextError
    Display <|.. XtaskError
    StdError <|.. XtaskError
    CodecError *-- JsonErrorKind : Json
    CodecError *-- TomlErrorKind : Toml
    PathError *-- IoError : Io
    FuzzError *-- IoError : Read, Write
    TextError *-- IoError : Io, NotWritable
    XtaskError *-- IoError : Read, Spawn, Output, Write
```

## 23. Planned types

The other 22 members are empty crates, so they have no class diagram yet.
Their specs name these types, which the tasks in `docs/roadmap.md` build;
the other files of `diagrams/` draw them dashed. A spec that gives no name
is described in words here.

- `tidepool-platform`: `Window` (`Window::open(config)`), `wait_events`
  filling a `Vec<Event>`, the `Waker` that interrupts it from any thread,
  and `present(&Framebuffer, &[DamageRect])`, planned (M2.2); `Pty`
  (`Pty::spawn(argv, env, cwd, size)`), planned (M5.9); the terminal's raw
  mode guard, planned (M7.1).
- `tidepool-gfx`: the premultiplied BGRA8 framebuffer of the `framebuffer`
  module, which `docs/subsystems/frontends.md` calls `gfx::Framebuffer`,
  planned (M2.3).
- `tidepool-ui`: the widget tree's `NodeId`, the `TextMetrics` trait and
  the `DrawList`, planned (M3.1); the theme's token set, planned (M3.2).
- `tidepool-gui` and `tidepool-tui`: the `Frontend` trait in
  `docs/subsystems/frontends.md` (`wait_events`, `waker`, `metrics`,
  `present`, `clipboard`). No task names the trait itself; the GUI's first
  frame is planned (M2.6) and the TUI's renderer and input decoder
  (M7.1).
- `tidepool-toolchain`: the registry's tool states `Missing`, `Found`,
  `Outdated`, `Installing` and `Broken`, planned (M5.1); the `Spawn`
  builder and the `SecretArg` and `SecretEnv` types that redact
  themselves, planned (M5.2).
- `tidepool-editor`: the modal keymap engine, planned (M4.6); vim mode's
  modes `Normal`, `Insert`, `Replace` and `OperatorPending`, planned
  (M7.2); `Visual`, `VisualLine` and `VisualBlock`, planned (M7.3);
  `CommandLine` and `Search`, planned (M7.4).

## Sources

The main files these diagrams were drawn from:

- `crates/core/src/lib.rs`, `error.rs`, `cli.rs`, `cancel.rs`, `rng.rs`,
  `hash.rs`, `hash/sha256.rs`, `civil.rs`, `geom.rs`, `color.rs`,
  `input.rs`, `log.rs`, `log/file.rs`, `log/redact.rs`, `log/writer.rs`,
  `testing.rs`, `testing/fuzz.rs`, `testing/fuzz/engine.rs`,
  `testing/fuzz/mutate.rs`, `testing/fuzz/paths.rs`, `path.rs`
- `crates/codec/src/lib.rs`, `limits.rs`, `error.rs`, `json.rs`,
  `json/number.rs`, `json/parse.rs`, `json/write.rs`, `toml.rs`,
  `toml/table.rs`, `toml/datetime.rs`, `toml/parse.rs`,
  `toml/document.rs`, `toml/write.rs`
- `crates/codec/tests/fuzz.rs`, `crates/text/tests/fuzz.rs`
- `crates/text/src/lib.rs`, `rope.rs`, `rope/node.rs`, `rope/iter.rs`,
  `pos.rs`, `pos/grapheme.rs`, `selection.rs`, `transaction.rs`,
  `history.rs`, `encoding.rs`, `file.rs`, `error.rs`
- `xtask/src/main.rs`, `check.rs`, `cargo.rs`, `error.rs`, `fuzz.rs`,
  `metadata.rs`, `matrix.rs`, `layers.rs`, `deps.rs`, `crates.rs`,
  `ucd.rs`, `Cargo.toml`
- `crates/codec/Cargo.toml`, `crates/text/Cargo.toml`
- `docs/01-architecture.md` (the dependency matrix), `docs/roadmap.md`,
  `docs/decisions.md` (ADR-0013, ADR-0019, ADR-0020, ADR-0027, ADR-0029)
- `docs/subsystems/core.md`, `text-and-formats.md`,
  `sys-and-platform.md`, `rendering.md`, `ui-and-theme.md`,
  `frontends.md`, `toolchains.md`, `vim-mode.md`
- The `README.md` of `crates/core`, `crates/codec` and `crates/text`
