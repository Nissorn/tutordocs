# TutorDocs

A free, open-source macOS app for studying, exam prep and interview prep. TutorDocs turns your own notes into a course with chapters, quizzes and remembered progress. The coding agents already on your machine write the content and visuals, so there are no paid API keys and no hand-built databases.

## Supported agents

TutorDocs drives the agent CLIs you already have, on your existing subscription login:

- Claude (Claude Code)
- Qoder
- agy (Antigravity)

More agents are on the way. The app probes each CLI at startup and lists the ones it finds, with a reason for any it cannot use.

## Install

1. Download the `.dmg` for your Mac from [GitHub Releases](https://github.com/Nissorn/tutordocs/releases). Apple Silicon and Intel builds are both published.
2. Open the `.dmg` and drag TutorDocs to Applications.
3. On first launch macOS Gatekeeper blocks the app, because TutorDocs is free and open-source without a paid Apple Developer certificate. Open **System Settings > Privacy & Security**, scroll to the Security section, and click **Open Anyway**. macOS remembers the choice, so later launches open normally.

If you prefer Terminal, run `xattr -dr com.apple.quarantine /Applications/TutorDocs.app` instead of the Open Anyway step. Recent macOS versions no longer offer the right-click Open shortcut.

## Keyless by design

TutorDocs uses your existing agent subscription, never a pay-as-you-go API key. Sign in to your agent CLI once in Terminal, and the app reuses that login. A set `ANTHROPIC_API_KEY` is removed from the environment before any agent starts, so a stray key cannot silently meter billing. The same scrub covers each agent's own credential and endpoint variables.

## Build from source

You need Rust, Node.js and Xcode on macOS.

```bash
npm install
npm run tauri dev      # run the app in development
bash scripts/check.sh  # formatting, clippy, tests, typecheck, prose guard
```

To build a release bundle locally, see [docs/RELEASING.md](docs/RELEASING.md). It covers the updater signing key, the version bump and the CI release workflow.

## Command line

`tutordocs-cli` calls the same store and engine functions the app uses, so a script can walk the backend without driving the screen. See [docs/cli.md](docs/cli.md).

## License

TutorDocs is MIT licensed, see [LICENSE](LICENSE). Portions of the engine are translated from [DeepTutor](https://github.com/HKUDS/DeepTutor), which is Apache-2.0. See [NOTICE](NOTICE) and [LICENSE-APACHE](LICENSE-APACHE).