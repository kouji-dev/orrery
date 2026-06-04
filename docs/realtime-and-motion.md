# Real‑time & Motion

**Context.** ORCHESTRA feels alive: running agents stream terminal output, timers tick,
progress advances, and status is animated. A global Run/Pause control and a motion toggle govern
this behavior.

Source: `orchestra.store.ts` (streaming tick), `styles.css` (animations),
`data.ts` (`LOGS`/`STREAM`), `workspace/terminal.component.ts`, `overview/mini-term.component.ts`.

## Live data

- [x] Streaming loop ticks ~every 1.1s while "running" is enabled
- [x] Running agents append new log lines from their stream pool (stochastically)
- [x] Running agents accrue elapsed time and progress each tick
- [x] Terminal pane and overview mini‑terminals reflect live logs
- [x] Run all ↔ Pause all toggles the whole simulation
- [x] Answering a blocked decision resumes streaming for that agent

## Animation & micro‑interactions

- [x] Pulsing status dots (running/blocked/queued) with glow
- [x] Indeterminate activity bars on running agents
- [x] Streaming caret blink in the terminal
- [x] Progress ring transitions
- [x] Card hover lift; row/commit hover highlights
- [x] Staggered "rise" entrance on cards, menus, modals
- [x] Animated graph edges (dashed flow) + pulsing graph nodes for running agents
- [x] Gradient tab/pane underlines and gradient brand/text accents

## Accessibility / control

- [x] Motion fully disabled via the Tweaks "Live motion" toggle (`data-motion=off`)
- [x] Respects `prefers-reduced-motion` for entrance animations
- [x] Entrance animations are transform‑only so content is never hidden if they don’t play
