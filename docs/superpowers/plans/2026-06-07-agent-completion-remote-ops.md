# Agent Completion Ops (commit / push / rebase / merge) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A finished-agent wrap-up flow — **Commit · Push · Rebase · Merge** — where Commit/Push offer a deterministic backend action plus an AI version, and Rebase/Merge are AI-only (predefined prompt typed into the agent's PTY). Remove the risky local `agent_merge`. No PR creation.

**Architecture:** AI actions go through one new command `agent_action(id, kind)` that resolves a predefined prompt (from a `prompts` module) and writes it into the running agent's PTY. Backend Push is a new `agent_push` shelling out to the `git` CLI (credential-helper auth). Backend Commit reuses the existing `agent_commit`. The frontend renders split buttons (primary = backend, attached ✨ = AI).

**Tech Stack:** Rust (`std::process::Command`, git2, Tauri commands), Angular 20 signals, xterm PTY.

**Spec:** `docs/specs/2026-06-07-agent-completion-remote-ops.md`

**Out of scope (already done separately):** the commit-feed bug fix and merge-button overflow are already implemented — do NOT redo them.

---

## Key v1 decision (refinement of the spec)

AI actions write into a **running** PTY. Rather than auto-starting an idle agent (racy: the tool isn't ready to receive input immediately after spawn), **AI buttons are enabled only when `agent.status === 'running'`** (tooltip prompts the user to Start/Resume first). `agent_action` returns an error if the process isn't running. Auto-start + queued-prompt is deferred.

---

## File Structure

- Create: `src-tauri/src/agents/prompts.rs` — `action_prompt(kind, branch, base)`, pure + tested.
- Modify: `src-tauri/src/agents/mod.rs` — `pub mod prompts;`.
- Modify: `src-tauri/src/git/service.rs` — `GitService::push` (git CLI) + test.
- Modify: `src-tauri/src/agents/service.rs` — `AgentService::push`; remove `AgentService::merge`.
- Modify: `src-tauri/src/agents/commands.rs` — add `agent_action`, `agent_push`; remove `agent_merge`.
- Modify: `src-tauri/src/lib.rs` — register `agent_action`, `agent_push`; remove `agent_merge`.
- Modify: `src/app/orchestra/data-source/bridge.ts` — add `AgentAction`, `AgentPush`; remove `AgentMerge`.
- Modify: `src/app/orchestra/stores/agents.store.ts` — add `action`, `push`; remove `merge`.
- Modify: `src/app/orchestra/utils.ts` — add `sparkles` icon to `ICONS`.
- Modify: `src/app/orchestra/agents/agent-actions.service.ts` — rework `act`, add `aiAction`/`pushAgent`, remove `mergeAgent`, update `agentMenu`.
- Modify: `src/app/orchestra/right-panel/git-tab.component.ts` — split buttons + AI rebase/merge buttons.

**Independence note:** shares `lib.rs` + `bridge.ts` with the cost plan. Run in separate worktrees, merge after (edits are in different regions).

---

## Task 1: Prompt templates (pure, TDD)

**Files:**
- Create: `src-tauri/src/agents/prompts.rs`
- Modify: `src-tauri/src/agents/mod.rs`

- [ ] **Step 1: Write the module with failing tests**

Create `src-tauri/src/agents/prompts.rs`:

```rust
//! Predefined natural-language prompts for AI-driven completion actions. katrix
//! types these into the agent's PTY; the agent runs the actual `git` with its own
//! tools (so its permission flow / hooks apply). Tool-agnostic.

/// The prompt for a completion `kind` (commit/push/rebase/merge), with the agent's
/// `branch` / `base` interpolated. `None` for an unknown kind.
pub fn action_prompt(kind: &str, branch: &str, base: &str) -> Option<String> {
    let p = match kind {
        "commit" => "Commit all current changes in this worktree with a clear, concise \
                     message describing what changed: run `git add -A` then `git commit`. \
                     Do not push."
            .to_string(),
        "push" => format!(
            "Push the current branch to origin: run `git push -u origin {branch}`. Do nothing else."
        ),
        "rebase" => format!(
            "Rebase this worktree onto `{base}`: run `git rebase {base}`, resolve any \
             conflicts, and complete the rebase. Do not push and do not merge."
        ),
        "merge" => format!(
            "Merge `{base}` INTO the current branch `{branch}` — bring `{base}`'s changes \
             into this branch. From within this worktree run `git merge {base}`, resolve any \
             conflicts, and complete the merge. Do NOT merge the other direction and do NOT \
             modify `{base}`. Do not push."
        ),
        _ => return None,
    };
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_interpolates_branch() {
        let p = action_prompt("push", "agent/fix_login", "main").unwrap();
        assert!(p.contains("git push -u origin agent/fix_login"), "got: {p}");
    }

    #[test]
    fn rebase_interpolates_base() {
        let p = action_prompt("rebase", "agent/x", "develop").unwrap();
        assert!(p.contains("git rebase develop"), "got: {p}");
    }

    #[test]
    fn merge_locks_direction_base_into_branch() {
        let p = action_prompt("merge", "agent/x", "main").unwrap();
        assert!(p.contains("Merge `main` INTO the current branch `agent/x`"), "got: {p}");
        assert!(p.contains("git merge main"), "got: {p}");
        assert!(p.contains("Do NOT merge the other direction"), "got: {p}");
    }

    #[test]
    fn unknown_kind_is_none() {
        assert_eq!(action_prompt("frobnicate", "a", "b"), None);
    }
}
```

- [ ] **Step 2: Declare the module**

In `src-tauri/src/agents/mod.rs`, add (near the other `pub mod` lines):

```rust
pub mod prompts;
```

- [ ] **Step 3: Run tests — verify pass (they were never failing-to-compile because module is new; confirm RED→GREEN by first running with the bodies stubbed if desired)**

Run: `cd src-tauri && cargo test --lib prompts::tests`
Expected: 4 tests PASS. (To honor RED first: temporarily replace each `match` arm body with `String::new()`, run → assertions FAIL, then restore.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/agents/prompts.rs src-tauri/src/agents/mod.rs
git commit -m "feat(agents): predefined completion-action prompt templates"
```

---

## Task 2: Backend git CLI push (TDD)

**Files:**
- Modify: `src-tauri/src/git/service.rs`

- [ ] **Step 1: Write the failing test**

In `src-tauri/src/git/service.rs` `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn push_sends_branch_to_a_local_origin() {
        let origin = tempfile::tempdir().unwrap();
        git2::Repository::init_bare(origin.path()).unwrap();

        let work = tempfile::tempdir().unwrap();
        let git = GitService::new();
        git.init(work.path()).unwrap();
        std::fs::write(work.path().join("a.txt"), "hi").unwrap();
        git.commit(work.path(), "init", &[]).unwrap();

        let repo = git2::Repository::open(work.path()).unwrap();
        repo.remote("origin", origin.path().to_str().unwrap()).unwrap();
        let branch = repo.head().unwrap().shorthand().unwrap().to_string();

        git.push(work.path(), "origin", &branch).unwrap();

        let bare = git2::Repository::open(origin.path()).unwrap();
        assert!(
            bare.find_reference(&format!("refs/heads/{branch}")).is_ok(),
            "origin should now have the pushed branch"
        );
    }
```

- [ ] **Step 2: Run — verify it fails**

Run: `cd src-tauri && cargo test --lib push_sends_branch_to_a_local_origin`
Expected: FAIL to compile — `no method named push`.

- [ ] **Step 3: Implement `push`**

In `src-tauri/src/git/service.rs`, add to `impl GitService` (near `commit`):

```rust
    /// Push `branch` to `remote` using the system `git` CLI (so the OS credential
    /// helper handles auth — the one place we shell out instead of using git2,
    /// because libgit2 push needs manual credential callbacks). Errors carry git's
    /// stderr.
    pub fn push(&self, worktree: &Path, remote: &str, branch: &str) -> AppResult<()> {
        let out = std::process::Command::new("git")
            .current_dir(worktree)
            .args(["push", "-u", remote, branch])
            .output()
            .map_err(|e| AppError::Other(format!("git push: {e}")))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(AppError::Other(format!(
                "git push failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }
```

- [ ] **Step 4: Run — verify it passes**

Run: `cd src-tauri && cargo test --lib push_sends_branch_to_a_local_origin`
Expected: PASS. (Requires the `git` CLI on PATH — present in dev/CI.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/git/service.rs
git commit -m "feat(git): push branch to a remote via the git CLI"
```

---

## Task 3: AgentService::push + remove AgentService::merge

**Files:**
- Modify: `src-tauri/src/agents/service.rs`

- [ ] **Step 1: Add `push`**

In `src-tauri/src/agents/service.rs` `impl AgentService` (near `commit`), add:

```rust
    /// Push the agent's branch to `origin` (deterministic backend push).
    pub fn push(&self, id: Uuid) -> AppResult<()> {
        let rec = self.record(id)?;
        self.git.push(Path::new(&rec.worktree), "origin", &rec.branch)
    }
```

- [ ] **Step 2: Remove `merge`**

Delete the `merge` method (the one calling `self.git.merge(project_path, &rec.branch)`):

```rust
    /// Merge the agent's branch into the source project's current branch.
    pub fn merge(&self, id: Uuid, project_path: &Path) -> AppResult<()> {
        let rec = self.record(id)?;
        self.git.merge(project_path, &rec.branch)
    }
```

(Leave `GitService::merge` — it's a tested primitive used elsewhere; just stop the agent layer from exposing the risky direction.)

- [ ] **Step 3: Build (will fail until commands updated — that's Task 4)**

Run: `cd src-tauri && cargo build`
Expected: error in `agents/commands.rs` (`agent_merge` calls the removed `svc.merge`). Fixed in Task 4 — proceed.

---

## Task 4: agent_action + agent_push commands; remove agent_merge; register

**Files:**
- Modify: `src-tauri/src/agents/commands.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add `agent_action` and `agent_push`; remove `agent_merge`**

In `src-tauri/src/agents/commands.rs`, DELETE the `agent_merge` command:

```rust
#[tauri::command]
pub fn agent_merge(
    svc: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    id: Uuid,
) -> AppResult<()> {
    let agent = svc.get(id)?;
    let project_path = projects.path_of(agent.project_id)?;
    svc.merge(id, std::path::Path::new(&project_path))
}
```

Add (near `agent_input`/`agent_allow`, which also use `RuntimeService`):

```rust
/// Backend push: push the agent's branch to origin (deterministic).
#[tauri::command]
pub fn agent_push(svc: State<'_, AgentService>, id: Uuid) -> AppResult<()> {
    svc.push(id)
}

/// AI-driven completion action: resolve the predefined prompt for `kind`
/// (commit/push/rebase/merge) and type it into the agent's RUNNING PTY. Errors if
/// the process isn't running (the UI enables these only when running) or `kind` is
/// unknown. Fire-and-forward — the agent runs the git with its own tools.
#[tauri::command]
pub fn agent_action(
    rt: State<'_, RuntimeService>,
    svc: State<'_, AgentService>,
    id: Uuid,
    kind: String,
) -> AppResult<()> {
    let agent = svc.get(id)?;
    let prompt = super::prompts::action_prompt(&kind, &agent.branch, &agent.base)
        .ok_or_else(|| AppError::Other(format!("unknown action: {kind}")))?;
    rt.write(id, &format!("{prompt}\r")).map_err(AppError::Other)
}
```

If `ProjectService` is now unused in `commands.rs` imports after removing `agent_merge`, remove that import to avoid a warning. (Check the `use` block at the top.)

- [ ] **Step 2: Update the handler in lib.rs**

In `src-tauri/src/lib.rs` `generate_handler!`, remove `agents::commands::agent_merge,` and add the two new ones (e.g. after `agent_discard`):

```rust
            agents::commands::agent_discard,
            agents::commands::agent_action,
            agents::commands::agent_push,
```

(Delete the line `agents::commands::agent_merge,`.)

- [ ] **Step 3: Build + full backend test suite**

Run: `cd src-tauri && cargo build && cargo test --lib`
Expected: builds clean; all tests pass (prompts + push + existing).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/agents/service.rs src-tauri/src/agents/commands.rs src-tauri/src/lib.rs
git commit -m "feat(agents): agent_action (AI ops) + agent_push; remove risky agent_merge"
```

---

## Task 5: Frontend bridge — add commands, remove AgentMerge

**Files:**
- Modify: `src/app/orchestra/data-source/bridge.ts`

- [ ] **Step 1: Edit the Commands map**

Remove `AgentMerge: 'agent_merge',`. Add (after `AgentDiscard: 'agent_discard',`):

```ts
  AgentAction: 'agent_action',
  AgentPush: 'agent_push',
```

- [ ] **Step 2: Commit (build deferred — store/UI still reference merge until Task 6/8)**

```bash
git add src/app/orchestra/data-source/bridge.ts
git commit -m "feat(agents): bridge commands for agent_action/agent_push; drop agent_merge"
```

---

## Task 6: Frontend store — add action/push, remove merge

**Files:**
- Modify: `src/app/orchestra/stores/agents.store.ts`

- [ ] **Step 1: Replace the `merge` method**

Remove:

```ts
  /** Merge the agent's branch into the source project's branch. */
  merge(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentMerge, { id });
  }
```

Add (near `commit`):

```ts
  /** Deterministic backend push of the agent's branch to origin. */
  push(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentPush, { id });
  }
  /** AI-driven completion action: type the predefined prompt for `kind`
   *  (commit/push/rebase/merge) into the agent's running PTY. */
  action(id: string, kind: "commit" | "push" | "rebase" | "merge"): Promise<void> {
    return this.bridge.invoke(Commands.AgentAction, { id, kind });
  }
```

- [ ] **Step 2: Typecheck (will still error in agent-actions.service until Task 8)**

Run: `pnpm exec tsc --noEmit -p tsconfig.app.json`
Expected: errors only in `agent-actions.service.ts` (references `agentsStore.merge`). Fixed in Task 8.

---

## Task 7: Sparkles icon

**Files:**
- Modify: `src/app/orchestra/utils.ts`

- [ ] **Step 1: Add the icon to `ICONS`**

In `src/app/orchestra/utils.ts`, find the `ICONS` map (name → single SVG path `d`, 24×24, stroked). Add a `sparkles` entry (a star + a small sparkle, single path):

```ts
  sparkles: "M12 3l1.6 4.4 4.4 1.6-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.6L12 3zM18.5 14l.6 1.7 1.7.6-1.7.6-.6 1.7-.6-1.7-1.7-.6 1.7-.6.6-1.7z",
```

- [ ] **Step 2: Typecheck**

Run: `pnpm exec tsc --noEmit -p tsconfig.app.json`
Expected: no NEW errors from this file. Render-check happens in Task 9's build.

- [ ] **Step 3: Commit**

```bash
git add src/app/orchestra/utils.ts
git commit -m "feat(ui): add sparkles icon for AI actions"
```

> Brand logos (Claude/Gemini/Cursor/Codex from icons8) are a deferred follow-up — they need a separate multi-path brand-icon component; `app-icon` only renders one stroke path.

---

## Task 8: Rework AgentActionsService

**Files:**
- Modify: `src/app/orchestra/agents/agent-actions.service.ts`

- [ ] **Step 1: Update `act()` — push→backend, merge→AI, drop pr**

Replace the `commit`/`merge`/`push`/`pr` cases in `act()` with:

```ts
      case "commit": // backend commit-all (the git-tab uses commitAgent with a selection)
        this.commitAgent(id, [], "wip: " + nm);
        break;
      case "discard":
        this.discardAgent(id, []);
        break;
      case "push": // deterministic backend push
        this.pushAgent(id);
        break;
      case "rebase":
      case "merge":
        this.aiAction(id, action as "rebase" | "merge");
        break;
```

(Delete the old `merge` and `pr` cases.)

- [ ] **Step 2: Add `pushAgent` + `aiAction`; remove `mergeAgent`**

Delete `mergeAgent(...)`. Add:

```ts
  /** Deterministic backend push of the agent's branch to origin. */
  pushAgent(id: string) {
    const ag = this.agents().find((a) => a.id === id);
    void this.agentsStore
      .push(id)
      .then(() => this.ui.flash("pushed " + (ag?.name ?? id)))
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "push failed"));
  }

  /** AI-driven completion action: type the predefined prompt into the agent's PTY.
   *  Only valid while running (the tool must be at its prompt to receive input);
   *  switches to the terminal so the user watches it run. */
  aiAction(id: string, kind: "commit" | "push" | "rebase" | "merge") {
    const ag = this.agents().find((a) => a.id === id);
    if (!ag || ag.status !== "running") {
      this.ui.flash("start the agent first");
      return;
    }
    this.ui.openAgent(id, "terminal");
    void this.agentsStore
      .action(id, kind)
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "action failed"));
  }
```

- [ ] **Step 3: Update `agentMenu` — drop PR, add Rebase, fix Merge**

In `agentMenu`, remove the "Open pull request" item. Replace the Merge item and add a Rebase item (both AI, enabled only when running):

```ts
      { label: "Push to origin", icon: "push", disabled: !ag.commits, onClick: () => this.act(id, "push") },
      {
        label: "Rebase onto " + branchTarget,
        icon: "sparkles",
        disabled: ag.status !== "running",
        onClick: () => this.act(id, "rebase"),
      },
      {
        label: "Merge " + branchTarget + " → " + ag.branch.replace("agent/", ""),
        icon: "sparkles",
        accent: "var(--st-done)",
        disabled: ag.status !== "running",
        onClick: () => this.act(id, "merge"),
      },
```

(`branchTarget` already = `proj ? proj.branch : "main"` = the base. Direction now reads base → branch.)

- [ ] **Step 4: Typecheck**

Run: `pnpm exec tsc --noEmit -p tsconfig.app.json`
Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add src/app/orchestra/agents/agent-actions.service.ts
git commit -m "feat(agents): backend push + AI rebase/merge actions; drop PR/merge-to-main"
```

---

## Task 9: Git-tab split buttons + AI rebase/merge

**Files:**
- Modify: `src/app/orchestra/right-panel/git-tab.component.ts`

- [ ] **Step 1: Replace the actions block**

In the template, replace the existing actions `<div style="padding:12px;display:grid;gap:7px">…</div>` block (the commit input + Commit/Push/Merge/Discard buttons) with split buttons for Commit/Push and AI-only Rebase/Merge:

```html
        <!-- actions -->
        <div style="padding:12px;display:grid;gap:7px">
          @if (changes().length) {
            <input
              [value]="commitMsg()"
              (input)="commitMsg.set($any($event.target).value)"
              placeholder="commit message…"
              style="background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:8px 10px;color:var(--ink);font-family:var(--font-mono);font-size:11.5px;outline:none"
            />
          }

          <!-- Commit: primary = backend, attached ✨ = AI -->
          <div style="display:flex;gap:4px">
            <button class="btn ghost-hair" [disabled]="changes().length === 0" (click)="commit(ag.id)" style="flex:1;min-width:0;justify-content:flex-start">
              <app-icon name="commit" size="sm" />Commit {{ selected().size ? selected().size + ' selected' : 'all' }}
            </button>
            <button class="btn ghost-hair" [disabled]="ag.status !== 'running'" [title]="ag.status === 'running' ? 'Let the agent commit' : 'Start the agent first'" (click)="agentActions.aiAction(ag.id, 'commit')" style="flex:none;padding:0 9px">
              <app-icon name="sparkles" size="sm" color="var(--accent)" />
            </button>
          </div>

          <!-- Push: primary = backend, attached ✨ = AI -->
          <div style="display:flex;gap:4px">
            <button class="btn ghost-hair" [disabled]="ag.commits === 0" (click)="agentActions.pushAgent(ag.id)" style="flex:1;min-width:0;justify-content:flex-start">
              <app-icon name="push" size="sm" />Push to origin
            </button>
            <button class="btn ghost-hair" [disabled]="ag.status !== 'running'" [title]="ag.status === 'running' ? 'Let the agent push' : 'Start the agent first'" (click)="agentActions.aiAction(ag.id, 'push')" style="flex:none;padding:0 9px">
              <app-icon name="sparkles" size="sm" color="var(--accent)" />
            </button>
          </div>

          <!-- Rebase (AI only) -->
          <button class="btn ghost-hair" [disabled]="ag.status !== 'running'" [title]="ag.status === 'running' ? '' : 'Start the agent first'" (click)="agentActions.aiAction(ag.id, 'rebase')" style="justify-content:flex-start;min-width:0">
            <app-icon name="sparkles" size="sm" color="var(--accent)" />
            <span style="min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">Rebase onto {{ project() ? project()!.branch : 'main' }}</span>
          </button>

          <!-- Merge (AI only): base → branch -->
          <button class="btn primary" [disabled]="ag.status !== 'running'" [title]="ag.status === 'running' ? '' : 'Start the agent first'" (click)="agentActions.aiAction(ag.id, 'merge')" style="justify-content:center;min-width:0">
            <app-icon name="sparkles" size="sm" />
            <span
              [title]="'Merge ' + (project() ? project()!.branch : 'main') + ' → ' + ag.branch.replace('agent/', '')"
              style="min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
            >Merge {{ project() ? project()!.branch : 'main' }} → {{ ag.branch.replace('agent/', '') }}</span>
          </button>

          <button class="btn ghost-hair" [disabled]="changes().length === 0" (click)="discard(ag.id)" style="justify-content:flex-start;color:var(--st-blocked)">
            <app-icon name="discard" size="sm" />Discard {{ selected().size ? selected().size + ' selected' : 'all' }}
          </button>
        </div>
```

(Note: Merge direction text is now `base → branch` — the safe direction — matching the AI prompt.)

- [ ] **Step 2: Build (typechecks templates)**

Run: `pnpm build`
Expected: clean (pre-existing bundle-budget warning only).

- [ ] **Step 3: Commit**

```bash
git add src/app/orchestra/right-panel/git-tab.component.ts
git commit -m "feat(git-tab): split commit/push (backend + AI) and AI rebase/merge"
```

---

## Task 10: Full verification

- [ ] **Step 1: Backend + frontend suites**

Run: `cd src-tauri && cargo test --lib` then (repo root) `pnpm test && pnpm build`
Expected: all Rust tests pass; 55+ FE tests pass; build clean.

- [ ] **Step 2: Manual smoke (run the app)**

Run: `pnpm dev`
Expected: with a running agent, the git tab shows Commit/Push split buttons (✨ attached) + AI Rebase + AI Merge (base → branch); the ✨ buttons are disabled with a tooltip when the agent isn't running; clicking an ✨ action switches to the terminal and types the predefined prompt. There is no "Open pull request" anywhere, and no "merge → main" action.

---

## Self-review notes

- **Spec coverage:** commit/push hybrid (Tasks 3/4/6/9), rebase/merge AI-only via `agent_action` (Tasks 1/4/8/9), no PR (removed in Tasks 5/8/9), merge = base→branch (prompt Task 1 + label Task 9), old `agent_merge` removed (Tasks 3/4/5/6), sparkles icon (Task 7), permissions reuse hooks (inherent — the agent runs git itself). ✓
- **Deviation from spec:** AI actions require a running agent (no auto-start) — documented above; auto-start deferred.
- **Type consistency:** `agent_action(id, kind)` ↔ store `action(id, kind)` ↔ `aiAction(id, kind)`; `kind` union `"commit"|"push"|"rebase"|"merge"` used consistently. `agent_push` ↔ `push`. ✓
- **Brand logos:** explicitly deferred; sparkles is the v1 AI glyph.
