#!/usr/bin/env bash
# Walk features A to D on a test build of TutorDocs. No screen control, no real agent.
# The app opens a private socket. tutordocs-ctl sends commands that run in the web view.
# Snapshots come from WKWebView takeSnapshot, so the window does not have to be in front.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-37.XXXXXX")"
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

printf '%s\n' '%PDF-1.1' '% cell membranes lecture' >"$IMPORT/cell-membranes.pdf"
printf '%s\n' '%PDF-1.1' '% transport lecture' >"$IMPORT/transport-lecture.pdf"
printf '%s\n' 'The lipid bilayer keeps the inside of a cell different from the outside.' >"$IMPORT/study-notes.md"
printf '%s\n' 'A receptor is a protein that receives a message at the membrane.' >"$IMPORT/glossary.txt"
printf '%s\n' 'not a document' >"$IMPORT/clip.mp4"

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
# custom-protocol embeds ui/dist. Without it a debug build waits on the dev server at localhost:1420.
cargo build -p tutordocs --bins --features e2e-control,custom-protocol
cargo build -p tutordocs-ingest --example demo_pdf
"$CARGO_TARGET_DIR/debug/examples/demo_pdf" "$IMPORT/cell-membranes.pdf"

[[ -x "$APP" && -x "$CTL" ]] || fail "test app binaries missing"

ctl() {
  "$CTL" --socket "$SOCK" "$@"
}

# Poll until selector text contains the needle. wait-for returns as soon as the
# element exists, which is too early when the same node is about to change.
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

# Wait until a selector exists. An empty needle matches any text, including none.
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
# HOME is a scratch folder so probes miss ~/.local and ~/.qoder.
# PATH starts at the fake bin so the absolute paths of the real CLIs are not searched first.
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

must_text "home" "button.home-new" "New course" 30000
snap "01-home-front.png"
echo "front before occlude: [$(front_app)]"
osascript -e 'tell application "Finder" to activate' >/dev/null || echo "could not activate Finder"
sleep 1
echo "front after occlude: [$(front_app)]"
snap "02-home-behind.png"

ctl click "button.home-new" >/dev/null || fail "click New course"
wait_sel "wizard title" '[aria-label="Course title"]' 20000
ctl type '[aria-label="Course title"]' "Cell Membranes" >/dev/null || fail "type title"
ctl click "button.outline-submit" >/dev/null || fail "click Next"
must_text "materials" ".drop-title" "Drop files" 20000

ctl drop "$IMPORT" >/dev/null || fail "drop import folder"
must_text "import" ".materials-summary" "4 imported, 1 skipped" 20000
snap "03-import.png"

ctl click "button.outline-submit" >/dev/null || fail "click Next to guide"
must_text "guide step" '.wizard-steps [aria-current="step"]' "Guide" 20000
ctl type '[aria-label="Guide"]' "Focus on the bilayer as a selective barrier." >/dev/null || fail "type guide"
ctl click "button.outline-submit" >/dev/null || fail "click Plan the course"

must_text "outline review" "section.review h1" "Cell Membranes" 180000
must_text "outline chapters" "input.draft-title" "Membrane structure" 20000
snap "04-outline.png"

ctl type "textarea.draft-comment" "Mention osmosis in this chapter." >/dev/null || fail "type comment"
# The revise control shares quiet-button with other buttons, so the click finds it by its label.
echo "== revise =="
revise_err=""
local_ok=0
for _ in $(seq 1 30); do
  if revise_err="$(ctl eval '(() => { const areas = [...document.querySelectorAll("textarea.draft-comment")].map((a) => a.value); const b = [...document.querySelectorAll("button")].find((n) => (n.textContent || "").trim() === "Revise with my comments"); if (!b) throw new Error("no revise button comments=" + JSON.stringify(areas)); if (b.disabled) throw new Error("revise disabled comments=" + JSON.stringify(areas)); b.click(); return "clicked"; })()' 2>&1)"; then
    local_ok=1
    break
  fi
  sleep 0.2
done
[[ "$local_ok" -eq 1 ]] || fail "click Revise with my comments ($revise_err)"
must_text "revised outline" ".review-notice" "Revised." 180000
must_text "revised chapter" "input.draft-title" "Membrane structure (revised)" 20000
snap "05-revised.png"

ctl click "button.outline-submit" >/dev/null || fail "click Approve"
must_text "approve choice" ".approve-lead" "How should the chapters be written" 10000
ctl click "section.approve-panel button.outline-submit" >/dev/null || fail "click Approve outline"
must_text "chapter in sidebar" 'button.chapter[data-chapter-id="membrane-structure"]' "Membrane structure (revised)" 20000

echo "== pdf reader before chapter is written (disabled Ask) =="
must_text "pdf source in sidebar before chapters written" 'button.chapter[data-source-name="cell-membranes.pdf"]' "cell-membranes.pdf" 20000
ctl click 'button.chapter[data-source-name="cell-membranes.pdf"]' >/dev/null || fail "open pdf source before chapter written"
must_text "pdf page indicator" ".pdf-page-total" "/ 2" 20000
local_ok=0
for _ in $(seq 1 20); do
  ctl select-text ".pdf-page" "surroundings" >/dev/null 2>&1 || true
  if wait_text 'button.popover-action[data-action="ask"]' "Ask" 1500 >/dev/null 2>&1; then
    local_ok=1
    break
  fi
done
[[ "$local_ok" -eq 1 ]] || fail "pdf selection popup before chapter written"
must_text "disabled ask reason" ".popover-disabled-reason" "No chapter is written yet" 20000
snap "06b2-pdf-ask-disabled.png"

ctl click 'button.chapter[data-chapter-id="membrane-structure"]' >/dev/null || fail "open chapter"
must_text "chapter text" ".prose" "selective barrier" 180000
must_text "study panel" ".study-title" "Study panel" 20000
must_text "study goals" "ul.study-goals" "Explain how a membrane" 20000
snap "06-chapter.png"

must_text "source in sidebar" 'button.chapter[data-source-name="study-notes.md"]' "study-notes.md" 20000
ctl click 'button.chapter[data-source-name="study-notes.md"]' >/dev/null || fail "open source"
must_text "source text" ".source-text" "lipid bilayer" 20000
snap "06b-source.png"

echo "== pdf reader =="
must_text "pdf source in sidebar" 'button.chapter[data-source-name="cell-membranes.pdf"]' "cell-membranes.pdf" 20000
ctl click 'button.chapter[data-source-name="cell-membranes.pdf"]' >/dev/null || fail "open pdf source"
must_text "pdf page indicator" ".pdf-page-total" "/ 2" 20000
must_text "pdf word on page 1" ".pdf-page" "surroundings" 20000
snap "06c-pdf-page-1.png"

echo "== pdf jump to page 2 =="
ctl eval '(() => { const inp = document.querySelector("input.pdf-page-input"); inp.value = "2"; inp.dispatchEvent(new Event("change")); return "jumped"; })()' >/dev/null || fail "jump to page 2"
must_text "pdf caveat on page 2" ".pdf-caveat" "transformed form" 20000
snap "06d-pdf-page-2.png"

echo "== pdf jump to page 1 =="
ctl eval '(() => { const inp = document.querySelector("input.pdf-page-input"); inp.value = "1"; inp.dispatchEvent(new Event("change")); return "jumped"; })()' >/dev/null || fail "jump to page 1"
must_text "pdf page 1 text" ".pdf-page" "surroundings" 20000

echo "== select pdf passage =="
local_ok=0
for _ in $(seq 1 20); do
  ctl select-text ".pdf-page" "surroundings" >/dev/null 2>&1 || true
  if wait_text 'button.popover-action[data-action="ask"]' "Ask" 1500 >/dev/null 2>&1; then
    local_ok=1
    break
  fi
done
[[ "$local_ok" -eq 1 ]] || fail "pdf selection popup"
ctl eval '(() => { if (document.querySelector("button.popover-action[data-action=\"fix\"]")) throw new Error("Fix button unexpectedly present on PDF selection"); return "ok"; })()' >/dev/null || fail "Fix button must be absent on PDF selection"
must_text "pdf ask button" 'button.popover-action[data-action="ask"]' "Ask" 20000
snap "06e-pdf-selection-popup.png"

ctl click 'button.popover-action[data-action="ask"]' >/dev/null || fail "click Ask in pdf popup"
wait_sel "pdf ask field" "textarea.popup-field" 10000
must_text "pdf saved with chapter" ".popup-chapter-hint" "Saved with chapter" 20000
ctl type "textarea.popup-field" "What does the plasma membrane separate?" >/dev/null || fail "type question in pdf popup"
ctl click "button.popup-submit" >/dev/null || fail "submit question in pdf popup"
must_text "pdf ask popup answer" ".popup-answer" "You asked:" 180000
snap "06f-pdf-ask-answer.png"
ctl click "button.popup-close" >/dev/null || fail "close pdf ask popup"

ctl click 'button.chapter[data-chapter-id="membrane-structure"]' >/dev/null || fail "reopen chapter"
must_text "chapter text" ".prose" "selective barrier" 20000

echo "== scroll chapter before quiz =="
# Scroll as far as 160px, or as far as the chapter allows, so a restyle that
# shortens the fixture chapter does not break the restore check.
SCROLL_TARGET="$(ctl eval '(() => { const d = document.querySelector("main.document"); d.scrollTop = Math.min(160, d.scrollHeight - d.clientHeight); return d.scrollTop; })()')" || fail "scroll chapter"
[[ "$SCROLL_TARGET" -ge 40 ]] || fail "the fixture chapter is too short to scroll: $SCROLL_TARGET"

echo "== quiz me (attempt 1 - fails) =="
ctl eval '(() => { const b = [...document.querySelectorAll("button")].find((n) => (n.textContent || "").trim() === "Quiz me"); if (!b) throw new Error("no Quiz me button"); b.click(); return "clicked"; })()' >/dev/null || fail "click Quiz me"
must_text "quiz failed" ".generation-status" "could not be written" 20000
wait_sel "try again" "button.quiz-retry" 10000
snap "07-quiz-retry.png"

echo "== click Try again =="
ctl click "button.quiz-retry" >/dev/null || fail "click Try again"
must_text "quiz loaded" ".quiz-prompt" "Why does oxygen cross" 20000
wait_sel "answer area" "textarea.quiz-answer" 10000
snap "08-quiz-loaded.png"

echo "== back to chapter (restore scroll) =="
ctl eval '(() => { const b = [...document.querySelectorAll("button")].find((n) => (n.textContent || "").trim() === "Back to the chapter"); if (!b) throw new Error("no Back to the chapter button"); b.click(); return "clicked"; })()' >/dev/null || fail "click Back to the chapter"
must_text "back to chapter" ".prose" "selective barrier" 20000
scroll_top="$(ctl eval 'document.querySelector("main.document").scrollTop')"
[[ "$scroll_top" -eq "$SCROLL_TARGET" ]] || fail "expected scrollTop $SCROLL_TARGET, got $scroll_top"
snap "09-chapter-scroll-restored.png"

echo "== quiz me (take quiz) =="
ctl eval '(() => { const b = [...document.querySelectorAll("button")].find((n) => (n.textContent || "").trim() === "Quiz me"); if (!b) throw new Error("no Quiz me button"); b.click(); return "clicked"; })()' >/dev/null || fail "click Quiz me second time"
must_text "quiz prompt" ".quiz-prompt" "Why does oxygen cross" 20000

echo "== submit answers =="
ctl type "textarea.quiz-answer" "Oxygen is small and uncharged so it diffuses down the concentration gradient." >/dev/null || fail "type quiz answer"
ctl click "button.outline-submit" >/dev/null || fail "click Check my answers"
must_text "quiz score" ".quiz-score" "Score: 1 correct, 0 partly right, 0 not quite, of 1." 20000
wait_sel "new questions button" "button.outline-submit" 10000
wait_sel "back to outline button" "button.quiet-button" 10000
snap "10-quiz-finished.png"

echo "== back to outline from quiz =="
ctl eval '(() => { const b = [...document.querySelectorAll("button")].find((n) => (n.textContent || "").trim() === "Back to the outline"); if (!b) throw new Error("no Back to the outline button"); b.click(); return "clicked"; })()' >/dev/null || fail "click Back to the outline"
must_text "outline review from quiz" "section.review h1" "Cell Membranes" 20000
snap "11-outline-from-quiz.png"

echo "== reopen chapter for new questions retake =="
ctl click 'button.chapter[data-chapter-id="membrane-structure"]' >/dev/null || fail "reopen chapter"
must_text "chapter re-opened" ".prose" "selective barrier" 20000
ctl eval '(() => { const b = [...document.querySelectorAll("button")].find((n) => (n.textContent || "").trim() === "Quiz me"); if (!b) throw new Error("no Quiz me button"); b.click(); return "clicked"; })()' >/dev/null || fail "click Quiz me third time"
must_text "quiz prompt" ".quiz-prompt" "Why does oxygen cross" 20000
ctl type "textarea.quiz-answer" "Oxygen diffuses freely." >/dev/null || fail "type quiz answer"
ctl click "button.outline-submit" >/dev/null || fail "click Check my answers"
must_text "quiz score" ".quiz-score" "Score: 1 correct" 20000

echo "== retake: new questions =="
ctl eval '(() => { const b = [...document.querySelectorAll("button")].find((n) => (n.textContent || "").trim() === "New questions"); if (!b) throw new Error("no New questions button"); b.click(); return "clicked"; })()' >/dev/null || fail "click New questions"
must_text "new questions loaded" ".quiz-prompt" "Why does oxygen cross" 20000
snap "12-quiz-retake.png"

ctl eval '(() => { const b = [...document.querySelectorAll("button")].find((n) => (n.textContent || "").trim() === "Back to the chapter"); if (!b) throw new Error("no Back to the chapter button"); b.click(); return "clicked"; })()' >/dev/null || fail "click Back to the chapter"
must_text "back to chapter for passage fix" ".prose" "selective barrier" 20000

echo "== select passage =="
local_ok=0
for _ in $(seq 1 20); do
  ctl select-text ".prose" "selective barrier" >/dev/null 2>&1 || true
  if wait_text 'button.popover-action[data-action="fix"]' "Fix this section" 1500 >/dev/null 2>&1; then
    local_ok=1
    break
  fi
done
[[ "$local_ok" -eq 1 ]] || fail "selection popup"
must_text "chapter fix button" 'button.popover-action[data-action="fix"]' "Fix this section" 20000
must_text "chapter ask button" 'button.popover-action[data-action="ask"]' "Ask" 20000
ctl click 'button.popover-action[data-action="fix"]' >/dev/null || fail "click Fix this section"
wait_sel "fix field" "textarea.popup-field" 10000
ctl type "textarea.popup-field" "Say that oxygen crosses on its own." >/dev/null || fail "type instruction"
ctl click "button.popup-submit" >/dev/null || fail "click Revise section"
must_text "revised section" ".prose" "REVISED SECTION" 180000
snap "13-section.png"

echo "== ask popup =="
local_ok=0
for _ in $(seq 1 20); do
  ctl select-text ".prose" "cross the bilayer on their own" >/dev/null 2>&1 || true
  if wait_text 'button.popover-action[data-action="ask"]' "Ask" 1500 >/dev/null 2>&1; then
    local_ok=1
    break
  fi
done
[[ "$local_ok" -eq 1 ]] || fail "ask selection popup"
ctl click 'button.popover-action[data-action="ask"]' >/dev/null || fail "click Ask in popup"
wait_sel "ask field" "textarea.popup-field" 10000
ctl type "textarea.popup-field" "How does diffusion work across the membrane?" >/dev/null || fail "type question in popup"
ctl click "button.popup-submit" >/dev/null || fail "submit question in popup"
must_text "ask popup answer" ".popup-answer" "You asked:" 180000
ctl eval '(() => { const r = document.querySelector(".mark-popover").getBoundingClientRect(); if (r.top <= 10) throw new Error("ask popup jumped to top: " + r.top); return r.top; })()' >/dev/null || fail "ask popup jumped to top"
snap "07b-ask-popup.png"

# Scroll document to verify popup stays attached
ctl eval '(() => { const doc = document.querySelector(".document"); const s = document.createElement("div"); s.id = "test-spacer"; s.style.height = "1000px"; doc.appendChild(s); const r1 = document.querySelector(".mark-popover").getBoundingClientRect(); doc.scrollTop = 60; doc.dispatchEvent(new Event("scroll")); const r2 = document.querySelector(".mark-popover").getBoundingClientRect(); if (Math.abs((r1.top - r2.top) - 60) > 2) throw new Error("popup did not move with scroll: r1=" + r1.top + " r2=" + r2.top); return "scrolled"; })()' >/dev/null || fail "popup scroll tracking"
snap "07c-ask-popup-scrolled.png"
ctl eval '(() => { document.getElementById("test-spacer")?.remove(); })()' >/dev/null || true
ctl click "button.popup-close" >/dev/null || fail "close ask popup"

ctl type '[aria-label="Ask about this chapter"]' "Why does the bilayer block charged molecules?" >/dev/null || fail "type question"
ctl click "form.ask-box button" >/dev/null || fail "click Ask"
must_text "answer" ".answer-body" "You asked:" 180000
snap "14-answer.png"

ctl click "button.jobs-toggle" >/dev/null || fail "open Jobs"
must_text "jobs outline" ".job-body" "Outline" 10000
must_text "jobs chapter" ".job-body" "Chapter" 10000
must_text "jobs quiz" ".job-body" "Quiz" 10000
must_text "jobs grade" ".job-body" "Grading" 10000
must_text "jobs section revise" ".job-body" "Section revise" 10000
must_text "jobs ask" ".job-body" "Ask" 10000
snap "15-jobs.png"

echo "== fake claude log =="
[[ -f "$FAKE_STATE/calls.log" ]] || fail "fake claude wrote no calls.log"
for kind in outline revise_outline chapter quiz grade revise_section ask; do
  awk '{print $2}' "$FAKE_STATE/calls.log" | grep -qx "$kind" || fail "fake claude did not run $kind"
done
if grep -o 'anthropic_key=[^ ]*' "$FAKE_STATE/calls.log" | grep -v '^anthropic_key=unset$' | grep -q .; then
  fail "a fake claude call saw an Anthropic key"
fi
grep -q "anthropic_key=unset" "$FAKE_STATE/calls.log" || fail "calls.log did not record an unset key"
cat "$FAKE_STATE/calls.log"

WALK_END="$(date +%s)"
echo "replay walk seconds: $((WALK_END - WALK_START))"
echo "snapshots: $SNAP"
ls -l "$SNAP"
if [[ -n "${REPLAY_SNAP_DIR:-}" ]]; then
  mkdir -p "$REPLAY_SNAP_DIR"
  cp -p "$SNAP"/*.png "$REPLAY_SNAP_DIR/"
else
  mkdir -p "$ROOT/target/e2e-snapshots/48-snapshots"
  cp -p "$SNAP"/*.png "$ROOT/target/e2e-snapshots/48-snapshots/"
  [[ -f "$SNAP/06c-pdf-page-1.png" ]] && cp -p "$SNAP/06c-pdf-page-1.png" "$ROOT/target/e2e-snapshots/49-pdf-page-1.png"
  [[ -f "$SNAP/06d-pdf-page-2.png" ]] && cp -p "$SNAP/06d-pdf-page-2.png" "$ROOT/target/e2e-snapshots/49-pdf-page-2.png"
  [[ -f "$SNAP/06e-pdf-selection-popup.png" ]] && cp -p "$SNAP/06e-pdf-selection-popup.png" "$ROOT/target/e2e-snapshots/49-pdf-selection-popup.png"
  [[ -f "$SNAP/06f-pdf-ask-answer.png" ]] && cp -p "$SNAP/06f-pdf-ask-answer.png" "$ROOT/target/e2e-snapshots/49-pdf-ask-answer.png"
  [[ -f "$SNAP/06b2-pdf-ask-disabled.png" ]] && cp -p "$SNAP/06b2-pdf-ask-disabled.png" "$ROOT/target/e2e-snapshots/49-pdf-ask-disabled.png"
fi
echo "front at end: [$(front_app)]"
