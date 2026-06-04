# Top Bar & Tabs

**Context.** The top bar holds the brand, the open agent tabs, and global run/theme controls.
The Orchestrator tab is always present; opening an agent adds a closable tab.

Source: `top-bar/top-bar.component.ts` + `logo.component.ts`.

## Brand

- [x] Gradient ORCHESTRA logo mark (uses the active accent palette)
- [x] Wordmark + "N projects · N agents" subline

## Tabs

- [x] Persistent "Orchestrator" tab (layers icon)
- [x] One tab per opened agent (status dot + project color chip + name)
- [x] Active tab marked with a gradient top border
- [x] Agent tabs have a close (×) button
- [x] Click selects a tab; closing the active tab falls back to Orchestrator
- [x] Right‑click an agent tab → agent context menu
- [x] Tabs scroll horizontally when they overflow
- [x] Orchestrator tab stays pinned (sticky) at the left while others scroll under it

## Controls

- [x] Run all ↔ Pause all (toggles live streaming globally; primary styling when paused)
- [x] Theme toggle (sun/moon) switching dark ↔ light
