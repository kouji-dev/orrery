# Sidebar

**Context.** The left sidebar is project‑centric: each project is a collapsible group with its
agents nested underneath. It includes a header with counts, an agent filter, and a footer with
add‑project / spawn‑agent actions.

Source: `sidebar/sidebar.component.ts` + `project-group`, `agent-row`.

## Header

- [x] "Projects" label with project count chip
- [x] Global running counter against a concurrency cap (`N/5`)
- [x] Agent filter input (matches agent name + task) with clear button

## Project groups

- [x] Collapsible per project (chevron), persisted per‑project collapse state
- [x] Project icon tile (accent‑tinted) + name
- [x] "needs attention" (blocked) badge and agent count
- [x] Per‑project spawn (+) button
- [x] Right‑click → project context menu
- [x] Connector line linking nested agents
- [x] "no agents — spawn one" placeholder for empty projects
- [x] Groups hidden when filtering yields no matches

## Agent rows

- [x] Status dot + agent name
- [x] "needs you" marker dot for blocked / pending agents
- [x] Tool monogram badge + elapsed time
- [x] Branch (short) + +adds/−dels when there are changes
- [x] Running rows show an activity bar
- [x] Active row highlighted with an accent rail
- [x] Hover highlight
- [x] Click opens the agent; right‑click → agent context menu
- [x] Status‑priority ordering within a project

## Footer

- [x] "Add project" button (opens the add‑project dialog)
- [x] "Agent" spawn button (opens the spawn dialog, unscoped)

## Layout

- [x] Sidebar fills full available height
- [x] Agent list scrolls internally; header and footer stay fixed
