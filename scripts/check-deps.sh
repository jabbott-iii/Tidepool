#!/bin/sh
# Tidepool dependency-policy check. See docs/DEPENDENCIES.md.
#
# Fails if any Cargo workspace in the repository resolves a package that is not a local path
# package (crates.io, git, or alternate-registry sources). Resolution runs offline and fails
# closed: if Cargo cannot resolve the graph, the check fails.
#
# Test fixtures for this script must be generated in a temporary directory at test time and
# never committed, otherwise the repository itself would fail the check.
#
# Usage: sh scripts/check-deps.sh [repo-root]
set -u
set -f            # no globbing of file names
IFS='
'                 # split lists on newlines only, so paths with spaces work

root=${1:-$(cd "$(dirname "$0")/.." && pwd)}
cd "$root" || { echo "check-deps: cannot enter $root" >&2; exit 1; }

manifests=$(find . -name Cargo.toml -not -path '*/target/*' -not -path './.git/*' | sort)
if [ -z "$manifests" ]; then
  echo "check-deps: no Cargo.toml found; nothing to check."
  exit 0
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "check-deps: FAIL: cargo not found on PATH; cannot verify the dependency graph." >&2
  exit 1
fi

# Workspace roots: manifests that declare [workspace]. If none do, check every manifest.
roots=
for m in $manifests; do
  if grep -q '^[[:space:]]*\[workspace\]' "$m"; then
    roots="$roots$m
"
  fi
done
[ -n "$roots" ] || roots=$manifests

tmpdir=$(mktemp -d) || exit 1
trap 'rm -rf "$tmpdir"' EXIT
trap 'exit 1' HUP INT TERM
status=0

for m in $roots; do
  if ! cargo metadata --format-version 1 --offline --manifest-path "$m" \
      >"$tmpdir/meta.json" 2>"$tmpdir/meta.err"; then
    echo "check-deps: FAIL: could not resolve $m offline (an external dependency may have been added):" >&2
    sed 's/^/  /' "$tmpdir/meta.err" >&2
    status=1
  elif grep -q '"source":"' "$tmpdir/meta.json"; then
    echo "check-deps: FAIL: $m resolves packages from non-path sources:" >&2
    grep -o '"source":"[^"]*"' "$tmpdir/meta.json" | sort -u | sed 's/^/  /' >&2
    status=1
  fi
done

# Lockfiles: name every package entry that carries a source.
for lock in $(find . -name Cargo.lock -not -path '*/target/*' -not -path './.git/*' | sort); do
  bad=$(awk '
    /^\[\[package\]\]/ { name = ""; version = "" }
    /^name = /         { name = $3 }
    /^version = /      { version = $3 }
    /^source = /       { gsub(/"/, "", name); gsub(/"/, "", version); print "  " name " " version " from " $3 }
  ' "$lock")
  if [ -n "$bad" ]; then
    echo "check-deps: FAIL: $lock contains external packages:" >&2
    printf '%s\n' "$bad" >&2
    status=1
  fi
done

if [ "$status" -eq 0 ]; then
  echo "check-deps: OK: only std and in-repo path packages."
fi
exit "$status"
