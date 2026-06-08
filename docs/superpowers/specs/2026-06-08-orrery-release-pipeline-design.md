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

## Frontend refactor: boot splash → routed LoadingComponent

Today the splash is inline in `src/index.html` (markup + animation script) and
`AppComponent` *is* the whole shell, calling `window.__orreryAppReady()` from
`ngAfterViewInit`. We restructure so the loading screen is a real, routed
component and the app boots through it.

- **`src/index.html`** → strip the inline splash markup and script. Keep only
  `<app-root>`. Set the first-paint background to the splash dark
  (`html, body { background: #07080d; }`) so there is no white flash before
  Angular paints (the bundle is local in a Tauri app, so this gap is brief).
- **`AppComponent` (`app-root`)** → template becomes just `<router-outlet />`.
  It no longer imports the shell pieces or calls `__orreryAppReady` (that
  mechanism is removed).
- **New `ShellComponent` (`src/app/shell/shell.component.ts`)** → receives the
  current `AppComponent` template + logic verbatim (top-bar, sidebar, workspace,
  modals, dev panel, etc.). This is "the real app."
- **New `LoadingComponent` (`src/app/loading/loading.component.ts`)** → ports the
  existing splash visuals into Angular: the epicycle SVG draw, the gradient
  progress bar, and the rotating status line, with progress/status bound to
  signals instead of DOM ids. Hosts the boot + updater sequence and redirects to
  the shell when done.
- **Routes (`src/app/app.routes.ts`)** — loading has first priority:
  - `{ path: '', component: LoadingComponent }`  (default / first)
  - `{ path: 'app', component: ShellComponent }`
  - `{ path: '**', redirectTo: '' }`

**Trade-off (accepted):** moving the splash into Angular means it paints after
bootstrap rather than on the literal first frame. The dark `body` background
covers the brief pre-paint gap, so the visible result is a dark window → splash,
never a white flash.

## Updater flow (inside LoadingComponent)

`UpdaterService` encapsulates the Tauri updater calls and exposes progress +
status as signals the LoadingComponent binds to its bar. On init:

1. Start the epicycle draw animation (visual continuity with today's splash).
2. Not running under Tauri (e.g. `ng serve` in a browser) → once the draw
   completes, `router.navigate(['/app'])`.
3. Under Tauri → run `check()` (≤10s timeout) concurrently with the draw:
   - No update / error / offline → navigate to `/app` once the draw completes.
   - Update found → status `downloading update · <ver>`, bind the bar to
     `downloadAndInstall(onEvent)` byte-progress, then `relaunch()` (from
     plugin-process) into the new version.
4. **Hard safety timeout** (~12s): navigate to `/app` no matter what, so a hung
   check or download can never trap the user on the loading screen.

## Error handling

The updater is fully best-effort. Any failure (no network, bad/missing manifest,
download error, verification failure) falls through to a normal boot — the app is
never bricked by the updater. The `LoadingComponent` hard safety timeout (~12s)
is the backstop: if a check or download hangs, it navigates to the app anyway.

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
- Deep-linking / multiple app routes — the shell stays a single `app` route; the
  router is introduced only to gate the app behind the loading screen.
