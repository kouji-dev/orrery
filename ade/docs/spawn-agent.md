# Spawn Agent Dialog

**Context.** The spawn dialog creates a new agent: it picks the project + source branch, the
coding tool, model (and reasoning effort for Codex), and an initial prompt. Spawning creates a
new git worktree + branch and starts the agent streaming.

Source: `modals/spawn-modal.component.ts`, `orchestra.store.ts` (`spawn`).

## Layout

- [x] Modal overlay with blurred backdrop; click‑outside / Cancel closes
- [x] Header: agent icon, "Spawn agent", "new git worktree + branch" chip
- [x] Scrollable body, fixed footer
- [x] Footer shows the target worktree path preview
- [x] Initial prompt textarea auto‑focused on open

## Fields

- [x] Project select (populated from current projects) with path subtext
- [x] Source branch select (populated from the chosen project’s branches) with base sha
- [x] Changing project resets the branch to that project’s default
- [x] Agent tool tiles: Claude Code / Codex / Cursor / Gemini (monogram + name)
- [x] Selected tool highlighted with its accent
- [x] Model select (populated from the chosen tool’s models)
- [x] Reasoning‑effort selector (Low/Medium/High) shown **only** for Codex
- [x] Changing tool resets model to first + effort to default (High for Codex)
- [x] Initial prompt textarea (placeholder + focus ring)

## Submit

- [x] Spawn button creates the agent with the chosen config
- [x] Empty prompt falls back to a sensible default
- [x] New agent gets `agent/<name>` branch + `<project>-<id>` worktree
- [x] Boot log + streaming pool seeded; terminal pane opened automatically
- [x] Toast: "spawned <name> in <project>"
