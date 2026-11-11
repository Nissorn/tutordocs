#!/usr/bin/env bash
# Ticket 51 proof: Control consolidation.
# Tests overflow menu in reader chrome at wide window and below 1080px,
# running Quiz me from both widths, with text assertions and snapshots.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target-e2e}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-51.XXXXXX")"
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

# Pre-touch quiz-failed flag so fake-claude generates quiz on first try
touch "$FAKE_STATE/quiz-failed"

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

echo "== 1. Wide window (1280px): verify standing controls and overflow menu =="
must_text "overflow trigger" "button.overflow-menu-button" "Chapter and course actions" 10000

ctl click "button.overflow-menu-button" >/dev/null || fail "click overflow button wide"
ctl wait-for ".overflow-menu-dropdown:not([hidden])" >/dev/null || fail "dropdown open wide"
must_text "rewrite action" "button.menu-action-rewrite" "Rewrite chapter" 10000
must_text "quiz action" "button.menu-action-quiz" "Quiz me" 10000

snap "01-menu-wide.png"

echo "== 2. Wide window: run Quiz me from overflow menu =="
ctl click "button.menu-action-quiz" >/dev/null || fail "click Quiz me wide"
must_text "quiz page title" "main.document h1" "Quiz: Membranes and Geometry" 25000
must_text "quiz back button" "main.document button.quiet-button" "Back to the chapter" 10000

snap "02-quiz-wide.png"

echo "== 3. Return to chapter view =="
ctl click "main.document button.quiet-button" >/dev/null || fail "click Back to the chapter"
must_text "chapter heading restored" "main.document h1" "Membranes and Geometry" 20000

echo "== 4. Narrow window (< 1080px): resize to 900x800 =="
ctl resize 900 800 >/dev/null || fail "resize window to 900x800"
sleep 0.5

echo "== 5. Narrow window: open overflow menu and verify visibility =="
ctl wait-for "button.overflow-menu-button" >/dev/null || fail "overflow button present narrow"
ctl click "button.overflow-menu-button" >/dev/null || fail "click overflow button narrow"
ctl wait-for ".overflow-menu-dropdown:not([hidden])" >/dev/null || fail "dropdown open narrow"
must_text "rewrite action narrow" "button.menu-action-rewrite" "Rewrite chapter" 10000
must_text "quiz action narrow" "button.menu-action-quiz" "Quiz me" 10000

snap "03-menu-narrow.png"

echo "== 6. Narrow window: run Quiz me from overflow menu =="
ctl click "button.menu-action-quiz" >/dev/null || fail "click Quiz me narrow"
must_text "quiz page title narrow" "main.document h1" "Quiz: Membranes and Geometry" 25000

snap "04-quiz-narrow.png"

echo "== copy durable snapshots =="
DURABLE_DIR="$ROOT/target/e2e-snapshots/51-snapshots"
mkdir -p "$DURABLE_DIR"
cp "$SNAP"/0*.png "$DURABLE_DIR/"

echo "replay proof complete: 4 snapshots generated in $DURABLE_DIR"
