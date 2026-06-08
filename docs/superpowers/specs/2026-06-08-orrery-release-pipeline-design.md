# Orrery Release Pipeline + Auto-Updater — Design

**Date:** 2026-06-08
**Status:** Approved (design); pending spec review
**Scope:** Windows-only for now (matrix-ready to add macOS/Linux later)

## Goal

Set up GitHub Actions in the **code repo** (`kouji-dev/orrery`) that, on a manual
trigger, builds a Windows installer and publishes a release into a **separate
releases repo** (`kouji-dev/orrery-releases`). The published release includes
Tauri auto-updater artifacts so the app can update itself on launch, driving the
existing boot splash's progress bar.

## Architecture (Option A)

The release workflow lives in `orrery`, next to the code. It runs in two phases:

1. **Build** — build the Windows installer + signed updater artifacts.
2. **Publish** — use a fine-grained PAT to create the release in
   `orrery-releases` and hand-write `latest.json` for the updater.

tauri-action only auto-generates `latest.json` for same-repo releases, so for a
cross-repo release we construct `latest.json` ourselves in the publish phase.

## One-time prerequisites (manual, outside CI)

- **`orrery-releases` is public.** The updater fetches `latest.json` over plain
  HTTPS with no auth; a private repo's assets are unreachable by the shipped app.
- **Fine-grained PAT** scoped to *only* `orrery-releases`, permission
  *Contents: Read and write*. Stored in `orrery` repo secrets as `RELEASES_TOKEN`.
- **Signing keypair** generated via `tauri signer generate`:
  - Public key → committed in `tauri.conf.json` (`plugins.updater.pubkey`).
  - Private key → `orrery` secret `TAURI_SIGNING_PRIVATE_KEY`.
  - Key password → `orrery` secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

## Trigger & version flow

- File: `.github/workflows/release.yml`.
- `workflow_dispatch` with inputs:
  - `version` (required, e.g. `0.2.0`)
  - `notes` (optional release-notes text; falls back to `--generate-notes`)
- Runs against `main` (default ref of the dispatch).
- A step **stamps** `version` into `tauri.conf.json`, `package.json`, and
  `Cargo.toml` **for the build only — no commit back to the repo.**
  - Why this matters: the updater compares the *installed app's* version against
    the manifest version. The built binary, the release tag `v<version>`, and
    `latest.json` must all agree. If they drift, the app either reports "update
    available" forever or never updates. Stamping at build time guarantees
    agreement without coupling releases to a committed version bump.

## Build phase (job: `build`, runs on `windows-latest`)

- Checkout, set up Node + pnpm, set up Rust (with cache).
- `pnpm install`.
- Run `tauri-apps/tauri-action` (build only — no `tagName`, so it does not try to
  create a same-repo release), with signing env vars present so updater artifacts
  and `.sig` files are produced.
- Bundle targets stay `"all"` (MSI + NSIS). The updater targets the **NSIS
  `*-setup.exe`** (Tauri's recommended Windows updater installer).
- Upload as workflow artifacts: the NSIS `*-setup.exe`, the MSI, the updater
  artifact, and the `.sig` file(s).

Structured as a single job now; can become a one-entry `matrix` to add
`macos-latest` (`--target universal-apple-darwin`) and `ubuntu-22.04` later
without restructuring.

## Publish phase (job: `publish`, `needs: build`)

- Download all workflow artifacts.
- Create the release in the other repo:
  `gh release create v<version> --repo kouji-dev/orrery-releases --target main`
  - Auth: `GH_TOKEN=${{ secrets.RELEASES_TOKEN }}` (the PAT).
  - Notes: the `notes` input if provided, else `--generate-notes`.
  - **Published immediately, not a draft, not a prerelease** — required so the
    updater's `releases/latest/download/...` endpoint resolves to it.
  - Upload assets: NSIS `*-setup.exe`, MSI, updater artifact.
- Build `latest.json` with a small script that reads the `.sig` contents and the
  uploaded NSIS asset's download URL:
  ```json
  {
    "version": "<version>",
    "notes": "<notes or short text>",
    "pub_date": "<ISO-8601, stamped at publish time>",
    "platforms": {
      "windows-x86_64": {
        "signature": "<contents of the .sig file>",
        "url": "https://github.com/kouji-dev/orrery-releases/releases/download/v<version>/<nsis-setup>.exe"
      }
    }
  }
  ```
- Upload `latest.json` to the release as an asset.

## Updater wiring (committed in `orrery`)

- **Cargo (`src-tauri/Cargo.toml`):** add `tauri-plugin-updater` and
  `tauri-plugin-process`; register both in the Rust builder.
- **npm (`package.json`):** add `@tauri-apps/plugin-updater` and
  `@tauri-apps/plugin-process`.
- **`tauri.conf.json`:**
  - `bundle.createUpdaterArtifacts: true`
  - `plugins.updater.endpoints = ["https://github.com/kouji-dev/orrery-releases/releases/latest/download/latest.json"]`
  - `plugins.updater.pubkey = "<generated public key>"`
- **`capabilities/default.json`:** add `updater:default` and
  `process:allow-restart`.

## Splash-integrated update UX (frontend)

The boot splash in `src/index.html` already has a progress bar
(`#orrery-boot-bar`), a status line (`#orrery-boot-status`), and a
`window.__orreryAppReady()` handoff. We reuse all of it.

**Splash API** — expose from the inline splash script:
`window.__orreryBoot = { takeover(), setStatus(text), setProgress(0..1), finish() }`
- `takeover()` sets a flag so the splash's own 2.2s bar/status animation stops
  writing to the DOM (no fight over the bar).
- `setStatus` / `setProgress` drive the existing elements directly.
- `finish()` is the existing app-ready handoff (fade + remove).

**UpdaterService** — runs during Angular bootstrap, *before* signaling app-ready:
1. If not running under Tauri (e.g. `ng serve` in a browser) → skip, `finish()`.
2. `check()` the manifest with a ~10s timeout.
3. No update / error / offline → `finish()` (app boots normally).
4. Update found → `takeover()`, status `downloading update · <ver>`, then
   `downloadAndInstall(onEvent)` mapping the byte-progress events to
   `setProgress(...)`. On finish → `relaunch()` (from plugin-process) into the
   new version.

## Error handling

The updater is fully best-effort. Any failure (no network, bad/missing manifest,
download error, verification failure) falls through to a normal boot — the app is
never bricked by the updater. The existing 8s splash safety-timeout remains as a
backstop in case a check hangs.

## Known limitation (accepted for now)

Builds are **unsigned** (no Windows code-signing cert). First install shows a
Windows SmartScreen warning. The minisign-based updater is unaffected and still
verifies updates. OS code-signing is deferred to a later iteration.

## Out of scope

- macOS / Linux builds (matrix-ready to add later).
- OS code-signing / notarization.
- In-app "check for updates" button / settings toggle / changelog UI (the launch
  flow is the only updater entry point for now).
- Committing version bumps back to the repo.
