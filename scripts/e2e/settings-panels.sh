#!/usr/bin/env bash
# Ticket 65 proof: Settings, a theme the app owns, text size, and resizable
# side panels. Drives the real app through tutordocs-ctl:
# - each theme, forced against the other macOS appearance, with a chapter that
#   has an SVG figure that carries its own dark palette
# - each text size, the reading font and the line width, and that the app chrome
#   does not change with them
# - a resized sidebar and study panel, a double-click reset, Reset panel widths
# - a reload, and that every choice persisted
# `ctl appearance` pins the window's macOS appearance, so both forced themes are
# shown against the opposite mode without touching the desktop setting.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target-e2e}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tutordocs-65.XXXXXX")"
LIB="$WORK/library"
SNAP="$WORK/snapshots"
SCRATCH_HOME="$WORK/home"
SOCK="$WORK/ctl.sock"
APP_LOG="$WORK/app.log"
APP_PID=""
CHAPTER="$LIB/courses/data-center/chapters/power"

mkdir -p "$CHAPTER/visuals" "$LIB/.tutordocs" "$SNAP" "$SCRATCH_HOME"
printf '%s\n' '{"driver":"claude"}' >"$LIB/.tutordocs/agent.json"

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

expect_near() {
  local label="$1" expected="$2" actual="$3"
  python3 - "$expected" "$actual" <<'PY' || fail "$label: expected about $expected, got $actual"
import sys
sys.exit(0 if abs(float(sys.argv[1]) - float(sys.argv[2])) <= 1.5 else 1)
PY
  echo "ok: $label = $actual (expected about $expected)"
}

# The page's paper colour as a brightness from 0 to 255. A canvas turns any CSS
# colour, oklch() included, into pixels, so the check does not depend on how the
# browser prints a computed colour.
paper_luma() {
  val '(() => { const c = document.createElement("canvas"); c.width = c.height = 1; const x = c.getContext("2d"); x.fillStyle = getComputedStyle(document.body).backgroundColor; x.fillRect(0, 0, 1, 1); const d = x.getImageData(0, 0, 1, 1).data; return Math.round(0.2126 * d[0] + 0.7152 * d[1] + 0.0722 * d[2]); })()'
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

RADIO='[role="radiogroup"][aria-labelledby="settings-%s"] [data-value="%s"]'
choose() {
  local group="$1" value="$2" selector
  # shellcheck disable=SC2059
  selector="$(printf "$RADIO" "$group" "$value")"
  ctl click "$selector" >/dev/null || fail "choose $group $value"
}

open_settings() {
  ctl click "button.settings-toggle" >/dev/null || fail "click the gear"
  ctl wait-for ".settings-scrim .settings" >/dev/null || fail "settings dialog did not open"
}

close_settings() {
  ctl click ".settings-close" >/dev/null || fail "close settings"
  expect_eq "settings dialog hidden" "true" "$(val 'document.querySelector(".settings-scrim").hidden')"
}

# A drag with synthetic pointer events. WKWebView refuses setPointerCapture for a
# pointer that is not real, so the replay stubs it on the separator. The unit tests
# cover the capture call itself.
drag() {
  local side="$1" from="$2" to="$3"
  ctl eval "(() => { const h = document.querySelector('.panel-resizer-$side'); h.setPointerCapture = () => {}; const fire = (type, x) => h.dispatchEvent(new PointerEvent(type, { clientX: x, button: 0, pointerId: 1, bubbles: true, cancelable: true })); fire('pointerdown', $from); fire('pointermove', $to); fire('pointerup', $to); return h.getAttribute('aria-valuenow'); })()" >/dev/null || fail "drag $side"
}

echo "== seed a course with a chapter, text and an SVG figure =="
cat >"$LIB/courses/data-center/course.json" <<'JSON'
{
  "id": "data-center",
  "title": "Data Center Power",
  "objectives": ["Trace power from the grid to the server tray", "Compare a central UPS with rack batteries"],
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

![Rack batteries keep the trays running](visuals/power-path.svg "Rack-level batteries remove the central UPS from the power path.")

## Why the route matters

A central UPS converts alternating current to direct current and back again. Every conversion costs a few percent, and the heat it makes has to be cooled by more power. Rack-level batteries sit on the direct current bus and skip two of those conversions, which is why large operators moved to them.

The second reason is failure. A central UPS is one box that every row depends on. A battery on each rack fails one rack at a time, and the other racks keep running without noticing.
MD

cat >"$CHAPTER/visuals/power-path.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 200" role="img" aria-label="Power path">
  <style>
    svg {
      --bg: #f6f5f2;
      --surface: #fffcf7;
      --ink: #241f1a;
      --muted: #5c564c;
      --line: #cfc6b8;
      --accent: #1d6b45;
    }
    @media (prefers-color-scheme: dark) {
      svg {
        --bg: #1a1917;
        --surface: #262420;
        --ink: #f3efe6;
        --muted: #b7b0a4;
        --line: #4d473f;
        --accent: #8ed4ae;
      }
    }
    text { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; font-size: 15px; fill: var(--ink); }
    .verb { fill: var(--muted); font-size: 13px; }
  </style>
  <rect width="640" height="200" fill="var(--bg)"/>
  <rect x="24" y="70" width="140" height="60" rx="6" fill="var(--surface)" stroke="var(--line)"/>
  <text x="94" y="105" text-anchor="middle">Utility grid</text>
  <rect x="250" y="70" width="140" height="60" rx="6" fill="var(--surface)" stroke="var(--line)"/>
  <text x="320" y="105" text-anchor="middle">Rack busway</text>
  <rect x="476" y="70" width="140" height="60" rx="6" fill="var(--surface)" stroke="var(--accent)" stroke-width="2"/>
  <text x="546" y="105" text-anchor="middle">Server tray</text>
  <path d="M164 100 H250" stroke="var(--accent)" stroke-width="2"/>
  <path d="M390 100 H476" stroke="var(--accent)" stroke-width="2"/>
  <text class="verb" x="207" y="90" text-anchor="middle">feeds</text>
  <text class="verb" x="433" y="90" text-anchor="middle">powers</text>
</svg>
SVG

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

ctl resize 1280 900 >/dev/null || fail "resize window"
must_text "home card" "button.card-open" "Data Center Power" 15000
ctl click "button.card-open" >/dev/null || fail "open course"
must_text "chapter heading" "main.document h1" "Power Distribution" 20000
ctl wait-for "figure.chapter-figure img" >/dev/null || fail "figure image"
expect_eq "figure loads with no theme query on System" "visual://localhost/power/power-path.svg" "$(val 'document.querySelector("figure.chapter-figure img").getAttribute("src")')"

echo "== the gear and Cmd+comma open Settings =="
expect_eq "gear sits right before Jobs" "jobs-toggle" "$(val 'document.querySelector("button.settings-toggle").nextElementSibling.className')"
ctl eval 'document.dispatchEvent(new KeyboardEvent("keydown", { key: ",", metaKey: true, bubbles: true, cancelable: true }))' >/dev/null
expect_eq "Cmd+comma opens" "false" "$(val 'document.querySelector(".settings-scrim").hidden')"
ctl eval 'document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }))' >/dev/null
expect_eq "Escape closes" "true" "$(val 'document.querySelector(".settings-scrim").hidden')"

echo "== chrome sizes before any change =="
CHROME_JOBS="$(val 'getComputedStyle(document.querySelector("button.jobs-toggle")).fontSize')"
CHROME_SIDEBAR="$(val 'getComputedStyle(document.querySelector("button.chapter")).fontSize')"
CHROME_REWRITE="$(val 'getComputedStyle(document.querySelector("button.chapter-rewrite")).fontSize')"
echo "jobs button $CHROME_JOBS, sidebar chapter $CHROME_SIDEBAR, Rewrite chapter $CHROME_REWRITE"

echo "== macOS dark, theme System: Settings open =="
ctl appearance dark >/dev/null || fail "appearance dark"
settle
expect_eq "macOS is dark" "true" "$(val 'matchMedia("(prefers-color-scheme: dark)").matches')"
open_settings
snap "01-settings-open-system-on-dark-macos.png"

echo "== force light against dark macOS =="
choose theme light
settle
expect_eq "data-theme" "light" "$(val 'document.documentElement.dataset.theme')"
expect_eq "macOS still dark" "true" "$(val 'matchMedia("(prefers-color-scheme: dark)").matches')"
expect_eq "figure asks for the light palette" "visual://localhost/power/power-path.svg?theme=light" "$(val 'document.querySelector("figure.chapter-figure img").getAttribute("src")')"
[[ "$(paper_luma)" -gt 200 ]] || fail "page paper is not light: $(paper_luma)"
echo "ok: page paper is light ($(paper_luma))"
snap "02-light-forced-on-dark-macos-settings-open.png"
close_settings
settle
snap "03-light-forced-on-dark-macos.png"

echo "== force dark against light macOS =="
ctl appearance light >/dev/null || fail "appearance light"
settle
expect_eq "macOS is light" "false" "$(val 'matchMedia("(prefers-color-scheme: dark)").matches')"
open_settings
choose theme dark
settle
expect_eq "data-theme" "dark" "$(val 'document.documentElement.dataset.theme')"
expect_eq "figure asks for the dark palette" "visual://localhost/power/power-path.svg?theme=dark" "$(val 'document.querySelector("figure.chapter-figure img").getAttribute("src")')"
[[ "$(paper_luma)" -lt 60 ]] || fail "page paper is not dark: $(paper_luma)"
echo "ok: page paper is dark ($(paper_luma))"
snap "04-dark-forced-on-light-macos-settings-open.png"
close_settings
settle
snap "05-dark-forced-on-light-macos.png"

echo "== back to System: the figure loses its query =="
open_settings
choose theme system
settle
expect_eq "data-theme removed" "false" "$(val 'document.documentElement.hasAttribute("data-theme")')"
expect_eq "figure has no theme query" "visual://localhost/power/power-path.svg" "$(val 'document.querySelector("figure.chapter-figure img").getAttribute("src")')"

echo "== text size scales the chapter, not the chrome =="
BODY_SELECTOR='.page .prose p'
# The chapter base is 1.2rem (19.2px at Medium), a reading face being set larger than the chrome.
for pair in small:17.28 medium:19.2 large:21.6 x-large:24; do
  size="${pair%%:*}"
  want="${pair##*:}"
  choose text-size "$size"
  settle
  got="$(val "parseFloat(getComputedStyle(document.querySelector('$BODY_SELECTOR')).fontSize)")"
  expect_near "chapter paragraph at $size" "$want" "$got"
  expect_eq "jobs button font at $size" "$CHROME_JOBS" "$(val 'getComputedStyle(document.querySelector("button.jobs-toggle")).fontSize')"
  expect_eq "sidebar chapter font at $size" "$CHROME_SIDEBAR" "$(val 'getComputedStyle(document.querySelector("button.chapter")).fontSize')"
  expect_eq "Rewrite chapter font at $size" "$CHROME_REWRITE" "$(val 'getComputedStyle(document.querySelector("button.chapter-rewrite")).fontSize')"
done
expect_near "chapter title scales with the text" "55.2" "$(val 'parseFloat(getComputedStyle(document.querySelector(".page h1")).fontSize)')"
snap "06-text-x-large-settings-open.png"

echo "== reading font and line width =="
choose reading-font serif
choose line-width wide
settle
expect_eq "serif stack on the chapter" "true" "$(val 'getComputedStyle(document.querySelector(".page .prose p")).fontFamily.includes("New York")')"
expect_eq "chrome keeps its font" "true" "$(val 'getComputedStyle(document.querySelector("button.jobs-toggle")).fontFamily.includes("New York") === false')"
expect_eq "Rewrite chapter keeps its font" "true" "$(val 'getComputedStyle(document.querySelector("button.chapter-rewrite")).fontFamily.includes("New York") === false')"
WIDE="$(val 'document.querySelector(".page").getBoundingClientRect().width')"
choose line-width narrow
settle
NARROW="$(val 'document.querySelector(".page").getBoundingClientRect().width')"
python3 - "$NARROW" "$WIDE" <<'PY' || fail "narrow measure ($NARROW) is not narrower than wide ($WIDE)"
import sys
sys.exit(0 if float(sys.argv[1]) < float(sys.argv[2]) else 1)
PY
echo "ok: narrow $NARROW < wide $WIDE"
choose line-width normal
close_settings
settle
snap "07-text-x-large-serif.png"

echo "== the Sans choice reads in the bundled sans, then back to Serif =="
open_settings
choose reading-font sans
close_settings
settle
expect_eq "sans drops the property" "" "$(val 'document.documentElement.style.getPropertyValue("--reading-font")')"
expect_eq "sans reads in Atkinson Hyperlegible Next" "true" "$(val 'getComputedStyle(document.querySelector(".page .prose p")).fontFamily.includes("Atkinson Hyperlegible Next")')"
expect_eq "the title follows the reading font" "true" "$(val 'getComputedStyle(document.querySelector(".page h1")).fontFamily.includes("Atkinson Hyperlegible Next")')"
snap "07b-text-x-large-sans.png"
open_settings
choose reading-font serif
close_settings
settle

echo "== resize both panels =="
expect_near "sidebar starts at its default" "240" "$(val 'document.querySelector(".sidebar").getBoundingClientRect().width')"
expect_eq "sidebar separator reports its width" "240" "$(val 'document.querySelector(".panel-resizer-left").getAttribute("aria-valuenow")')"
drag left 240 330
settle
expect_near "sidebar after a drag to 330" "330" "$(val 'document.querySelector(".sidebar").getBoundingClientRect().width')"
expect_eq "sidebar separator follows" "330" "$(val 'document.querySelector(".panel-resizer-left").getAttribute("aria-valuenow")')"
drag right 1000 880
settle
expect_near "study panel after a drag to 400" "400" "$(val 'document.querySelector(".margin").getBoundingClientRect().width')"
snap "08-panels-resized.png"

echo "== keyboard steps by 16 =="
ctl eval 'document.querySelector(".panel-resizer-left").focus()' >/dev/null
ctl press ArrowRight >/dev/null
settle
expect_near "sidebar after ArrowRight" "346" "$(val 'document.querySelector(".sidebar").getBoundingClientRect().width')"
ctl press ArrowLeft >/dev/null
settle
expect_near "sidebar after ArrowLeft" "330" "$(val 'document.querySelector(".sidebar").getBoundingClientRect().width')"

echo "== clamps keep the reading column readable =="
drag left 330 3000
settle
expect_near "sidebar stops where the reading column reaches 480px" "400" "$(val 'document.querySelector(".sidebar").getBoundingClientRect().width')"
expect_eq "reading column is at least 480px" "true" "$(val 'document.querySelector("main.document").getBoundingClientRect().width >= 480')"
drag left 420 330
settle

echo "== double-click resets one panel =="
ctl eval 'document.querySelector(".panel-resizer-right").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }))' >/dev/null
settle
expect_near "study panel back to default" "282" "$(val 'document.querySelector(".margin").getBoundingClientRect().width')"
drag right 1000 880
settle

echo "== Reset panel widths in Settings =="
open_settings
expect_eq "Reset is enabled" "false" "$(val 'document.querySelector(".settings-reset").disabled')"
ctl click ".settings-reset" >/dev/null || fail "reset"
settle
expect_near "sidebar default again" "240" "$(val 'document.querySelector(".sidebar").getBoundingClientRect().width')"
expect_near "study panel default again" "282" "$(val 'document.querySelector(".margin").getBoundingClientRect().width')"
expect_eq "Reset is disabled again" "true" "$(val 'document.querySelector(".settings-reset").disabled')"
close_settings

echo "== final choices, then reload =="
open_settings
choose theme dark
close_settings
drag left 240 330
drag right 1000 880
settle
snap "09-final-choices-before-reload.png"
ctl eval '(setTimeout(() => location.reload(), 50), true)' >/dev/null
sleep 2
must_text "chapter heading after reload" "main.document h1" "Power Distribution" 20000
ctl wait-for "figure.chapter-figure img" >/dev/null || fail "figure image after reload"
settle
expect_eq "theme persisted" "dark" "$(val 'document.documentElement.dataset.theme')"
expect_eq "text size persisted" "1.25" "$(val 'document.documentElement.style.getPropertyValue("--text-scale")')"
expect_eq "reading font persisted" "true" "$(val 'document.documentElement.style.getPropertyValue("--reading-font").includes("serif")')"
expect_eq "line width persisted" "42rem" "$(val 'document.documentElement.style.getPropertyValue("--line-width")')"
expect_near "sidebar width persisted" "330" "$(val 'document.querySelector(".sidebar").getBoundingClientRect().width')"
expect_near "study panel width persisted" "400" "$(val 'document.querySelector(".margin").getBoundingClientRect().width')"
expect_eq "figure keeps the dark palette" "visual://localhost/power/power-path.svg?theme=dark" "$(val 'document.querySelector("figure.chapter-figure img").getAttribute("src")')"
snap "10-after-reload.png"
open_settings
expect_eq "dialog shows dark" "true" "$(val 'document.querySelector("[aria-labelledby=\"settings-theme\"] [data-value=\"dark\"]").getAttribute("aria-checked") === "true"')"
expect_eq "dialog shows Extra large" "true" "$(val 'document.querySelector("[aria-labelledby=\"settings-text-size\"] [data-value=\"x-large\"]").getAttribute("aria-checked") === "true"')"
snap "11-settings-open-after-reload.png"
close_settings

ctl appearance system >/dev/null || fail "appearance system"

SNAP_OUT="${REPLAY_SNAP_DIR:-$ROOT/target/e2e-snapshots/65-snapshots}"
mkdir -p "$SNAP_OUT"
cp -p "$SNAP"/*.png "$SNAP_OUT/"
echo "replay proof complete: snapshots copied to $SNAP_OUT"
