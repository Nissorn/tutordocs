#!/usr/bin/env bash
# Tab view state replay proof for ticket 44:
# - Finding 3: Quiz in progress survives tab switch with typed answers and grades
# - Finding 10: Outline review view survives tab switch
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target-e2e}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-44.XXXXXX")"
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

# Pre-touch quiz-failed flag so fake-claude generates quiz on first try
touch "$FAKE_STATE/quiz-failed"

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

echo "== build ui =="
npm --prefix ui run build

echo "== build test app (e2e-control) =="
find crates src-tauri/src -name '*.rs' -exec touch {} +
cargo build -p tutordocs --bins --features e2e-control,custom-protocol
cargo build -p tutordocs-ingest --example demo_pdf
"$CARGO_TARGET_DIR/debug/examples/demo_pdf" "$IMPORT/cell-membranes.pdf"

[[ -x "$APP" && -x "$CTL" ]] || fail "test app or ctl binaries missing"

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

# 1. Create course through wizard
ctl click "button.home-new" >/dev/null || fail "click New course"
wait_sel "wizard title" '[aria-label="Course title"]' 20000
ctl type '[aria-label="Course title"]' "Cell Membranes" >/dev/null || fail "type title"
ctl click "button.outline-submit" >/dev/null || fail "click Next"
must_text "materials" ".drop-title" "Drop files" 20000

ctl drop "$IMPORT" >/dev/null || fail "drop import folder"
must_text "import" ".materials-summary" "2 imported" 20000

ctl click "button.outline-submit" >/dev/null || fail "click Next to guide"
must_text "guide step" '.wizard-steps [aria-current="step"]' "Guide" 20000
ctl type '[aria-label="Guide"]' "Focus on the lipid bilayer." >/dev/null || fail "type guide"
ctl click "button.outline-submit" >/dev/null || fail "click Plan the course"

must_text "outline review" "section.review h1" "Cell Membranes" 180000
must_text "outline chapters" "input.draft-title" "Membrane structure" 20000

# Approve outline
ctl click "button.outline-submit" >/dev/null || fail "click Approve"
must_text "approve choice" ".approve-lead" "How should the chapters be written" 10000
ctl click "section.approve-panel button.outline-submit" >/dev/null || fail "click Approve outline"

must_text "chapter in sidebar" 'button.chapter[data-chapter-id="membrane-structure"]' "Membrane structure" 20000

# Open chapter
ctl click 'button.chapter[data-chapter-id="membrane-structure"]' >/dev/null || fail "open chapter"
must_text "chapter text" ".prose" "double layer of lipids" 180000

# 2. Start quiz, type draft answer, submit for grading
echo "== start quiz =="
ctl click "button.quiet-button:nth-child(2)" >/dev/null 2>&1 || ctl eval '(() => { const btns = [...document.querySelectorAll("button")]; const q = btns.find(b => b.textContent === "Quiz me"); q.click(); })()' >/dev/null || fail "click Quiz me"
must_text "quiz page" "section.quiz h1" "Quiz: Membrane structure" 20000
wait_sel "quiz question" ".quiz-question" 20000

echo "== type answer and grade =="
ctl type "textarea.quiz-answer" "Oxygen diffuses across because it is small and uncharged." >/dev/null || fail "type answer"
ctl click "button.outline-submit" >/dev/null || fail "click Check my answers"
must_text "verdict" ".quiz-verdict" "Correct" 20000
snap "44-01-quiz-graded.png"

# 3. Switch to Home tab
echo "== switch to Home tab =="
ctl click ".tab-home button" >/dev/null || fail "click Home tab"
must_text "home view" ".home" "Courses" 20000
snap "44-02-switched-to-home.png"

# 4. Switch back to Cell Membranes tab -> quiz view with graded answer restores
echo "== switch back to Cell Membranes =="
ctl click 'button[data-focus-key="tab:cell-membranes"]' >/dev/null || fail "switch back"
must_text "quiz view restored" "section.quiz h1" "Quiz: Membrane structure" 20000
must_text "verdict restored" ".quiz-verdict" "Correct" 10000
snap "44-03-quiz-restored.png"

# 5. Leave quiz back to chapter, open outline review
echo "== back to chapter, then edit outline =="
ctl click "section.quiz button.quiet-button" >/dev/null || fail "click Back to chapter"
must_text "chapter body" ".prose" "double layer of lipids" 20000

ctl click ".course-tools button" >/dev/null || fail "click Edit outline"
must_text "review view" "section.review h1" "Cell Membranes" 20000
snap "44-04-review-screen.png"

# 6. Switch to Home and switch back -> outline review restores
echo "== switch away from review and back =="
ctl click ".tab-home button" >/dev/null || fail "click Home tab from review"
must_text "home again" ".home" "Courses" 20000

ctl click 'button[data-focus-key="tab:cell-membranes"]' >/dev/null || fail "switch back to review"
must_text "review restored" "section.review h1" "Cell Membranes" 20000
snap "44-05-review-restored.png"

# 7. Open PDF source, zoom in and jump to page 2, switch away and back -> PDF view, page and zoom restore
echo "== open PDF source, zoom and page 2 =="
ctl click 'button.chapter[data-source-name="cell-membranes.pdf"]' >/dev/null || fail "open PDF source"
must_text "pdf page indicator" ".pdf-page-total" "/ 2" 20000

# Zoom in
ctl click 'button[aria-label="Zoom in"]' >/dev/null || fail "zoom in"

# Jump to page 2
ctl eval '(() => { const inp = document.querySelector("input.pdf-page-input"); inp.value = "2"; inp.dispatchEvent(new Event("change")); return "jumped"; })()' >/dev/null || fail "jump to page 2"
must_text "pdf caveat on page 2" ".pdf-caveat" "transformed form" 20000
snap "44-06-pdf-zoomed-page-2.png"

# Switch away to Home
echo "== switch to Home from PDF =="
ctl click ".tab-home button" >/dev/null || fail "click Home from PDF"
must_text "home from pdf" ".home" "Courses" 20000

# Switch back to Cell Membranes -> PDF view restores with page 2 and zoom
echo "== switch back to PDF tab =="
ctl click 'button[data-focus-key="tab:cell-membranes"]' >/dev/null || fail "switch back to PDF"
must_text "pdf restored page indicator" ".pdf-page-total" "/ 2" 20000
must_text "pdf restored caveat page 2" ".pdf-caveat" "transformed form" 20000
snap "44-07-pdf-restored.png"

WALK_END="$(date +%s)"
echo "replay walk seconds: $((WALK_END - WALK_START))"
echo "snapshots: $SNAP"
ls -la "$SNAP"

mkdir -p "$ROOT/target/e2e-snapshots/44-snapshots"
cp -p "$SNAP"/*.png "$ROOT/target/e2e-snapshots/44-snapshots/"
echo "Replay proof completed successfully!"
