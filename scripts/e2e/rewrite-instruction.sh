#!/usr/bin/env bash
# Ticket 67 proof: rewrite a chapter with the learner's own instruction.
# Drives the real app through tutordocs-ctl against the committed fake claude,
# never a real agent. It checks that:
# - the confirm box shows four chips and a note box, in the app font
# - a chip is a single choice that a second click clears
# - the fake agent receives the chip's sentence and the note, in a block that
#   comes after the rules, and the rewritten chapter shows it was used
# - a rewrite with nothing chosen sends no learner request at all
# `ctl appearance` pins the macOS appearance, so the open box is shown in
# light and in dark.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target-e2e}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-67.XXXXXX")"
LIB="$WORK/library"
SNAP="$WORK/snapshots"
SCRATCH_HOME="$WORK/home"
SOCK="$WORK/ctl.sock"
APP_LOG="$WORK/app.log"
FAKE_STATE="$WORK/fake-state"
FAKE_BIN="$WORK/bin"
APP_PID=""
CHAPTER="$LIB/courses/data-center/chapters/power"

mkdir -p "$CHAPTER" "$LIB/courses/data-center/sources" "$LIB/.tutordocs" "$SNAP" "$SCRATCH_HOME" "$FAKE_STATE" "$FAKE_BIN"
printf '%s\n' '{"driver":"claude"}' >"$LIB/.tutordocs/agent.json"
cp "$ROOT/scripts/e2e/fake-claude" "$FAKE_BIN/claude"
chmod +x "$FAKE_BIN/claude"

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

CTL="$CARGO_TARGET_DIR/debug/tutordocs-ctl"
APP="$CARGO_TARGET_DIR/debug/tutordocs"

dump() {
  echo "---- app log ----" >&2
  tail -n 100 "$APP_LOG" >&2 || true
  echo "---- fake claude calls ----" >&2
  cat "$FAKE_STATE/calls.log" >&2 || true
  echo "---- page ----" >&2
  "$CTL" --socket "$SOCK" text body >&2 || true
}

fail() {
  echo "FAIL: $*" >&2
  dump
  exit 1
}

echo "== build ui =="
npm --prefix ui run build

echo "== build test app (e2e-control) =="
find crates src-tauri/src -name '*.rs' -exec touch {} +
cargo build -p tutordocs --bins --features e2e-control,custom-protocol

[[ -x "$APP" && -x "$CTL" ]] || fail "test app binaries missing"

ctl() {
  "$CTL" --socket "$SOCK" "$@"
}

# The value of a JS expression, with the JSON quotes removed.
val() {
  ctl eval "$1" | sed -e 's/^"//' -e 's/"$//'
}

expect_eq() {
  local label="$1" expected="$2" actual="$3"
  [[ "$actual" == "$expected" ]] || fail "$label: expected [$expected], got [$actual]"
  echo "ok: $label = $actual"
}

wait_text() {
  local selector="$1" needle="$2" timeout_ms="${3:-15000}"
  local start now got=""
  start="$(date +%s)"
  while true; do
    if got="$(ctl text "$selector" 2>/dev/null)"; then
      case "$got" in
        *"$needle"*)
          return 0
          ;;
      esac
    fi
    now="$(date +%s)"
    if ((now * 1000 > start * 1000 + timeout_ms)); then
      echo "timed out waiting for [$needle] in $selector; last=[$got]" >&2
      return 1
    fi
    sleep 0.3
  done
}

must_text() {
  local label="$1" selector="$2" needle="$3" timeout_ms="${4:-15000}"
  echo "== wait: $label =="
  wait_text "$selector" "$needle" "$timeout_ms" || fail "$label (needle [$needle] in $selector)"
}

snap() {
  local name="$1"
  local path="$SNAP/$name"
  ctl snapshot "$path" >/dev/null || fail "snapshot $name"
  echo "snapshot $name saved to $path"
}

settle() {
  sleep 0.6
}

# The Nth prompt file the fake claude saved for a chapter job.
chapter_prompt() {
  ls "$FAKE_STATE"/prompt-*-chapter.txt 2>/dev/null | sed -n "${1}p"
}

echo "== seed a course with a written chapter =="
cat >"$LIB/courses/data-center/course.json" <<'JSON'
{
  "id": "data-center",
  "title": "Data Center Power",
  "objectives": ["Trace power from the grid to the server tray"],
  "chapters": [
    {
      "id": "power",
      "title": "Power Distribution",
      "summary": "How power reaches a server rack.",
      "sources": []
    }
  ],
  "outlineStatus": "approved"
}
JSON

cat >"$CHAPTER/index.md" <<'MD'
# Power Distribution

A data center takes power from the utility grid and steps it down in stages until it reaches the server tray. Each stage loses a little energy as heat, so the route matters as much as the equipment.

## Why the route matters

A central UPS converts alternating current to direct current and back again. Every conversion costs a few percent, and the heat it makes has to be cooled by more power.
MD

echo "== launch =="
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
    fail "app exited before socket appeared"
  fi
  sleep 0.3
done
[[ -S "$SOCK" ]] || fail "control socket did not appear"

ctl resize 1280 900 >/dev/null || fail "resize window"
must_text "home card" "button.card-open" "Data Center Power" 30000
ctl click "button.card-open" >/dev/null || fail "open course"
must_text "chapter heading" "main.document h1" "Power Distribution" 20000

echo "== the Rewrite chapter button opens the box =="
expect_eq "no box before the click" "0" "$(val 'document.querySelectorAll(".rewrite-confirm-box").length')"
ctl click "button.chapter-rewrite" >/dev/null || fail "click Rewrite chapter"
ctl wait-for ".rewrite-confirm-box" >/dev/null || fail "confirm box did not open"
expect_eq "four chips" "Explain it more simply|Add more examples|Make it shorter|Go deeper" \
  "$(val 'Array.from(document.querySelectorAll(".rewrite-chip")).map((c) => c.textContent).join("|")')"
expect_eq "note label" "Tell the AI what to change (optional)" "$(val 'document.querySelector(".rewrite-note-label").textContent')"
expect_eq "old prompt kept" "Rewrite this chapter? Rewriting replaces the text." "$(val 'document.querySelector(".rewrite-confirm-prompt").textContent')"
expect_eq "Confirm and Cancel kept" "Confirm rewrite|Cancel" "$(val 'Array.from(document.querySelectorAll(".rewrite-actions button")).map((b) => b.textContent).join("|")')"
expect_eq "no chip chosen yet" "0" "$(val 'document.querySelectorAll(".rewrite-chip[aria-pressed=\"true\"]").length')"

echo "== the box uses the app font =="
APP_FONT="$(val 'getComputedStyle(document.querySelector("button.chapter-rewrite")).fontFamily')"
expect_eq "note box font" "$APP_FONT" "$(val 'getComputedStyle(document.querySelector(".rewrite-note-input")).fontFamily')"
expect_eq "chip font" "$APP_FONT" "$(val 'getComputedStyle(document.querySelector(".rewrite-chip")).fontFamily')"
expect_eq "prompt font" "$APP_FONT" "$(val 'getComputedStyle(document.querySelector(".rewrite-confirm-prompt")).fontFamily')"

echo "== a chip is a single choice and a second click clears it =="
chosen() {
  val 'Array.from(document.querySelectorAll(".rewrite-chip[aria-pressed=\"true\"]")).map((c) => c.dataset.preset).join(",")'
}
ctl click '.rewrite-chip[data-preset="shorter"]' >/dev/null || fail "click Make it shorter"
expect_eq "shorter chosen" "shorter" "$(chosen)"
ctl click '.rewrite-chip[data-preset="shorter"]' >/dev/null || fail "click Make it shorter again"
expect_eq "second click clears it" "" "$(chosen)"
ctl click '.rewrite-chip[data-preset="deeper"]' >/dev/null || fail "click Go deeper"
ctl click '.rewrite-chip[data-preset="simpler"]' >/dev/null || fail "click Explain it more simply"
expect_eq "choosing another replaces the first" "simpler" "$(chosen)"

NOTE="Use a cooking analogy for the power stages."
ctl type ".rewrite-note-input" "$NOTE" >/dev/null || fail "type the note"
expect_eq "note typed" "$NOTE" "$(val 'document.querySelector(".rewrite-note-input").value')"

echo "== the open box, light and dark =="
ctl appearance light >/dev/null || fail "appearance light"
settle
expect_eq "macOS is light" "false" "$(val 'matchMedia("(prefers-color-scheme: dark)").matches')"
snap "01-rewrite-box-light.png"
ctl appearance dark >/dev/null || fail "appearance dark"
settle
expect_eq "macOS is dark" "true" "$(val 'matchMedia("(prefers-color-scheme: dark)").matches')"
snap "02-rewrite-box-dark.png"

echo "== confirm with a chip and a note =="
ctl click "button.rewrite-confirm" >/dev/null || fail "click Confirm rewrite"
must_text "rewritten chapter shows the request" "main.document" "Rewritten as the learner asked" 60000
snap "03-rewritten-with-instruction-dark.png"

FIRST="$(chapter_prompt 1)"
[[ -n "$FIRST" ]] || fail "the fake agent received no chapter job"
SIMPLER_SENTENCE="Rewrite for a reader who found this chapter hard. Use plainer words, shorter sentences and one concrete example per idea, and define each term the first time it appears."
grep -qF "BEGIN LEARNER REQUEST" "$FIRST" || fail "the first prompt has no learner request block"
grep -qF "END LEARNER REQUEST" "$FIRST" || fail "the first prompt has no end marker"
grep -qF "$SIMPLER_SENTENCE" "$FIRST" || fail "the chip's sentence did not reach the agent"
grep -qF "$NOTE" "$FIRST" || fail "the note did not reach the agent"
RULES_END="$(grep -nF -- "- If you cannot write the chapter" "$FIRST" | head -n 1 | cut -d: -f1)"
BLOCK_START="$(grep -nFx "BEGIN LEARNER REQUEST" "$FIRST" | head -n 1 | cut -d: -f1)"
[[ -n "$RULES_END" && -n "$BLOCK_START" && "$BLOCK_START" -gt "$RULES_END" ]] ||
  fail "the request block is not after the rules (rules end $RULES_END, block starts $BLOCK_START)"
echo "ok: the agent received the chip and the note in a block after the rules (lines $RULES_END then $BLOCK_START)"
expect_eq "the text on screen is the rewrite" "true" "$(val 'document.querySelector("main.document").textContent.includes("A cell membrane is a double layer")')"

echo "== a rewrite with nothing chosen sends no request =="
ctl wait-for "button.chapter-rewrite" >/dev/null || fail "Rewrite chapter is back"
ctl click "button.chapter-rewrite" >/dev/null || fail "click Rewrite chapter again"
ctl wait-for ".rewrite-confirm-box" >/dev/null || fail "confirm box did not reopen"
expect_eq "the reopened box is empty" "0" "$(val 'document.querySelectorAll(".rewrite-chip[aria-pressed=\"true\"]").length')"
expect_eq "the reopened note is empty" "" "$(val 'document.querySelector(".rewrite-note-input").value')"
ctl click "button.rewrite-confirm" >/dev/null || fail "click Confirm rewrite, plain"
for _ in $(seq 1 100); do
  [[ -n "$(chapter_prompt 2)" ]] && break
  sleep 0.3
done
SECOND="$(chapter_prompt 2)"
[[ -n "$SECOND" ]] || fail "the plain rewrite never reached the fake agent"
if grep -q "LEARNER REQUEST" "$SECOND"; then
  fail "a plain rewrite carried a learner request"
fi
grep -qF "Rules:" "$SECOND" || fail "the plain prompt lost its rules"
echo "ok: the plain rewrite prompt has no learner request"
echo "== wait: the plain rewrite replaces the text =="
for _ in $(seq 1 200); do
  [[ "$(val 'document.querySelector("main.document").textContent.includes("Rewritten as the learner asked")')" == "false" ]] && break
  sleep 0.3
done
expect_eq "the request line is gone after a plain rewrite" "false" "$(val 'document.querySelector("main.document").textContent.includes("Rewritten as the learner asked")')"
expect_eq "the plain rewrite is on screen" "true" "$(val 'document.querySelector("main.document").textContent.includes("double layer of lipids")')"

echo "== no API key reached the fake agent =="
if grep -o 'anthropic_key=[^ ]*' "$FAKE_STATE/calls.log" | grep -v '^anthropic_key=unset$' | grep -q .; then
  fail "an ANTHROPIC_API_KEY reached the fake agent"
fi
cat "$FAKE_STATE/calls.log"

if [[ -n "${REPLAY_SNAP_DIR:-}" ]]; then
  mkdir -p "$REPLAY_SNAP_DIR"
  cp -p "$SNAP"/*.png "$REPLAY_SNAP_DIR/"
  echo "replay proof complete: snapshots copied to $REPLAY_SNAP_DIR"
else
  mkdir -p "$ROOT/target/e2e-snapshots/67-snapshots"
  cp -p "$SNAP"/*.png "$ROOT/target/e2e-snapshots/67-snapshots/"
  cp -p "$FIRST" "$ROOT/target/e2e-snapshots/67-snapshots/prompt-with-instruction.txt"
  echo "replay proof complete: snapshots copied to target/e2e-snapshots/67-snapshots"
fi
