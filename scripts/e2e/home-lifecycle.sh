#!/usr/bin/env bash
# E2E replay proof for ticket 40: Home course lifecycle, planning state, trash, and rename.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target-e2e}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-40.XXXXXX")"
LIB="$WORK/library"
IMPORT="$WORK/import"
SNAP="$WORK/snapshots"
FAKE_STATE="$WORK/fake-state"
SCRATCH_HOME="$WORK/home"
FAKE_BIN="$WORK/bin"
SOCK="$WORK/ctl.sock"
APP_LOG="$WORK/app.log"
APP_PID=""
WALK_START=0

mkdir -p "$LIB/courses" "$LIB/.tutordocs" "$IMPORT" "$SNAP" "$FAKE_STATE" "$SCRATCH_HOME" "$FAKE_BIN"
printf '%s\n' '{"driver":"claude"}' >"$LIB/.tutordocs/agent.json"
cp "$ROOT/scripts/e2e/fake-claude" "$FAKE_BIN/claude"
chmod +x "$FAKE_BIN/claude"

printf '%s\n' '%PDF-1.1' '% biochemistry notes' >"$IMPORT/biochem.pdf"
printf '%s\n' 'Proteins fold into functional three-dimensional conformations.' >"$IMPORT/notes.md"

echo "work dir: $WORK"
echo "snapshots: $SNAP"

cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$APP_PID" ]]; then
    local pids pid
    pids="$(tree_pids "$APP_PID")"
    for pid in $pids; do
      kill "$pid" 2>/dev/null || true
    done
    local i
    for i in 1 2 3 4 5; do
      local alive=0
      for pid in $pids; do
        if kill -0 "$pid" 2>/dev/null; then
          alive=1
        fi
      done
      if [[ "$alive" -eq 0 ]]; then
        break
      fi
      sleep 1
    done
    for pid in $pids; do
      if kill -0 "$pid" 2>/dev/null; then
        kill -9 "$pid" 2>/dev/null || true
      fi
    done
  fi
  if [[ "$status" -ne 0 ]]; then
    echo "replay failed; work dir left at $WORK" >&2
  fi
  exit "$status"
}

tree_pids() {
  local pid="$1"
  echo "$pid"
  local child
  for child in $(pgrep -P "$pid" 2>/dev/null || true); do
    tree_pids "$child"
  done
}

trap cleanup EXIT

dump() {
  echo "---- app log ----" >&2
  tail -n 100 "$APP_LOG" >&2 || true
  echo "---- fake calls ----" >&2
  cat "$FAKE_STATE/calls.log" >&2 || true
  echo "---- page ----" >&2
  "$CTL" --socket "$SOCK" text body >&2 || true
}

fail() {
  echo "FAIL: $*" >&2
  dump
  exit 1
}

CTL="$CARGO_TARGET_DIR/debug/tutordocs-ctl"
APP="$CARGO_TARGET_DIR/debug/tutordocs"

echo "== build ui =="
npm --prefix ui run build

echo "== build test app (e2e-control) =="
find crates src-tauri/src -name '*.rs' -exec touch {} +
cargo build -p tutordocs --bins --features e2e-control,custom-protocol

[[ -x "$APP" && -x "$CTL" ]] || fail "test app binaries missing"

ctl() {
  "$CTL" --socket "$SOCK" "$@"
}

wait_text() {
  local selector="$1" needle="$2" timeout_ms="$3"
  local start now got=""
  start="$(date +%s)"
  while true; do
    if got="$(ctl text "$selector" 2>/dev/null)"; then
      case "$got" in
        *"$needle"*)
          printf '%s\n' "$got"
          return 0
          ;;
      esac
    fi
    now="$(date +%s)"
    if ((now * 1000 > start * 1000 + timeout_ms)); then
      echo "timed out waiting for [$needle] in $selector; last=[$got]" >&2
      return 1
    fi
    sleep 0.4
  done
}

must_text() {
  local label="$1" selector="$2" needle="$3" timeout_ms="$4"
  echo "== $label =="
  wait_text "$selector" "$needle" "$timeout_ms" >/dev/null || fail "$label"
}

wait_sel() {
  local label="$1" selector="$2" timeout_ms="$3"
  echo "== $label =="
  wait_text "$selector" "" "$timeout_ms" >/dev/null || fail "$label"
}

snap() {
  local name="$1"
  local path="$SNAP/$name"
  ctl snapshot "$path" >/dev/null || fail "snapshot $name"
  echo "snapshot $name saved to $path"
}

echo "== launch =="
WALK_START="$(date +%s)"
env \
  -u ANTHROPIC_API_KEY \
  -u ANTHROPIC_AUTH_TOKEN \
  HOME="$SCRATCH_HOME" \
  PATH="$FAKE_BIN:/usr/bin:/bin:/usr/sbin:/sbin" \
  TUTORDOCS_LIBRARY="$LIB" \
  TUTORDOCS_CTL_SOCKET="$SOCK" \
  FAKE_STATE="$FAKE_STATE" \
  FAKE_PAUSE="4.0" \
  "$APP" >"$APP_LOG" 2>&1 &
APP_PID=$!
echo "app pid: $APP_PID"

for _ in $(seq 1 60); do
  if [[ -S "$SOCK" ]]; then
    break
  fi
  if ! kill -0 "$APP_PID" 2>/dev/null; then
    fail "app exited before the socket appeared"
  fi
  sleep 0.5
done
[[ -S "$SOCK" ]] || fail "control socket did not appear"

# Ticket 68: REPLAY_APPEARANCE=light|dark pins the window's macOS appearance for a
# whole replay, and REPLAY_SNAP_DIR collects the snapshots somewhere other than the
# ticket that first wrote them.
if [[ -n "${REPLAY_APPEARANCE:-}" ]]; then
  ctl appearance "$REPLAY_APPEARANCE" >/dev/null || fail "appearance $REPLAY_APPEARANCE"
fi

must_text "home initial" "button.home-new" "New course" 30000
snap "40-01-home-initial.png"

echo "== 1. wizard creation and title rename in goal step =="
ctl click "button.home-new" >/dev/null || fail "click New course"
wait_sel "wizard title" '[aria-label="Course title"]' 20000
ctl type '[aria-label="Course title"]' "Biochemistry" >/dev/null || fail "type title"
ctl click "button.outline-submit" >/dev/null || fail "click Next"
must_text "materials step" ".drop-title" "Drop files" 20000

# Go back to goal step and verify title is editable, rename to Advanced Biochemistry
ctl click ".wizard-nav button.quiet-button" >/dev/null || fail "click Back"
must_text "goal step" '.wizard-steps [aria-current="step"]' "Goal" 20000
ctl type '[aria-label="Course title"]' "Advanced Biochemistry" >/dev/null || fail "rename title"
ctl click "button.outline-submit" >/dev/null || fail "click Next after rename"

must_text "materials step again" ".drop-title" "Drop files" 20000
ctl drop "$IMPORT" >/dev/null || fail "drop import files"
must_text "import summary" ".materials-summary" "imported" 20000

ctl click "button.outline-submit" >/dev/null || fail "click Next to guide"
must_text "guide step" '.wizard-steps [aria-current="step"]' "Guide" 20000
ctl type '[aria-label="Guide"]' "Focus on enzyme kinetics and structure." >/dev/null || fail "type guide"
ctl click "button.outline-submit" >/dev/null || fail "click Plan the course"

echo "== 2. leave planning wizard back to home while running =="
wait_sel "leave button" ".wizard-nav button" 20000
must_text "leave button text" ".wizard-nav button" "Back to Home, plan keeps running" 10000
ctl click ".wizard-nav button" >/dev/null || fail "click Back to Home, plan keeps running"

must_text "home view" ".home" "Courses" 20000
must_text "planning badge" ".card-status-planning" "Planning" 20000
snap "40-02-home-planning.png"

echo "== 3. completion notice appears on home when plan finishes =="
must_text "home completion notice" ".home-notice" "Outline ready for Cell Membranes." 30000
snap "40-03-home-notice.png"
ctl click "button.notice-dismiss" >/dev/null || fail "dismiss completion notice"

echo "== 4. rename course via card menu =="
ctl click "button.card-menu-button" >/dev/null || fail "click card menu button"
must_text "card menu rename" "button.card-action-rename" "Rename" 10000
ctl click "button.card-action-rename" >/dev/null || fail "click Rename"
wait_sel "rename input" "input.card-rename-input" 10000
ctl type "input.card-rename-input" "Cell Biochemistry" >/dev/null || fail "type new title"
ctl click "button.card-rename-save" >/dev/null || fail "click Save rename"
must_text "renamed card" "h2.card-title button" "Cell Biochemistry" 20000
snap "40-04-card-renamed.png"

echo "== 5. create zero-chapter course and discard =="
ctl click "button.home-new" >/dev/null || fail "click New course for zero-chapter shell"
wait_sel "wizard title 2" '[aria-label="Course title"]' 20000
ctl type '[aria-label="Course title"]' "Unfinished Genetics" >/dev/null || fail "type title"
ctl click "button.outline-submit" >/dev/null || fail "click Next to make course shell"
must_text "materials step shell" ".drop-title" "Drop files" 20000
# Go back to Home without planning
ctl click '.tab-home button' >/dev/null || fail "click Home tab"

must_text "zero chapter status" ".card-status-setup" "Setup not finished" 20000
must_text "continue button" "button.card-continue" "Continue" 10000
must_text "discard button" "button.card-discard" "Discard" 10000
snap "40-05-zero-chapter-card.png"

# Discard zero-chapter course
ctl eval '(() => { const cards = [...document.querySelectorAll("article.card")]; const c = cards.find(x => x.textContent.includes("Unfinished Genetics")); c.querySelector("button.card-discard").click(); })()' >/dev/null || fail "click Discard"
wait_sel "confirm discard" "button.confirm-discard" 10000
ctl click "button.confirm-discard" >/dev/null || fail "confirm discard"
sleep 1
snap "40-06-discarded.png"

echo "== 6. trash course via card menu =="
ctl eval '(() => { const cards = [...document.querySelectorAll("article.card")]; const c = cards.find(x => x.textContent.includes("Cell Biochemistry")); c.querySelector("button.card-menu-button").click(); })()' >/dev/null || fail "click card menu"
ctl click "button.card-action-trash" >/dev/null || fail "click Move to trash"
wait_sel "confirm trash" "button.confirm-trash" 10000
ctl click "button.confirm-trash" >/dev/null || fail "confirm trash"
sleep 1
snap "40-07-trashed.png"

echo "== 7. trash view: restore and empty with confirm =="
ctl click "button.home-trash" >/dev/null || fail "click Trash"
must_text "trash header" ".trash-head h1" "Trash" 20000
must_text "trash item biochem" ".trash-list" "Cell Biochemistry" 20000
must_text "trash item genetics" ".trash-list" "Unfinished Genetics" 20000
snap "40-08-trash-view.png"

# Restore Cell Biochemistry
ctl eval '(() => { const items = [...document.querySelectorAll("li.trash-item")]; const item = items.find(x => x.textContent.includes("Cell Biochemistry")); item.querySelector("button.trash-restore").click(); })()' >/dev/null || fail "click restore"
sleep 1

ctl click "button.home-back-courses" >/dev/null || fail "click Back to courses"
must_text "restored on home" "h2.card-title button" "Cell Biochemistry" 20000
snap "40-09-restored.png"

# Go back to trash view and empty Unfinished Genetics
ctl click "button.home-trash" >/dev/null || fail "click Trash again"
must_text "trash list has genetics" ".trash-list" "Unfinished Genetics" 20000
ctl eval '(() => { const items = [...document.querySelectorAll("li.trash-item")]; const item = items.find(x => x.textContent.includes("Unfinished Genetics")); item.querySelector("button.trash-empty").click(); })()' >/dev/null || fail "click Empty"
wait_sel "confirm empty" "button.confirm-empty" 10000
ctl click "button.confirm-empty" >/dev/null || fail "confirm empty"

must_text "trash empty" ".home-empty" "Trash is empty." 20000
snap "40-10-trash-emptied.png"

WALK_END="$(date +%s)"
echo "replay walk seconds: $((WALK_END - WALK_START))"
echo "snapshots: $SNAP"
ls -la "$SNAP"

SNAP_OUT="${REPLAY_SNAP_DIR:-$ROOT/target/e2e-snapshots/40-snapshots}"
mkdir -p "$SNAP_OUT"
cp -p "$SNAP"/*.png "$SNAP_OUT/"
echo "Replay proof completed successfully!"
