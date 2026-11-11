#!/bin/sh
# Fake qoder/agy CLI for unit tests. No network, no quota, no auth.
# The installer bakes shell-local assignments above this line: FAKE_DIR,
# FAKE_VERSION, FAKE_HELP_STREAM (out|err), FAKE_MODE, FAKE_ARGV_DUMP,
# FAKE_ENV_DUMP, FAKE_STDERR. It reads help.txt and stream.ndjson from FAKE_DIR.
case "$1" in
  --version|-v)
    if [ "$FAKE_MODE" = "version_hang" ]; then exec sleep 30; fi
    echo "$FAKE_VERSION"
    exit 0
    ;;
  --help|-h)
    if [ "$FAKE_HELP_STREAM" = "err" ]; then cat "$FAKE_DIR/help.txt" >&2; else cat "$FAKE_DIR/help.txt"; fi
    exit 0
    ;;
esac
if [ -n "$FAKE_ENV_DUMP" ]; then env > "$FAKE_ENV_DUMP"; fi
if [ -n "$FAKE_ARGV_DUMP" ]; then printf '%s\0' "$@" > "$FAKE_ARGV_DUMP"; fi
case "$FAKE_MODE" in
  hang)
    # An init line, then silence: the reported non-TTY hang shape.
    head -n 1 "$FAKE_DIR/stream.ndjson"
    exec sleep 30
    ;;
  hang_silent)
    exec sleep 30
    ;;
  fail)
    echo "$FAKE_STDERR" >&2
    exit 3
    ;;
esac
cat "$FAKE_DIR/stream.ndjson"
exit 0
