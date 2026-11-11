#!/usr/bin/env bash
# Ticket 50 proof: Visual states, dark highlights, narrow-window margin drawer.
# Tests visual placeholder/status and retry, dark palette and tokens in light and dark,
# and margin drawer toggle with snapshots taken in both themes.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-50.XXXXXX")"
LIB="$WORK/library"
SNAP="$WORK/snapshots"
SCRATCH_HOME="$WORK/home"
SOCK="$WORK/ctl.sock"
APP_LOG="$WORK/app.log"
APP_PID=""

mkdir -p "$LIB/courses/cell-bio/chapters/cells/visuals/cube" "$LIB/.tutordocs" "$SNAP" "$SCRATCH_HOME"

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

[[ -x "$APP" && -x "$CTL" ]] || fail "test app or ctl binaries missing"

ctl() {
  "$CTL" --socket "$SOCK" "$@"
}

wait_text() {
  local selector="$1" needle="$2" timeout_ms="${3:-15000}"
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
  local label="$1" selector="$2" needle="$3" timeout_ms="${4:-15000}"
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

echo "== seed course with visuals and chapters =="
cat >"$LIB/courses/cell-bio/course.json" <<'JSON'
{
  "id": "cell-bio",
  "title": "Cell Biology",
  "objectives": [
    "Understand the structure of lipid membranes",
    "Explore 3D geometry of cellular components"
  ],
  "chapters": [
    {
      "id": "cells",
      "title": "Membranes and Geometry",
      "summary": "Introduction to membranes and geometric solids.",
      "sources": ["notes.md"]
    }
  ],
  "outlineStatus": "approved"
}
JSON

cat >"$LIB/courses/cell-bio/chapters/cells/index.md" <<'MD'
# Membranes and Geometry

The lipid bilayer is a thin polar membrane made of two layers of lipid molecules. These membranes are flat sheets that form a continuous barrier around all cells.

```visual cube
A geometric cube representing molecular crystal structure.
```

Membrane proteins span the lipid bilayer and transport molecules across the cell boundary.

```visual missing-artifact
This visual artifact is unavailable. Showing fallback description instead.
```

Integral membrane proteins are permanently attached to the biological membrane.
MD

cat >"$LIB/courses/cell-bio/chapters/cells/visuals/cube/manifest.json" <<'JSON'
{
  "schemaVersion": 1,
  "id": "cube",
  "kind": "svg",
  "entry": "index.html",
  "title": "Crystal Cube",
  "chapterId": "cells",
  "dataSchema": {},
  "events": ["ready"],
  "vendor": [],
  "csp": "strict",
  "estimatedHeight": 220
}
JSON

cat >"$LIB/courses/cell-bio/chapters/cells/visuals/cube/index.html" <<'HTML'
<!doctype html>
<html>
<head>
<style>
body {
  margin: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  height: 220px;
  background: #f7f7f5;
  transition: background 150ms ease;
}
body[data-theme="dark"] {
  background: #202020;
}
@media (prefers-color-scheme: dark) {
  body:not([data-theme="light"]) {
    background: #202020;
  }
}
svg {
  width: 90px;
  height: 90px;
  filter: drop-shadow(0 4px 12px rgba(0, 0, 0, 0.15));
}
</style>
</head>
<body>
<svg viewBox="0 0 100 100">
  <rect x="10" y="10" width="80" height="80" rx="12" fill="#7a5af8" />
  <circle cx="50" cy="50" r="24" fill="#ffffff" opacity="0.85" />
</svg>
<script>
window.addEventListener("message", (e) => {
  if (e.data && e.data.type === "theme") {
    document.body.dataset.theme = e.data.theme;
  }
});
parent.postMessage({ tutordoc: 1, type: "ready", visualId: "cube", height: 220 }, "*");
</script>
</body>
</html>
HTML

echo "== launch app =="
env \
  -u ANTHROPIC_API_KEY \
  -u ANTHROPIC_AUTH_TOKEN \
  HOME="$SCRATCH_HOME" \
  PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
  TUTORDOCS_LIBRARY="$LIB" \
  TUTORDOCS_CTL_SOCKET="$SOCK" \
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

must_text "home screen card" "button.card-open" "Cell Biology" 15000
ctl click "button.card-open" >/dev/null || fail "open course"

must_text "chapter heading" "main.document h1" "Membranes and Geometry" 20000
must_text "prose body" ".prose" "continuous barrier" 10000
must_text "study panel title" ".study-title" "Study panel" 10000

echo "== verify visual states =="
ctl wait-for "figure.visual[data-visual-id='cube'] iframe" >/dev/null || fail "cube iframe"
ctl wait-for "figure.visual[data-visual-id='missing-artifact'] .visual-notice[role='status']" >/dev/null || fail "notice role=status"
must_text "retry button" "figure.visual[data-visual-id='missing-artifact'] .visual-notice button.retry" "Try again" 10000

echo "== make highlights =="
ctl select-text ".prose" "lipid bilayer is a thin polar membrane" >/dev/null || fail "select passage 1"
ctl wait-for '[role="toolbar"] button[data-color="yellow"]' >/dev/null || fail "toolbar"
ctl click '[role="toolbar"] button[data-color="yellow"]' >/dev/null || fail "yellow mark"

ctl select-text ".prose" "Membrane proteins span" >/dev/null || fail "select passage 2"
ctl wait-for '[role="toolbar"] button[data-color="green"]' >/dev/null || fail "toolbar 2"
ctl click '[role="toolbar"] button[data-color="green"]' >/dev/null || fail "green mark"

ctl select-text ".prose" "permanently attached" >/dev/null || fail "select passage 3"
ctl wait-for '[role="toolbar"] button[data-color="blue"]' >/dev/null || fail "toolbar 3"
ctl click '[role="toolbar"] button[data-color="blue"]' >/dev/null || fail "blue mark"

echo "== snapshot: light appearance =="
ctl eval 'document.documentElement.dataset.theme = "light"' >/dev/null
sleep 0.4
snap "01-visuals-highlights-light.png"

echo "== snapshot: dark appearance =="
ctl eval 'document.documentElement.dataset.theme = "dark"' >/dev/null
sleep 0.4
snap "02-visuals-highlights-dark.png"

echo "== toggle study panel drawer =="
ctl click "button.panel-toggle" >/dev/null || fail "click toggle collapse"
sleep 0.3
PANEL_LAYOUT="$(ctl attr ".reader" "data-panel")"
[[ "$PANEL_LAYOUT" == "collapsed" ]] || fail "expected collapsed panel, got: $PANEL_LAYOUT"

snap "03-drawer-collapsed-dark.png"

ctl click "button.panel-toggle" >/dev/null || fail "click toggle expand"
sleep 0.3
PANEL_LAYOUT_EXP="$(ctl attr ".reader" "data-panel")"
[[ "$PANEL_LAYOUT_EXP" == "open" ]] || fail "expected open panel, got: $PANEL_LAYOUT_EXP"

snap "04-drawer-open-dark.png"

ctl click "button.panel-toggle" >/dev/null || fail "click toggle collapse again"
ctl eval 'document.documentElement.dataset.theme = "light"' >/dev/null
sleep 0.3
snap "05-drawer-collapsed-light.png"

echo "replay proof complete: 5 snapshots generated in $SNAP"
