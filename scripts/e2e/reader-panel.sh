#!/usr/bin/env bash
# Reader and study panel fixes replay proof for ticket 47:
# - Finding 16: Rewrite confirmation and scroll preservation
# - Finding 17: Study panel collapsible sections and non-destructive answer append
# - Finding 26: Outline busy disables action buttons only, not fieldset
# - Finding 27: Five color swatches on note cards for recolor
# - Finding 28: Note cards diffed by id so typing survives mark changes
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target-e2e}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-47.XXXXXX")"
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

echo "== build test app and cli =="
cargo build -p tutordocs --bins --features e2e-control,custom-protocol
cargo build -p tutordocs-cli

[[ -x "$APP" && -x "$CTL" && -x "$CLI" ]] || fail "test app or ctl binaries missing"

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
    sleep 0.3
  done
}

wait_sel() {
  local label="$1" selector="$2" timeout_ms="$3"
  echo "== $label =="
  wait_text "$selector" "" "$timeout_ms" >/dev/null || fail "$label"
}

must_text() {
  local label="$1" selector="$2" needle="$3" timeout_ms="$4"
  echo "== $label =="
  wait_text "$selector" "$needle" "$timeout_ms" >/dev/null || fail "$label"
}

front_app() {
  lsappinfo info -only name "$(lsappinfo front 2>/dev/null)" 2>/dev/null | tr '\n' ' ' || echo unknown
}

pixel_report() {
  local path="$1"
  python3 - "$path" <<'PY'
import struct, sys, zlib
path = sys.argv[1]
data = open(path, "rb").read()
if data[:8] != b"\x89PNG\r\n\x1a\n":
    print(f"not-png bytes={len(data)}")
    sys.exit(0)
pos = 8
width = height = None
ihdr = None
idat = b""
while pos + 8 <= len(data):
    length = struct.unpack(">I", data[pos:pos + 4])[0]
    kind = data[pos + 4:pos + 8]
    chunk = data[pos + 8:pos + 8 + length]
    pos += 12 + length
    if kind == b"IHDR":
        ihdr = chunk
        width, height, bit, color = struct.unpack(">IIBB", chunk[:10])
    elif kind == b"IDAT":
        idat += chunk
    elif kind == b"IEND":
        break
if ihdr is None or bit != 8 or color not in (2, 6):
    print(f"unparsed bytes={len(data)} ihdr={ihdr[:10].hex() if ihdr else None}")
    sys.exit(0)
channels = 3 if color == 2 else 4
raw = zlib.decompress(idat)
rows = []
i = 0
stride = width * channels
prev = bytearray(stride)
for _y in range(height):
    filt = raw[i]
    i += 1
    row = bytearray(raw[i:i + stride])
    i += stride
    if filt == 1:
        for x in range(len(row)):
            left = row[x - channels] if x >= channels else 0
            row[x] = (row[x] + left) & 255
    elif filt == 2:
        for x in range(len(row)):
            row[x] = (row[x] + prev[x]) & 255
    elif filt == 3:
        for x in range(len(row)):
            left = row[x - channels] if x >= channels else 0
            row[x] = (row[x] + ((left + prev[x]) // 2)) & 255
    elif filt == 4:
        def paeth(a, b, c):
            p = a + b - c
            pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
            if pa <= pb and pa <= pc:
                return a
            if pb <= pc:
                return b
            return c
        for x in range(len(row)):
            a = row[x - channels] if x >= channels else 0
            b = prev[x]
            c = prev[x - channels] if x >= channels else 0
            row[x] = (row[x] + paeth(a, b, c)) & 255
    elif filt != 0:
        print(f"unknown-filter {filt} bytes={len(data)}")
        sys.exit(0)
    prev = row
    rows.append(row)
colors = set()
step_x = max(1, width // 24)
step_y = max(1, height // 24)
for y in range(0, height, step_y):
    row = rows[y]
    for x in range(0, width, step_x):
        o = x * channels
        colors.add((row[o] // 16, row[o + 1] // 16, row[o + 2] // 16))
print(f"{width}x{height} bytes={len(data)} colors={len(colors)}")
PY
}

snap() {
  local name="$1"
  local path="$SNAP/$name"
  ctl snapshot "$path" >/dev/null || fail "snapshot $name"
  local info front
  info="$(pixel_report "$path")"
  front="$(front_app)"
  echo "snapshot $name $info front=[$front]"
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

must_text "home" "button.home-new" "New course" 30000

# Create course
ctl click "button.home-new" >/dev/null || fail "click New course"
wait_sel "wizard title" '[aria-label="Course title"]' 20000
ctl type '[aria-label="Course title"]' "Cell Membranes" >/dev/null || fail "type title"
ctl click "button.outline-submit" >/dev/null || fail "click Next"
must_text "materials" ".drop-title" "Drop files" 20000

ctl drop "$IMPORT" >/dev/null || fail "drop import folder"
must_text "import" ".materials-summary" "1 imported" 20000

ctl click "button.outline-submit" >/dev/null || fail "click Next to guide"
must_text "guide step" '.wizard-steps [aria-current="step"]' "Guide" 20000
ctl type '[aria-label="Guide"]' "Focus on the lipid bilayer." >/dev/null || fail "type guide"
ctl click "button.outline-submit" >/dev/null || fail "click Plan the course"

must_text "outline review" "section.review h1" "Cell Membranes" 180000
must_text "outline chapters" "input.draft-title" "Membrane structure" 20000

# Finding 26 verification: fieldset is not disabled
ctl eval '(() => { const f = document.querySelector("fieldset.review-fields"); if (f.disabled) throw new Error("review-fields is disabled"); return "ok"; })()' >/dev/null || fail "review-fields was disabled"

# Approve outline
ctl click "button.outline-submit" >/dev/null || fail "click Approve"
must_text "approve choice" ".approve-lead" "How should the chapters be written" 10000
ctl click "section.approve-panel button.outline-submit" >/dev/null || fail "click Approve outline"

must_text "chapter in sidebar" 'button.chapter[data-chapter-id="membrane-structure"]' "Membrane structure" 20000

# Placeholder chapter label verification
ctl eval '(() => { const b = document.querySelector("button.chapter[data-chapter-id=\"passive-transport\"]"); if (!b) throw new Error("no placeholder chapter button"); if (b.title !== "Writes on open") throw new Error("placeholder title is " + b.title); return "ok"; })()' >/dev/null || fail "placeholder chapter title not Writes on open"

# Open chapter
ctl click 'button.chapter[data-chapter-id="membrane-structure"]' >/dev/null || fail "open chapter"
must_text "chapter text" ".prose" "double layer of lipids" 180000
must_text "study panel" ".study-title" "Study panel" 20000
snap "01-chapter-open.png"

# Finding 17: Collapsible sections in Study Panel
echo "== test collapsible sections =="
ctl eval '(() => { const h = document.querySelector(".study-section[data-section=\"goals\"] .section-head"); if (!h) throw new Error("no goals head"); h.click(); })()' >/dev/null || fail "collapse goals section"
ctl eval '(() => { const s = document.querySelector(".study-section[data-section=\"goals\"]"); if (s.dataset.collapsed !== "true") throw new Error("goals not collapsed"); const b = s.querySelector(".section-body"); if (!b.hidden) throw new Error("goals body not hidden"); return "collapsed"; })()' >/dev/null || fail "goals section not collapsed"
snap "02-section-collapsed.png"

# Expand goals section again
ctl eval '(() => { const h = document.querySelector(".study-section[data-section=\"goals\"] .section-head"); h.click(); })()' >/dev/null || fail "expand goals section"

# Finding 27: Highlight swatches and recolor
echo "== select passage & highlight =="
local_ok=0
for _ in $(seq 1 20); do
  ctl select-text ".prose" "double layer of lipids" >/dev/null 2>&1 || true
  if wait_text 'button[data-color="yellow"]' "" 1500 >/dev/null 2>&1; then
    local_ok=1
    break
  fi
done
[[ "$local_ok" -eq 1 ]] || fail "selection popover"
ctl click 'button[data-color="yellow"]' >/dev/null || fail "highlight yellow"
must_text "note card" ".note-card" "double layer of lipids" 10000

# Recolor via note card swatch
ctl eval '(() => { const c = document.querySelector(".note-card"); const s = c.querySelector("button.swatch[data-color=\"green\"]"); if (!s) throw new Error("no green swatch on note card"); s.click(); })()' >/dev/null || fail "click green swatch"
ctl eval '(() => { const c = document.querySelector(".note-card"); if (c.dataset.color !== "green") throw new Error("card color not green: " + c.dataset.color); const m = document.querySelector("mark.hl"); if (m.dataset.color !== "green") throw new Error("mark color not green: " + m.dataset.color); return "ok"; })()' >/dev/null || fail "recolor failed"
snap "03-note-recolored.png"

# Finding 28: Typing survives another mark change
echo "== note typing survives mark addition =="
ctl type ".note-field" "Important note about lipids." >/dev/null || fail "type note"
for _ in $(seq 1 20); do
  ctl select-text ".prose" "Water-loving heads" >/dev/null 2>&1 || true
  if wait_text 'button[data-color="pink"]' "" 1500 >/dev/null 2>&1; then
    local_ok=1
    break
  fi
done
ctl click 'button[data-color="pink"]' >/dev/null || fail "highlight pink"
ctl eval '(() => { const cards = document.querySelectorAll(".note-card"); if (cards.length < 2) throw new Error("expected at least 2 cards"); const textareas = [...cards].map((c) => c.querySelector("textarea")?.value); if (!textareas.includes("Important note about lipids.")) throw new Error("in-progress typing was lost: " + JSON.stringify(textareas)); return "ok"; })()' >/dev/null || fail "note typing lost"

# Finding 17: Ask question & answer append without full rebuild
echo "== ask whole chapter =="
ctl type '[aria-label="Ask about this chapter"]' "Why are lipid tails oily?" >/dev/null || fail "type question"
ctl click "form.ask-box button" >/dev/null || fail "click Ask"
must_text "answer" ".answer-body" "You asked:" 180000
snap "04-answer-added.png"

# Finding 16: Rewrite confirmation and scroll preservation
echo "== rewrite confirmation =="
ctl eval '(() => { const b = [...document.querySelectorAll("button")].find((n) => n.textContent === "Rewrite chapter"); if (!b) throw new Error("no Rewrite chapter button"); b.click(); })()' >/dev/null || fail "click Rewrite chapter"

must_text "confirm prompt" ".rewrite-confirm-prompt" "Rewrite this chapter?" 10000
snap "05-rewrite-confirm.png"

# Cancel confirmation
ctl click "button.rewrite-cancel" >/dev/null || fail "cancel rewrite"
ctl eval '(() => { const p = document.querySelector(".rewrite-confirm-prompt"); if (p) throw new Error("confirm prompt still present after cancel"); return "cancelled"; })()' >/dev/null || fail "cancel failed"

# Re-engage rewrite with scroll preservation
ctl eval '(() => { const doc = document.querySelector(".document"); doc.scrollTop = 60; return doc.scrollTop; })()' >/dev/null || fail "set scroll"

ctl eval '(() => { const b = [...document.querySelectorAll("button")].find((n) => n.textContent === "Rewrite chapter"); if (!b) throw new Error("no Rewrite chapter button"); b.click(); })()' >/dev/null || fail "click Rewrite chapter again"
must_text "confirm prompt" ".rewrite-confirm-prompt" "Rewrite this chapter?" 10000
ctl click "button.rewrite-confirm" >/dev/null || fail "confirm rewrite"

# Wait for rewrite to complete
must_text "rewritten chapter" ".prose" "double layer of lipids" 180000
ctl eval '(() => { const doc = document.querySelector(".document"); if (doc.scrollTop < 40) throw new Error("scroll position lost: " + doc.scrollTop); return "preserved"; })()' >/dev/null || fail "scroll position lost after rewrite"
snap "06-rewritten-scroll-preserved.png"

WALK_END="$(date +%s)"
echo "replay walk seconds: $((WALK_END - WALK_START))"
echo "snapshots: $SNAP"
ls -la "$SNAP"
echo "== replay succeeded =="
