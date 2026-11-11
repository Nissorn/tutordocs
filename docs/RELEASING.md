# Release Guide

This document describes the release pipeline for TutorDocs, including one-time signing setup, version bumping, triggering CI release builds, and user installation instructions for macOS.

## One-time Secrets Setup

The release workflow uses Tauri's updater signing keys to sign update bundles and generate `latest.json`.

1. Generate keypair (if not already created):
   ```bash
   mkdir -p ~/.tauri
   npm run tauri signer generate -- --ci -w ~/.tauri/tutordocs-updater.key
   ```
2. Put the public key in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
3. Add repository secrets in GitHub under `Settings > Secrets and variables > Actions`:
   - `TAURI_SIGNING_PRIVATE_KEY`: The text of your private key from `~/.tauri/tutordocs-updater.key`.
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the key's password. GitHub refuses an empty secret, so if the key has no password, do not create this secret at all. The workflow then passes an empty string, which is what a password-less key needs.

> [!WARNING]
> Back up `~/.tauri/tutordocs-updater.key` in a secure location (such as an encrypted vault or password manager). Losing this updater private key means existing installed apps can never update again, because they verify updates against the embedded public key. Never commit or paste the private key into the repository.

## Files Holding the Version

When bumping the version (for example, from `0.1.0` to `1.0.0`), update the version string in every one of the following files:

1. `src-tauri/tauri.conf.json` (`version` field)
2. `Cargo.toml` (`[workspace.package].version` field)
3. `package.json` (`version` field)
4. `ui/package.json` (`version` field)
5. `ui/package-lock.json` (top-level and packages[""] `version` fields, or run `npm --prefix ui install --package-lock-only`)

## Step-by-Step Release Process

1. **Bump the version**: Update the version number across all 5 files listed above.
2. **Verify locally**:
   ```bash
   bash scripts/check.sh
   ```
   To build the signed bundles on this Mac, set both variables and close stdin. Without `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `tauri build` waits forever at a hidden password prompt after the bundles are made:
   ```bash
   TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/tutordocs-updater.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" npm run tauri build < /dev/null
   ```
3. **Commit the version bump**:
   ```bash
   git commit -am "Release v1.0.0"
   ```
4. **Create a tag**:
   ```bash
   git tag v1.0.0
   ```
5. **Push commit and tag to GitHub**:
   ```bash
   git push origin main
   git push origin v1.0.0
   ```
6. **Wait for CI build**: GitHub Actions will run `.github/workflows/release.yml`, building macOS artifacts for both Apple Silicon (`aarch64-apple-darwin`) and Intel (`x86_64-apple-darwin`). It bundles `.dmg`, `.app.tar.gz`, signature `.sig` files, and `latest.json`.
7. **Publish draft release**: Navigate to GitHub Releases, locate the newly generated draft release, review the artifacts and changelog, and click **Publish release**.

## First-time Installation on macOS

Because TutorDocs is open-source and free without a paid Apple Developer certificate, macOS Gatekeeper prevents opening downloaded applications from unidentified developers by default.

When opening TutorDocs for the first time:
If macOS says TutorDocs cannot be opened, open **System Settings > Privacy & Security**, scroll to the Security section, and click **Open Anyway**. Recent macOS versions no longer offer the right-click Open shortcut for this. Users comfortable with Terminal can instead run `xattr -dr com.apple.quarantine /Applications/TutorDocs.app`. macOS remembers the choice, so later launches open normally.
