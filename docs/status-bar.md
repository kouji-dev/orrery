# Status Bar

**Context.** The bottom status bar is a persistent 24px footer summarizing orchestrator health
and live counts, and surfacing transient action toasts.

Source: `status-bar/status-bar.component.ts`, `orchestra.store.ts` (`toast`, `flash`).

## Contents

- [x] Running count with a pulsing dot
- [x] "N need attention" (blocked) indicator, shown only when > 0
- [x] Project + agent totals
- [x] Worktree root path
- [x] "orchestrator: healthy" indicator
- [x] Transient toast (gradient text) for the latest action, auto‑dismissing
- [x] Pinned flush to the viewport bottom; updates live as state changes
