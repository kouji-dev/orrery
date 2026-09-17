# Git Inspection (Phase 1 — Read-Only) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the agent-view git inspection surfaces from the design handoff — expandable per-branch commit history with range-select (right panel), and per-commit diff / range diff / file history / blame views (center "Diff" tab) — full-stack and performant.

**Architecture:** A single read-only `diff(from, to)` engine in the Rust `git/service.rs` (over any tree-ish) underpins per-commit/range/history diffs; add `blame` and `file_history` ops. A new `GitInspectStore` mirrors `AgentWorkStore`'s `Loadable`-map + generation-guard pattern for the new data. Angular components port the design 1:1 from `docs/design/git-inspection/*.jsx`, **reusing the existing workspace diff renderer** (`diff-view`'s hunk rows) rather than re-implementing it. Big lists (blame, long diffs) use CDK virtual scroll; all components OnPush + signals.

**Tech Stack:** Rust + `git2`, Tauri commands, Angular 20 (standalone, OnPush, signals), `@angular/cdk` virtual scroll, vitest (jsdom) for the store, the density-token CSS system.

**Spec:** `docs/superpowers/specs/2026-06-17-git-inspection-features-design.md`
**Design reference (port the UI from these):** `docs/design/git-inspection/agent-git.jsx`, `repo-diff.jsx`, `repo-extra.jsx` (+ `shots/`).
**Scope note:** This is **Phase 1 (read-only)** only. The stateful **Phase 2** (conflict resolution `repo-conflict.jsx`, hunk-level partial commit/discard) is a separate plan, built after this lands.

---

## File Structure

**Backend (Rust)**
- Modify `src-tauri/src/git/service.rs` — add: `diff_treeish(repo, from, to)` engine; `commit_files(oid)`; `commit_file_diff(oid, path)`; `range_diff(from_oid, to_oid)`; `blame(path, rev)`; `file_history(path, limit, offset)`. Reuse existing `FileChange`/`FileDiff`/`LogEntry` models.
- Modify `src-tauri/src/git/` model module (or `service.rs`) — add `BlameLine`, `FileHistoryEntry` structs.
- Modify the agent git command layer (where `AgentChanges`/`AgentCommits` are registered — `src-tauri/src/agents/commands.rs` or the git commands module) — add commands: `agent_commit_diff`, `agent_range_diff`, `agent_blame`, `agent_file_history`. Each resolves the agent → worktree path, then calls the service.

**Frontend data**
- Create `src/app/agents/git-inspect.store.ts` — `GitInspectStore`: keyed `Loadable` maps for `commitDiff(id,sha)`, `rangeDiff(id,shas)`, `blame(id,path,rev)`, `fileHistory(id,path)`. Mirrors `agent-work.store.ts` (generation guards, dispose).
- Modify `src/app/data-source/bridge.ts` — add the four `Commands.*` enum entries.
- Modify `src/app/models.ts` — add `BlameLine`, `FileHistoryEntry`, `CommitFile` (per-commit file w/ state+add+del), `GitView` discriminated union (`{kind:'commit',sha,path?}` | `{kind:'range',shas}` | `{kind:'filehistory',path}`).

**Frontend UI (port from design)**
- Create `src/app/right-panel/agent-commit-history.component.ts` — design `AgentCommitHistory` + `AgentCommitRow` (expandable rows, range select bar).
- Create `src/app/workspace/git/commit-diff-view.component.ts` — design `AgentCommitDiffView` (file list + `DiffOrBlame`).
- Create `src/app/workspace/git/range-diff-view.component.ts` — design `AgentRangeDiffView`.
- Create `src/app/workspace/git/file-blame.component.ts` — design `FileBlameGutter` (virtualized).
- Create `src/app/workspace/git/file-history-view.component.ts` — design `FileHistoryView` (from `repo-extra.jsx`).
- Create `src/app/workspace/git/diff-or-blame.component.ts` — design `DiffOrBlame` (diff with an Annotate toggle).
- Create `src/app/shared/git/` small atoms as needed (`author-avatar`, `sha-chip`, `state-badge`) — only if not already present; reuse `app-icon`, status dots, and the existing `+N/−N` add/del markup.
- Modify the agent **center** host (the pane that renders the diff tab — `src/app/workspace/pane-node.component.ts` / `file-view`/`diff-view` host) to render `AgentGitView` when a `GitView` is active.
- Modify `src/app/right-panel/git-tab.component.ts` — replace the flat "commits on this branch" feed with `agent-commit-history`, wiring `openCommit`/`openRange`/`openFileHistory` to set the active `GitView`.

**Tests**
- `src-tauri/src/git/service.rs` `#[cfg(test)]` — unit tests per new op against a temp repo fixture.
- `src/app/agents/git-inspect.store.spec.ts` — store behavior (loading/ready/error, generation guard, dispose) with a mock bridge.
- `e2e/git-inspection.spec.ts` — Playwright: open an agent, expand a commit, open its diff, toggle Annotate, select two commits → range diff.

---

## Phase 1 task list (build order: backend engine → ops → store → components → integration → perf)

### Task 1 — Backend: the `diff(from, to)` engine

**Files:** Modify `src-tauri/src/git/service.rs` (near the existing `file_diff`/`status`); Test: same file `#[cfg(test)]`.

- [ ] **Step 1 — failing test.** Add a test that builds a temp repo with two commits (file `a.txt` changed), then asserts `diff_treeish(&repo, Treeish::Commit(parent), Treeish::Commit(head))` returns one `FileChange` for `a.txt` with the right `state`/`add`/`del`.

```rust
#[test]
fn diff_treeish_reports_changed_file_between_two_commits() {
    let (dir, repo) = fixture_two_commits(); // helper: commit1 adds a.txt="x\n", commit2 makes it "x\ny\n"
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let parent = head.parent(0).unwrap();
    let svc = GitService::new();
    let changes = svc.diff_treeish(&repo, head.parent(0).unwrap().tree().unwrap(), head.tree().unwrap());
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "a.txt");
    assert_eq!(changes[0].add, 1);
}
```

- [ ] **Step 2 — run, expect FAIL** (`cargo test -p <crate> diff_treeish` → "no method `diff_treeish`"). Run from `src-tauri`.
- [ ] **Step 3 — implement.** Add a private `diff_trees(&self, repo, from_tree: Option<&Tree>, to_tree: Option<&Tree>) -> Vec<FileChange>` using `repo.diff_tree_to_tree`, with `find_similar` for rename detection, mapping deltas → `FileChange` (status A/M/D/R, `add`/`del` from `diff.stats()` per-file via a `foreach`/`Patch::from_diff`). Reuse the exact `FileChange` shape that `status()` returns so the frontend model is unchanged. Public `diff_treeish` resolves tree-ish (commit/parent/workdir/index) → `Tree` then calls `diff_trees`.
- [ ] **Step 4 — run, expect PASS.**
- [ ] **Step 5 — commit.** `git add -A && git commit -m "feat(git): diff(from,to) tree-ish engine"`

### Task 2 — Backend: per-commit files + per-commit file diff

**Files:** Modify `src-tauri/src/git/service.rs`; Modify the git command module; Test: service `#[cfg(test)]`.

- [ ] **Step 1 — failing test.** `commit_files(&repo, oid)` returns the file list (= `diff_treeish(commit^, commit)`); merge commits diff against first parent. `commit_file_diff(&repo, oid, "a.txt")` returns a `FileDiff` with hunks (reuse the existing `file_diff` hunk builder against the two blobs).
- [ ] **Step 2 — run, expect FAIL.**
- [ ] **Step 3 — implement.** `commit_files` = resolve commit, `parent = commit.parent(0).ok()`, `diff_treeish(parent_tree_or_empty, commit_tree)`. `commit_file_diff` = build a `git2::Diff` for that one path (`pathspec`) and feed the existing hunk-extraction used by `file_diff` (refactor `file_diff`'s hunk builder into a shared `hunks_from_diff(diff) -> FileDiff` if needed). Add Tauri command `agent_commit_diff(id, sha)` → `{ files: FileChange[], }` and `agent_commit_file_diff(id, sha, path)` → `FileDiff` (resolve agent worktree path like `agent_changes` does).
- [ ] **Step 4 — run, expect PASS.** Also `cargo build` the command wiring.
- [ ] **Step 5 — commit.** `feat(git): per-commit files + file diff + commands`

### Task 3 — Backend: range diff

**Files:** Modify `src-tauri/src/git/service.rs` + command module; Test: service.

- [ ] **Step 1 — failing test.** `range_diff(&repo, oldest_parent_oid, newest_oid)` = `diff_treeish(oldest^, newest)`; with 3 linear commits selecting commits 2 and 3, the cumulative file list equals the union of their changes.
- [ ] **Step 2 — run, expect FAIL.**
- [ ] **Step 3 — implement.** `range_diff(repo, from, to)` resolves `from` to its tree and `to` to its tree and calls `diff_treeish`. The command `agent_range_diff(id, shas: Vec<String>)` sorts the shas by commit time (topo) to find oldest/newest, then diffs `oldest^ .. newest`. Returns `{ files: FileChange[] }`; per-file diff reuses `agent_commit_file_diff` semantics via `agent_range_file_diff(id, from, to, path)` OR returns enough for the frontend to call `commit_file_diff` on `newest` (decide: return `from`/`to` oids so the frontend fetches per-file via a `range_file_diff(id, from, to, path)` command). Implement `agent_range_file_diff(id, from, to, path)`.
- [ ] **Step 4 — run, expect PASS + build.**
- [ ] **Step 5 — commit.** `feat(git): range diff + commands`

### Task 4 — Backend: file history

**Files:** Modify `src-tauri/src/git/service.rs` + command module; add `FileHistoryEntry`; Test: service.

- [ ] **Step 1 — failing test.** `file_history(&repo, "a.txt", 20, 0)` returns the commits (newest-first) that touched `a.txt`, each `{ sha, author, email, when, summary, add, del }`.
- [ ] **Step 2 — run, expect FAIL.**
- [ ] **Step 3 — implement.** Walk `repo.revwalk()` from HEAD (topo+time), for each commit compute `diff_treeish(parent, commit)` filtered to `path` (pathspec); keep commits whose diff touches the path; page via `limit`/`offset`. (Rename-follow deferred.) Command `agent_file_history(id, path, limit, offset)`.
- [ ] **Step 4 — run, expect PASS + build.**
- [ ] **Step 5 — commit.** `feat(git): file history + command`

### Task 5 — Backend: blame

**Files:** Modify `src-tauri/src/git/service.rs` + command module; add `BlameLine`; Test: service.

- [ ] **Step 1 — failing test.** `blame(&repo, "a.txt", None)` returns one `BlameLine` per line: `{ n, sha, author, when, summary, line }`, with the right commit sha per line in a 2-commit fixture where line 1 came from commit1 and line 2 from commit2.
- [ ] **Step 2 — run, expect FAIL.**
- [ ] **Step 3 — implement.** `git2::Blame` via `repo.blame_file(path, opts)`; iterate `blame.iter()` hunks → expand to per-line entries; read the file content (HEAD or `rev`) for the line text; resolve each hunk's `final_commit_id` → author/summary/when. Command `agent_blame(id, path, rev?)`.
- [ ] **Step 4 — run, expect PASS + build.**
- [ ] **Step 5 — commit.** `feat(git): blame + command`

### Task 6 — Frontend: `GitInspectStore` + models + bridge commands

**Files:** Create `src/app/agents/git-inspect.store.ts`; Modify `src/app/models.ts`, `src/app/data-source/bridge.ts`; Test: `src/app/agents/git-inspect.store.spec.ts`.

- [ ] **Step 1 — models + commands.** Add to `models.ts`: `CommitFile {path,state,add,del}`, `BlameLine`, `FileHistoryEntry`, `GitView` union. Add to `bridge.ts` `Commands`: `AgentCommitDiff`, `AgentCommitFileDiff`, `AgentRangeFiles`, `AgentRangeFileDiff`, `AgentBlame`, `AgentFileHistory` (string values matching the Rust command names).
- [ ] **Step 2 — failing store test.** Mirror `agent-work.store` pattern: `blameFor(id,path)` returns `idle` until `loadBlame`, then `loading`→`ready`; a newer `loadBlame` supersedes an in-flight older one (generation guard); `dispose(id)` drops entries. Use a mock `BRIDGE` returning a deferred promise.
- [ ] **Step 3 — run, expect FAIL** (`pnpm test git-inspect`).
- [ ] **Step 4 — implement** `GitInspectStore` with keyed `Loadable` maps + generation guards for commitDiff/rangeFiles/blame/fileHistory, following `agent-work.store.ts` exactly (keys: `id` for commitDiff-by-sha use `id+'/'+sha`; blame `id+'/'+path`; etc.).
- [ ] **Step 5 — run, expect PASS.**
- [ ] **Step 6 — commit.** `feat(git): GitInspectStore + models + commands`

### Task 7 — Frontend: shared git atoms (port `repo-diff.jsx` atoms)

**Files:** Create `src/app/shared/git/git-atoms.ts` (or component files) for `AuthorAvatar`, `Sha`, `RStateBadge`; reuse existing add/del markup. Test: none (pure presentational).

- [ ] **Step 1 — port** `AuthorAvatar` (initials chip; agent = rounded-square, human = circle, colored per author), `Sha` (chip, optional click→accent hover), `RStateBadge` (A/M/D/R colored badge) from `docs/design/git-inspection/repo-diff.jsx:10-53`, translating React→Angular standalone OnPush, **using density tokens** (no hardcoded px — the CI scanner enforces it: snap per the project's spacing/type scale).
- [ ] **Step 2 — build** `pnpm build` succeeds.
- [ ] **Step 3 — commit.** `feat(git): shared git atoms (avatar/sha/state badge)`

### Task 8 — Frontend: `agent-commit-history` (right panel)

**Files:** Create `src/app/right-panel/agent-commit-history.component.ts`; Modify `src/app/right-panel/git-tab.component.ts`. Test: build + manual.

- [ ] **Step 1 — port** `AgentCommitHistory` + `AgentCommitRow` from `docs/design/git-inspection/agent-git.jsx:57-135`: expandable commit rows (chevron, msg, HEAD chip, author avatar, sha, file count, ±, rel time); a per-row select checkbox; the "N selected → Clear / Diff (N)" range bar; expanded rows list the commit's files (state badge, name, ±, file-history button). Inputs: `agent`. Outputs: `openCommit(sha, path?)`, `openRange(shas)`, `openFileHistory(path)`. Data: `GitInspectStore.commitFilesFor` (lazy-load each commit's files on first expand via `AgentCommitDiff` — files only) + existing `AgentWorkStore.commitsFor` for the commit list/paging.
- [ ] **Step 2 — wire** into `git-tab.component.ts`: replace the flat "Commits on this branch" `app-commit-feed` block with `<app-agent-commit-history>`, forwarding its outputs to a `gitView` setter on the agent center (Task 13).
- [ ] **Step 3 — build + commit.** `feat(git): expandable commit history + range select (right panel)`

### Task 9 — Frontend: `commit-diff-view` (center)

**Files:** Create `src/app/workspace/git/commit-diff-view.component.ts`; create `src/app/workspace/git/diff-or-blame.component.ts`. Test: build.

- [ ] **Step 1 — port** `AgentCommitDiffView` (`agent-git.jsx:196-216`): `CommitContextHeader` (commit msg + author + sha + rel) over a `232px 1fr` grid of `DiffFileList` (reuse the existing diff-view's tree/flat file list — Task 9a) + `DiffOrBlame`. Load files via `GitInspectStore` commit-diff; per selected file load its diff via `AgentCommitFileDiff`.
- [ ] **Step 1a** — reuse the existing `diff-view`'s file-list + hunk renderer. If they're not extractable as-is, extract the hunk-row renderer from `src/app/workspace/diff-view.component.ts` into a shared `diff-hunks.component.ts` (matching `repo-diff.jsx HunkRows`) and have both `diff-view` and the new git views use it (DRY). Add the optional `lead`/`lineStyle` hooks now (used by Phase 2 hunk selection) but unused here.
- [ ] **Step 2 — port** `DiffOrBlame` (`agent-git.jsx:173-194`): a diff with an "Annotate" toggle that flips to `file-blame` (Task 11).
- [ ] **Step 3 — build + commit.** `feat(git): per-commit diff view + annotate toggle`

### Task 10 — Frontend: `range-diff-view` (center)

**Files:** Create `src/app/workspace/git/range-diff-view.component.ts`. Test: build.

- [ ] **Step 1 — port** `AgentRangeDiffView` (`agent-git.jsx:218-246`): header "Range diff · N commits" with sha chips; the same `232px 1fr` file-list + `DiffOrBlame` grid. Load the union file list via `AgentRangeFiles(id, shas)`; per-file diff via `AgentRangeFileDiff(id, from, to, path)`.
- [ ] **Step 2 — build + commit.** `feat(git): range diff view`

### Task 11 — Frontend: `file-blame` (virtualized)

**Files:** Create `src/app/workspace/git/file-blame.component.ts`. Test: build + manual.

- [ ] **Step 1 — port** `FileBlameGutter` (`agent-git.jsx:12-55`): per-line gutter (author avatar+name+rel+sha on the first line of each commit-run, age-shaded background `ageBgA`), a line-number column, and the highlighted code line; hover tooltip (author, sha, commit msg, "click → open commit diff"); click → `openCommit(sha)`. Data via `GitInspectStore.blameFor`.
- [ ] **Step 2 — PERFORMANCE.** Blame files can be thousands of lines → render with **`cdk-virtual-scroll-viewport`** (fixed item size = line height) so only visible rows mount. Keep the hover tooltip in a single fixed-position element (one `@if`), not per-row DOM. `track` by line number.
- [ ] **Step 3 — build + commit.** `feat(git): virtualized blame gutter`

### Task 12 — Frontend: `file-history-view`

**Files:** Create `src/app/workspace/git/file-history-view.component.ts`. Test: build.

- [ ] **Step 1 — port** `FileHistoryView` from `docs/design/git-inspection/repo-extra.jsx` (read it; it lists the commits that touched a file and diffs the file across two picked revisions). Data via `AgentFileHistory(id, path)` + `AgentCommitFileDiff` for the selected revision pair.
- [ ] **Step 2 — build + commit.** `feat(git): file history view`

### Task 13 — Integration: agent diff tab + gitView state

**Files:** Modify the agent center host (`src/app/workspace/pane-node.component.ts` or the file/diff host) and `src/app/ui/ui.store.ts` (or a local signal) for the active `GitView`. Create `src/app/workspace/git/agent-git-view.component.ts` (the dispatcher = design `AgentGitView`).

- [ ] **Step 1 — port** `AgentGitView` (`agent-git.jsx:249-255`): dispatch `commit`→`commit-diff-view`, `range`→`range-diff-view`, `filehistory`→`file-history-view` (conflict→Phase 2). Host it in the agent center's **Diff tab** so selecting a commit/range/file-history from the right panel opens the corresponding center view; default (no gitView) keeps today's working-tree diff.
- [ ] **Step 2 — wire** the right-panel `agent-commit-history` outputs → set the active `GitView` signal that the center reads.
- [ ] **Step 3 — build + commit.** `feat(git): wire commit/range/history views into the agent diff tab`

### Task 14 — Performance pass

**Files:** all new components.

- [ ] **Step 1 — audit:** every new component is `ChangeDetectionStrategy.OnPush`; all derived UI state is `computed()` signals; `@for` uses `track`. The blame viewport and any >200-row diff use CDK virtual scroll. Commit-files load lazily on expand; diffs load lazily on file select; store entries are deduped via generation guards (already). No work in templates (move `reduce`/`map` into `computed`).
- [ ] **Step 2 — verify** no `px` literals leaked (run `node tools/density/check-tokens.mjs` → 0) and `pnpm build` clean.
- [ ] **Step 3 — commit.** `perf(git): OnPush + virtual scroll + lazy loading audit`

### Task 15 — Verification (E2E + suite)

**Files:** Create `e2e/git-inspection.spec.ts`.

- [ ] **Step 1 — E2E** (Playwright vs `ng serve`, using the app's mock/seed data path): open an agent → right panel shows commit history → expand a commit shows its files → click a file opens its diff in the center → toggle **Annotate** shows blame → select two commits → **Diff (2)** opens the range diff. Assert each surface renders.
- [ ] **Step 2 — full suite:** `pnpm test` (Rust `cargo test` + vitest) all green; `pnpm e2e` green; `pnpm build` clean; scanner 0.
- [ ] **Step 3 — commit.** `test(git): e2e + suite green for read-only inspection`

---

## Self-Review

- **Spec coverage (Phase 1):** per-commit diff (Tasks 2, 8, 9) ✓; range diff (Tasks 3, 8, 10) ✓; blame (Tasks 5, 11) ✓; file history (Tasks 4, 12) ✓; shared diff engine (Task 1) ✓; data store (Task 6) ✓; integration (Task 13) ✓; performance (Task 14) ✓. **Phase 2** (conflict `repo-conflict.jsx`, hunk-level) is explicitly out — separate plan.
- **Design fidelity:** UI tasks port directly from `docs/design/git-inspection/agent-git.jsx`/`repo-diff.jsx`/`repo-extra.jsx` (cited line ranges), reusing the existing diff hunk renderer (Task 9a extracts it shared/DRY).
- **Type consistency:** backend ops return the existing `FileChange`/`FileDiff` plus new `BlameLine`/`FileHistoryEntry`/`CommitFile`; the store keys (`id`, `id/sha`, `id/path`) and `Commands.*` names are defined once in Task 6 and reused by Tasks 8–13.
- **Performance:** virtual scroll for blame/long diffs, OnPush + signals everywhere, lazy load on expand/select, generation-guarded dedupe — called out in Task 14 and baked into each component task.

## Notes for the executor
- Read the cited design file + line ranges before each UI task; translate React→Angular idioms (hooks→signals, JSX→template), keep the **exact visual output** (the README mandates pixel-faithful), and use **density tokens** for every spacing/size/font value.
- Confirm the exact current `Commands` enum + agent command registration (`agents/commands.rs`) and the agent center pane host before Tasks 6/13 — match the existing registration pattern.
