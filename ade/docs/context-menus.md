# Context Menus

**Context.** Right‑clicking agents (tabs or sidebar rows) and project headers opens an action
menu. Menus are viewport‑clamped, dismiss on outside‑click / Escape, and disable actions that
don’t apply to the current state.

Source: `context-menu/context-menu.component.ts`, `orchestra.store.ts`
(`openMenu`, `agentMenu`, `projectMenu`).

## Behavior

- [x] Opens at the cursor on right‑click
- [x] Clamped to stay within the viewport
- [x] Closes on outside mousedown or Escape
- [x] Separators between action groups
- [x] Disabled items (greyed, non‑interactive) based on agent state
- [x] Danger items (e.g. delete) styled in red with a red hover
- [x] Optional accent color and keyboard‑shortcut hint per item

## Agent menu

- [x] Open workspace / Open terminal / View diff
- [x] Pause ↔ Resume (Resume disabled when done)
- [x] Commit changes (disabled when no working changes)
- [x] Push to origin / Open pull request (disabled when no commits)
- [x] Merge → default branch (disabled when no commits)
- [x] Rename branch / Duplicate agent
- [x] Discard changes (disabled when clean)
- [x] Delete worktree (danger)

## Project menu

- [x] Spawn agent here (accent)
- [x] Pull latest
- [x] Open in terminal
- [x] Copy path
- [x] Remove project (danger)

## Entry points

- [x] Right‑click an agent tab in the top bar
- [x] Right‑click an agent row in the sidebar
- [x] Right‑click a project header in the sidebar
