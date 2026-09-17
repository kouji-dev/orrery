# Projects

**Context.** A project represents a single git repository and is the top‑level grouping in the
orchestrator. Each project owns a name, org, working‑directory path, default branch + HEAD, a
list of branches, a tracked file list, plus an icon and accent color used throughout the UI.
Agents are always created against a project and share its repo context.

Source: `models.ts` (`Project`), `data.ts` (`PROJECTS`), `orchestra.store.ts`,
`add-project-modal.component.ts`, `sidebar/project-group.component.ts`.

## Project model

- [x] Project = one git repository (id, name, org, path, repo URL)
- [x] Default branch + short HEAD sha tracked per project
- [x] Branch list per project (used by the spawn dialog)
- [x] Tracked file list per project (used by the worktree file tree)
- [x] Per‑project icon (8 presets) and accent color (7 presets)
- [x] `hasGit` flag distinguishing initialized vs. uninitialized repos

## Add a project

- [x] "Add project" entry point in the sidebar footer
- [x] Add‑project dialog (see [add-project.md](add-project.md))
- [x] Working directory via free‑text path **or** folder Browse… picker
- [x] Project name auto‑derived from the directory’s last path segment
- [x] Git detection: existing `.git` recognized vs. "Run git init" offered
- [x] Icon picker + color picker with live preview in the dialog header
- [x] New project appended to the sidebar and selectable immediately

## Manage projects

- [x] Projects render as collapsible groups in the sidebar
- [x] Per‑project running counter and "needs attention" (blocked) badge
- [x] Per‑project agent count
- [x] Spawn an agent scoped to a specific project (+ button / context menu)
- [x] Project context menu: pull latest, open in terminal, copy path, remove
- [x] Remove project (also removes its agents) with confirmation toast
- [x] Removing a project cascades: its agents disappear from all views

## Project usage across the app

- [x] Project chip/color shown on agent cards, tabs, workspace header, right panel
- [x] Graph visualization renders the orchestrator → project → agent hierarchy
- [x] Status bar shows total project + agent counts
