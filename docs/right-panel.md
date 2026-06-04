# Right Panel

**Context.** The right panel is scoped to the currently selected agent (or "All projects" when
the orchestrator is active). It has three tabs — Files, Inbox, Git — and can be toggled off
entirely via Tweaks.

Source: `right-panel/right-panel.component.ts` + `files-tab`, `file-tree`, `tree-node`,
`inbox-tab`, `pending-card`, `git-tab`, `commit-feed`, `empty-state`, `tree.ts`.

## Scope header & tabs

- [x] Header shows the selected agent (status dot, name, project chip) or "All projects"
- [x] Tab bar: Files / Inbox / Git with active underline
- [x] Inbox tab shows a pending‑count badge
- [x] Panel can be hidden via the Tweaks "Right panel" toggle
- [x] Each tab body scrolls internally (full available height)

## Files tab (worktree explorer)

- [x] Nested file tree built from the project file list + agent’s changed files
- [x] Folders first, then files, alphabetical
- [x] Expand/collapse folders (default expanded) with chevron + folder icons
- [x] Changed files carry `A`/`M`/`D` badges and accent coloring
- [x] Folders show a changed‑descendant count
- [x] Header shows worktree name + "N changed"
- [x] Empty state when no agent is selected

## Inbox tab (pending actions)

- [x] Scoped to the selected agent, or aggregated across all agents
- [x] Three pending kinds: permission, decision, review (icon + color + verb)
- [x] Card shows title, relative time, the command/context, and actions
- [x] Permission: Allow / Deny / Always (∞)
- [x] Decision: "Answer in chat" (jumps to the chat pane)
- [x] Review: Merge / Review diff
- [x] Allow/deny append a log line and flash a toast
- [x] "All projects · N pending" header when unscoped
- [x] Inbox‑zero empty state

## Git tab

- [x] Branch header: branch name, base sha, "N ahead", +adds/−dels
- [x] Working‑tree changes list with `A`/`M`/`D` + per‑file diff stats
- [x] Stage all ↔ Unstage all toggle
- [x] Commit (all/staged), Push to origin, Open pull request
- [x] Merge → project default branch (primary)
- [x] Discard working changes (danger styling)
- [x] Action buttons disable correctly (no files → no commit/discard; no commits → no push/PR/merge)
- [x] This branch’s commits listed as a timeline feed
- [x] Unscoped (orchestrator) view shows a global commit feed across all worktrees
- [x] Commit feed: message, author agent (status‑colored), sha chip, file count, relative time; click opens the agent
