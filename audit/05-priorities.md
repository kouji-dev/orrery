# 05 — Priorities: the ten things worth fixing first

Synthesis of passes 1–4, at v0.22.2. Those passes kept **738 findings** after adversarial verification: 5 critical, 156 high, 302 medium, 275 low. This file ranks what to do about them by impact against effort, and gives each item a first step you can start today.

Ranking logic, so you can disagree with it deliberately: data loss outranks everything, because it costs the user work that cannot be recovered and it costs you the user. Next comes anything that makes the product's central claim untrue. Then the cheap structural fix that stops defects recurring. Then credibility, because a private beta lives or dies on first impressions. Effort is my estimate of engineering time for someone who knows this codebase: **XS** under an hour, **S** under a day, **M** a few days, **L** a week or more.

| # | Fix | Impact | Effort | Passes that found it |
|---|---|---|---|---|
| 1 | Project removal destroys uncommitted work | Critical | S | bugs, security, architecture |
| 2 | `merge()` force-resets over dirty files | Critical | S | bugs, security, architecture |
| 3 | Agent config files are replaced when unparseable | Critical | S | bugs, security, UX |
| 4 | CI verifies almost nothing | High | S | architecture |
| 5 | Two settings are silently discarded on save | High | XS | bugs, architecture, UX |
| 6 | The signing key is reachable by third-party code | High | M | security |
| 7 | The supervision loop cannot be trusted | High | M–L | UX, bugs |
| 8 | First run dead-ends | High | S–M | UX, bugs |
| 9 | Unsaved work has no guard on three paths | High | S | bugs, UX |
| 10 | Public-facing contradictions and 101 MB of build output | Medium | S | architecture, UX, positioning |

---

## 1. Project removal destroys every agent's uncommitted work

**Why first.** It is unrecoverable, it is one unconfirmed click, and the code documents the opposite guarantee. `WorktreeDisposal` exists so a delete "can never cost someone work they had not committed", and `agent_remove` honours it. `remove_for_project` takes no disposal, runs `remove_dir_all` on every agent worktree, and skips the trash rename, git deregistration, PTY stop, watcher unregister and history purge the per-agent path performs. A comment in the same file names this function as the reason there is no foreign key, so the integrity mechanism is the incomplete one.

`src-tauri/src/agents/service.rs:499-518`, `src-tauri/src/projects/commands.rs:74-86`, `src/app/projects/project-actions.service.ts:145`

**First step.** Give `remove_for_project` a `WorktreeDisposal` parameter and make `KeepFolder` the default, then have it call the same teardown `agent_remove` uses rather than its own loop. Add the confirmation dialog the docs already promise. One afternoon.

## 2. `merge()` force-resets the worktree with no dirty check

**Why second.** Native git is the wedge you have chosen to compete on, and this is the wedge feature silently destroying work that git itself refuses to touch. Both the fast-forward and true-merge paths call `reset_to_tree`, which adds every non-added status path to the restore set and overwrites it — including files the merge does not touch.

`src-tauri/src/git/gix_backend.rs:1601-1683`, `reset_to_tree` at `:1158-1189`

**First step.** `checkout_branch` at `:1454-1468` already has exactly the pre-check this needs. Lift it into a helper, call it at the top of `merge()`, and return a typed "worktree has uncommitted changes" error the UI can explain. Then add the backend test that merges into a dirty worktree, which does not exist today.

## 3. A malformed agent config is replaced with an Orrery-only file

**Why third.** It runs unconditionally at every launch, on every user's machine, against files Orrery does not own. Any read or parse failure — a BOM, a trailing comma, a comment, non-UTF-8 bytes — collapses to an empty map, and the user's entire `~/.claude/settings.json` becomes just Orrery's hooks. The TOML path for `~/.codex/config.toml` has the same shape, and additionally overwrites the user's own `[hooks]` entries. The write is not atomic.

`src-tauri/src/agents/adapters/mod.rs:484-495,527`, `src-tauri/src/agents/adapters/codex.rs:104-125`

**First step.** Distinguish "file absent" from "file unreadable". Absent means create; unreadable means log, skip, and surface a notice — never write. Add a `.bak` copy before the first modification and write via temp-file-plus-rename. While you are there, tell the user in the UI that Orrery installs hooks into these files at all.

## 4. CI verifies almost nothing

**Why fourth, above bigger problems.** It is the cheapest item on this list and it is why several of the others survived to be found by an audit. Today CI runs `vitest` and stops. No `cargo test`, no clippy, no type-check, no build, no e2e. Consequences that are already real: all 100 Tauri commands are untested; `clippy.toml` bans three APIs that nothing enforces, and the repo's own integration test violates the ban; nothing type-checks the app until a release tag is pushed; 13,181 lines of test code are never type-checked; and 26 Rust tests are Windows-gated with no Windows runner in a Windows-first product.

`.github/workflows/test.yml:14-15`, `src-tauri/clippy.toml:1-5`, `tsconfig.app.json:13`

**First step.** Add to the existing job: `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy -- -D warnings`, `npx tsc -p tsconfig.app.json --noEmit`, and `pnpm build`. Then add a `windows-latest` matrix leg so the Windows-gated tests can run. Note that `cargo build` in `src-tauri` needs `scripts/stage-updater-stub.mjs` first — a prerequisite documented nowhere, which is its own small finding.

## 5. Two settings are silently discarded on every save

**Why here.** It is the smallest fix on the list and it makes a shipped feature simply not work. The frontend persists the manual tool path as `toolPath`; Rust calls it `tool_paths`. `settings_set` round-trips through the typed struct, so the key vanishes with no error. The launch path reads the Rust field, so the override never reaches the launcher at all. `keymapTerminal` has no Rust field whatsoever. Both test suites stay green throughout, which is the real lesson.

`src/app/settings/settings.store.ts:33,42`, `src/app/models.ts:446,463`, `src-tauri/src/settings/model.rs:29`, `src-tauri/src/settings/commands.rs:16-24`

**First step.** Rename the frontend key to `toolPaths` and add `keymap_terminal` to the Rust struct. Then write the test that would have caught it: send a fully-populated settings document through `settings_set`, read it back with `settings_get`, and assert deep equality. That one test is the beginning of the contract testing the IPC surface has none of.

## 6. The updater signing key is reachable by third-party code

**Why sixth, despite being existential.** The minisign key is the only thing standing between an attacker and silent code execution on every installation, because the shipped updater validates only that key and a fixed endpoint. It currently sits in the environment of a job that runs three third-party Actions pinned to mutable tags and one pinned to a branch, and it is present while every crate `build.rs` and npm lifecycle script executes. There is no approval gate between a tag push and a signed release. Separately, `latest.json` is unsigned, so manifest tampering can force a downgrade to any previously signed build. It ranks below the data-loss items only because exploitation requires someone else to act first.

`.github/workflows/release.yml:96,106,107,110-115`, `src-tauri/tauri.conf.json:46-50`

**First step.** Pin every non-`actions/*` step to a full commit SHA. Then split signing into its own job that runs no third-party actions, consumes an unsigned artifact, and sits behind a GitHub Environment with required reviewers.

## 7. The supervision loop cannot be trusted

**Why here rather than higher.** This is the product's central claim — one console that tells you which agent needs you — and it is undermined by five independent defects, but fixing it properly is design work, not a patch. A Cursor agent waiting for approval is invisible by construction. "Finished" is wired to process exit, and these CLIs do not exit. A crashed agent renders as a green "finished — review or merge its work" card with Push one click away. A second permission request arriving while the first is unanswered is silently dropped, so Approve approves something the user never saw. Every "needs you" indicator in the status bar filters on a status string the backend never writes, and four more predicates branch on `Agent.pending`, which is never populated.

`src/app/agents/agent-runtime.service.ts:124,193,565`, `src/app/stores/notifications.store.ts:44`, `src/app/status-bar/status-bar.component.ts:297`, `src-tauri/src/hooks/mod.rs:262,285`

**First step.** Before writing code, make the state machine explicit: enumerate the states an agent can be in, which signal writes each one, and what the UI shows when no signal is available for a given tool. That document will immediately show which of the six declared `AgentStatus` values are reachable — currently two — and whether Cursor can be supported at all without the PTY heuristics fallback it does not enable.

## 8. First run dead-ends

**Why here.** Every new user hits all of it, in order. The primary button throws with zero projects — all five spawn entry points dereference `projects.all()[0].id` inside an effect, so nothing happens and nothing is shown. There is no empty state anywhere; the landing tab is four zeros above a blank grid. A UAC dialog naming "Windows PowerShell" appears three seconds in with no explanation. Four config files in the user's home are rewritten silently. A CLI showing "not installed" is a dead end with no install instructions, and nothing re-runs detection without an app restart.

`src/app/modals/spawn-modal.component.ts:351`, `src/app/overview/grid-view.component.ts:11`, `src-tauri/src/defender.rs:200`, `src-tauri/src/lib.rs:165`, `src/app/modals/runtime-row.component.ts:190`

**First step.** Guard the spawn entry points on `projects.all().length` and route an empty state to "Add a project". That is a two-hour fix that turns a broken first impression into a working one, and it is prerequisite to any beta feedback being about the product rather than about the first thirty seconds.

## 9. Unsaved editor work has no guard on three paths

**Why here.** Cheap, and it is the kind of loss users do not forgive. The titlebar close button calls the window close directly. Quitting has no dirty-buffer check at all. Rename and delete in the sidebar file tree discard unsaved edits and leave the pane pointing at a dead path. A tab close, by contrast, is guarded — so the guard exists and simply is not wired to these three.

`src/app/top-bar/window-controls.component.ts:97`, `src/app/workspace/tab-close-guard.service.ts:20`, `src/app/sidebar/files/file-tree.component.ts:227`

**First step.** Route the titlebar close through the existing `TabCloseGuardService`, and add a Tauri `onCloseRequested` handler that vetoes the close while dirty buffers exist.

## 10. Public-facing contradictions and 101 MB of committed build output

**Why last, and why still on the list.** None of it breaks the app; all of it costs credibility with exactly the people you want in a beta, and all of it is an afternoon. The landing page sells a CI integration that does not exist ("merge only what passes"), hands Mac visitors a Windows `.exe`, never warns the installer is unsigned, and its second-largest hero button 404s on a private repo. The version `v0.4.0` is baked into four files. `src-tauri/target-test` holds 374 tracked files and 101 MB of Windows debug output — 89% of the repository — with a developer's home path embedded in it. The docs describe a mock-data prototype called ORCHESTRA in a directory that does not exist, and eleven files cite a store file that was deleted.

`landing/console-mock.js:68`, `landing/index.html:389-390`, `landing/dl-button.js:74`, `src-tauri/.gitignore:3`, `docs/README.md:8`, `README.md:1`

**First step.** `git rm -r --cached src-tauri/target-test` and add it to `.gitignore`. Then fix the landing claims, since those are read by every prospect and take an hour.

---

## The strongest thing that did not make the list

**Keyboard and screen-reader accessibility.** Eight high findings: the sidebar (primary navigation), workspace tabs, the file tree and the Orchestrator in all three views are mouse-only; there is no live region anywhere, so every toast and state change is silent to a screen reader; agent status is conveyed by hue and blink rate alone, with blocked and done as red and green. I left it at eleven because for a private single-platform beta the items above either destroy data or make the product's core claim untrue. It should not stay at eleven for long — it is the one class of problem that gets structurally harder the more UI you build on top of it.

## What I would not spend time on

The audit found written rationale that holds up for: fire-and-forget hooks with best-effort keystroke approvals, no fan-out, no mobile companion, no SSH remote worktrees, no embedded browser, gitoxide with system git for network operations, the compiled-out cost feature, Windows-first path handling, and decorator inputs where the test runner forces them. Several of those are Orca's headline features, and `06-positioning.md` argues the decisions are sound. The gap worth reconsidering is not in that list: Orca now does agent-to-agent orchestration, and your roadmap does not mention it.

## Reading the detail

Each pass file is sorted by severity, with full detail for the serious findings and one line each below that. The complete verified data, including everything refuted and why, is in `.audit-work/pass{1,2,3,4}-merged.json`.

| File | Contents |
|---|---|
| `00-overview.md` | What the product is, the stack, architecture, entry points, and the surprises found while mapping |
| `01-bugs.md` | 5 critical, 42 high, 101 medium, 137 low |
| `02-security.md` | 9 high, 27 medium, 58 low, by threat actor |
| `03-architecture.md` | 34 high, 59 medium, 42 low, with measured counts |
| `04-ux.md` | 71 high, 115 medium, 38 low |
| `06-positioning.md` | Orrery against Orca and Grok Bot |
