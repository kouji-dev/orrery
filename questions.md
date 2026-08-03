# Questions / Decisions Log — prepare_for_v1 (2026-08-03)

Answers welcome; I proceeded with the stated assumptions where a default was obvious.

## Scope chosen for this pass (needs your validation)

The roadmap is multi-quarter; one session can't land all of it. I followed the roadmap's own
sequencing rules ("instrument first, registry first", "A3.5+A3.6 → B4.2 is the flagship chain")
and picked the three tracks below, each with a matching Claude Design surface:

1. **Perf & telemetry foundations** — A0.7 Phase 1 (emit funnel + clippy ban + NDJSON/aggregate
   telemetry + `summarize.mjs`), A7.7 recursive process tree, A0.6 status-bar RSS split.
   Design: `perf-panels.jsx`, `Performance Panel.html`, `devpanel.jsx`.
2. **Navigation & search DX** — B2.2 command registry + palette, B2.1 Search Everywhere,
   B2.3 recent files / go-to-line, B3.1 find-in-files (native streaming search).
   Design: `commands.jsx`, `search-panel.jsx`.
3. **Git DX flagship chain** — A3.5 merge-without-bailing, A3.6 conflict session model,
   B4.2 3-way conflict view, A4.1 dual-path split button + A4.3 cost estimator (heuristic v1).
   Design: `repo-conflict.jsx`, `gitactions.jsx`, `cost-panel.jsx`.

**Q1.** Is this the right first slice? Deliberately deferred: Phase 0 (macOS/Linux CI — needs
Apple signing decision), A0.5 headless fork (a decision, not code), A0.1/A0.2 process-model work,
B1.1 writable editor, A3.1/A3.2 remotes & branch ops, B4.1 commit graph, terminal-plus,
github-panel. Say which you want next.

## Roadmap open decisions that blocked or shaped implementation

**Q2 (A4.3 estimator).** No actuals ledger exists yet (A6.1 not built). v1 estimator is the static
heuristic (op kind + diff bytes + conflict count + editable rate table in settings). Calibration
against actuals waits for A6.1. OK?

**Q3 (A4 AI variants).** `AgentActionsService.aiAction()` covers commit/push/rebase/merge. For
ops with no existing AI path (cherry-pick, revert, conflict-resolve-with-AI), the dropdown shows
the estimate but the action is stubbed behind the same aiAction plumbing where trivial, otherwise
hidden. Full coverage matrix needs A3.3/A3.4 natives first.

**Q4 (A0.7 raw-trace policy).** Went with the roadmap's own recommendation: opt-in, auto-disable
after 30 min or 200MB, visible indicator while on. Confirm.

**Q5 (A7.7 memory metric).** Windows-only for now (matches current platform support):
private working set via winapi, RSS secondary. macOS/Linux shims deferred to Phase 0. OK?

**Q6 (B3.1 engine).** Used the `grep-searcher`/`grep-regex` crates (library, no bundled ripgrep
binary), sharing the `ignore` walker. Roadmap allowed either. Confirm.

**Q7 (Search Everywhere ranking).** Design shows files/symbols/agents/tickets/commands/git refs.
Symbol search needs B2.4's tree-sitter layer (not built) — v1 ranks files, agents, tickets,
commands, branches; symbols omitted. OK?

**Q8 (headless, A0.5).** The roadmap says decide before more A1 work. That's a product decision I
can't make for you — flagging that it gates A1.3/A1.4/A1.6.

**Q9 (macOS signing / open-source).** Roadmap open decisions 1–2 — yours to answer, nothing coded.

## Design-fidelity notes

**Q10.** The design prototype (`orrery.html`) is a full parallel console (boot screen, its own
topbar/sidebar/workspace). I translated only the NEW panels/surfaces into the existing Angular
shell, matching the design system tokens (`design/orrery/project/_ds/.../tokens/*.css`) against
the app's existing styles — not replacing the app shell wholesale. Flag if you wanted the full
console re-skin instead.

**Q11.** Design loads Google Fonts (JetBrains Mono / Space Grotesk) from CDN. Desktop app should
not fetch remote fonts at runtime — kept the app's existing font stack where the two differ.
Bundle the fonts locally if you want exact type fidelity.
