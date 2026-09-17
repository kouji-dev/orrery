# Orchestrator Dashboard

**Context.** The Orchestrator is the hero view — all agents across all projects at a glance. It
has a stat header and a switchable visualization area offering four metaphors: Grid, Board
(kanban), Graph (radial hierarchy), and Timeline (swimlanes).

Source: `overview/overview.component.ts` + `stat-block`, `agent-card`, `mini-term`,
`grid-view`, `kanban-view`, `graph-view`, `timeline-view`.

## Header & stats

- [x] "Orchestrator" title with agents‑across‑projects subtitle and org
- [x] Stat blocks: Running (pulsing), Need you, Waiting (+queued), Done
- [x] Visualization switcher (Grid / Board / Graph / Timeline)
- [x] Spawn button
- [x] Body scrolls internally; header stays fixed

## Grid view

- [x] Responsive auto‑fill card grid (min 320px columns)
- [x] Per‑card progress ring with % label
- [x] Agent name + filled status pill
- [x] Project chip (icon + color) and branch
- [x] Task description
- [x] Blocked agents show an inline block‑reason callout
- [x] Mini terminal showing the last 3 live log lines
- [x] Footer stats: files, +adds, −dels, commits, elapsed
- [x] Context action button adapts to status (Merge / Answer / Start now / Pause·Resume) + Open
- [x] Hover lift + border highlight

## Board (kanban) view

- [x] Four columns: Queued, Running, Needs you (blocked+waiting), Done
- [x] Column header with status‑colored underline and count
- [x] Compact cards (status dot, name, task, branch, +adds)
- [x] Running cards show an activity bar
- [x] Empty‑column placeholder

## Graph view

- [x] SVG radial hierarchy: ORCH root → projects → agents
- [x] Root core glow + org label
- [x] Project nodes (initial, name, branch · head) colored per project
- [x] Agent nodes colored by status with status/commits/branch label
- [x] Curved edges colored per project / per status
- [x] Animated dashed edges + pulsing node for running agents
- [x] Dimmed edges for queued agents
- [x] "no agents" hint for empty projects
- [x] Clicking an agent node opens its workspace

## Timeline view

- [x] Column headers (Agent / Elapsed·progress / Commits)
- [x] Bar length scaled to elapsed time (relative to the longest)
- [x] Inner progress fill + running activity shimmer
- [x] Elapsed + progress % label beside each bar
- [x] Per‑status bar color, commit count column
- [x] Row hover highlight; click opens the workspace
