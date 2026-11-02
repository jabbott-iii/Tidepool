#!/bin/sh
# Copyright 2026 Joseph Anthony Abbott III
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Claude Code PostToolUse hook (Edit|Write).
# When an edit touches a Cargo manifest, a lockfile or cargo's configuration, check the dependency
# policy (`xtask deps`) and the layer matrix (`xtask layers`), and feed any problem back to Claude
# (exit 2). Other edits exit 0 at once. Policy: docs/02-dependency-policy.md; matrix:
# docs/01-architecture.md -> "Dependency matrix".
#
# The checkout checked is the one holding the edited file: a linked worktree of this repository
# inside the project, else the project itself. An edit outside the project exits 0: it cannot
# change this repository's graph. A file path with a JSON escape in it is not decoded; the project
# is checked.
#
# Until the graph has been checked, nothing is compiled but xtask and the root-workspace members it
# uses, from the checkout's own files, and no Cargo.lock is written:
#   1. The rustup toolchain files and the cargo configuration files that would apply here (in the
#      checkout and each directory above it, up to the project) are read first. A toolchain file
#      may hold only [toolchain] with channel, components, targets and profile (a `path` would put
#      another cargo in place), and .cargo/config and .cargo/config.toml only [alias]. Anything
#      else (a rustc wrapper, rustflags, [env], [patch], [source] ...) is reported, and cargo is
#      not run. No value is shown, since one could be a secret: a report names the file and line
#      with the first part of a table header, or a plain key.
#   2. `cargo metadata --offline --locked` resolves the graph. It compiles and writes nothing.
#   3. `cargo tree` (compiles nothing) must show that every package xtask is built from is a
#      member of the checkout's root workspace, in its own directory, reached without a symbolic
#      link and holding none, with every target's source file inside it, and with no build script
#      and no proc-macro target, so that compiling runs no package's code. Only then is xtask built
#      (--locked, offline), into the checkout's target/deps-guard, which no other checkout uses.
#      Cargo judges freshness by file times, so a stamp of the sources' contents is kept there too:
#      when it changes, the old build is removed first.
#   4. That xtask runs `deps`, then `layers`. Both must pass for exit 0.
# A Cargo.lock that is only out of date, as it is right after adding an in-repo path dependency, is
# reported as that, with how to update it. What the root workspace's manifests declare is still
# checked first (`cargo metadata --no-deps`, which needs no lockfile). The hook never passes in that
# state.
# If xtask cannot be built, scripts/check-deps.sh runs as a fallback. It does not look for path
# packages outside the repository or check the matrix, so even when it passes the hook exits 2 and
# says the check is incomplete. Source lines that cargo quotes in its errors are left out of what
# the hook relays, and a report longer than 60 lines is cut to its first 40 and last 5.
# Configuration outside the project (CARGO_HOME, parent directories, the environment) is trusted,
# except that PATH keeps only its absolute entries and rustup may not install a toolchain.

# An empty or relative PATH entry would find programs in the checkout once the hook is in it.
rest=$PATH
PATH=
while [ -n "$rest" ]; do
  entry=${rest%%:*}
  case "$rest" in
    *:*) rest=${rest#*:} ;;
    *) rest= ;;
  esac
  case "$entry" in
    /*) PATH=${PATH:+$PATH:}$entry ;;
  esac
done
export PATH
# A toolchain that rust-toolchain.toml names but that is not installed is reported, not downloaded.
RUSTUP_AUTO_INSTALL=0
export RUSTUP_AUTO_INSTALL

input=$(cat)

lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

# The edited file: the tool input's "file_path", read up to its closing quote or its first
# backslash. A value with an escape in it is left empty, so the whole input decides below.
raw=$(printf '%s' "$input" | sed -n 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"\\]*[\\"]\).*/\1/p' | head -n 1)
file=
case "$raw" in
  *'"') file=${raw%?} ;;
esac

if [ -n "$file" ]; then
  case "/$(lower "$file")" in
    */cargo.toml | */cargo.lock | */.cargo/config | */.cargo/config.toml) ;;
    *) exit 0 ;;
  esac
else
  # No usable file path: decide on the whole input.
  case "$(lower "$input")" in
    *cargo.toml* | *cargo.lock* | *.cargo/config*) ;;
    *) exit 0 ;;
  esac
fi

proj=$(cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null && pwd -P) || exit 0

# The git directory that the project's worktrees share: its .git directory, or, when the project is
# itself a linked worktree, the one its git directory's `commondir` names. Empty if there is none.
common=
if [ -d "$proj/.git" ] && [ ! -L "$proj/.git" ]; then
  common=$(cd "$proj/.git" && pwd -P)
elif [ -f "$proj/.git" ]; then
  g=$(sed -n 's/^gitdir: //p' "$proj/.git" | head -n 1)
  case "$g" in
    '' | /*) ;;
    *) g="$proj/$g" ;;
  esac
  if [ -n "$g" ] && [ -f "$g/commondir" ]; then
    c=$(head -n 1 "$g/commondir")
    case "$c" in
      /*) ;;
      *) c="$g/$c" ;;
    esac
    common=$(cd "$c" 2>/dev/null && pwd -P) || common=
  fi
fi

# Whether directory $1 is a linked worktree of this repository, registered in both directions: its
# `.git` is a regular file naming a git directory under the shared one's worktrees/, whose `gitdir`
# file points back at it. Either link may be relative (git's `worktree.useRelativePaths`): the
# first to $1, the second to the git directory.
is_worktree() {
  [ -n "$common" ] || return 1
  [ -f "$1/.git" ] && [ ! -L "$1/.git" ] || return 1
  g=$(sed -n 's/^gitdir: //p' "$1/.git" | head -n 1)
  [ -n "$g" ] || return 1
  case "$g" in
    /*) ;;
    *) g="$1/$g" ;;
  esac
  [ "$(cd "$g/.." 2>/dev/null && pwd -P)" = "$common/worktrees" ] || return 1
  [ -f "$g/gitdir" ] || return 1
  back=$(head -n 1 "$g/gitdir")
  case "$back" in
    /*) ;;
    *) back="$g/$back" ;;
  esac
  back=$(cd "$(dirname -- "$back")" 2>/dev/null && pwd -P) || return 1
  [ "$back" = "$(cd "$1" && pwd -P)" ]
}

root=$proj
if [ -n "$file" ]; then
  case "$file" in
    /*) ;;
    *) file="$proj/$file" ;;
  esac
  parent=$(dirname -- "$file")
  dir=$(cd "$parent" 2>/dev/null && pwd -P) || dir=
  # In the project if the directory as given or as resolved is in it (so an edit made through a
  # symbolic link is checked), or if it cannot be resolved.
  inside=
  case "$dir" in
    '' | "$proj" | "$proj"/*) inside=1 ;;
  esac
  case "$parent/" in
    "$proj"/*) inside=1 ;;
  esac
  if [ -n "${CLAUDE_PROJECT_DIR:-}" ]; then
    case "$parent/" in
      "${CLAUDE_PROJECT_DIR%/}"/*) inside=1 ;;
    esac
  fi
  [ -n "$inside" ] || exit 0
  d=
  case "$dir" in
    "$proj"/*) d=$dir ;;
  esac
  if [ -z "$d" ]; then
    case "$parent" in
      *'/./'* | *'/../'* | *'/.' | *'/..') ;;
      "$proj"/*) d=$parent ;;
    esac
  fi
  while [ -n "$d" ] && [ "$d" != "$proj" ]; do
    if is_worktree "$d"; then
      root=$(cd "$d" && pwd -P) || root=$proj
      break
    fi
    d=${d%/*}
  done
fi
cd "$root" || exit 0

policy='Tidepool dependency policy (docs/02-dependency-policy.md): only Rust std and in-repo path crates are allowed. If a line above reports a dependency from a registry, git or a path outside the repository, or copied code that is not a workspace member, revert it, or stop and run /evaluate-dependency for the user to decide. Anything else above (a manifest or lockfile error, a cargo configuration line, build output or another workspace in the repository) is not a dependency decision: fix what it says, then run `cargo xtask deps`.'
matrix='Tidepool layer matrix (docs/01-architecture.md -> "Dependency matrix"): a crate may depend only on the crates its row lists, and every workspace member needs a row. Do not edit the matrix to make code compile; if the task needs a new edge or crate, stop and ask the user.'

# $1 with the user information of every URL in it (`https://user:token@`) replaced by `...`: cargo
# names the URL of a dependency it cannot fetch, credentials and all.
redacted() {
  case "$1" in
    *://*) printf '%s\n' "$1" | awk '{ gsub(/:\/\/[^\/@"'"'"'` \t]*@/, "://...@"); print }' ;;
    *) printf '%s\n' "$1" ;;
  esac
}

# Reports $1, with URL credentials removed and cut to its first 40 and last 5 lines if it has
# more than 60, and the guidance $2, if any, and stops with exit 2.
fail() {
  m=$(redacted "$1") || m='deps-guard: the report could not be cleaned of URL credentials, so it is not shown.'
  n=$(($(printf '%s\n' "$m" | wc -l)))
  if [ "$n" -gt 60 ]; then
    printf '%s\n' "$m" | head -n 40 >&2
    printf '  ... %s more lines; `cargo xtask deps` and `cargo xtask layers` print them all ...\n' "$((n - 45))" >&2
    printf '%s\n' "$m" | tail -n 5 >&2
  else
    printf '%s\n' "$m" >&2
  fi
  if [ -n "${2:-}" ]; then
    printf '%s\n' "$2" >&2
  fi
  exit 2
}

violation() { fail "$1" "$policy"; }

# Cargo's error text $1 without the source lines it quotes (`  4 | key = "value"`), which could
# hold a secret from a manifest or configuration file it could not parse, and with the user
# information of any URL (`https://user:token@`) replaced by `...`.
unquoted() {
  printf '%s\n' "$1" | awk '
    /^[[:space:]]*[0-9]*[[:space:]]*[|]/ { if (!gap) print "  (quoted source lines left out)"; gap = 1; next }
    { gap = 0; gsub(/:\/\/[^\/@"'"'"'` \t]*@/, "://...@"); print }
  '
}

# Reports out-of-date lockfiles ($1: what found them; $2: what was checked all the same) and stops.
stale() {
  fail "$1" "deps-guard: a Cargo.lock in $root is out of date, so the full dependency graph has not been checked. $2 Update the lockfile offline, which compiles nothing (\`cargo metadata --offline --format-version 1 >/dev/null\` in the workspace's directory), then run \`cargo xtask deps\` and \`cargo xtask layers\` and fix anything they report."
}

# xtask could not be used for the reason in $1: run the interim script and stop.
fallback() {
  if [ ! -f "$root/scripts/check-deps.sh" ]; then
    fail "$1
deps-guard: scripts/check-deps.sh is missing too, so the dependency graph was not checked."
  fi
  if ! cout=$(sh "$root/scripts/check-deps.sh" "$root" 2>&1); then
    violation "$1
$(unquoted "$cout")"
  fi
  fail "$1
deps-guard: only scripts/check-deps.sh ran. It passed, but it does not look for path packages outside the repository or check the layer matrix, so the check is incomplete. Make xtask build, then run \`cargo xtask deps\` and \`cargo xtask layers\`."
}

# 1. Configuration rustup and cargo would read when run here. Both awk programs share this part:
#    `bad` reports a line as "  <file>:<line>: <shown>", and `track` follows multi-line strings
#    and arrays, whose lines are never shown. One UTF-8 byte-order mark at the start is skipped.
bom=$(printf '\357\273\277')
common_awk='
  function count(s, t,   n, i) {
    n = 0
    while ((i = index(s, t)) > 0) { n++; s = substr(s, i + length(t)) }
    return n
  }
  # The first bare part of a table header, or a plain key; never a value.
  function shown(l,   e, two, seg, k) {
    if (cont) return "..."
    if (substr(l, 1, 1) == "[") {
      two = (substr(l, 1, 2) == "[[")
      seg = substr(l, 2 + two)
      sub(/^[[:space:]]+/, "", seg)
      if (!match(seg, /^[A-Za-z0-9_-]+/)) return two ? "[[...]]" : "[...]"
      k = substr(seg, 1, RLENGTH)
      seg = substr(seg, RLENGTH + 1)
      sub(/^[[:space:]]+/, "", seg)
      if (substr(seg, 1, 1) != "]") k = k "..."
      return two ? "[[" k "]]" : "[" k "]"
    }
    e = index(l, "=")
    if (!e) return "..."
    k = substr(l, 1, e - 1)
    sub(/[[:space:]]+$/, "", k)
    if (k !~ /^[A-Za-z0-9_-]+([[:space:]]*\.[[:space:]]*[A-Za-z0-9_-]+)*$/) return "..."
    return k " = ..."
  }
  function bad(l) { print "  " f ":" NR ": " shown(l) }
  # Whether the lines after l continue a multi-line string or array.
  function track(l,   e, s) {
    if (mls != "") {
      e = index(l, mls)
      if (!e) return
      l = substr(l, e + 3)
      mls = ""
    } else if (!depth) {
      if (substr(l, 1, 1) == "[") {
        e = index(l, substr(l, 1, 2) == "[[" ? "]]" : "]")
        if (!e) return
        l = substr(l, e + 1)
        sub(/^[[:space:]]*#.*/, "", l)
      }
      e = index(l, "=")
      if (!e) return
      l = substr(l, e + 1)
    }
    if (count(l, dq3) % 2) { mls = dq3; return }
    if (count(l, sq3) % 2) { mls = sq3; return }
    s = l
    gsub(/"([^"\\]|\\.)*"/, "", s)
    gsub(sq "[^" sq "]*" sq, "", s)
    sub(/#.*/, "", s)
    depth += count(s, "[") - count(s, "]")
    if (depth < 0) depth = 0
  }
  BEGIN { dq3 = "\"\"\""; sq3 = sq sq sq }
  NR == 1 && index($0, bom) == 1 { $0 = substr($0, length(bom) + 1) }
  { line = $0; sub(/^[[:space:]]+/, "", line); cont = (mls != "" || depth > 0) }
  END { if (mls != "" || depth > 0) print "  " f ": a value that is never closed" }
'
# A toolchain file: [toolchain] with a plain channel and profile, and components and targets
# arrays; a legacy file may hold just a toolchain name on one line.
toolchain_awk=$common_awk'
  cont { track(line); next }
  line == "" || substr(line, 1, 1) == "#" { next }
  substr(line, 1, 1) == "[" {
    t = line; sub(/#.*/, "", t); gsub("[[:space:]\"" sq "]", "", t)
    if (t != "[toolchain]") bad(line)
    seen = 1
    track(line)
    next
  }
  !index(line, "=") {
    if (seen || !legacy || line !~ /^[A-Za-z0-9._-]+[[:space:]]*$/) bad(line)
    seen = 1
    next
  }
  {
    e = index(line, "="); k = substr(line, 1, e - 1); v = substr(line, e + 1)
    gsub("[[:space:]\"" sq "]", "", k); sub(/^toolchain\./, "", k); sub(/^[[:space:]]+/, "", v)
    if (k == "channel" || k == "profile") {
      if (v !~ ("^[\"" sq "][A-Za-z0-9._-]+[\"" sq "][[:space:]]*(#.*)?$")) bad(line)
    } else if (k == "components" || k == "targets") {
      if (substr(v, 1, 1) != "[") bad(line)
    } else bad(line)
    seen = 1
    track(line)
  }
'
# A cargo configuration file: only [alias]. Its values are left to `xtask deps`.
config_awk=$common_awk'
  cont { if (!ok) bad(line); track(line); next }
  line == "" || substr(line, 1, 1) == "#" { next }
  substr(line, 1, 1) == "[" {
    t = line; sub(/#.*/, "", t); gsub(/[[:space:]]/, "", t)
    ok = (t == "[alias]")
    if (!ok) bad(line)
    track(line)
    next
  }
  { if (!ok) bad(line); track(line) }
'
conf=
d=$root
while :; do
  # rustup reads the legacy name first; both are checked.
  for f in "$d/rust-toolchain" "$d/rust-toolchain.toml" "$d/.cargo/config" "$d/.cargo/config.toml"; do
    [ -f "$f" ] || continue
    case "$f" in
      */rust-toolchain) prog=$toolchain_awk legacy=1 ;;
      */rust-toolchain.toml) prog=$toolchain_awk legacy=0 ;;
      *) prog=$config_awk legacy=0 ;;
    esac
    if ! found=$(awk -v f="$f" -v bom="$bom" -v sq="'" -v legacy="$legacy" "$prog" "$f" 2>&1); then
      fail "deps-guard: could not read $f, so cargo was not run and nothing was checked."
    fi
    conf=$conf$found
  done
  [ "$d" = "$proj" ] && break
  case "$d" in
    "$proj"/*) d=${d%/*} ;;
    *) break ;;
  esac
done
if [ -n "$conf" ]; then
  fail "deps-guard: in this repository, cargo configuration may hold only [alias] tables, and a rustup toolchain file only [toolchain] with channel, components, targets and profile (docs/04-quality.md -> \`xtask\` checks). These lines break that, so cargo was not run and nothing was checked (values are not shown):
$conf" "Remove them. If the task needs such a setting, stop and ask the user."
fi

if ! command -v cargo >/dev/null 2>&1; then
  fail "deps-guard: cargo not found on PATH; the dependency graph was not checked."
fi
if ! cv=$(cargo --version 2>&1); then
  fail "$(unquoted "$cv")
deps-guard: cargo could not start here, so nothing was checked. If the toolchain that rust-toolchain.toml names is not installed, install it (\`rustup toolchain install\` in $root): the hook does not install toolchains."
fi

# Problems with what the manifests declare, from `cargo metadata --no-deps` ($1): registry or git
# dependencies, and path dependencies or packages outside the checkout.
declared_problems() {
  printf '%s' "$1" | grep -o '"source":"[^"]*"' | sort -u | sed 's/^/  declared from a registry or git: /; s#://[^/@"]*@#://...@#g'
  printf '%s' "$1" | grep -oE '"(path|manifest_path)":"[^"]*"' | sort -u |
    while IFS= read -r kv; do
      p=${kv#*:\"}
      p=${p%\"}
      case "$kv" in
        '"manifest_path"'*) p=$(dirname -- "$p") ;;
      esac
      case "$p" in
        *'\'*) printf '  not comparable (escaped): %s\n' "$p"; continue ;;
      esac
      r=$(cd "$p" 2>/dev/null && pwd -P) || r=
      case "$r" in
        "$root" | "$root"/*) ;;
        '') printf '  path not found: %s\n' "$p" ;;
        *) printf '  outside %s: %s\n' "$root" "$p" ;;
      esac
    done
}

# What the root workspace declares, from `cargo metadata --no-deps`; empty if cargo cannot read it.
nd=$(cargo metadata --format-version 1 --offline --locked --no-deps --manifest-path "$root/Cargo.toml" 2>/dev/null) || nd=

# Checks the declarations when the graph cannot be resolved ($1: cargo's error).
check_declared() {
  [ -n "$nd" ] || return 1
  bad=$(declared_problems "$nd")
  if [ -n "$bad" ]; then
    violation "$1
deps-guard: the root workspace's manifests declare dependencies the policy does not allow:
$bad"
  fi
  return 0
}

# 2. Resolve the graph.
if ! meta=$(cargo metadata --format-version 1 --offline --locked --manifest-path "$root/Cargo.toml" 2>&1); then
  meta=$(unquoted "$meta")
  case "$meta" in
    *'--locked was passed to prevent this'*)
      if check_declared "$meta"; then
        if grep -Eq "^[[:space:]]*[\"']?source[\"']?[[:space:]]*=" "$root/Cargo.lock" 2>/dev/null; then
          violation "$meta
deps-guard: $root/Cargo.lock names a source for a package, so it holds a package from a registry or git."
        fi
        stale "$meta" "The root workspace's manifests were checked and declare no registry, git or outside-the-repository dependency, and its Cargo.lock names no package source; the rest of the graph was not checked."
      fi
      fail "$meta
deps-guard: cargo could not read the manifests either, so the dependency graph was not checked."
      ;;
  esac
  check_declared "$meta"
  fallback "deps-guard: cargo could not resolve the workspace in $root offline:
$meta"
fi

# 3. Check what building xtask would compile, then build it.
tdir="$root/target/deps-guard"
members=$(printf '%s' "$nd" | grep -o '"manifest_path":"[^"]*"' | sed 's/^"manifest_path":"//; s/"$//; s#/[^/]*$##')
# Per member directory: "B <dir>" for a build script or proc-macro target, "S <dir>" for a target
# whose source file is not inside the directory. Cargo writes a package's targets before its
# manifest_path, so they are in the text just before that key.
if ! targets=$(printf '%s' "$nd" | awk '{
  n = split($0, part, /"manifest_path":"/)
  for (i = 2; i <= n; i++) {
    p = part[i]; sub(/".*/, "", p); sub(/\/[^\/]*$/, "", p)
    t = part[i - 1]
    if (t ~ /"kind":\["(custom-build|proc-macro)"\]/) print "B " p
    while (match(t, /"src_path":"[^"]*"/)) {
      s = substr(t, RSTART + 12, RLENGTH - 13)
      t = substr(t, RSTART + RLENGTH)
      if (index(s, p "/") != 1 || s ~ /\/\.\.?\//) { print "S " p; break }
    }
  }
}' 2>&1); then
  fallback "deps-guard: could not read the targets of the root workspace's members, so xtask was not built."
fi
tree=$(cargo tree --package xtask --edges normal,build --prefix none --offline --locked --manifest-path "$root/Cargo.toml" 2>/dev/null) || tree=
bad=$(printf '%s\n' "$tree" | while IFS= read -r line; do
  line=${line% (\*)}
  case "$line" in
    '' | '['*']') continue ;;
    *' (proc-macro)' | *' (proc-macro) ('*) printf '  %s: a proc macro, which runs code as it compiles\n' "$line"; continue ;;
  esac
  p=$(printf '%s\n' "$line" | sed -n 's/^[^ ][^ ]* v[^ ]* (\(\/.*\))$/\1/p')
  case "$p" in
    '') printf '  %s: not a path package\n' "$line"; continue ;;
    "$root/target" | "$root/target/"*) printf '  %s: in the build directory\n' "$line"; continue ;;
    "$root/"*) ;;
    *) printf '  %s: outside %s\n' "$line" "$root"; continue ;;
  esac
  if [ "$(cd "$p" 2>/dev/null && pwd -P)" != "$p" ]; then
    printf '  %s: reached through a symbolic link\n' "$line"
    continue
  fi
  if ! printf '%s\n' "$members" | grep -qxF -- "$p"; then
    printf '  %s: not a member of the root workspace\n' "$line"
    continue
  fi
  if printf '%s\n' "$targets" | grep -qxF -- "B $p"; then
    printf '  %s: has a build script or proc-macro target, which would run as xtask compiles\n' "$line"
  fi
  if printf '%s\n' "$targets" | grep -qxF -- "S $p"; then
    printf '  %s: a target'\''s source file lies outside its directory\n' "$line"
  fi
  l=$(find "$p" -type l -print 2>/dev/null | head -n 1)
  [ -z "$l" ] || printf '  %s: holds a symbolic link, %s\n' "$line" "$l"
done)
[ -n "$tree" ] || bad="  (cargo tree listed nothing)"
for t in "$root/target" "$tdir"; do
  if [ -L "$t" ]; then
    bad="$bad${bad:+
}  $t is a symbolic link, so the build would leave this checkout"
  fi
done
if [ -d "$tdir" ] && [ ! -L "$tdir" ]; then
  l=$(find "$tdir" -type l -print 2>/dev/null | head -n 1)
  [ -z "$l" ] || bad="$bad${bad:+
}  $l is a symbolic link, so another build could stand in for this one (remove $tdir)"
fi
if [ -n "$bad" ]; then
  fallback "deps-guard: xtask may be built only from members of this checkout's root workspace, each in its own directory, with no symbolic links and no build script or proc macro, so it was not built:
$bad"
fi
dirs=$(printf '%s\n' "$tree" | sed -n 's/ (\*)$//; s/^[^ ][^ ]* v[^ ]* (\(\/.*\))$/\1/p' | sort -u)
stamp=$({
  printf '%s\n' "$dirs" | while IFS= read -r p; do
    find "$p" -type f -exec cksum {} +
  done
  cksum "$root/Cargo.toml" "$root/Cargo.lock"
} 2>&1 | LC_ALL=C sort | cksum)
if [ "$(cat "$tdir/source.stamp" 2>/dev/null)" != "$stamp" ]; then
  rm -rf "$tdir/xtask"
fi
rm -f "$tdir/xtask/xtask" "$tdir/xtask/xtask.exe"
if ! berr=$(CARGO_BUILD_BUILD_DIR=$tdir cargo build --profile xtask --package xtask --locked --offline --quiet --target-dir "$tdir" --manifest-path "$root/Cargo.toml" 2>&1); then
  fallback "deps-guard: xtask did not build:
$(printf '%s\n' "$berr" | tail -n 20)"
fi
printf '%s\n' "$stamp" > "$tdir/source.stamp" 2>/dev/null
bin="$tdir/xtask/xtask"
if [ -f "$bin.exe" ]; then
  bin="$bin.exe"
fi
if [ ! -x "$bin" ]; then
  fallback "deps-guard: xtask built, but its binary is not at $bin (a build.target setting in cargo's configuration, or CARGO_BUILD_TARGET, moves it)."
fi

# 4. Run the checks.
out=$("$bin" deps --root "$root" 2>&1)
status=$?
last=$(printf '%s\n' "$out" | tail -n 1)
case "$status:$last" in
  '0:deps: ok') ;;
  '1:deps: FAILED ('*'; only out-of-date lockfiles)') stale "$out" "xtask deps found nothing else wrong, so this alone is not a policy violation." ;;
  '1:deps: FAILED'*) violation "$out" ;;
  *) fallback "deps-guard: xtask deps gave no verdict (exit $status):
$out" ;;
esac
out=$("$bin" layers --root "$root" 2>&1)
status=$?
last=$(printf '%s\n' "$out" | tail -n 1)
case "$status:$last" in
  '0:layers: ok') exit 0 ;;
  '1:layers: FAILED'*) fail "$out" "$matrix" ;;
esac
fail "deps-guard: xtask deps passed, but xtask layers gave no verdict (exit $status), so the layer matrix was not checked:
$out"
