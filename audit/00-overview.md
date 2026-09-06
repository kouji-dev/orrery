# 00 — Overview: what Orrery is and how it is built

Audit pass 0. Everything below was read from the repository at `0950854` (release v0.22.2, 2026-09-06) on branch `claude/repository-audit-36c5hc`. Claims carry `path:line` references; where a reader flagged something I have not yet re-verified myself it is marked *(to verify)*.

## 1. What the product is

Orrery is a desktop application that runs several coding-agent CLIs at once against local git repositories. Each agent (Claude Code, Codex, Cursor, Gemini) is spawned into its own pseudo-terminal, in its own git worktree on its own branch, and the app gives the user one console to watch all of them, answer their permission prompts, inspect their diffs, and merge what passes.

- Ships as a Windows installer (NSIS `-setup.exe` and per-user MSI) with a minisign-signed auto-updater (`src-tauri/tauri.conf.json:34-51`, `.github/workflows/release.yml:107-134`). An unsigned macOS `.dmg` is also built and uploaded (`release.yml:84-90`) but the landing page hides it (`landing/dl-button.js:14-18`).
- Free, no account, no hosted component: it drives agent CLIs already installed on the user's machine (`landing/llms.txt:5-9`).
- The source repo is private; releases and the machine-readable changelog live in the public `kouji-dev/orrery-releases` repo (`release.yml:184`, `src/app/shared/links.ts:4-10`).
- Positioning as written by the owner: "the AGent IDE that is light on RAM, light on CPU, and light on tokens — with IntelliJ-grade git as the wedge. Every git operation implemented natively is work the LLM no longer pays tokens to do." (`docs/2026-08-03-orrery-roadmap-perf-and-ide-dx.md:7-10`).

## 2. Who it is for

Individual developers who already pay for one or more agent CLIs and want to supervise many runs from one pane instead of one terminal tab per agent, and who want to review each agent's diff per branch before merging (`landing/llms.txt:11-24`). The landing copy targets "Windows" explicitly; there is no team, cloud, or CI story anywhere in the repo, and the roadmap lists fan-out, mobile, SSH-remote and an embedded browser as explicit non-goals (§8 below).

## 3. Stack

| Layer | What | Where |
|---|---|---|
| Shell | Tauri 2 (Rust), single WebView2 window, undecorated, CSP disabled | `src-tauri/Cargo.toml:24`, `src-tauri/tauri.conf.json:12-25` |
| Backend | Rust 2021: gitoxide `gix` 0.87 (no network features) for all in-process git; `git2` only as a test oracle; `portable-pty` for terminals; `notify` file watcher; `grep-searcher`/`grep-regex` for find-in-files; `rusqlite` (bundled SQLite); `sysinfo`; `clap` (the exe doubles as a CLI); Windows Job Objects via `windows-sys` | `src-tauri/Cargo.toml:26-90` |
| Frontend | Angular 22 standalone components, signals, zoneless change detection, inline templates and styles only (0 `.html`/`.css` component files); `@kouji-ui/*` (the owner's own component/theme packages); Monaco 0.55 editor; `@xterm/xterm` 6 with WebGL; Lexical rich-text; `marked` + `mermaid` for markdown | `package.json:19-58`, `src/app/app.config.ts:21-30` |
| Persistence | One SQLite file `app_data/orrery.db` behind a single `Arc<Mutex<Connection>>`; tables `projects`, `agents`, `tickets`, `comments`, `settings` (key/value JSON), `workspace` (JSON blob) | `src-tauri/src/core/database.rs:10-24` |
| Tests | vitest 4 + jsdom (unit), Playwright 1.61 (e2e, browser-only, no Tauri), `cargo test` with git2 oracle | `vitest.config.ts`, `playwright.config.ts`, `src-tauri/src/git/backend_tests.rs` |
| Tooling | pnpm (lockfile v9, pnpm 11 in CI), Angular CLI, a density-token linter and a template-literal linter | `pnpm-workspace.yaml`, `tools/density/check-tokens.mjs`, `tools/check-template-literals.mjs` |
| Landing | Zero-build static site on Render, GA4 analytics, GitHub API at runtime for the download button | `landing/`, `render.yaml`, `.github/workflows/deploy-landing.yml` |

Sizes (non-test lines): Rust 26,847 across 72 files; TypeScript 31,396 across 160 files; 60 frontend spec files (8,408 lines); 37 Playwright spec files (4,689 lines, 172 tests); 95 commits from one human author since the history starts at v0.12.0 on 2026-07-24 (there is no earlier history in the clone).

## 4. Repository layout

| Path | Role | Notes |
|---|---|---|
| `src-tauri/src/` | Rust backend, 20 modules | see §5 |
| `src-tauri/updater-stub/` | Separate `orrery-updater.exe` (tao + wry) that installs the MSI/NSIS package while the app is closed | `updater-stub/src/main.rs:1-14` |
| `src-tauri/target-test/` | **101 MB of committed Windows cargo debug output** (374 files: `.rlib`, `.rmeta`, `.pdb`, build-script `.exe`) | §9 |
| `src/app/` | Angular app, ~30 domain folders | see §5 |
| `e2e/` | Playwright suite | drives the app via `window.ng` dev-mode APIs |
| `landing/` | Marketing site + changelog page | |
| `scripts/release/` | `bump.mjs` → `stamp-version.mjs` → tag → CI; `make-latest-json.mjs`; `changelog-json.mjs` | |
| `scripts/landing/`, `scripts/telemetry/`, `tools/perf-smoke/` | og.png/sitemap generators; NDJSON emit-telemetry summarizer; manual PTY load harness | |
| `docs/` | 14 per-domain feature checklists (stale, §9), a 1,100-line roadmap, a git gap analysis, 27 agent-authored spec/plan files under `docs/superpowers/` and `docs/specs/` | product-decision records rather than developer docs |
| `design/` | 3.3 MB of exported Claude Design canvases, screenshots, the v2 design prompt, and a design-system bundle for a *different* product ("Vortex") | `design/_ds/vortex-*/readme.md:1-27` |
| `questions.md` | Agent-authored decision log with open questions Q1–Q11 addressed to the owner | |
| `README.md` | The stock Tauri + Angular template text, 3 lines | |
| `CLAUDE.md` | Only RTK (`rtk` command-prefix) boilerplate; no project instructions | |

## 5. Architecture

### 5.1 Process model

```
orrery.exe (GUI, Tauri) ──manages──> WebView2 (Angular)
   │  invoke (101 commands)          ▲ events (27 named channels, e.g. agent://output)
   │                                        │
   ├─ AgentService / ProjectService / TicketService / SettingsService / WorkspaceService  (one SQLite conn)
   ├─ GitService → GixBackend (in-process)   ── clone/push/fetch/pull shell out to system `git`
   ├─ RuntimeService: one portable-pty per agent or project shell
   │     reader → batcher(8ms/16KB) → 1MB scrollback ring → global OutputMux (16ms frames) → agent://output
   ├─ WatchService: one `notify` watcher per project → git status scan → agent://changed (+ local-history snapshot)
   ├─ SearchService (grep crates, streaming), HistoryService (content-addressed snapshots)
   ├─ HookBridge: loopback HTTP on 127.0.0.1:<ephemeral>, UUID bearer token
   ├─ metrics thread (5s/20s), perf push (2s), UI-lag probe (1s), [cost loop: compiled out]
   └─ Windows Job Object (kill-on-close) wrapping every child

agent CLI (claude/codex/cursor-agent/gemini) in a ConPTY, cwd = worktree, env ORRERY_AGENT_ID/TOOL/ENDPOINT/TOKEN
   └─ its own hooks run `orrery.exe hook --event <E>` (same binary, CLI mode) → POST to the HookBridge → 204
```

- **Same executable, two modes.** `src-tauri/src/main.rs:8-12` checks argv for `hook` before Tauri starts; `orrery hook` reads the hook payload from stdin and POSTs it to the running app (`src-tauri/src/cli/hook.rs`). The bridge is fire-and-forget by design, copied from how `stablyai/orca` does it (`src-tauri/src/hooks/mod.rs:7-11`; `docs/specs/2026-06-05-agent-hooks-permissions.md:1-9`); agents are never held for a decision, the user approves in the agent's own TUI, and Orrery's "allow/deny" buttons are best-effort keystrokes into the PTY (`src-tauri/src/agents/adapters/mod.rs:239-268`).
- **Global hook install on every launch.** Startup merges Orrery's hook entries into `~/.claude/settings.json`, `~/.codex/config.toml`, `~/.cursor/hooks.json`, `~/.gemini/settings.json`, baking in the absolute path of the running exe (`src-tauri/src/lib.rs:169-175`, `adapters/claude.rs:85-113`, `codex.rs:96-125`, `cursor.rs:71`, `gemini.rs:62`). Files are created even for tools that are not installed (`adapters/mod.rs:310-324`).
- **Worktree per agent.** `AgentService::spawn` derives a slug, a branch from a template, and a worktree path under `app_data/worktrees` (or a user-set root), then `GixBackend::create_worktree` writes the `.git/worktrees/<name>` registration by hand and checks out on all cores (`src-tauri/src/agents/service.rs:183-260`, `git/gix_backend.rs:2419-2497`). This is why gitoxide replaced libgit2: single-threaded checkout took 10–30 s on Windows (`git/backend.rs:4-11`).
- **Interest subscription.** The frontend publishes which agents are visible as `stream` (terminal pane), `digest` (overview card, 5 folded lines at 1 Hz) or `none`; the backend only emits for those (`src/app/data-source/bridge.ts:56-66`, `src-tauri/src/runtime/output_mux.rs`).
- **Windows-specific machinery.** Job Object for process-tree kill (`runtime/jobobj.rs`), npm-shim resolution to avoid a `cmd.exe`/`pwsh` wrapper per agent (`adapters/mod.rs:565-853`), WebView2 browser-pid capture for the process tree (`lib.rs:56-68`), and an elevated PowerShell run at startup that adds the worktree root to Windows Defender's exclusion list, with a UAC prompt (`src-tauri/src/defender.rs:1-20`, `lib.rs:141-147`). The Rust tree has 66 `cfg(windows)` sites vs 8 unix/macos ones.
- **Self-update.** `tauri-plugin-updater` verifies the signature, then on Windows hands the package to `%TEMP%\orrery-updater.exe`, exits, and the stub installs silently and relaunches (`src-tauri/src/update.rs`, `updater-stub/src/main.rs`).

### 5.2 Frontend

- **Boot:** `src/main.ts` → `AppComponent` → route `''` = `LoadingComponent` (splash: loads settings, hydrates the workspace document, runs the startup update check with a 12 s backstop) → route `app` = `ShellComponent` (`src/app/app.routes.ts`, `src/app/loading/loading.component.ts:187-190`).
- **Shell:** top bar with tabs (orchestrator, backlog, ticket, agent, project) / sidebar or compact rail / center column showing overview, backlog, ticket page or the pane manager / bottom tool window (branches, commit graph, local history) / status bar. Modals are service-launched through `@kouji-ui`'s dialog service (`src/app/shell/shell.component.ts:55-115`).
- **State:** signal stores. `UiStore` (tabs, pane roots, tweaks persisted to `localStorage`), `WorkspaceStore` (layout document persisted through `workspace_set`), `SettingsStore` (whole-document, 300 ms debounced `settings_set`), `AgentsStore`/`ProjectsStore`/`TicketsStore` (a generic `EntityStore` + `EntityFacade` that mirrors a backend table via one list command plus created/updated/deleted events), `AgentWorkStore`/`GitInspectStore`/`ConflictStore`/`BranchesStore` (keyed `Loadable` maps with LRU eviction), `AgentRuntimeService` (merges live PTY/hook state over the stored records).
- **IPC contract:** everything goes through the `Bridge` token; `Commands` and `Events` string catalogs in `src/app/data-source/bridge.ts` are the entire frontend↔backend surface. Wire types are hand-maintained on both sides with no codegen (`src/app/models.ts:395-398` says `Settings` "mirrors the Rust Settings struct exactly").
- **Workspace:** per-tab binary split tree of panes; each leaf shows a terminal, the working-tree diff, a Monaco file editor with save/autosave/dirty guards, or a git-inspection view (commit diff, range, blame, file history, diff3 conflict resolver). Inline review comments are pasted into the agent's PTY as a bracketed paste (`src/app/agents/agent-review.service.ts:7-32`).
- **Browser mode is accidental:** `app.config.ts:42-52` always provides `TauriBridge`; stores catch the rejected `invoke` and start empty. The Playwright suite works by seeding stores through Angular's dev-mode `window.ng` API and monkey-patching private fields (`e2e/review-flow.spec.ts:12-33`).

### 5.3 Data flows worth knowing

1. **Spawn → start:** `agent_spawn` inserts the row and creates the worktree (failure only logged, `service.rs:210-217`); `agent_start` reads settings for auto-approve policy and manual tool path, builds argv through the adapter, spawns the PTY, and only then flips the row to `running` (`agents/commands.rs:308-382`).
2. **PTY exit:** the WAIT thread emits `agent://exit`, completes the attached ticket on natural exit, and the *frontend* writes status `idle` back (`runtime/mod.rs:405-459`, `src/app/agents/agent-runtime.service.ts:561-564`).
3. **File change → UI:** notify event → per-agent debounce → project scan thread → git status (cached, keyed by a full worktree fingerprint walk) → local-history snapshot → `agent://changed` only if the fingerprint changed (`watch/mod.rs:189-231`, `gix_backend.rs:708-728`).
4. **Hook → status:** agent CLI hook → `orrery hook` → loopback POST → `protocol::parse` into an `AgentEvent` taxonomy → dedup → `agent://status|activity|permission` (`hooks/protocol.rs`, `hooks/mod.rs:171-190`).
5. **Settings:** one JSON document; frontend debounces and writes the whole thing; backend deserializes into the typed struct and re-serializes, dropping unknown keys (`src-tauri/src/settings/commands.rs:16-24`, `service.rs:85-96`).

## 6. Main entry points

| Entry | File |
|---|---|
| Native main / CLI dispatch | `src-tauri/src/main.rs:8-12` |
| Tauri builder, managed state, background threads, all 101 command registrations, shutdown teardown | `src-tauri/src/lib.rs:33-404` |
| Hook CLI | `src-tauri/src/cli/hook.rs` |
| Frontend bootstrap and providers | `src/main.ts`, `src/app/app.config.ts` |
| Routes | `src/app/app.routes.ts` |
| Shell layout | `src/app/shell/shell.component.ts` |
| IPC catalog | `src/app/data-source/bridge.ts` |
| Domain model | `src/app/models.ts`, `src-tauri/src/*/model.rs` |
| Release | `scripts/release/bump.mjs` → `.github/workflows/release.yml` |
| Landing | `landing/index.html`, `landing/version.js` |

## 7. Build, test, and release pipeline

- **Dev:** `pnpm dev` = `tauri dev`, which runs `scripts/stage-updater-stub.mjs` (builds and copies the updater exe on Windows) then `ng serve` on :1420 (`tauri.conf.json:6-10`). A fresh clone cannot `cargo build` in `src-tauri` until that script has run, because `tauri.windows.conf.json` declares the stub as a bundled resource; nothing in `README.md` or `CLAUDE.md` says so (`scripts/stage-updater-stub.mjs:3-6`).
- **CI on PRs:** `.github/workflows/test.yml` runs exactly one thing: `pnpm test` (vitest). No `cargo test`, no `cargo clippy` (despite `clippy.toml` banning two APIs repo-wide), no `ng build`/`tsc`, no Playwright. Windows-only code paths are never compiled in CI (ubuntu runner only).
- **Release:** `pnpm release[:patch]` bumps all three manifests, commits, tags `vX.Y.Z`; the tag triggers `release.yml`: `prepare` (version + auto notes) → `build` matrix (windows-latest, macos-14) → `publish` (`make-latest-json.mjs`, `gh release create` in `orrery-releases`) → `changelog` (append to `changelog.json` in the public repo). A missing `.msi.sig` is tolerated and silently drops the MSI updater entry (`release.yml:126`, `make-latest-json.mjs:53-56`).
- **Landing deploy:** Render static site, redeployed via API only when `landing/**` changes or after any successful Release (`deploy-landing.yml:8-20`).
- **Local baselines for this audit:** vitest passes here (67 files, 602 tests, 71 s). The Rust crate cannot be compiled in this container (no webkit2gtk), so all Rust findings in later passes are from reading. `pnpm audit` and `cargo audit` outputs are saved for Pass 2.

## 8. Deliberate trade-offs recorded in the repo

These are choices with written rationale. Later passes will not flag them as defects.

- **Fire-and-forget hooks, no remote allow/deny:** matched to stablyai/orca after checking its behaviour; the blocking round-trip design is kept as a deferred spec (`docs/specs/2026-06-05-agent-hooks-permissions.md:1-12`).
- **Non-goals:** fan-out ("3× the tokens to discard two results"), embedded Chromium/design mode (memory budget), mobile companion, SSH remote worktrees (deferred), compilation/IntelliSense, a permanent staging-area UI (`docs/2026-08-03-orrery-roadmap-perf-and-ide-dx.md` §Non-goals). Every one of these is a headline feature of Orca (§10).
- **gitoxide over libgit2** for multi-threaded checkout; network ops stay on the system `git` so the OS credential helper handles auth (`git/backend.rs:4-11`, `Cargo.toml:36-37`).
- **Windows-first**: every A0–A7 roadmap item and the Defender/Job-Object/shim machinery assume Windows; macOS signing is an open decision the owner has not made (`questions.md:70-71`, roadmap "Open decisions" 1).
- **Cost feature compiled out** behind `COST_FEATURES_ENABLED = false` on both sides until calibration data exists (`src-tauri/src/cost/mod.rs:12-17`, `src/app/cost/cost-flags.ts:12`).
- **Adaptive metrics cadence** (5 s while agents run, 20 s idle) and a perf push every 2 s "dev + prod" are intentional (`lib.rs:187-201`, `perf/mod.rs:151-160`).
- **Decorator inputs instead of signal inputs** in several components, to keep vitest's JIT compiler working (`src/app/workspace/pane-node.component.ts:380-390` and others).

## 9. What surprised me (verified)

1. **101 MB of Windows cargo build output is committed.** `src-tauri/target-test/` holds 374 tracked files (`.rlib`, `.rmeta`, `.pdb`, build-script `.exe`) out of a 111 MB tracked tree; `.rustc_info.json` embeds the developer's home path `C:\Users\narut\...`. Added in `8b26c17` (2026-08-04). `src-tauri/.gitignore` ignores only `/target/`. Nothing references the directory.
2. **A stray placeholder is tracked** at `.clone/worktrees/git-inspection/src/app/workspace/git/commit-diff-view.component.ts` (11 bytes, content "placeholder"), added with `04f34c5`.
3. **The docs describe a product that no longer exists.** `docs/README.md:1-34` calls it "ORCHESTRA", says all code is under `src/app/orchestra/` and "all data is currently mocked"; nine `docs/*.md` files cite `orchestra.store.ts` as the source of truth. None of those paths exist. The roadmap header (`docs/2026-08-03-…md:12-17`) says Angular 20 + CodeMirror 6 + git2; the code is Angular 22 + Monaco + gix. The switch from CodeMirror to Monaco reversed a recorded decision (`docs/specs/2026-06-05-agent-worktree-runtime.md:87-97`) with no record of the reversal, and stale CodeMirror comments survive in `src/app/workspace/monaco-loader.ts:5-6` and `src/app/utils.ts:286-287`.
4. **`README.md` is the template stub and `CLAUDE.md` contains only `rtk` boilerplate.** Neither says what Orrery is, how to build it, or that the updater stub must be staged first.
5. **Two settings keys are silently dropped by the backend.** The frontend persists manual tool paths as `toolPath` (`src/app/settings/settings.store.ts:33`, `src/app/models.ts:446`) but the Rust struct field is `tool_paths` → `toolPaths` (`src-tauri/src/settings/model.rs:29,158`); `settings_set` round-trips through the typed struct, so the key is lost and the launch path always sees an empty map. `keymapTerminal` has no Rust counterpart at all. Both settings therefore survive only until restart. *(Reader finding; key names verified by grep, runtime effect to confirm in Pass 1.)*
6. **The desktop app fetches Google Fonts on every launch** (`src/index.html:8-13`) with **no Content-Security-Policy** (`src-tauri/tauri.conf.json:24`), contradicting the owner's own note that the desktop app should not fetch remote fonts (`questions.md:87-89`). The updater stub's banner does the same (`src-tauri/updater-stub/src/banner.html:8-10`).
7. **Startup rewrites four user config files unconditionally**, and a file that fails to parse (a trailing comma in `~/.claude/settings.json`) is treated as empty and replaced with only Orrery's hooks (`src-tauri/src/agents/adapters/mod.rs:484-495,527`). Writes are non-atomic.
8. **CI enforces almost nothing** (§7): the two clippy bans, the 65-test gitoxide oracle suite, the Windows-only modules, the build, and the 172-test Playwright suite all run only on the developer's machine.
9. **Platform claims contradict each other.** Roadmap and `questions.md` say Windows-only; `release.yml` builds and publishes an unsigned mac dmg and `make-latest-json.mjs:63` writes a `darwin-aarch64` updater entry; the landing's JSON-LD, `llms.txt` and og:description say Windows; `scripts/landing/gen-og.mjs:96` bakes "Free · Windows & macOS" into the shared og.png.
10. **The landing page bakes `v0.4.0`/`v0.4.1`** into four files (`landing/index.html:469`, `dl-button.js:88`, `console-mock.js:177`, `changelog.html:142,162`) and relies on two unauthenticated GitHub API calls per visit to overwrite them; a rate-limited visitor sees a version 18 releases old.
11. **A fake org name ships in production UI.** `ORG = "northwind"` (`src/app/data.ts:4`) is rendered via `ui.store.ts:160` in the overview, backlog and graph views. The rest of `data.ts` (fake `PROJECTS`, `STREAM`) is dead.
12. **The public site advertises the private repo.** `landing/llms.txt:36` and the JSON-LD `sameAs` link `github.com/kouji-dev/orrery`, which `release.yml:184` says is private.
13. **`design/` includes a 220 KB design-system bundle for an unrelated product** ("Vortex … FastAPI + TanStack Start, Postgres/pgvector", `design/_ds/vortex-*/readme.md:1-27`), and `questions.md:83` points at a token path that does not exist.
14. **The beta update channel has no publisher.** `update.rs:45-47` derives `latest-beta.json`; nothing in `release.yml` produces it, so choosing "beta" in Settings yields a failed check.
15. **`pnpm-workspace.yaml` documents its supply-chain exemptions with a reference to the developer's desktop path** (`~/Desktop/projects/kouji-ui`) and a "TEMPORARY" peer-dependency override that is still load-bearing (`pnpm-workspace.yaml:12-28`).

## 10. Reader-flagged items queued for verification (not yet audited)

The subsystem readers surfaced roughly 330 code-level observations, stored under `.audit-work/maps/` (git-excluded). The ones with the largest blast radius, to be confirmed and graded in Passes 1–2:

- `GixBackend::merge` resets the worktree to the merged tree with no dirty check, so uncommitted edits to tracked files may be discarded on both the fast-forward and true-merge paths (`gix_backend.rs:1601-1683`, `reset_to_tree` :1158-1189).
- `remove_for_project` `remove_dir_all`s every agent worktree unconditionally, bypassing the opt-in hard-delete, trash rename, and process stop that `agent_remove` performs (`agents/service.rs:499-519` vs `commands.rs:79-129`).
- `agent_remove` with a project id renames the *project directory* to `<project>.trash-…` and then returns NotFound (`service.rs:400-448`, `:743-777`).
- Two PTY lifecycle races (double-start overwrite; stop-then-relaunch attributes the old exit to the new run) in `runtime/mod.rs:175-177,384-391,416-458`.
- The hook bridge reads an unbounded `Content-Length` body before checking the token, has no read timeout, and opens any `transcript_path` the payload names (`hooks/protocol.rs:704-719`, `mod.rs:171-181`, `transcript.rs:41,195`).
- `fs::list_dir` joins a frontend-supplied path with no traversal guard (`src-tauri/src/fs/mod.rs:47-51`); `open_path`/`reveal_path` accept any absolute path (`core/commands.rs:32-47`).
- `settings_set` persists any document unvalidated; the telemetry auto-disable does a read-modify-write of the whole settings blob from a background thread (`settings/commands.rs:16-24`, `lib.rs:122-132`).
- The 12 s splash backstop always resolves "no update", so a slow auto-install continues in the background after the shell opens (`src/app/loading/loading.component.ts:187-190`).
- Runtime-shipped `dompurify` 3.2.7 (via Monaco) has 17 open advisories; `quick-xml`, `rkyv`, `crossbeam-epoch` in `Cargo.lock` have RustSec entries. Dev-only advisories (Angular build chain, jsdom) are separate.

## 11. Competitive context (for the positioning pass)

- **Orca** (`stablyai/orca`, MIT, ~62k stars): "the AI Orchestrator for 100x builders". macOS/Windows/Linux desktop, iOS/Android companion, SSH remote worktrees, "40+ agents", fan-out one prompt across N worktrees and merge the winner, embedded-Chromium Design Mode, GitHub/Linear integration, annotate AI diffs, CLI. Orrery's hook bridge is explicitly modeled on it, and every Orca headline feature Orrery lacks is listed as an Orrery non-goal. The roadmap's open decision 2 names this fight directly: "Competing on feature velocity against a daily-shipping MIT project from a private Windows-only beta is the hardest version of this fight."
- **Grok Bot** (xAI, beta 2026-08-11): always-on agents on a persistent cloud VM shared per user, sign into web apps with the user's real logins, keep working when the laptop is closed; not sold standalone, bundled with SuperGrok Heavy ($300/mo), Cursor Ultra ($200/mo) or Cursor Teams Premium ($120/seat/mo). It is a general knowledge-work agent roster distributed through Cursor, not a local worktree orchestrator. (Primary xAI pages were blocked by this container's egress proxy; facts are from secondary coverage and will be re-checked in Pass 5.)
- **Grok Build** (xAI CLI, 2026-05): a terminal coding agent that itself runs up to 8 parallel sub-agents locally. Relevant because it is a fifth agent CLI Orrery could host, and because it competes with the "many agents, one console" premise from inside a single agent.

The full comparison will be `audit/06-positioning.md`.
