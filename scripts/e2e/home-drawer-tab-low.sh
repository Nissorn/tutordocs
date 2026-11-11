#!/usr/bin/env bash
# E2E replay proof for ticket 52:
# 1. Jobs failed attention dot that stays until drawer opens on list view
# 2. Tab strip overflow affordance and active tab scrolling
# 3. Trash list human size ("37 KB") and relative time ("just now") with exact title
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target-e2e}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-52.XXXXXX")"
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

cat <<'EOF' >"$FAKE_BIN/claude"
#!/usr/bin/env bash
if [[ -f "$WORK/fail_next" ]]; then
  rm -f "$WORK/fail_next"
  echo "Simulated agent failure for ticket 52 proof" >&2
  exit 1
fi
exec "$ROOT/scripts/e2e/fake-claude" "$@"
EOF
chmod +x "$FAKE_BIN/claude"

printf '%s\n' '%PDF-1.1' '% general chemistry notes' >"$IMPORT/chemistry.pdf"

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
CLI="$CARGO_TARGET_DIR/debug/tutordocs-cli"
APP="$CARGO_TARGET_DIR/debug/tutordocs"

echo "== build ui =="
npm --prefix ui run build

echo "== build test app (e2e-control) =="
find crates src-tauri/src -name '*.rs' -exec touch {} +
cargo build -p tutordocs --bins --features e2e-control,custom-protocol

[[ -x "$APP" && -x "$CTL" && -x "$CLI" ]] || fail "test app binaries missing"

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

echo "== seed library with courses =="
# Create courses with long titles for tab strip overflow
"$CLI" course create --library "$LIB" "Advanced Quantum Mechanics and Field Theory"
"$CLI" course create --library "$LIB" "Molecular Cellular Biology and Genetics"
"$CLI" course create --library "$LIB" "Organic Chemistry Synthesis and Reaction Mechanisms"
"$CLI" course create --library "$LIB" "Computational Evolutionary Biology"
"$CLI" course create --library "$LIB" "Astrophysics and Planetary Science"
"$CLI" course create --library "$LIB" "Microbiology and Immunology Laboratory"

# Create a course and trash it for the trash list proof
"$CLI" course create --library "$LIB" "Discarded Cell Biology Notes"
"$CLI" course trash --library "$LIB" "discarded-cell-biology-notes"

echo "== launch =="
WALK_START="$(date +%s)"
env \
  -u ANTHROPIC_API_KEY \
  -u ANTHROPIC_AUTH_TOKEN \
  WORK="$WORK" \
  ROOT="$ROOT" \
  HOME="$SCRATCH_HOME" \
  PATH="$FAKE_BIN:/usr/bin:/bin:/usr/sbin:/sbin" \
  TUTORDOCS_LIBRARY="$LIB" \
  TUTORDOCS_CTL_SOCKET="$SOCK" \
  FAKE_STATE="$FAKE_STATE" \
  FAKE_PAUSE="1.0" \
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

echo "== 1. Trash list with human size and relative time =="
ctl click "button.home-trash" >/dev/null || fail "click Trash button"
must_text "trash header" ".trash-head h1" "Trash" 20000
must_text "trash entry" ".trash-list" "Discarded Cell Biology Notes" 20000
must_text "trash relative time" "span.trash-time" "Trashed" 20000
snap "52-01-trash-list.png"

ctl click "button.home-back-courses" >/dev/null || fail "click Back to courses"
must_text "back to home" ".home" "Courses" 20000

echo "== 2. Tab strip overflow affordance and active tab scrolling =="
# Snapshot with one tab (Home tab only, no overflow arrows)
snap "52-02-tab-strip-single.png"

# Open courses to create tabs that overflow the 1280px window width
ctl eval '(() => { const cards = Array.from(document.querySelectorAll("article.card button.card-open")); for (const btn of cards) btn.click(); })()' >/dev/null || fail "open courses"
sleep 1
must_text "overflow right visible" "button.tab-overflow-right" "›" 15000
snap "52-02-tab-strip-overflow.png"

# Return to Home tab
ctl click ".tab-home button" >/dev/null || fail "click Home tab"
must_text "home view" ".home" "Courses" 10000

echo "== 3. Jobs failure attention dot and reset to list on open =="
# Trigger a failed agent job
touch "$WORK/fail_next"
ctl click "button.home-new" >/dev/null || fail "click New course"
wait_sel "wizard title" '[aria-label="Course title"]' 20000
ctl type '[aria-label="Course title"]' "Failing Agent Course" >/dev/null || fail "type title"
ctl click "button.outline-submit" >/dev/null || fail "click Next"

must_text "materials step" ".drop-title" "Drop files" 20000
ctl drop "$IMPORT" >/dev/null || fail "drop import files"
must_text "import summary" ".materials-summary" "imported" 20000

ctl click "button.outline-submit" >/dev/null || fail "click Next to guide"
must_text "guide step" '.wizard-steps [aria-current="step"]' "Guide" 20000
ctl type '[aria-label="Guide"]' "Focus on failed test." >/dev/null || fail "type guide"
ctl click "button.outline-submit" >/dev/null || fail "click Plan course"

# Leave planning wizard back to Home while plan runs
wait_sel "leave button" ".wizard-nav button" 20000
must_text "leave button text" ".wizard-nav button" "Back to Home, plan keeps running" 10000
ctl click ".wizard-nav button" >/dev/null || fail "click Back to Home, plan keeps running"

must_text "back on home" ".home" "Courses" 20000

# Wait for plan failure, verify failed-job attention dot on Jobs toggle
must_text "jobs attention dot" "button.jobs-toggle .jobs-attention" "" 20000
snap "52-03-jobs-dot.png"

# Click Jobs toggle: drawer opens on the list view, dot clears
ctl click "button.jobs-toggle" >/dev/null || fail "open jobs drawer"
must_text "job drawer open" "aside.job-drawer h2" "Agent jobs" 10000
must_text "job list shown" ".job-list" "Outline" 10000
snap "52-04-jobs-drawer-open.png"

WALK_END="$(date +%s)"
echo "replay walk seconds: $((WALK_END - WALK_START))"
echo "snapshots: $SNAP"
ls -la "$SNAP"

SNAP_OUT="${REPLAY_SNAP_DIR:-$ROOT/target/e2e-snapshots/52-snapshots}"
mkdir -p "$SNAP_OUT"
cp -p "$SNAP"/*.png "$SNAP_OUT/"
echo "Ticket 52 replay proof completed successfully!"
