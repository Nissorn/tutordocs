# TutorDocs glossary

Domain terms for this effort, as locked by [PROPOSAL.md](PROPOSAL.md). Use these
words in tickets, tests and code; do not drift to synonyms. Definitions stay
short on purpose; the proposal owns the reasoning.

## Content

- **shelf**: The folder the user chooses. Holds `courses/<course-id>/` and a
  root `.tutordocs/` with app-wide settings only (`agent.json`).
- **library**: One course folder on the shelf. Holds `sources/`,
  `chapters/<id>/`, `course.json` and a `.tutordocs/` with `store.db`, staging
  and `meta.json`. The markdown files are the source of truth; anything
  derived is a rebuildable cache.
- **source**: An imported document under `sources/`: a PDF, a markdown file or
  a URL capture. Sources feed agent research; they are not edited by the app.
- **course**: A set of chapters with objectives and an order, described by
  `course.json`, living in one library folder. Its id is the folder name and
  does not change. Created outline-first.
- **outline**: The chapter order and objectives the agent writes into
  `course.json` before any chapter content exists.
- **chapter**: Agent-written markdown under `chapters/<id>/` plus its
  `visuals/` directory. Generated lazily, one agent job, when first opened.
- **placeholder**: A chapter directory that has no content yet. Opening it
  triggers the generation job; the UI paints a cheap skeleton meanwhile.

## Marks

- **annotation**: An anchor plus a payload, stored in SQLite. The payload is a
  highlight, a margin note or a bookmark. Never derived from chapter content,
  so regeneration cannot silently destroy it.
- **anchor**: Where an annotation attaches: a W3C text quote selector with a
  text position selector fallback. On regeneration the host re-anchors by
  quote match and flags every failure for the user.
- **highlight**: An annotation payload that marks a passage with a color.
- **margin note**: An annotation payload holding the reader's own note,
  rendered in the right margin beside its anchor.

## Learning

- **mastery event**: One append-only log entry per objective: event type,
  payload, session, turn and timestamp. Event sourcing gives undo and audit.
- **repetition state**: The spaced-repetition state derived from mastery
  events for one objective. Derived, rebuildable, never hand-edited.

## Generation

- **job**: One agent invocation: a prompt, an optional JSON schema, a workdir,
  a capability set, a timeout and a resumable session id.
- **driver**: The Rust adapter that runs one installed agent CLI under the
  `AgentDriver` trait. Keyless only; the driver scrubs `ANTHROPIC_API_KEY`
  before spawn.
- **visual artifact**: A directory holding `index.html` plus `manifest.json`,
  zero network, rendered in an iframe sandboxed to exactly `allow-scripts`.
  For the three tier the only sibling script is the vendored
  `three.module.min.js` pinned to r169.