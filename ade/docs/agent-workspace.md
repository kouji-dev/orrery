# Agent Workspace

**Context.** Opening an agent shows its workspace in the center: a header with identity + git
actions, and three switchable panes — Diff, Terminal, and Chat.

Source: `workspace/workspace.component.ts` + `diff-view`, `terminal`, `chat`.

## Header

- [x] Agent name + filled status pill
- [x] Running activity bar when active
- [x] Quick actions: Pause/Resume (hidden when done), Commit, Merge to main (primary when done)
- [x] Task description line
- [x] Meta row: project (icon+color), branch, worktree, tool · model · effort, commits, elapsed
- [x] Pane tabs with active underline; Diff shows a changed‑file count badge; Chat shows `!` when blocked
- [x] Active pane is restored from a "pane hint" when opened via a deep action (e.g. View diff)

## Diff pane

- [x] Two‑column layout: changed‑file list + diff body
- [x] File list shows `A`/`M`/`D` state chip, name, directory, +adds/−dels
- [x] Selecting a file highlights it
- [x] Sticky diff header with file path + language chip
- [x] Hunk meta line (`@@ … @@`) styling
- [x] Per‑line gutter number, +/−/context marker, add/del/context coloring
- [x] Empty state when no diff preview exists

## Terminal pane

- [x] Session header (`── session: <worktree> · <branch> ──`)
- [x] Color‑coded log lines by kind (cmd/out/ok/warn/err/sys) with prefix glyphs
- [x] Live streaming append for running agents
- [x] Blinking caret while streaming
- [x] Auto‑scrolls to the latest line as logs arrive

## Chat pane

- [x] User vs. agent vs. system message bubbles (alignment + styling per role)
- [x] Role label + relative timestamp per message
- [x] Decision messages render inline answer buttons (e.g. Use Redis / Use Postgres) while blocked
- [x] Answering a decision resumes the agent and appends a system log line
- [x] Composer textarea with send button
- [x] Enter sends, Shift+Enter newlines
- [x] Auto‑scrolls to the latest message
