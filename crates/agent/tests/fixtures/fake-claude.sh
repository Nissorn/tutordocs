#!/bin/sh
# Fake claude CLI for unit tests. No network, no quota, no auth.
case "$1" in
  --version|-v)
    echo "2.1.319 (Claude Code)"
    exit 0
    ;;
  --help|-h)
    if [ "$FAKE_CLAUDE_HELP_MODE" = "minimal" ]; then
      echo "Usage: claude [options] [prompt]"
      echo "  -p, --print  Print response and exit"
      exit 0
    fi
    cat <<'HELP'
Usage: claude [options] [prompt]
  -p, --print                     Print response and exit
  --output-format <format>        text|json|stream-json
  --verbose                       Override verbose mode setting from config
  --json-schema <schema>          JSON Schema for structured output
  -r, --resume [value]            Resume a conversation by session ID
  --append-system-prompt <prompt> Append a system prompt
  --allowedTools, --allowed-tools <tools...>
  --permission-mode <mode>        Permission mode (choices: "acceptEdits", "dontAsk", "plan")
HELP
    if [ "$FAKE_CLAUDE_HELP_MODE" != "no_tools" ]; then
      echo "  --tools <tools...>              Specify the list of available tools"
    fi
    exit 0
    ;;
esac
# The real CLI refuses stream-json in print mode without --verbose.
case " $* " in
  *" stream-json "*)
    case " $* " in
      *" --verbose "*) ;;
      *) echo "Error: When using --print, --output-format=stream-json requires --verbose" >&2; exit 1 ;;
    esac
    ;;
esac
if [ -n "$FAKE_CLAUDE_ENV_DUMP" ]; then env > "$FAKE_CLAUDE_ENV_DUMP"; fi
if [ -n "$FAKE_CLAUDE_ARGV_DUMP" ]; then printf '%s\n' "$@" > "$FAKE_CLAUDE_ARGV_DUMP"; fi
case "$FAKE_CLAUDE_MODE" in
  hang)
    exec sleep 30
    ;;
  release)
    printf '%s\n' '{"type":"system","subtype":"init","session_id":"sess-fake"}'
    while [ ! -f "$FAKE_CLAUDE_RELEASE" ]; do
      sleep 0.05
    done
    printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"Finished after the wait."}]},"session_id":"sess-fake"}'
    printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"Finished after the wait.","session_id":"sess-fake"}'
    exit 0
    ;;
  badjson)
    printf '%s\n' '{"type":"system","subtype":"init","session_id":"sess-fake"}'
    printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"{\"answer\":\"wrong-type\"}","session_id":"sess-fake"}'
    exit 0
    ;;
  goodjson)
    printf '%s\n' '{"type":"system","subtype":"init","session_id":"sess-fake"}'
    printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"{\"answer\":42}","session_id":"sess-fake"}'
    exit 0
    ;;
esac
printf '%s\n' '{"type":"system","subtype":"init","session_id":"sess-fake","tools":[]}'
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]},"session_id":"sess-fake"}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"Hello","session_id":"sess-fake","usage":{"input_tokens":10,"output_tokens":2}}'
exit 0