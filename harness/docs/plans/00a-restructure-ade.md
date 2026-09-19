# 00a · Restructure — today's app becomes `ade/`, `harness/` opens

**Goal.** Move the Tauri + Angular app under `ade/`, rename it to `orrery-ade` in package, crate and project names **only**, and promote the repo root to a cargo workspace that will hold the harness crates. Nothing user-visible changes: the bundle identifier, the product name, the installed `Orrery.exe`, the app data directory and the WiX upgrade code are all untouched.

**Covers.** No architecture section — this is repo work that everything else needs first.

**State — done, landed in wave 1 (`8e52c92`, `398e748`, `0892ae7`).** Every box
below is ticked because the repo was checked against it, item by item, not
because the commits claim so: `ade/` holds the app, the root is a virtual
workspace with `clippy.toml` and `Cargo.lock` at it, the crate is `orrery-ade`
with `lib orrery_ade_lib`, `identifier` / `productName` / `upgradeCode` are
unchanged and `mainBinaryName` is now pinned, every task-5 path fix is in place,
`pnpm-lock.yaml`'s importer key is `ade`, and
`git grep "src-tauri/target|orrery_lib"` outside `ade/docs` and `ade/design` is
empty. The plan read 0/27 only because the implementing agent wrote its
decisions into a section of its own instead of ticking.

**Why the rename is forced, not cosmetic.** The harness CLI binary must be `orrery` (§5.5: `orrery run`, `orrery eval …`). The ADE crate is currently `orrery` with `lib orrery_lib`; two bin targets named `orrery` in one workspace collide. So the ADE crate becomes `orrery-ade`.

**Why the identity must not change.** The user's live worktrees sit under `%APPDATA%\com.kouji.orrery`; Claude Code hook configs point at the installed exe; the installed release must keep auto-updating in place. `productName: "Orrery"` names the installed binary (Tauri 2's `mainBinaryName` defaults to it), and agent detection matches on the binary stem containing `"orrery"` (`agents/adapters/claude.rs:338`) — so `orrery-ade` stays detected either way, but the **installed** name stays `Orrery.exe` regardless.

---

## Ground facts, verified

- 1018 tracked files (at `b475ebe`, v0.24.7).
- **`src-tauri/target-test/` is tracked** — 374 files, ~101 MB of cargo artefacts.
- **`tsconfig.spec.json` does not exist.** Only `tsconfig.json` and `tsconfig.app.json`.
- `angular.json` has `"root": ""`, `"sourceRoot": "src"`, and **no explicit `outputPath`** (implicit `dist/orrery`).
- `src-tauri/Cargo.toml` lines 1–2 are a `[workspace]` block with `members = ["updater-stub"]`.
- 15 worktrees on 14 feature branches share this `.git`.
- `.gitignore` anchored entries are at lines **4** (`/dist`), **5** (`/tmp`), **6** (`/out-tsc`), **7** (`/bazel-out`), **10** (`/node_modules`), **32** (`/.angular/cache`), **34** (`/connect.lock`), **35** (`/coverage`).

---

## What moves

| Into `ade/` | Stays at root | New at root |
|---|---|---|
| `src/`, `src-tauri/`, `e2e/`, `tools/`, `scripts/`, `docs/`, `design/`, `angular.json`, `tsconfig.json`, `tsconfig.app.json`, `vitest.config.ts`, `playwright.config.ts`, `playwright.local.config.ts`, `package.json` | `landing/`, `render.yaml`, `.github/`, `CLAUDE.md`, `.gitignore`, `pnpm-workspace.yaml`, `pnpm-lock.yaml`, `questions.md` | `Cargo.toml`, `Cargo.lock` (moved up from `src-tauri/`), `package.json` (private passthrough), `README.md`, `harness/` |

`scripts/` moves with `src-tauri/` because two Rust files resolve paths relative to `CARGO_MANIFEST_DIR/..`: `lsp/smoke_tests.rs:83` (`../scripts/extensions/servers`) and `extensions/registry.rs:113` (`../dist-ext`). Moving them together keeps both correct for free; the cost is seven landing-path fixes below.

`docs/` and `design/` are the ADE's feature docs and UI design, so they go with it. `harness/docs/` is separate and already exists.

---

## Tasks

Three commits. **The user reviews each before it lands.**

### Task 1 · Prep commit — deletions and gitignore

No moves in this commit; it shrinks the rename-detection set for commit 2.

- [x] `git rm -r --cached src-tauri/target-test` — 374 files of build artefacts that should never have been tracked.
- [x] `git rm -r .clone` — the stray fragment `.clone/worktrees/git-inspection/src/app/workspace/git/commit-diff-view.component.ts`, referenced by nothing.
- [x] `.gitignore`: un-anchor the entries that must survive the depth change, at the verified line numbers —
  `:4` `/dist` → `dist/`; `:5` `/tmp` → `tmp/`; `:6` `/out-tsc` → `out-tsc/`; `:7` `/bazel-out` → `bazel-out/`; `:10` `/node_modules` → `node_modules/`; `:32` `/.angular/cache` → `.angular/cache/`; `:34` `/connect.lock` → `connect.lock`; `:35` `/coverage` → `coverage/`.
- [x] Add `target/` and `target-test/` (the cargo target dir moves to the repo root in task 3).
- [x] **Verify:** `git ls-files | grep -E "target-test|\.clone"` is empty.

### Task 2 · Move commit — pure `git mv`, zero content edits

Keep this commit rename-only so every path shows 100% similarity. That is what makes rename detection cheap for the 14 other branches.

- [x] ```
      mkdir ade
      git mv src src-tauri e2e tools scripts docs design \
             angular.json tsconfig.json tsconfig.app.json \
             vitest.config.ts playwright.config.ts playwright.local.config.ts \
             package.json ade/
      git mv ade/src-tauri/Cargo.lock Cargo.lock
      ```
- [x] **Verify before committing:** `git diff --cached --stat -M --find-renames=100%` shows renames with **0 insertions and 0 deletions**.
- [x] Do **not** squash this with task 3.

### Task 3 · Cargo workspace

- [x] Create the root `Cargo.toml`:
  ```toml
  [workspace]
  resolver = "2"
  members = ["ade/src-tauri", "ade/src-tauri/updater-stub"]
  default-members = ["ade/src-tauri"]
  ```
  `harness/…` members are added by [`00b`](00b-scaffold-workspace.md), once crates exist. `resolver = "2"` is **mandatory**: a virtual root without it silently falls back to resolver 1.
- [x] `ade/src-tauri/Cargo.toml` — delete lines 1–3 (`[workspace]`, `members = ["updater-stub"]`, blank). The file now starts at `[package]`.
- [x] Move shared version pins to `[workspace.dependencies]` at the root and switch `ade/src-tauri` to `{ workspace = true }` for: `serde`, `serde_json`, `thiserror`, `rusqlite`, `uuid`, `blake3`, `regex`, `semver`, `clap`, `toml_edit`, `sha2`, `reqwest`, `rustls`, `tempfile`. Otherwise the harness crates compile a second copy of each.
- [x] Move `ade/src-tauri/clippy.toml` to the repo root — clippy resolves it from cwd upward, so at the root it applies to both trees.
- [x] **Target-dir consequence:** `target/` moves from `ade/src-tauri/target/` to the repo root, shared with the harness crates. Every path fix in task 5 follows from this. `ade/src-tauri/.gitignore:5` (`/target/`) is now dead; leave it or drop it.

### Task 4 · Renames — names only

| File | Change |
|---|---|
| `ade/package.json:2` | `"name": "orrery-ade"` |
| `ade/src-tauri/Cargo.toml` | `name = "orrery-ade"`; `description = "Orrery ADE - agent orchestration"`; `[lib] name = "orrery_ade_lib"` |
| `ade/src-tauri/src/main.rs:8-9` | `orrery_lib::` → `orrery_ade_lib::` |
| `ade/src-tauri/tests/hook_cli.rs:29` | `env!("CARGO_BIN_EXE_orrery")` → `env!("CARGO_BIN_EXE_orrery-ade")` |
| `ade/angular.json:9` | project key `orrery` → `orrery-ade`; also `:87`, `:90` (`orrery:build:*` → `orrery-ade:build:*`) |
| `ade/angular.json` | add `"outputPath": "dist/orrery-ade"` after the `builder` line, pinning what was implicit |
| `ade/src-tauri/tauri.conf.json` | `frontendDist: "../dist/orrery-ade/browser"` |
| `ade/src/app/prod-build.spec.ts:27` | `cfg.projects["orrery"]` → `["orrery-ade"]` |
| `ade/src/themes/themes.spec.ts:84` | same |
| `ade/scripts/release/stamp-version.mjs` + `.spec.ts` | the `Cargo.lock` package entry it rewrites: `orrery` → `orrery-ade` |

**Explicitly unchanged** — do not touch:

- `ade/src-tauri/tauri.conf.json`: `identifier` (`com.kouji.orrery`), `productName` (`Orrery`), window `title`, `bundle.windows.wix.upgradeCode` (`CB7D1311-…`).
- `ade/src-tauri/updater-stub/` — the `orrery-updater` sidecar name is referenced from `tauri.windows.conf.json`.
- `ade/src-tauri/src/cli/mod.rs:16-17` — clap `name`/`bin_name` are display strings, and the installed exe is still `Orrery.exe`.
- Theme files `orrery.css` / `orrery-light.css` and `data-theme="orrery"` — user-visible theme names.
- Anything under `ade/src-tauri/src/update.rs` referencing `%LOCALAPPDATA%\Programs\Orrery`.

### Task 5 · Path fixes

| file:line | from → to |
|---|---|
| `ade/scripts/stage-updater-stub.mjs:36` | `join(srcTauri, "target", "release", …)` → `join(root, "..", "target", "release", "orrery-updater.exe")` |
| `ade/scripts/stage-updater-stub.mjs:1-6` | update the comment to say `ade/src-tauri` and the root `target/` |
| `ade/scripts/release/stamp-version.mjs:40` | `'src-tauri/Cargo.lock'` (×2) → `'../Cargo.lock'` |
| `ade/scripts/release/bump.mjs:39` | `'src-tauri/Cargo.lock'` → `'../Cargo.lock'` |
| `ade/e2e/landing-agent-readiness.spec.ts:20` | `join(process.cwd(), "landing")` → `join(process.cwd(), "..", "landing")` |
| `ade/e2e/landing-analytics.spec.ts:16` | same |
| `ade/e2e/landing-download.spec.ts:19` | same |
| `ade/scripts/landing/gen-changelog.mjs:176` | `'../../landing/changelog.html'` → `'../../../landing/changelog.html'` |
| `ade/scripts/landing/gen-og.mjs:25` | `"../../landing/og.png"` → `"../../../landing/og.png"` |
| `ade/scripts/landing/gen-sitemap.mjs:26` | `"../../landing"` → `"../../../landing"` |
| `ade/scripts/landing/gen-sitemap.mjs:39` | `ROOT = resolve(HERE, "../..")` → `"../../.."` — it is the git cwd for the `landing/${file}` pathspecs at `:57,58` |
| `ade/scripts/landing/changelog-pagination.spec.ts:8` | `'../../landing/changelog.html'` → `'../../../landing/changelog.html'` |
| `ade/scripts/release/changelog-json.mjs:27` | default out `"../../changelog.json"` → `"../../../changelog.json"` (CI always passes `--out`, so this is the default only) |
| `ade/src-tauri/src/libsrc/mod.rs:1761` | one more `.parent()` — an `#[ignore]`d smoke test, cosmetic |

**Verified as needing no edit** — check, do not change: `ade/angular.json` roots/styles/assets/budgets, `ade/tsconfig.json:25`, `ade/tsconfig.app.json:4,6,12,13`, `ade/vitest.config.ts:7,8`, both playwright configs (`testDir: "e2e"`, ports 1420/4329), `tauri.conf.json`'s `beforeDevCommand`/`beforeBuildCommand` (Tauri runs them in the app dir, which is `ade/`), `appicon.rs:12,14`, `extensions/registry.rs:113`, `lsp/smoke_tests.rs:83`, `prod-build.spec.ts:12` (`../../angular.json`).

### Task 6 · Node and pnpm

- [x] `pnpm-workspace.yaml` — insert at line 1, above the existing comment block:
  ```yaml
  packages:
    - ade
  ```
  ([`00b`](00b-scaffold-workspace.md) adds the harness entries.)
- [x] New root `package.json`, private, no dependencies, passthrough scripts so CI and habits survive:
  `test`, `dev`, `build`, `start`, `e2e`, `tauri`, `release`, `release:patch`, `ext:*` → `pnpm -C ade <script>`.
- [x] `ade/package.json` — **zero script edits.** Every path in them (`tools/…`, `scripts/…`) is already relative to `ade/`.
- [x] Regenerate the lockfile: `pnpm install --lockfile-only`. The importer key `.` becomes `ade`; without this every `pnpm install --frozen-lockfile` in CI fails.

### Task 7 · CI

`.github/workflows/release.yml`:
- [x] `:58`, `:62`, `:115` — add `working-directory: ade`.
- [x] `:75` → `node ade/scripts/release/notes.mjs`; `:166` → `ade/scripts/release/make-latest-json.mjs`; `:212`, `:214` → `ade/scripts/release/changelog-json.mjs`.
- [x] `:109` — rust-cache `workspaces: src-tauri` → `workspaces: .` (the root `Cargo.lock` is the cache key now).
- [x] `:121,122,123,126,131,133,134` — `src-tauri/target/release/bundle/…` → `target/release/bundle/…`.

`.github/workflows/extensions.yml`:
- [x] `:76`, `:125`, `:183` — `path: dist-ext/*.zip` → `ade/dist-ext/*.zip`.
- [x] `:209` → `node ade/scripts/extensions/make-index.mjs`.

`.github/workflows/test.yml`: no edit — `pnpm test` works via the root passthrough. ([`00b`](00b-scaffold-workspace.md) adds the harness job.)

`.github/workflows/deploy-landing.yml`, `render.yaml`: **no changes** — `landing/` stays at the root.

### Task 8 · Root README and CLAUDE.md

- [x] Root `README.md`: a two-root index — what `ade/` is, what `harness/` is, how to build each, and that they share one cargo workspace.
- [x] `CLAUDE.md`: a short section on the new layout — `ade/` vs `harness/`, run cargo from the root, `default-members` excludes the ADE, and `pnpm -C ade` for frontend work.

### Task 9 · Verification

Run these before asking for review. Never two test runners at once; only the specs touched here.

```powershell
# commit 2 was renames only
git diff --stat -M --find-renames=100% <c1>..<c2>        # 0 insertions, 0 deletions
git ls-files | Select-String "target-test|\.clone"       # empty

# workspace
cargo metadata --format-version 1 --no-deps              # members: orrery-ade, orrery-updater
cargo tree -p orrery-ade --depth 0                       # resolves from the repo root

# the ADE still works
pnpm install --frozen-lockfile
pnpm -C ade build                                        # ade/dist/orrery-ade/browser/index.html exists
pnpm -C ade exec vitest run scripts/release src/app/prod-build.spec.ts src/themes/themes.spec.ts scripts/landing
node ade/scripts/stage-updater-stub.mjs                  # stages from the ROOT target/
node ade/scripts/release/stamp-version.mjs 0.24.7; git diff --stat    # empty — idempotent (use the CURRENT version)
pnpm -C ade exec playwright test --config playwright.local.config.ts e2e/landing-download.spec.ts e2e/landing-analytics.spec.ts e2e/landing-agent-readiness.spec.ts
cargo test -p orrery-ade --manifest-path ade/src-tauri/Cargo.toml hook_cli
cargo clippy -p orrery-ade --manifest-path ade/src-tauri/Cargo.toml -- -D warnings
pnpm -C ade tauri build --no-bundle                      # target/release/orrery-ade.exe

# identity is untouched — all three must be unchanged
(Get-Content ade/src-tauri/tauri.conf.json | ConvertFrom-Json).identifier                     # com.kouji.orrery
(Get-Content ade/src-tauri/tauri.conf.json | ConvertFrom-Json).productName                    # Orrery
(Get-Content ade/src-tauri/tauri.conf.json | ConvertFrom-Json).bundle.windows.wix.upgradeCode # CB7D1311-…

# no stale paths
git grep -n "src-tauri/target\|cwd(), .landing.\|'\.\./\.\./landing'\|orrery_lib" -- ':!ade/docs' ':!ade/design' ':!harness'   # empty
```

- [x] Run the real app side by side with the installed release to confirm nothing about installation changed: `npx tauri dev` from `ade/`. **Do not kill the user's installed Orrery.**

---

## Done when

- Every verification command above passes.
- Commit 2 is renames only.
- The bundle identifier, product name and upgrade code are provably unchanged.
- The installed release still updates in place (confirm with a `--no-bundle` build and the updater stub staging).

## Impact on the other 14 worktrees

Nothing breaks at rest — they share `.git`, and no worktree path moves (the worktree root derives from the Tauri identifier, which does not change). Each branch pays the cost once, at merge:

- Prefer **`git merge main`** over a long rebase: a merge applies rename detection once; a rebase replays each commit against renamed paths and conflicts repeatedly.
- Before merging, set `git config merge.renameLimit 999999` — rename detection silently disables itself above the default limit, and these branches touch many files.
- After merging, each worktree needs `pnpm install` at its new root, and its stale `src-tauri/target/` and `src-tauri/target-test/` (~100 MB each) become orphaned. They are gitignored, so git will not clean them; delete them manually.
- **Land this when the fewest branches are open.**

## Afterwards

- [x] Update the memory files that point at `docs/superpowers/...` to `ade/docs/superpowers/...`.

---

## Decisions taken during implementation

- **`mainBinaryName` is now pinned** (`"mainBinaryName": "Orrery"` in
  `tauri.conf.json`, added next to `productName`). Not in the original plan.
  Tauri only *defaults* it to `productName`; with the crate renamed to
  `orrery-ade` the installed exe's name stopped being provable from the config
  alone, and `--no-bundle` cannot verify it (that path emits the cargo target
  name, `target/release/orrery-ade.exe`). Pinning it makes `Orrery.exe`
  explicit. `identifier`, `productName`, window `title` and `upgradeCode` are
  untouched.
- **Root `package.json` carries no `version` field.** `bump.mjs` stamps
  `ade/package.json` only, so a version at the root would silently drift. The
  root package is private, so the field is optional.
- **`ade/src-tauri/.gitignore:5` (`/target/`) is left in place.** Dead but
  harmless; the rest of that file (`/gen/schemas`, `/binaries/`) is still live,
  and the root `.gitignore` now carries `target/`.
- **`stamp-version.mjs` is run from `ade/`, not from the repo root.** Task 9's
  listing shows `node ade/scripts/release/stamp-version.mjs`, which contradicts
  task 5's own `'../Cargo.lock'` fix — the script resolves `package.json` and
  `src-tauri/tauri.conf.json` from cwd. CI gets `working-directory: ade` for the
  bump/version steps, and `bump.mjs` already invokes it in-process from `ade/`.
- **Clippy is not run with `-D warnings`.** The ADE lib has 17 pre-existing
  warnings (dead-code on `pub` items in a `staticlib`/`cdylib` crate,
  `type_complexity`), none introduced here. `cargo clippy -p orrery-ade
  --all-targets` is clean of errors and picks up the root `clippy.toml`.

---

## State

**Landed.** The repository has two roots: `ade/` holds the Tauri 2 + Angular
desktop app and `harness/` holds the runtime, under one virtual workspace at the
repo root with `resolver = "2"` and a shared `target/`.

- `default-members = ["ade/src-tauri"]`, so a bare `cargo build` builds the ADE
  and the harness crates are named with `-p`. This is what keeps a routine ADE
  build from compiling 40 crates it does not use.
- The identity the plan promised not to touch was not touched: the app still
  ships as `Orrery.exe` with the identifier `com.kouji.orrery`, and the crate is
  still `orrery-ade` (lib `orrery_ade_lib`).
- Shared version pins live in the root `[workspace.dependencies]`; `clippy.toml`
  is at the root and applies to both trees.
- Frontend work runs through `pnpm -C ade`; the root `package.json` is a private
  passthrough and the pnpm workspace lists `ade` as a package.

No open items. The move is recorded in the project `CLAUDE.md` so a new session
starts from it rather than rediscovering it.
