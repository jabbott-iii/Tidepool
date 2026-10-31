#!/bin/sh
# Claude Code PostToolUse hook (Edit|Write).
# When an edit touches a Cargo manifest or lockfile, re-run the dependency-policy check and
# feed any violation back to Claude (exit 2). Other edits exit 0 immediately.
# Policy: docs/DEPENDENCIES.md
input=$(cat)

case "$input" in
  *Cargo.toml*|*Cargo.lock*) ;;
  *) exit 0 ;;
esac

cd "${CLAUDE_PROJECT_DIR:-.}" || exit 0

if [ ! -f scripts/check-deps.sh ]; then
  echo "deps-guard: scripts/check-deps.sh not found in $(pwd); dependency check not run." >&2
  exit 1   # non-blocking notice: misconfiguration, not a policy violation
fi

if ! out=$(sh scripts/check-deps.sh 2>&1); then
  printf '%s\n' "$out" >&2
  printf '%s\n' "Tidepool dependency policy violation (docs/DEPENDENCIES.md): only Rust std and in-repo path crates are allowed. Revert the dependency, or stop and propose an ADR for owner approval." >&2
  exit 2
fi
exit 0
