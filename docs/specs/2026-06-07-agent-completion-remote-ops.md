# Agent completion: commit / push / rebase / merge (+ commit-feed fix) — design spec

Date: 2026-06-07
Status: **agreed, not yet implemented**

Gives a finished agent a wrap-up flow in the git tab / workspace. **No PR creation** — the
user opens the PR themselves on GitHub after pushing. Also fixes a **companion bug**: agent
commits never show in the git panel.

This is one of two specs from the 2026-06-07 brainstorm. The other is
`2026-06-07-agent-cost-ccusage.md` (independent feature).

---

## Scope

**In:**
- Four completion actions — **Commit · Push · Rebase · Merge**.
- **Commit & Push are hybrid**: a deterministic **backend** action (primary button) **plus**
  an **AI** action (a right-attached button that forwards a predefined prompt into the PTY).
- **Rebase & Merge are AI-only** (single button → predefined prompt into the PTY).
- **Companion fix:** agent worktree commits appear in the git panel, tagged to their agent.

**Out (deferred):** PR creation (user does it manually), deterministic backend rebase/merge,
conflict-resolution UI, per-host tuning.

---

## Key decisions (from brainstorm)

- **AI-driven is the simple default for git ops** — rebase & merge run *only* via a
  predefined prompt forwarded into the agent's PTY (the agent runs `git` with its own tools).
- **Commit & Push keep a backend path too** ("for now"): the primary button runs the
  deterministic backend op; a **right-attached button** runs the AI version. This avoids
  spending tokens on trivial commit/push while still offering the AI path.
- **No PR creation.** Removed. After pushing, the user opens the PR on GitHub.
- **Merge means `<base>` → the agent's branch** (safe, non-destructive): inside the worktree,
  `git merge <base>` brings upstream changes **into** the agent branch. It **never** touches
  `<base>`/main. This **replaces** the current risky "merge agent → main" button + the
  `agent_merge` backend command. The AI merge prompt is emphatic about direction.
- **Permissions reuse the hook UI** — the agent's `git` calls surface "allow git push?" in
  the inbox via the existing native hooks, no extra work.

### Current state being replaced

The frontend already stubs these: `AgentActionsService.act(id, 'push')` → "push not
configured yet"; `act(id,'pr')` → "PR not configured yet"; `mergeAgent` merges **agent →
main** (risky). The git-tab has a primary **"Merge agent/x → main"** button and a Push
button hitting the stub. All of this is superseded by this spec.

### Trade-offs (accepted)

- AI rebase/merge cost an AI turn; mitigated because they're the conflict-prone ops where AI
  earns its keep. Commit/push avoid the token cost via their backend primary button.
- AI merge could go the wrong direction — mitigated by a direction-explicit prompt; the user
  watches live and can intervene.

---

## Mechanism

### AI path — one command, predefined prompt → PTY

- **`agent_action(id, kind)`**, `kind ∈ { "commit", "push", "rebase", "merge" }`:
  1. Resolve the predefined **prompt template** for `kind`, interpolating `branch`/`base`.
  2. **Ensure running**: if idle, start (resume-into-session when `session_id` exists, else a
     bare start — never re-send the original task prompt).
  3. `rt.write(id, "<prompt>\r")` into the PTY. Fire-and-forward (like the allow/deny
     keystroke commands); katrix does not await the op.

Prompt templates (shared constants, e.g. `src-tauri/src/agents/prompts.rs`, tool-agnostic):
- **commit:** `"Commit all current changes with a clear, concise message: run `git add -A` then `git commit`. Do not push."`
- **push:** `"Push the current branch to origin: run `git push -u origin <branch>`. Do nothing else."`
- **rebase:** `"Rebase this worktree onto `<base>`: run `git rebase <base>`, resolve conflicts, complete the rebase. Do not push and do not merge."`
- **merge:** `"Merge `<base>` INTO the current branch `<branch>` — bring `<base>`'s changes into this branch. From within this worktree run `git merge <base>`, resolve conflicts, complete the merge. Do NOT merge the other direction and do NOT modify `<base>`. Do not push."`

### Backend path — deterministic commit & push

- **Commit:** reuse the existing `agent_commit(id, message, paths)` (git2). No change.
- **Push (new `agent_push(id)`):** shell out to the system **`git` CLI** —
  `git -C <worktree> push -u origin <branch>` — so the OS credential helper handles auth
  ("assume the user is already logged into GitHub"). This is the **one** new subprocess in a
  backend that otherwise uses git2 (libgit2 push needs manual credential callbacks; the CLI
  is simpler and matches the "already logged in" assumption). No remote → typed `AppError`;
  failure → surface git's stderr.

### Removed

- Old **`agent_merge`** (backend command + merge-only service method + `lib.rs` entry) and its
  "merge → main" UI. Merge is now the AI action above.

---

## Frontend (Angular)

### Split buttons for Commit & Push

A small **split-button** pattern: primary button (backend op) + a right-attached button
(AI op), sharing one rounded container with a hairline divider.

```
[  Commit all          ][✨]      ← primary = backend agent_commit ; attached = agent_action(id,'commit')
[  Push to origin       ][✨]      ← primary = backend agent_push   ; attached = agent_action(id,'push')
[  Rebase onto main          ]    ← single = agent_action(id,'rebase')
[  Merge main → branch       ]    ← single = agent_action(id,'merge')   (direction: base → branch)
```

- Replace the current "Merge agent/x → main" primary button with **"Merge `<base>` →
  `<branch>`"** wired to `agent_action(id,'merge')`.
- Replace the Push stub with the split button above.
- Triggering any **AI** action switches the workspace to the **terminal** tab so the user
  watches it run.
- Mirror the same actions in the agent context menu (`agentMenu`), dropping the "Open pull
  request" item.

### Attached-button icon

- **v1 (fits the existing icon system):** a **`sparkles`** glyph added to the `ICONS`
  registry (`utils.ts`) — a single stroke `path`, rendered by `<app-icon name="sparkles">`.
  Source: icons8 *sparkles* (re-drawn as a single stroke path to match the 24×24 stroke set).
- **Optional (nicer, more work):** show the **agent's tool brand logo**
  (Claude/Gemini/Cursor/Codex) so the user sees *which* AI will run it. Brand logos are
  multi-path/filled and **do not fit `app-icon`** → add a small **brand-icon** component (or
  `<img>` assets) holding the four icons8 logos, keyed by `agent.tool`, with the sparkles
  glyph as fallback. **Note:** icons8 assets carry a license/attribution requirement — handle
  at implementation. *Recommendation: ship sparkles in v1, add brand logos as a follow-up.*

---

## Companion bug fix — agent commits don't show in the git panel

**Root cause** (`src-tauri/src/projects/service.rs::commits` + `git/service.rs::log`):
1. `git.log()` walks `push_head()` on the **project's main repo path** (HEAD = `main`). Agent
   commits live on `agent/<name>` branches in separate worktrees — unreachable from `main`'s
   HEAD → they never enter the log.
2. `commits()` sets `agent: e.author` (the git **author name**), but the frontend filters
   `c.agent === ag.id` (a **UUID**) → never matches even if commits appeared.

**Fix:**
- **New `agent_commits(id, limit)`** command → `git.log(<agent worktree path>, limit)` (the
  worktree's HEAD *is* the agent branch, so its commits are reachable), mapping every row's
  `agent` field to **the agent id** (not the author). The git tab's "Commits on this branch"
  calls this instead of filtering `projects.commits()` by author.
- **Merged "all worktrees" feed** (`ProjectActionsService.commits()` used by the agentless
  git tab): aggregate per-agent commits across the project's agents (each tagged with its
  agent id), newest-first — instead of (or in addition to) the main-repo HEAD log.
- Frontend: `git-tab` `agentCommits()` reads from `agent_commits(id)`; the author-based
  filter is dropped.

---

## Error handling

- `agent_action` fails only on start/write → typed `AppError` → notification. Past the
  keystroke, failures surface in the agent's terminal + inbox (normal agent flow).
- `agent_push` (backend): no remote / auth / other → typed `AppError` carrying git stderr;
  agent state unchanged on failure.
- `agent_commits`: a worktree with no commits returns `[]` (panel shows "no commits yet").

---

## Testing

- **Rust unit:** prompt-template rendering (branch/base interpolation; merge template asserts
  direction wording); `agent_commits` returns the worktree branch's commits tagged with the
  agent id (build a repo + worktree + commit in a temp dir); push-URL/no-remote error path.
- **Frontend:** git-tab shows commits from `agent_commits`; split-button wires primary→backend
  and attached→`agent_action`.
- **E2E (per global rule — new functionality → write E2E → run → confirm pass):** in a
  throwaway repo with a **local bare repo as `origin`**: (a) commit in an agent worktree →
  assert it appears in "Commits on this branch" tagged to the agent (covers the bug fix);
  (b) backend Push → assert the branch lands in the bare origin; (c) each **AI** action →
  assert the correct predefined prompt reaches the PTY (the git *outcome* of AI actions is
  non-deterministic and is an integration/manual check, not the E2E contract).

---

## Open assumptions (confirm at spec review)

- Rebase/merge target is the agent's stored **`base`** (typically `main`).
- AI rebase/merge have **no** backend fallback yet ("for now").
- Attached-button icon ships as **sparkles** in v1; brand logos are a follow-up.
- Prompt-template wording is tunable; strings above are a starting point.
