# Agent worktrees, file tree & runtime — design spec

Date: 2026-06-05
Status: **agreed, not yet implemented** (captured so we don't lose the requirements)

This spec covers slices **A–C** as one design (per decision), and records slice **D**
(the agent runtime/comms) so it isn't forgotten. Build order: A → B → C → D.

---

## Core model: a worktree per agent

An **agent = (source project, source worktree)**. Each agent works in its **own git
worktree**, never directly in the project checkout.

- On **spawn**, the user gives a **task**. The task is:
  - the agent's **display label** (shown under the agent in the sidebar), and
  - the source for the **worktree name** → `snake_case(task)`.
- Create a git **worktree** named `snake_case(task)` under the source project, on branch
  `agent/<name>`, from the selected **base**.
  - Names must be **filesystem-safe + unique**: truncate long tasks and append a short id suffix.
  - If the project repo has **no commits**, create an **initial commit** first (can't branch a
    worktree without a HEAD).
- Persist `source project` + `worktree path` on the agent record.
- On agent **delete**, run `git worktree remove`.

### Lazy lifecycle (don't run until asked)

Spawn does **not** launch a process. It only creates the agent record (`status: idle`) + the
worktree. The agent process launches **only** when the user clicks **Start** or **opens the
agent's terminal** → `status: running`. Then the user can chat with it.

---

## Agent ↔ orchestrator communication

When an agent runs we don't otherwise know which branch it's on or what it changed. Solution:
a small **`kat` CLI** (Rust executable) + a **skill injected into the agent's runtime** that
teaches the agent to report back through `kat`.

### Identity — stamped by the orchestrator, not guessed

At spawn the app sets environment variables on the agent process:

- `KATRIX_AGENT_ID=<uuid>`
- `KATRIX_ENDPOINT=<loopback addr + per-agent token>`

`kat` reads these from its own environment → it knows **who** it is and **where** to report.
No detection/guessing — we control the spawn, so we brand it.

### Transport — loopback HTTP (confirmed)

The app hosts a tiny **`127.0.0.1` HTTP server**, guarded by the per-agent token. `kat`
POSTs JSON updates → app updates the agent → emits `agent://…` events → UI updates live.
(A DB-only channel can't *push* to the UI, so the local server is required.)

### `kat` commands (initial)

`kat status <state>` · `kat worktree <path>` · `kat note "<msg>"` ·
`kat ask "<question>"` (→ becomes a pending **inbox** item) · `kat done`

### Skill injection

Inject a skill / instruction file per tool — Claude Code **skill**, Codex **AGENTS.md**,
Cursor **rules**, Gemini equivalent — teaching the agent to use `kat` for status, branch,
file changes, and permission asks.

---

## Right panel: project file tree (must scale to ~100 open projects)

- **Scan** with the Rust **`ignore` crate** (respects `.gitignore`, skips `node_modules`/`target`)
  → tree model `{ name, path, isDir, children, ext }`.
- **Watch** with the **`notify` crate**, debounced/coalesced, on a **background thread**.
  Emit **incremental** events (created / removed / modified) — never full re-scans.
- **Lazy watching**: only watch the **active / expanded** project; drop watchers when not viewed.
  100 simultaneous recursive watchers is the thing to avoid.
- **Frontend**: **virtualize** the tree (render only visible rows); patch nodes from incremental
  events (no full re-render).

---

## Git changes + diff view

- Backend: `git status` → changed-files tree (state `A`/`M`/`D` + add/del counts);
  `git diff` per file.
- Frontend: **CodeMirror 6** (read-only) — **Lezer** syntax highlighting by file
  extension/language + **`@codemirror/merge`** for diffs. Main content shows diffs when the
  right-panel git changes are present.

### Editor decision: CodeMirror 6 (not Monaco)

For a read-only highlighting + diff viewer that must stay smooth across many files/projects:

- Bundle: CM ~50–300 KB tree-shakeable vs Monaco ~2–5 MB gzipped (1–3 s to first-interactive).
- Highlighting: CM's Lezer is an **incremental** parser — fast even on 100k-line files.
- Diffs via `@codemirror/merge`; precedent: Sourcegraph migrated Monaco→CM (~43% less page JS).
- Monaco only wins for a **full IDE editor** (IntelliSense, multi-cursor) — not our use case.
- (Lightest pure-highlight alternative = **Shiki**, if we ever want static HTML and no editor.)

We are **not** compiling — syntax highlighting only.

---

## Build order

- **A** — Worktree-per-agent on spawn + lazy lifecycle. *(Unblocks real git status/diff/actions.)*
- **B** — Project file tree: backend scan + lazy watcher → frontend virtualized tree.
- **C** — Git changes + diff view (CodeMirror 6); agent git actions on the real worktree.
- **D** — `kat` CLI + loopback IPC + identity; skill injection; process spawn + PTY streaming;
  permissions/inbox.

Checklist tasks: #4, #5 (A) · #9, #10 (B) · #11, #6 (C) · #12, #13, #7 (D).

---

Sources for the editor decision:
- https://www.pkgpulse.com/guides/monaco-editor-vs-codemirror-6-vs-sandpack-in-browser-2026
- https://sourcegraph.com/blog/migrating-monaco-codemirror
