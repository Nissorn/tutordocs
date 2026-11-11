#!/usr/bin/env bash
# E2E replay proof for ticket 42: Outline variants and compare UI.
# Flow:
# 1. Create course, plan outline (Draft A)
# 2. Add comment to Draft A
# 3. Branch Draft A -> Draft B
# 4. Edit Draft B and add comment to Draft B
# 5. Revise Draft B with fake claude
# 6. Switch back to Draft A and revise Draft A with fake claude
# 7. Open Compare view to compare Draft A and Draft B side by side
# 8. Approve Draft B from Compare view without regenerating
# 9. Verify course opens with Draft B approved
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target-e2e}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-42.XXXXXX")"
LIB="$WORK/library"
IMPORT="$WORK/import"
SNAP="${REPLAY_SNAP_DIR:-$ROOT/target/e2e-snapshots/42-snapshots}"
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

printf '%s\n' '%PDF-1.1' '% cell membranes lecture' >"$IMPORT/cell-membranes.pdf"
printf '%s\n' 'The lipid bilayer keeps the inside of a cell different from the outside.' >"$IMPORT/study-notes.md"

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
CLI="$CARGO_TARGET_DIR/debug/tutordocs-cli"

echo "== build ui =="
npm --prefix ui run build

echo "== build test app (e2e-control) =="
find crates src-tauri/src -name '*.rs' -exec touch {} +
cargo build -p tutordocs -p tutordocs-cli --bins --features e2e-control,custom-protocol

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

click_button() {
  local name="$1"
  echo "== click button [$name] =="
  local err="" ok=0
  for _ in $(seq 1 30); do
    if err="$(ctl eval "(() => { const b = [...document.querySelectorAll('button')].find(n => (n.textContent || '').trim().startsWith('$name')); if (!b) throw new Error('no button $name'); if (b.disabled) throw new Error('button $name is disabled'); b.click(); return 'clicked'; })()" 2>&1)"; then
      ok=1
      break
    fi
    sleep 0.3
  done
  [[ "$ok" -eq 1 ]] || fail "click button $name ($err)"
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
  FAKE_PAUSE="${FAKE_PAUSE:-0.2}" \
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

echo "== 1. create course and plan outline =="
ctl click "button.home-new" >/dev/null || fail "click New course"
wait_sel "wizard title" '[aria-label="Course title"]' 20000
ctl type '[aria-label="Course title"]' "Cell Membranes" >/dev/null || fail "type title"
ctl click "button.outline-submit" >/dev/null || fail "click Next (Goal)"

must_text "materials step" ".drop-title" "Drop files" 20000
ctl drop "$IMPORT" >/dev/null || fail "drop import files"
must_text "import summary" ".materials-summary" "imported" 20000
ctl click "button.outline-submit" >/dev/null || fail "click Next to guide"

must_text "guide step" '.wizard-steps [aria-current="step"]' "Guide" 20000
ctl type '[aria-label="Guide"]' "Focus on the bilayer as a selective barrier." >/dev/null || fail "type guide"
ctl click "button.outline-submit" >/dev/null || fail "click Plan the course"

must_text "outline review" "section.review h1" "Cell Membranes" 180000
must_text "outline chapters" "input.draft-title" "Membrane structure" 20000
snap "42-01-draft-a-initial.png"

echo "== 2. add comment to Draft A =="
ctl type "textarea.draft-comment" "Mention osmosis in this chapter." >/dev/null || fail "comment Draft A"
click_button "Save"
sleep 0.5

echo "== 3. branch Draft A to Draft B =="
click_button "Branch draft"
wait_sel "branch input" "input.variant-branch-label" 10000
ctl type "input.variant-branch-label" "Draft B" >/dev/null || fail "type branch label"
click_button "Create branch"

must_text "draft b active" ".variant-select" "Draft B" 20000
snap "42-02-draft-b-created.png"

echo "== 4. edit Draft B and add comment =="
ctl type 'li.draft-chapter:nth-child(2) input.draft-title' "Transport across membranes" >/dev/null || fail "rename chapter 2 in Draft B"
ctl type 'li.draft-chapter:nth-child(2) textarea.draft-comment' "Focus on active transport pumps." >/dev/null || fail "comment chapter 2 in Draft B"
click_button "Save"
sleep 0.5
snap "42-03-draft-b-edited.png"

echo "== 5. revise Draft B with fake claude =="
click_button "Revise with my comments"
must_text "draft b revised" ".review-notice" "Revised." 180000
must_text "draft b revised chapter title" "li.draft-chapter:nth-child(2) input.draft-title" "Transport across membranes (revised)" 20000
snap "42-04-draft-b-revised.png"

echo "== 5b. set Draft B current, edit Draft A, save, and assert with tutordocs-cli =="
click_button "Set as current"
must_text "draft b active notice" ".review-notice" "Set as current variant." 20000
must_text "draft b labeled active" ".variant-select" "Draft B (Active)" 20000

# Switch to Draft A
ctl eval "(() => { const s = document.querySelector('.variant-select'); if (!s) throw new Error('no select'); s.value = 'current'; s.dispatchEvent(new Event('change', { bubbles: true })); return 'switched'; })()" >/dev/null || fail "switch to Draft A"
must_text "draft a selected" ".variant-select" "Current" 20000

# Edit Draft A and save
ctl type "li.draft-chapter:first-child input.draft-title" "Membrane structure edited in Draft A" >/dev/null || fail "edit Draft A"
click_button "Save"
sleep 1

# Assert with tutordocs-cli that course.json still holds Draft B and outlines/current.json holds the edit
COURSE_OUTLINE="$("$CLI" --library "$LIB" --course cell-membranes outline show)"
printf '%s\n' "$COURSE_OUTLINE" | grep -q "Transport across membranes (revised)" || fail "course.json does not hold Draft B"
if printf '%s\n' "$COURSE_OUTLINE" | grep -q "Membrane structure edited in Draft A"; then
  fail "course.json was overwritten by Draft A edit (data-loss bug!)"
fi

DRAFT_A_OUTLINE="$("$CLI" --library "$LIB" --course cell-membranes outline variant-show current)"
printf '%s\n' "$DRAFT_A_OUTLINE" | grep -q "Membrane structure edited in Draft A" || fail "outlines/current.json does not hold Draft A edit"

VARIANTS_JSON="$("$CLI" --library "$LIB" --course cell-membranes outline variants)"
printf '%s\n' "$VARIANTS_JSON" | grep -q '"id": *"draft-b"' || fail "variants list missing draft-b"
printf '%s\n' "$VARIANTS_JSON" | grep -q '"id": *"current"' || fail "variants list missing current"

echo "== 6. switch back to Draft A and revise Draft A =="
ctl type "li.draft-chapter:first-child textarea.draft-comment" "Mention osmosis in this chapter." >/dev/null || fail "comment Draft A"
click_button "Revise with my comments"
must_text "draft a revised" ".review-notice" "Revised." 180000
must_text "draft a revised chapter title" "li.draft-chapter:first-child input.draft-title" "Membrane structure edited in Draft A (revised)" 20000
snap "42-05-draft-a-revised.png"

echo "== 7. compare Draft A and Draft B side by side =="
click_button "Compare"
wait_sel "compare view" ".compare-view" 20000
must_text "compare left col" ".compare-column:first-child" "Membrane structure edited in Draft A (revised)" 20000
must_text "compare right col" ".compare-column:nth-child(2)" "Transport across membranes (revised)" 20000
must_text "compare diff badge" ".compare-diff-badge" "Changed" 20000
snap "42-06-compare-view.png"

echo "== 8. approve Draft B without regenerating =="
ctl click '.compare-column[data-variant-id="draft-b"] button.compare-approve' >/dev/null || fail "click Approve Draft B"
must_text "approve choice" ".approve-lead" "How should the chapters be written" 10000
snap "42-07-approve-choice.png"

click_button "Approve outline"
must_text "course reader open" "p.empty-note" "Choose a chapter from the sidebar" 30000
must_text "chapter 2 in sidebar" 'button.chapter[data-chapter-id="passive-transport"]' "Transport across membranes (revised)" 20000
snap "42-08-course-opened.png"

echo "== 9. verify course.json on disk =="
COURSE_JSON="$LIB/courses/cell-membranes/course.json"
[[ -f "$COURSE_JSON" ]] || fail "course.json missing"
grep -q '"outlineStatus": *"approved"' "$COURSE_JSON" || fail "outlineStatus is not approved"
grep -q "Transport across membranes (revised)" "$COURSE_JSON" || fail "course.json does not contain Draft B chapter"

WALK_END="$(date +%s)"
echo "outline-variants replay completed in $((WALK_END - WALK_START)) s"
