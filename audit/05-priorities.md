# 05 — Priorities: what to fix, in what order

Synthesis of passes 1–4, at v0.22.2. Those passes kept **738 findings** after adversarial verification: 5 critical, 156 high, 302 medium, 275 low.

Every recommendation below was then handed to a separate reviewer who re-opened the cited code and tried to break the advice. All ten diagnoses survived. **Nine of the ten first steps did not** — they were plausible and would have failed, or would have silently not fixed the bug. Those corrections are folded in, and where a step was wrong I have said so rather than quietly replacing it, because the reason it was wrong is usually the most useful thing on the page.

Effort: **XS** under an hour, **S** under a day, **M** a few days, **L** a week or more.

---

## Day one: five fixes, about three hours total

These were buried inside larger items until the validation pass pulled them out. Each is independently safe, needs no design decision, and can ship today.

1. **Drop 89% of the repository.** `git rm -r --cached src-tauri/target-test` and add `/target-test/` beside `/target/` in `src-tauri/.gitignore`. Verified safe: repo-wide grep for `target-test` returns zero hits, there is no `CARGO_TARGET_DIR` anywhere, and `--cached` leaves your local build untouched. This takes 101 MB off every checkout, including every CI run. Note the limit: it does not remove the developer home path embedded in `.rustc_info.json` from history. If that is the goal, you need `git filter-repo --path src-tauri/target-test --invert-paths` and a force-push — a different job, decide which one you want.

2. **Stop the app claiming Cursor reports permissions.** `agent-runtime.service.ts:124` lists cursor in `HOOK_TOOLS`, while `cursor.rs:14-16` says Cursor has no permission event. Split that constant into `STATUS_HOOK_TOOLS` and `PERMISSION_HOOK_TOOLS` — two call sites, `:509` and `:622`.

3. **Turn on the fallback Cursor's own comment claims exists.** `cursor.rs` does not override `pty_status_fallback()`, so it inherits `false` and `runtime/mod.rs:232-236` never spawns the heuristics tee. One override.

4. **Stop discarding agent exit status.** `runtime/mod.rs:406` does `let _ = child.wait()`, which is why a crashed agent renders identically to a successful one. Carry the status into the `agent://exit` payload.

5. **Make the spawn modal non-throwing.** `spawn-modal.component.ts:351` → `this.defaultProject || this.projects.all()[0]?.id || ""`, plus a null-safe fallback at `:365`. This alone stops a new user's first click from throwing, before any empty-state work.

---

## The ranked ten

| # | Fix | Impact | Effort | Validator changed |
|---|---|---|---|---|
| 1 | Project removal destroys uncommitted work | Critical | S | first step |
| 2 | `merge()` force-resets over dirty files | Critical | S | first step |
| 3 | Agent config files overwritten at every launch | Critical | S | framing + first step |
| 4 | Manual tool path never reaches the launcher | High | XS | cheaper direction |
| 5 | CI verifies almost nothing | High | **M**, not S | effort + first step |
| 6 | The signing key is reachable by third-party code | High | M | first step |
| 7 | The supervision loop cannot be trusted | High | **M**, not M–L | effort + first step |
| 8 | First run dead-ends | High | S + M | split |
| 9 | Unsaved work has no guard on close | High | **M**, not S | first step reversed |
| 10 | Landing and docs contradict the product | Medium | S + L | split |

### 1. Project removal destroys every agent's uncommitted work

`src-tauri/src/agents/service.rs:499-518` · `src-tauri/src/projects/commands.rs:74-86` · `src/app/projects/project-actions.service.ts:145`

Unrecoverable, one unconfirmed click, and the codebase documents the opposite guarantee in three places. `remove_for_project` takes no disposal, deletes in place, and skips the trash rename, git deregistration, PTY stop, watcher unregister and history purge that `agent_remove` performs.

**Corrected first step.** The original advice — "have `remove_for_project` call the same teardown" — is impossible: that teardown lives in the command layer, not in `AgentService`, which holds none of those services. Worse, `project_remove` deletes the project row *before* the cascade, so `path_of` fails afterwards and git deregistration would silently no-op for every agent. Do it in this order instead: in `projects/commands.rs`, take `RuntimeService` and `WatchService` as state, capture `project_path` and the doomed agent ids *before* deleting the project row, run the same four teardown calls per agent, then have `remove_for_project` delegate to `AgentService::remove` with a `WorktreeDisposal` — swallowing per-agent errors, because `remove` returns `NotFound` on a zero-row delete and a failed rename is a hard error, either of which would abort the cascade halfway.

**Do not ship the default flip alone.** Defaulting to `KeepFolder` without the confirm dialog's "also delete the folders" checkbox trades data loss for an unbounded disk leak: the worktrees survive with no project record, and `sweep_trash` only looks for `*.trash-*`, so nothing will ever find them.

### 2. `merge()` force-resets the worktree with no dirty check

`src-tauri/src/git/gix_backend.rs:1601-1683` · `reset_to_tree` at `:1158-1189`

The wedge feature of the product, destroying uncommitted work that git itself refuses to touch.

**Corrected first step.** Lifting `checkout_branch`'s guard, as originally advised, would not fix this. That check is *path-scoped* to the HEAD-to-target diff, while `reset_to_tree` clobbers every dirty path. Concretely: merge `feature` into `main` with an unrelated `a.txt` modified locally; the scoped guard passes, `a.txt` is in the merged tree, it lands in the restore set, and the edit is gone. The guard in `merge()` must be blanket — any dirty path at all. Two placement constraints: for the true-merge path the target tree does not exist until `:1661`, so a diff-scoped check is not even computable at the top; and the check belongs *after* the already-up-to-date early return at `:1617`, or a no-op merge on a dirty worktree starts erroring.

### 3. Agent config files are overwritten at every launch

`src-tauri/src/agents/adapters/mod.rs:484-495,527` · `src-tauri/src/agents/adapters/codex.rs:104-125` · `src-tauri/src/lib.rs:165-175`

**Reframed.** The audit titled this around malformed configs, which is the rare half. The common half is unconditional: any Codex user who has ever set their own `[hooks]` entries loses them on every single launch, valid file or not.

**Corrected first step.** Two of the four original sub-steps would have produced a fix that protects nothing. The `.bak` must be write-once — `install_global_hooks` runs at every launch, so an unconditional copy overwrites the pristine backup with the already-modified file on the second run, and after two launches the backup is worthless. And the temp file for the atomic write must be created in the target's own directory, not the system temp dir, or `rename` fails with `EXDEV` whenever `$HOME` and `/tmp` are different mounts. Note `tempfile` is currently a dev-dependency and would need promoting.

### 4. The manual tool path never reaches the launcher

`src/app/settings/settings.store.ts:33,42` · `src-tauri/src/settings/model.rs:29`

**Moved up from fifth.** This reads like tidiness but it disables Orrery's only escape hatch for an agent binary that is not on `PATH`, while the UI reports success. The launcher always sees an empty map.

**Cheaper direction.** Renaming the frontend key touches ten sites, and missing one throws a runtime template error at `settings-modal.component.ts:467`. Instead put `#[serde(rename = "toolPath")]` on the Rust field: one line, five Rust usages untouched. Nothing was ever persisted under either key, so both directions are migration-free. When you add `keymap_terminal`, it must be `BTreeMap<String, bool>` — declaring it as `String` makes the whole settings deserialization fail, which kills every save and is swallowed into a toast.

### 5. CI verifies almost nothing

`.github/workflows/test.yml:14-15`

**Effort corrected to M.** Writing the YAML is an hour; getting it green is not, and a red pipeline is worth nothing.

**What the original step missed.** `cargo clippy -- -D warnings` fails today — not because of the test file, but at `adapters/mod.rs:392`, where `run_probe` calls the banned `std::process::Command::new` in ordinary library code with no `allow`. The code is functionally correct, since it re-applies `CREATE_NO_WINDOW` by hand; it just needs the annotation `core/proc.rs:34` already models. Bare `cargo clippy` also lints none of the 387 unit tests or the integration test, so you want `--all-targets`, which surfaces two further violations. Add the ubuntu system dependencies for the Tauri build, and expect the Windows leg to be the expensive part.

### 6. The updater signing key is reachable by third-party code

`.github/workflows/release.yml:96,106,107,110-115` · `src-tauri/tauri.conf.json:46-50`

The minisign key is the only thing standing between an attacker and silent code execution on every install, and the public key is baked into every shipped binary with no revocation channel.

**First step confirmed feasible, with a correction.** Pinning to commit SHAs works verbatim, do it first. The split is genuinely possible — `tauri build --no-sign` and the standalone `tauri signer sign` both exist, verified in the shipped CLI binary rather than assumed. But you cannot simply drop the env block: without the key *and* without `--no-sign` the build hard-fails, and with `--no-sign` the workflow's `.sig` copy steps become hard errors that need removing at the same time.

### 7. The supervision loop cannot be trusted

`src/app/agents/agent-runtime.service.ts:124,193,565` · `src/app/stores/notifications.store.ts:44` · `src-tauri/src/hooks/mod.rs:262,285`

**Effort corrected to M.** L was too pessimistic once the parts are counted.

**First step reordered.** "Write down the state machine" gates an XS correctness fix behind a multi-day design exercise. Ship the Day-one Cursor and exit-status fixes first — they remove code that contradicts its own comments — then do the design work for what remains, which is real: "has status hooks" and "has a permission event" are currently one boolean on each side of the bridge, and they are not the same thing.

### 8. First run dead-ends

`src/app/modals/spawn-modal.component.ts:351` · `src/app/overview/grid-view.component.ts:11` · `src-tauri/src/defender.rs:200` · `src-tauri/src/lib.rs:165`

**Split.** The frontend half is S and belongs near the top by value per hour. The disclosure half — explaining the UAC prompt and the config rewrites — is M and is really a product decision.

**Corrected first step.** Guarding at `UiStore.openSpawn` as originally advised creates a dependency cycle: `ProjectActionsService` already injects `UiStore`, so injecting it back fails at runtime. Inject `ProjectsStore` instead, which depends only on the bridge. Also note one entry point sets the spawn signal directly at `project-actions.service.ts:107` and bypasses the choke point entirely.

### 9. Unsaved editor work has no guard on close

`src/app/top-bar/window-controls.component.ts:97` · `src/app/workspace/tab-close-guard.service.ts:20`

**Effort corrected to M, and the original first step would have bricked the app.** In Tauri 2.11.2, merely registering a JS close-requested listener makes Rust stop closing the window; the JS side then has to finish the close by calling `destroy()`. That requires `core:window:allow-destroy`, which is neither in `capabilities/default.json` nor in the default permission set. Following the original advice yields a window that cannot be closed from any path. Add the permission in the same change, or do not start.

### 10. Landing and docs contradict the product

`landing/console-mock.js:68` · `landing/index.html:389-390` · `docs/README.md:8` · `README.md:1`

**Split.** The landing fixes are S: two false CI claims, five links to a private repo, five baked `v0.4.0` strings, a missing unsigned-installer warning, and Mac visitors being handed a Windows `.exe`. The docs half is L, because eleven files cite a store that no longer exists and the roadmap's stack section is wrong on all three technologies — that is a rewrite, not an edit. Do the landing this week; schedule the docs.

---

## The strongest item that is not on this list

**Keyboard and screen-reader accessibility.** Eight high findings: the sidebar, workspace tabs, the file tree and the Orchestrator in all three views are mouse-only; no live region exists anywhere, so every toast and state change is silent to a screen reader; status is hue plus blink rate, with blocked and done as red and green. It sits at eleven only because everything above either destroys data or makes the product's central claim untrue. It gets structurally harder every week more UI is built on the current base.

## What not to spend time on

The audit found rationale that holds up for fire-and-forget hooks, no fan-out, no mobile, no SSH remote, no embedded browser, gitoxide with system git for network operations, the compiled-out cost feature, Windows-first paths, and decorator inputs where the test runner forces them. Several are Orca's headline features and `06-positioning.md` argues the calls are right. The gap worth reconsidering is not among them: Orca now does agent-to-agent orchestration, and the roadmap does not mention it.

## Where the detail is

| File | Contents |
|---|---|
| `00-overview.md` | Product, stack, architecture, entry points, mapping surprises |
| `01-bugs.md` | 5 critical, 42 high, 101 medium, 137 low |
| `02-security.md` | 9 high, 27 medium, 58 low, by threat actor |
| `03-architecture.md` | 34 high, 59 medium, 42 low, with measured counts |
| `04-ux.md` | 71 high, 115 medium, 38 low |
| `06-positioning.md` | Against Orca and Grok Bot |

Machine-readable findings, including everything refuted and why, are in `.audit-work/pass{1,2,3,4}-merged.json`; the validation verdicts are in `.audit-work/pass5-checks.json`.
