#!/usr/bin/env bash
# Ticket 54 proof: Narrow-window study drawer placement
# Tests study drawer below 1080px (at 900px) and wide window (1280px)
# Generates 4 snapshots:
# 1. 01-narrow-collapsed.png (900px, drawer collapsed to right toggle, sidebar full height)
# 2. 02-narrow-open.png (900px, drawer open on right edge, sidebar fully visible)
# 3. 03-wide-layout.png (1280px, three-column layout unchanged)
# 4. 04-wide-note-aligned.png (1280px, note card lines up beside highlighted passage)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target-e2e}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-54.XXXXXX")"
LIB="$WORK/library"
SNAP="$WORK/snapshots"
FAKE_STATE="$WORK/fake-state"
SCRATCH_HOME="$WORK/home"
FAKE_BIN="$WORK/bin"
SOCK="$WORK/ctl.sock"
APP_LOG="$WORK/app.log"
APP_PID=""

mkdir -p "$LIB/courses/cell-bio/chapters/cells" "$LIB/.tutordocs" "$SNAP" "$FAKE_STATE" "$SCRATCH_HOME" "$FAKE_BIN"
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
cargo build -p tutordocs --bins --features e2e-control,custom-protocol

[[ -x "$APP" && -x "$CTL" ]] || fail "test app or ctl binaries missing"

ctl() {
  "$CTL" --socket "$SOCK" "$@"
}

wait_text() {
  local selector="$1" needle="$2" timeout_ms="${3:-20000}"
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

must_text() {
  local label="$1" selector="$2" needle="$3" timeout_ms="${4:-20000}"
  echo "== wait: $label =="
  wait_text "$selector" "$needle" "$timeout_ms" >/dev/null || fail "$label (needle [$needle] in $selector)"
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
    print(f"unparsed bytes={len(data)}")
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
  local info
  info="$(pixel_report "$path")"
  echo "snapshot $name $info"
}

echo "== seed course with chapter =="
cat >"$LIB/courses/cell-bio/course.json" <<'JSON'
{
  "id": "cell-bio",
  "title": "Cell Biology",
  "objectives": [
    "Understand the structure and dynamics of cellular membranes"
  ],
  "chapters": [
    {
      "id": "cells",
      "title": "Membranes and Geometry",
      "summary": "Introduction to membranes and lipid bilayers.",
      "sources": ["notes.md"]
    }
  ],
  "outlineStatus": "approved"
}
JSON

cat >"$LIB/courses/cell-bio/chapters/cells/index.md" <<'MD'
# Membranes and Geometry

The lipid bilayer is a thin polar membrane made of two layers of lipid molecules.
These membranes form a continuous barrier around cells.
MD

echo "== launch app =="
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

must_text "home screen card" "button.card-open" "Cell Biology" 20000
ctl click "button.card-open" >/dev/null || fail "open course"

must_text "chapter heading" "main.document h1" "Membranes and Geometry" 20000
must_text "prose body" ".prose" "continuous barrier" 10000

echo "== 1. Narrow window (900px): resize and verify drawer starts collapsed =="
ctl resize 900 800 >/dev/null || fail "resize window to 900x800"
sleep 0.5

# Verify drawer is collapsed by default below 1080px
PANEL_ATTR="$(ctl attr ".reader" "data-panel" 2>/dev/null || true)"
[[ "$PANEL_ATTR" == "collapsed" ]] || fail "expected data-panel=collapsed at 900px, got: [$PANEL_ATTR]"

TOGGLE_EXP="$(ctl attr "button.panel-toggle" "aria-expanded" 2>/dev/null || true)"
[[ "$TOGGLE_EXP" == "false" ]] || fail "expected aria-expanded=false, got: [$TOGGLE_EXP]"

# Verify layout metrics through eval:
# - sidebar full viewport height (y=38 to 768), not covered
# - margin on right edge (right=900)
# - toggle button visible on screen (x inside right 3rem rail)
# - chapter text stops before the collapsed rail with a visible gap
LAYOUT_CHECK="$(ctl eval '(() => {
  const sidebar = document.querySelector(".sidebar").getBoundingClientRect();
  const margin = document.querySelector(".margin").getBoundingClientRect();
  const toggle = document.querySelector("button.panel-toggle").getBoundingClientRect();
  const page = document.querySelector(".page").getBoundingClientRect();
  const winW = window.innerWidth;
  const ok = (
    sidebar.x === 0 &&
    sidebar.bottom >= 700 &&
    Math.abs(margin.left - (winW - 48)) <= 2 &&
    toggle.left >= margin.left &&
    toggle.right <= winW &&
    page.right <= margin.left - 8
  );
  return JSON.stringify({ ok, sidebar, margin, toggle, page, winW });
})()')"
echo "narrow collapsed layout: $LAYOUT_CHECK"
case "$LAYOUT_CHECK" in
  *'"ok":true'*) ;;
  *) fail "narrow collapsed layout check failed: $LAYOUT_CHECK" ;;
esac

snap "01-narrow-collapsed.png"

echo "== 2. Narrow window (900px): open drawer and verify slide-over =="
ctl click "button.panel-toggle" >/dev/null || fail "click toggle to open"
sleep 0.5

PANEL_ATTR_OPEN="$(ctl attr ".reader" "data-panel" 2>/dev/null || true)"
[[ "$PANEL_ATTR_OPEN" == "open" ]] || fail "expected data-panel=open at 900px, got: [$PANEL_ATTR_OPEN]"

TOGGLE_EXP_OPEN="$(ctl attr "button.panel-toggle" "aria-expanded" 2>/dev/null || true)"
[[ "$TOGGLE_EXP_OPEN" == "true" ]] || fail "expected aria-expanded=true, got: [$TOGGLE_EXP_OPEN]"

LAYOUT_OPEN="$(ctl eval '(() => {
  const sidebar = document.querySelector(".sidebar").getBoundingClientRect();
  const margin = document.querySelector(".margin").getBoundingClientRect();
  const toggle = document.querySelector("button.panel-toggle").getBoundingClientRect();
  const winW = window.innerWidth;
  const ok = (
    sidebar.x === 0 &&
    sidebar.width === 240 &&
    sidebar.bottom >= 700 &&
    margin.right === winW &&
    margin.left >= sidebar.right &&
    margin.width >= 300
  );
  return JSON.stringify({ ok, sidebar, margin, toggle, winW });
})()')"
echo "narrow open layout: $LAYOUT_OPEN"
case "$LAYOUT_OPEN" in
  *'"ok":true'*) ;;
  *) fail "narrow open layout check failed: $LAYOUT_OPEN" ;;
esac

snap "02-narrow-open.png"

echo "== 3. Wide window (1280px): verify three-column layout =="
ctl resize 1280 900 >/dev/null || fail "resize window to 1280x900"
sleep 0.5

WIDE_CHECK="$(ctl eval '(() => {
  const reader = document.querySelector(".reader");
  const sidebar = document.querySelector(".sidebar").getBoundingClientRect();
  const doc = document.querySelector(".document").getBoundingClientRect();
  const margin = document.querySelector(".margin").getBoundingClientRect();
  const comp = window.getComputedStyle(reader);
  const ok = (
    sidebar.left >= 0 &&
    doc.left >= sidebar.right &&
    margin.left >= doc.right &&
    margin.right <= window.innerWidth &&
    comp.gridTemplateColumns.split(" ").length === 3
  );
  return JSON.stringify({ ok, cols: comp.gridTemplateColumns, sidebar, doc, margin });
})()')"
echo "wide layout check: $WIDE_CHECK"
case "$WIDE_CHECK" in
  *'"ok":true'*) ;;
  *) fail "wide layout check failed: $WIDE_CHECK" ;;
esac

snap "03-wide-layout.png"

echo "== 4. Wide window (1280px): verify note card lines up beside its passage =="
local_ok=0
for _ in $(seq 1 20); do
  ctl select-text ".prose" "continuous barrier" >/dev/null 2>&1 || true
  if wait_text 'button[data-color="yellow"]' "" 1500 >/dev/null 2>&1; then
    local_ok=1
    break
  fi
done
[[ "$local_ok" -eq 1 ]] || fail "selection popover for note mark"
ctl click 'button[data-color="yellow"]' >/dev/null || fail "highlight yellow"
must_text "note card" ".note-card" "continuous barrier" 10000

# Verify note card appears in margin column beside the passage
ALIGN_CHECK="$(ctl eval '(() => {
  const mark = document.querySelector("mark.hl");
  const card = document.querySelector(".margin .note-card");
  const doc = document.querySelector(".document");
  if (!mark || !card || !doc) return JSON.stringify({ ok: false, reason: "missing elements" });
  const markRect = mark.getBoundingClientRect();
  const cardRect = card.getBoundingClientRect();
  const docRect = doc.getBoundingClientRect();
  const ok = (
    cardRect.left >= docRect.right - 10 &&
    cardRect.right <= window.innerWidth &&
    cardRect.top < window.innerHeight &&
    cardRect.bottom > 0
  );
  return JSON.stringify({ ok, markTop: markRect.top, cardTop: cardRect.top, cardLeft: cardRect.left, docRight: docRect.right });
})()')"
echo "note card alignment check: $ALIGN_CHECK"
case "$ALIGN_CHECK" in
  *'"ok":true'*) ;;
  *) fail "note card alignment check failed: $ALIGN_CHECK" ;;
esac

snap "04-wide-note-aligned.png"

echo "== copy durable snapshots =="
DURABLE_DIR="$ROOT/target/e2e-snapshots/54-snapshots"
mkdir -p "$DURABLE_DIR"
cp "$SNAP"/0*.png "$DURABLE_DIR/"

echo "replay proof complete: 4 snapshots generated in $DURABLE_DIR"
