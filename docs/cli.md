# tutordocs-cli

The desktop app stays `tutordocs`. This binary is `tutordocs-cli`. It calls the same store and engine functions the app commands call, so a script can walk the backend without driving the screen.

Stdout is one JSON value. A job prints each `agent-job-line` payload as one JSON line on stderr while it runs. Exit 0 is success. Exit 1 is a refused or failed command, and stdout is `{"error": "..."}`. Exit 2 is a usage error, and stdout is empty. Ctrl-C during `outline approve --generate all` lets the running chapter job finish, skips the rest, prints an error whose text contains `interrupted`, and exits 130.

The library is `--library`, then `TUTORDOCS_LIBRARY`, then `~/TutorDocs`. The course is `--course`, an id from `courses`. `--agent`, `--model` and `--effort` override the saved picker settings for that process. They do not write `.tutordocs/agent.json`.

## Commands

```text
tutordocs-cli courses
tutordocs-cli course create <title> [--tag <tag>]...
tutordocs-cli course setup get --course <id>
tutordocs-cli course setup set --course <id> --template <id> --level <new|some|strong> [--hours <n>] --language <th|en> [--guide <text>]
tutordocs-cli course setup set --course <id> --file setup.json
tutordocs-cli templates
tutordocs-cli import --course <id> <path>...
tutordocs-cli sources --course <id>
tutordocs-cli outline generate --course <id> [--topic <text>] [--source <name>]...
tutordocs-cli outline show --course <id>
tutordocs-cli outline save --course <id> <file>
tutordocs-cli outline revise --course <id>
tutordocs-cli outline approve --course <id> [--generate on_open|all]
tutordocs-cli queue --course <id>
tutordocs-cli chapter list --course <id>
tutordocs-cli chapter open --course <id> --chapter <id>
tutordocs-cli chapter rewrite --course <id> --chapter <id>
tutordocs-cli section revise --course <id> --chapter <id> --quote <text> --instruction <text>
tutordocs-cli ask --course <id> --chapter <id> [--quote <text>] <question>
tutordocs-cli answers --course <id> --chapter <id>
tutordocs-cli context --course <id> --chapter <id>
```

`outline approve` defaults to `--generate on_open`, which marks the outline approved and writes nothing. `--generate all` writes every unwritten chapter in this process, one at a time, in outline order. `outline revise`, `chapter open` and `outline approve --generate all` run until the job finishes, then print the result. `queue` reports only the queue in this process.

## Worked example

Create a course, import two notes, plan an outline, approve it, and write every chapter. Point `--library` at a folder you can throw away. The agent is the same keyless CLI the app uses. A set `ANTHROPIC_API_KEY` is removed before that CLI starts.

```sh
export TUTORDOCS_LIBRARY=/tmp/tutordocs-demo
tutordocs-cli course create "Cell membranes" --tag biology
# take the id from the JSON, then:
tutordocs-cli course setup set --course "$ID" \
  --template exam --level some --hours 3 --language en \
  --guide "focus on transport"
tutordocs-cli import --course "$ID" ./notes.md ./glossary.txt
tutordocs-cli outline generate --course "$ID" --topic "Cell membranes" \
  --source notes.md --source glossary.txt
tutordocs-cli outline approve --course "$ID" --generate all
tutordocs-cli chapter list --course "$ID"
```

Job lines arrive on stderr. The outline or chapter JSON arrives on stdout when the command exits 0.
