#!/bin/sh
# Fake claude CLI for the URL box tests. No network, no quota, no auth.
# Told to fetch a page, it reads URL and OUTPUT_FILE from the prompt and acts
# as FAKE_MODE says, then speaks stream-json. The test bakes the variables in.
case "$1" in
  --version|-v)
    echo "2.1.319 (Claude Code)"
    exit 0
    ;;
  --help|-h)
    cat <<'HELP'
Usage: claude [options] [prompt]
  -p, --print                     Print response and exit
  --output-format <format>        text|json|stream-json
  --verbose                       Override verbose mode setting from config
  --json-schema <schema>          JSON Schema for structured output
  -r, --resume [value]            Resume a conversation by session ID
  --append-system-prompt <prompt> Append a system prompt
  --tools <tools...>              Restrict the tools a job may use
  --allowedTools, --allowed-tools <tools...>
  --permission-mode <mode>        Permission mode
HELP
    exit 0
    ;;
esac
PROMPT="$2"
URL=$(printf '%s\n' "$PROMPT" | sed -n 's/^URL: //p' | head -n 1)
OUT=$(printf '%s\n' "$PROMPT" | sed -n 's/^OUTPUT_FILE: //p' | head -n 1)
printf '%s\n' "$URL" >> "$FAKE_URL_LOG"
printf '%s\n' '{"type":"system","subtype":"init","session_id":"sess-fake"}'
case "$FAKE_MODE" in
  write)
    mkdir -p "$(dirname "$OUT")"
    printf '# Example page\n\nFetched by the fake agent.\n' > "$OUT"
    printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"Saved the page.","session_id":"sess-fake"}'
    exit 0
    ;;
  fail)
    mkdir -p "$(dirname "$OUT")"
    printf '# Half a page' > "$OUT"
    printf '%s\n' '{"type":"result","subtype":"success","is_error":true,"result":"Could not fetch the page: HTTP 404 Not Found","session_id":"sess-fake"}'
    exit 1
    ;;
esac
echo "fake-claude: unknown FAKE_MODE '$FAKE_MODE'" >&2
exit 2
