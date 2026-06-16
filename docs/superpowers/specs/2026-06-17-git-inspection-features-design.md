# Human-Driven Git Inspection & Operations — Design

**Date:** 2026-06-17
**Status:** Functional design approved (brainstorm). UI to be designed separately (Claude design); implementation plan to follow.

## Goal & context

Add a set of **human-driven git-client features** over the **project's repository** — inspect
history and diffs, blame, file history, plus two stateful operations (conflict resolution, partial
commit/discard). These are standard git-client capabilities (the kind IntelliJ provides), **operated
directly by the human, independent of the agent/worktree flow**.

The existing **agent git actions stay unchanged** (the per-agent git tab: AI commit/push/rebase/merge,
backend commit/push/discard/merge). These new features sit *alongside* that surface, not replacing it.

Source of truth for the gap analysis these come from: `docs/git-feature-gap-analysis.md`.

## Shared backbone

A single **`diff(from, to)`** engine in `src-tauri/src/git/service.rs` over any tree-ish — a commit
oid, `commit^` (parent), the working tree, the index, or `rev:path` — returning the existing
`FileChange[]` (status + add/del) and per-file `FileDiff`. Every inspection feature composes from it.
All operate on the **project repo** (resolvable to any branch/commit). Read-only except #4 and #6.

## The six features (functional)

**1. File changes per commit + per-commit diff**
`commit_files(oid)` → `diff(oid^, oid)` → `FileChange[]`; `commit_file_diff(oid, path)` → `FileDiff`.
Merge commits diff against the **first parent** by default.

**2. Multi-commit / range diff**
`range_diff(from, to)` → `diff(oldest^, newest)` across the selection (cumulative tree diff between
the boundary commits — standard "compare these revisions" semantics). Same `FileChange[]` + per-file
diffs.

**3. Git blame / annotate**
`blame(path, rev?)` → per-line `[{line, oid, author, date, summary}]` via `git2` blame, on the file
at HEAD or any revision; computed on demand per file. Rename-follow optional (defer; add if needed).

**4. Conflict resolution (human 3-way)** — *stateful; Orrery drives the operation*
`merge_start(branch)` / `rebase_start(onto)` → list of conflicted files. Because Orrery runs the op,
the index carries stages 1/2/3, so `conflict_sides(path)` → **base / ours / theirs** content (live,
no reconstruction). Human resolves → `resolve_file(path, content)` (writes + marks stage-0 resolved)
→ `merge_commit` / `rebase_continue`; plus `abort`. A small "merge/rebase session" state tracks the
in-progress operation.

**5. File history / diff vs revision**
`file_history(path)` → commits touching the path (log walk filtered by path) → `[{oid, author, date,
summary}]`; then compare the file at any two revisions via the `diff(revA:path, revB:path)` engine.

**6. Hunk/line-level review (partial commit + partial discard)** — *stateful; patch surgery*
Generate the file's hunks; the human selects hunks/lines. **Partial discard** = reverse-apply
selected hunks to the working tree. **Partial commit** = apply selected hunks to a temp index/tree
and commit only those — a lightweight, transient staging step (no permanent staging-area concept
added). Via `git2` patch apply.

## Risk classification & sequencing

All six are treated in **one pass** (one effort, one worktree, one plan), but **sequenced by risk** —
the read-only features first to land value early and de-risk the shared engine, the **stateful
high-risk** features last.

| Phase | Features | Risk | Why this order |
|---|---|---|---|
| **0 — Foundation** | Shared `diff(from, to)` engine + command/bridge wiring | low | Everything composes from it; build + test once. |
| **1 — Read-only inspection (start here)** | 1 per-commit diff · 2 range diff · 5 file history · 3 blame | **low** | Pure reads over the repo; no working-tree/state mutation; reuse the diff engine. Ship value fast, prove the surface. |
| **2 — Stateful operations (end)** | 4 conflict resolution (merge/rebase session) · 6 hunk-level partial commit/discard | **high** | Mutate the working tree / index and carry in-progress session state. Must not corrupt a repo. Done last, on a proven foundation, with the most testing. |

Within Phase 1: Foundation → 1 → 2 → 5 → 3. Within Phase 2: 4 → 6.

## Out of scope

- Any change to the **agent git actions** (they stay as-is).
- Remote sync (fetch/pull), interactive rebase, cherry-pick, tags, stash/shelve, submodules, LFS,
  in-app GitHub PR — not part of this effort (see the gap analysis for the full landscape).
- A permanent staging-area concept (partial commit uses a transient tree, not a persisted index UI).

## UI & implementation

- **UI** is designed separately via Claude design (a prompt is provided alongside this spec); the
  human surfaces (where commits expand, blame gutter, 3-way view, hunk selection, history view) come
  from that design.
- **Implementation** happens in a dedicated **worktree**, planned via the writing-plans skill once
  the UI design is in hand. The plan will follow the phase order above.
