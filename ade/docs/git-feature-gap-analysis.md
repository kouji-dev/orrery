# Case Study: Orrery's Git vs. IntelliJ IDEA — Feature Gap Analysis

**Date:** 2026-06-17

## 0. The lens that matters

IntelliJ is a full **manual git client** — a human drives every operation through rich UI.
Orrery is a **multi-agent orchestrator** whose git model is *worktree-per-agent*, where the
heavy git lifting (rebase, conflict resolution) is **delegated to the AI agent's CLI**, and the
human gets a thin **review-and-integrate** surface. So every "missing" feature is judged twice:
*(a) is it absent?* and *(b) given the agent-delegation model, is its absence a real gap or a
deliberate non-goal?*

## 1. What Orrery actually implements (ground truth from the code)

**Native git ops** (`src-tauri/src/git/service.rs`, via `git2`):
`init`/detect · `log` (paged) · `status` · `file_diff` · `commit` (per-file selection) ·
`discard` · `merge` (main→branch; FF or merge-commit, **bails on conflict**) · `push` (origin) ·
`branches` (list only) · `head_info`/`head_oid` · worktree `create`/`remove` · `ensure_main_branch`.

**AI-delegated ops** (agent runs git via CLI, LLM resolves conflicts): `commit` · `push` ·
`rebase onto main` · `merge main→branch`.

**UI:**
- **Git tab**: branch header (base, N-ahead, +/−), selectable changed-files list, commit message,
  Commit / Push / Rebase(AI) / Merge(AI) / Discard buttons, paged "commits on this branch" feed,
  all-worktrees commit feed.
- **Diff view**: inline (unified) diff with word-level highlighting, tree/flat grouping,
  collapsible hunks, moved/renamed-file rendering.

## 2. Gap matrix (vs. IntelliJ IDEA) — ✅ present · ⚠️ partial/indirect · ❌ absent

### A. Remote sync
- Fetch ❌ · Pull / **Update Project** ❌ · Push ⚠️ (plain, no force-with-lease / upstream / tags) ·
  Manage remotes ❌.
- **Impact:** Biggest *structural* gap. Orrery is **local-authority**; once a teammate pushes to
  `origin/main`, there's no in-app way to ingest it.

### B. History & investigation
- Log **graph** (DAG, filters, search) ❌ (flat paged list only) · **Blame/Annotate** ❌ ·
  **File History** / diff vs arbitrary revision ❌ · Compare branches ❌.
- **Impact:** High, and core to Orrery's premise — auditing *why/when* an agent changed a line.

### C. Local change management
- **Staging area** / partial (hunk/line) staging ❌ · **Shelve/Stash** ❌ · **Changelists** ⚠️
  (worktrees isolate de facto) · **Local History** ❌ · Rollback hunks/lines ⚠️ (discard is file-level).
- **Impact:** Mixed — worktrees subsume changelists/shelving, but no **hunk/line review** is a real
  reviewer-quality gap (accept-an-agent's-diff is all-or-nothing per file).

### D. Branch & commit surgery
- Create/checkout/rename/delete branch ❌ · **Interactive rebase** ❌ · **Amend**/reword ❌ ·
  **Cherry-pick** ❌ · **Revert** a commit ❌ · **Reset**/Undo Commit ❌ · **Tags** ❌ · squash ❌.
- **Impact:** The agent-delegation philosophy is clearest here. Squash/reword/cherry-pick are
  defensible non-goals; **amend/revert/undo** are felt because they're the everyday review safety net.

### E. Conflict resolution
- **3-way merge editor** ❌ · conflict-marker navigation ❌ · resolve during merge/rebase ⚠️ **delegated**.
- **Impact:** Orrery's signature divergence. Native `merge` returns *"merge conflicts — resolve
  manually"*; the product answer is the AI merge/rebase buttons. Novel and arguably better — but two
  holes: **no human fallback UI**, and **no visualization** of what the agent did (black box).

### F. Editor integration
- **Gutter change markers** / click-to-rollback ❌ · inline diff in editor ⚠️ (separate pane) ·
  **Side-by-side** diff ❌ (unified only) · annotate gutter ❌.
- **Impact:** Medium. The editor is a read-only viewer of agent output; **side-by-side** is the real
  reviewer-ergonomics gap on large diffs.

### G. Plumbing & ecosystem
- Patch create/apply ❌ · `.gitignore` UI ❌ · Submodules ❌ · LFS ❌ · **In-app GitHub PR** ❌
  (push stops at origin) · commit-hook UI ⚠️ (Orrery has *agent* hooks) · templates/co-authors ❌.
- **Impact:** Mostly niche **except in-app GitHub PR** — the natural terminus of "ship agent work."

## 3. Strategic takeaway

Orrery didn't "miss 30 IntelliJ features" — it made a deliberate choice: replace *manual git
surgery* with *agent-delegated git*, and *changelists/shelving* with *worktrees*. Coherent, and for
conflict resolution + isolation arguably ahead of IntelliJ for the agentic workflow. The real gaps
cluster not in "git surgery" (correctly delegated) but in **review & trust instrumentation** — the
human's ability to *inspect* what agents did. That's the high-leverage, *complementary* investment
area.

---

# Primordial Features — Prioritized Shortlist

The essential git-management features for Orrery, ranked by **priority × added value** (not
IntelliJ parity). Each is *review-and-trust* instrumentation that complements the agent model.
Value = ★ (1–5) for Orrery's workflow; Effort leverages the existing `git2` backend.

| # | Pri | Feature | What it unlocks | Value | Effort |
|---|-----|---------|-----------------|-------|--------|
| 1 | **P0** | **File changes per commit + per-commit diff** | Click any commit in the feed → its changed files → that commit's diff. Turns the (currently opaque) commit feed into a real review tool. | ★★★★★ | **M** — backend has `log`+`file_diff`; add commit-vs-parent diff. |
| 2 | **P0** | **Multi-commit / range diff** | Select N commits (or a range) → one combined diff. The core "review everything this agent did" motion. | ★★★★★ | **M** — diff between two OIDs. |
| 3 | **P1** | **Git blame / annotate** | Per-line authorship + revision, click-to-jump-to-commit. How you trace a suspicious agent line. | ★★★★☆ | **M** — `git2` blame. |
| 4 | **P1** | **Read-only 3-way conflict review** | After the agent "resolves conflicts," show base / ours / theirs / result so the human can *verify* the resolution. Closes the black-box trust hole — unique to Orrery's model. | ★★★★☆ | **M–L** |
| 5 | **P1** | **File history / diff vs any revision** | For a file: the commits that touched it + diff against any of them. How a file evolved across agent commits. | ★★★★☆ | **M** |
| 6 | **P2** | **Hunk/line-level review** (partial accept / partial discard) | Accept part of an agent's file diff, discard the rest — instead of all-or-nothing per file. | ★★★★☆ | **L** — index/patch surgery. |

**Honorable mentions (P2/P3):** side-by-side diff toggle (★★★, M) · quick **amend/revert** safety
actions (★★★, M) · log **graph + filters/search** (★★★, L) · **fetch/pull** (gated on team-remote
direction, ★★★★ then).

## Recommended sequencing

**Build 1 → 2 first.** They share machinery (diff a commit/range against a base), they're the
highest value-to-effort ratio, and together they convert the commit feed from labels into the
product's primary review surface. **Then 4** (conflict-resolution review) — it's the gap most
specific to Orrery's differentiator. **3 and 5** (blame + file history) round out auditability.
**6** (hunk-level) last — highest effort, and only worth it once per-commit review exists.

The throughline: invest in **inspecting** what agents do, not in **manual git surgery** — that
keeps Orrery complementary to the agent model rather than drifting toward being a manual git client.
