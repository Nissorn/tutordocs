#!/usr/bin/env bash
# The one check command (docs/agents/worktrees.md). A ticket is not done while
# this is red. Runs formatting, clippy, Rust tests, the TypeScript typecheck,
# the CSS ownership check, the contrast check, the UI seam tests and the prose
# guardrail.
set -euo pipefail
cd "$(dirname "$0")/.."

# Rust is installed but off the interactive PATH on this machine.
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi
# Each checkout builds into its own target dir (docs/agents/worktrees.md).
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$(git rev-parse --show-toplevel)/target}"
# A target dir cloned or shared from another checkout can hold workspace-crate
# artifacts newer than this checkout's sources, and cargo then links them as
# fresh. Touching our own sources forces those crates to rebuild; dependencies
# stay cached.
find crates src-tauri/src -name '*.rs' -exec touch {} +

echo "== cargo fmt =="
cargo fmt --all -- --check

echo "== cargo clippy =="
cargo clippy --workspace --all-targets -- -D warnings

echo "== cargo test =="
cargo test --workspace

echo "== TypeScript typecheck =="
npm --prefix ui run typecheck

echo "== CSS owners =="
python3 scripts/test-check-css-owners.py
python3 scripts/check-css-owners.py

echo "== contrast =="
python3 scripts/test-check-contrast.py
python3 scripts/check-contrast.py

echo "== UI seam tests =="
npm --prefix ui run test

echo "== prose guardrail =="
python3 scripts/check-prose.py

echo "check: all green"