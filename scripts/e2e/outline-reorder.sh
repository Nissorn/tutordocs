#!/usr/bin/env bash
# Outline reorder replay proof: handle DOM structure, hit target size, keyboard
# arrow reordering, and pointer-event drag reordering with WKWebView snapshots.
# Tauri's native file-drop handler swallows HTML5 dragover/drop in WKWebView
# (measured in notes/46-premise), so the drag proof dispatches pointer events,
# which tutordocs-ctl eval can deliver. A real mouse drag still needs the user.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-46.XXXXXX")"
LIB="$WORK/library"
IMPORT="$WORK/import"
SNAP="$WORK/snapshots"
FAKE_STATE="$WORK/fake-state"
SCRATCH_HOME="$WORK/home"
FAKE_BIN="$WORK/bin"
SOCK="$WORK/ctl.sock"
APP_LOG="$WORK/app.log"
APP_PID=""

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

echo "== prepare course with outline =="
CLI_ENV=(
  env
  -u ANTHROPIC_API_KEY
  -u ANTHROPIC_AUTH_TOKEN
  HOME="$SCRATCH_HOME"
  PATH="$FAKE_BIN:/usr/bin:/bin:/usr/sbin:/sbin"
  TUTORDOCS_LIBRARY="$LIB"
  FAKE_STATE="$FAKE_STATE"
  FAKE_PAUSE=0.02
)
"${CLI_ENV[@]}" "$CLI" course create "Outline Reorder Test" >/dev/null
COURSE_ID=$("${CLI_ENV[@]}" "$CLI" courses | jq -r '.[0].id')
"${CLI_ENV[@]}" "$CLI" import --course "$COURSE_ID" "$IMPORT/study-notes.md" >/dev/null
"${CLI_ENV[@]}" "$CLI" outline generate --course "$COURSE_ID" --topic "Cell membranes" --source study-notes.md >/dev/null

echo "== launch app =="
env \
  -u ANTHROPIC_API_KEY \
  -u ANTHROPIC_AUTH_TOKEN \
  HOME="$SCRATCH_HOME" \
  PATH="$FAKE_BIN:/usr/bin:/bin:/usr/sbin:/sbin" \
  TUTORDOCS_LIBRARY="$LIB" \
  TUTORDOCS_CTL_SOCKET="$SOCK" \
  FAKE_STATE="$FAKE_STATE" \
  FAKE_PAUSE=0.02 \
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
  sleep 0.4
done
[[ -S "$SOCK" ]] || fail "socket did not appear"

wait_sel "home open button" "button.card-open" 20000
ctl click "button.card-open" >/dev/null || fail "click Open card"

must_text "outline review" "input.draft-title" "Membrane structure" 20000
snap "01-outline-initial.png"

echo "== verify handle DOM and hit target =="
DOM_CHECK="$(ctl eval '(() => {
  const handle = document.querySelector(".drag-handle");
  if (!handle) return "error: no .drag-handle";
  if (handle.tagName !== "DIV") return "error: handle is " + handle.tagName + ", expected DIV";
  if (document.querySelector("button.drag-handle")) return "error: button.drag-handle still exists";
  if (handle.closest("button, input, textarea, select, a")) return "error: handle is inside form/link control";
  if (handle.getAttribute("role") !== "button") return "error: role is not button: " + handle.getAttribute("role");
  if (handle.getAttribute("draggable") === "true") return "error: handle still uses HTML5 draggable";
  if (handle.tabIndex !== 0) return "error: tabIndex is " + handle.tabIndex;
  const rect = handle.getBoundingClientRect();
  if (rect.width < 44 || rect.height < 44) return "error: hit target too small: " + rect.width + "x" + rect.height;
  return "ok " + Math.round(rect.width) + "x" + Math.round(rect.height);
})()')"
echo "handle dom check: $DOM_CHECK"
case "$DOM_CHECK" in
  ok*) ;;
  *) fail "handle DOM check failed: $DOM_CHECK" ;;
esac

echo "== verify keyboard reorder =="
INITIAL_TITLES="$(ctl eval '[...document.querySelectorAll("input.draft-title")].map(i => i.value)')"
echo "initial titles: $INITIAL_TITLES"

KB_RESULT="$(ctl eval '(() => {
  const rows = [...document.querySelectorAll("li.draft-chapter")];
  if (rows.length < 2) return "error: too few rows";
  const h0 = rows[0].querySelector(".drag-handle");
  if (!h0) return "error: no handle in row 0";
  h0.focus();
  h0.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
  const rowsAfter = [...document.querySelectorAll("li.draft-chapter")];
  const h1 = rowsAfter[1].querySelector(".drag-handle");
  const focusedNew = document.activeElement === h1;
  return JSON.stringify({
    focusedNew,
    titles: rowsAfter.map(r => r.querySelector("input.draft-title").value)
  });
})()')"
echo "keyboard reorder result: $KB_RESULT"
KB_OK="$(echo "$KB_RESULT" | jq -r '.focusedNew and (.titles[0] == "Passive transport") and (.titles[1] == "Membrane structure")')"
if [[ "$KB_OK" != "true" ]]; then
  fail "keyboard reorder failed: $KB_RESULT"
fi
snap "02-outline-keyboard-reordered.png"

echo "== verify pointer drag reorder =="
DRAG_RESULT="$(ctl eval '(() => {
  const rows = [...document.querySelectorAll("li.draft-chapter")];
  if (rows.length < 3) return "error: too few rows";
  const handle = rows[2].querySelector(".drag-handle");
  const target = rows[0];
  const rect = target.getBoundingClientRect();
  const x = rect.x + 10;
  const y = rect.y + 5;
  const pe = (type, cx, cy) => new PointerEvent(type, {
    bubbles: true, cancelable: true, pointerId: 1, button: 0,
    buttons: type === "pointerup" ? 0 : 1, clientX: cx, clientY: cy
  });
  handle.dispatchEvent(pe("pointerdown", 10, 10));
  // happy-dom has no hit testing and the real webview does; point the
  // implementation at the target row either way.
  const origFromPoint = document.elementFromPoint.bind(document);
  document.elementFromPoint = () => target;
  handle.dispatchEvent(pe("pointermove", x, y));
  const placeholderDuringDrag = !!document.querySelector(".drag-placeholder");
  const ghostDuringDrag = !!document.querySelector(".drag-ghost");
  handle.dispatchEvent(pe("pointerup", x, y));
  document.elementFromPoint = origFromPoint;
  const rowsAfter = [...document.querySelectorAll("li.draft-chapter")];
  return JSON.stringify({
    placeholderDuringDrag,
    ghostDuringDrag,
    placeholderAfter: !!document.querySelector(".drag-placeholder"),
    ghostAfter: !!document.querySelector(".drag-ghost"),
    titles: rowsAfter.map(r => r.querySelector("input.draft-title").value)
  });
})()')"
echo "pointer drag result: $DRAG_RESULT"
DRAG_OK="$(echo "$DRAG_RESULT" | jq -r '.placeholderDuringDrag and .ghostDuringDrag and (.placeholderAfter | not) and (.ghostAfter | not) and (.titles[0] == "Active transport")')"
if [[ "$DRAG_OK" != "true" ]]; then
  fail "pointer drag failed: $DRAG_RESULT"
fi
snap "03-outline-drag-reordered.png"

echo "== save the drag order and prove it persists =="
SAVE_CLICKED="$(ctl eval '(() => {
  const save = [...document.querySelectorAll("button")].find(b => b.textContent === "Save");
  if (!save) return "error: no Save button";
  if (save.disabled) return "error: Save disabled, drag did not dirty the draft";
  save.click();
  return "ok";
})()')"
[[ "$SAVE_CLICKED" == ok* ]] || fail "save after drag: $SAVE_CLICKED"
must_text "saved state" ".review-save" "All changes saved" 10000
DISK_ORDER="$(jq -r '.chapters[].title' "$LIB/courses/$COURSE_ID/course.json" | head -3 | tr '\n' '|')"
echo "course.json order: $DISK_ORDER"
[[ "$DISK_ORDER" == "Active transport|Passive transport|Membrane structure|" ]] || fail "drag order not on disk: $DISK_ORDER"

ctl click ".tab-home button" >/dev/null || fail "click Home tab"
wait_sel "home after save" "button.card-open" 10000
ctl click "button.card-open" >/dev/null || fail "reopen course"
must_text "reopened outline" "input.draft-title" "Active transport" 10000
REOPEN_TITLES="$(ctl eval '[...document.querySelectorAll("input.draft-title")].map(i => i.value)')"
REOPEN_OK="$(echo "$REOPEN_TITLES" | jq -r '.[0] == "Active transport"')"
[[ "$REOPEN_OK" == "true" ]] || fail "reopened order wrong: $REOPEN_TITLES"
snap "04-outline-reopened-persisted.png"

echo "== replay proof finished successfully =="
echo "Note: the drag proof uses pointer events through tutordocs-ctl eval. One real mouse drag in the dev app still closes the ticket."
echo "snapshots in: $SNAP"
ls -la "$SNAP"
