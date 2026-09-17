# ORCHESTRA — Feature Documentation

ORCHESTRA is a futuristic, IntelliJ‑inspired **multi‑agent git orchestrator IDE**. A project
is one git repo; each project can spawn multiple coding agents (Claude Code / Codex / Cursor /
Gemini) that each run in their own git worktree, branch, terminal, and conversation.

This folder documents every feature in the design as per‑domain checklists. Every item below
is **implemented** in the Angular app (`src/app/orchestra/`). Checkboxes record availability —
use them to track regressions, scope new work, or onboard.

## Domains

| Domain | File | Scope |
| --- | --- | --- |
| Projects | [projects.md](projects.md) | Project model, add/remove, git detection |
| Agents | [agents.md](agents.md) | Agent lifecycle, status, properties, actions |
| Orchestrator dashboard | [orchestrator-dashboard.md](orchestrator-dashboard.md) | Hero overview, stats, 4 visualization metaphors |
| Agent workspace | [agent-workspace.md](agent-workspace.md) | Diff / Terminal / Chat panes |
| Right panel | [right-panel.md](right-panel.md) | Files / Inbox / Git, scoped to selected agent |
| Sidebar | [sidebar.md](sidebar.md) | Projects → agents tree, filter, spawn |
| Top bar & tabs | [top-bar-and-tabs.md](top-bar-and-tabs.md) | Brand, agent tabs, run controls, theme toggle |
| Spawn agent | [spawn-agent.md](spawn-agent.md) | Spawn dialog (project/branch/tool/model/effort/prompt) |
| Add project | [add-project.md](add-project.md) | Add‑project dialog (dir picker, git‑init, icon, color) |
| Context menus | [context-menus.md](context-menus.md) | Right‑click action menus |
| Theming & layout | [theming-and-layout.md](theming-and-layout.md) | Tweaks panel, themes, density, palettes, motion |
| Status bar | [status-bar.md](status-bar.md) | Footer health/counters/toast |
| Real‑time & motion | [realtime-and-motion.md](realtime-and-motion.md) | Streaming logs, live timers, animations |
| Design system | [design-system.md](design-system.md) | Tokens, typography, icons, shared components |

## Conventions

- `[x]` = feature present and verified in the running app.
- File/component references point at the source of truth so docs stay navigable.
- All data is currently mocked (`src/app/orchestra/data.ts`); wiring to a real Tauri/Rust
  backend is future work and is **not** ticked anywhere in these checklists.
