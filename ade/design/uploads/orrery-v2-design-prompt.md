# Orrery v2 — Claude Design prompt

> Paste everything below into Claude Design as a single prompt. Append the contents of
> `orrery-tokens.css` at the very end, under the `## Design tokens` heading, before sending.

---

## What you are designing

**Orrery** is a desktop (Tauri) multi-agent git orchestrator for professional developers. One
project = one git repo. Each spawned coding agent (Claude Code / Codex / Cursor / Gemini) runs in
its own git worktree, on its own branch, with its own PTY session. The human's job is to
**supervise agents, unblock them, review their diffs, and integrate the work**.

This is a **v2 layout revision** of an existing, shipping app. The visual language is settled and
must not be reinvented — see *Locked decisions*. What changes is **where things live** and
**how the user moves between them**.

Design it as a real professional tool — JetBrains / Zed register. Dense, quiet, monospace-first.
Not a product demo, not a dashboard, no marketing gloss.

---

## The two problems v2 solves

1. **Unblocking an agent destroys the user's context.** Today a notification click replaces the
   entire active workspace tab with that agent's terminal. Answering three blocked agents means
   three full context switches, all mouse-driven, with no way back but the tab bar. Unblocking is
   the single most frequent action in the product and it has no queue.

2. **Git is scattered across four surfaces that overlap.** A right-panel Git tab, the center diff,
   a center commit-inspection view, and a bottom commit graph — between them, the changed-file
   list is implemented five times and commit history appears three times. Meanwhile git *actions*
   (commit / push / merge) sit ~1000px away from the diff the user is judging.

**The organising principle for v2 — one home per verb:**

| Verb | Home |
|---|---|
| **Decide** — answer a blocked agent | Right panel (Inbox) + a peek overlay |
| **Read** — judge a change | Center |
| **Navigate history** | Bottom graph panel |
| **Act** — commit / push / merge | An action bar docked directly under the diff |

---

## Existing shell (keep this skeleton)

```
┌──────────────────────────────────────────────────────────────────────┐
│ top bar  44px   brand(252) │ tab strip │ bell · run/theme/settings   │
├────────┬──────────────────────────────────────────┬──────────────────┤
│sidebar │ center                                   │ right panel      │
│ 252px  │                                          │ 312px            │
│(or 54  │                                          │                  │
│ rail)  │                                          │                  │
│        ├──────────────────────────────────────────┤                  │
│        │ bottom git graph — resizable, collapsible│                  │
├────────┴──────────────────────────────────────────┴──────────────────┤
│ status bar 24px                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

- Sidebar: projects → agents tree, collapsible to a 54px icon rail.
- Center: an agent's pane tree — leaves can be split/tiled, each showing `terminal`, `diff`, or an
  open `file`. There is **no chat pane**; agents are talked to through the PTY.
- Right panel width is currently derived from the top-bar action cluster so the two read as one
  column. Preserve that alignment.
- Bottom graph spans the center column only (not under the sidebar or right panel).

---

## Screens to produce

Produce each as a full-window frame at **1512 × 945**, dark theme unless noted.

### S1 — Agent workspace, review state *(the hero screen)*

The default state after an agent finishes. Show:

- **Center, top:** agent header — name, filled status pill, task line, meta row (project chip,
  branch, worktree, tool · model · effort, commits, elapsed).
- **Center, body:** a two-column diff — changed-file list (left, resizable, ~232px) and the diff
  body (right). Unified diff with a line-number gutter, `+`/`−`/context markers, hunk `@@` meta
  bands, word-level highlighting.
- **Center, bottom — NEW: the git action bar.** A single dense strip docked under the diff,
  never in the right panel:
  - commit-message input (single line, expands on focus)
  - `Stage all` / `Unstage all` toggle
  - `Commit` · `Push` · `Rebase onto main` · `Merge main → branch` · `Discard` (danger, last,
    visually separated)
  - Buttons that have both a native and an AI path render as a **split button**: primary segment
    (deterministic backend op) + attached segment with a sparkles glyph (AI op). The AI segment
    shows an **estimated cost** on hover — this is a hard requirement, never hide agent cost.
  - Disabled states are real: no changes → no commit/discard; no commits → no push/merge.
- **Right panel:** two tabs only — **Files** and **Inbox**. The Git tab is gone. Files shows the
  worktree tree with `A`/`M`/`D` badges and changed-descendant counts.
- **Bottom graph:** collapsed to a ~28px title strip showing branch name + "N commits" + a
  chevron. It must be obvious it expands.
- **Status bar:** running count, "N need attention", project/agent totals, worktree root,
  cpu/mem gauge, cost readout.

**Critical:** the changed-file list appears **once** on screen. When the center owns it, the right
panel shows the tree, never a second flat list.

### S2 — Peek overlay: the unblock queue *(the most important new surface)*

Triggered by `n` from anywhere, or by clicking a notification. It must **not** replace the active
tab — the workspace stays visible and dimmed behind it.

- A centered panel, ~720px wide, max ~70vh, elevated over a scrim.
- **Header:** `Needs you · 3 of 7` with prev/next affordances showing `⇧N` / `N`.
- **Body:** the current pending item —
  - agent name + status dot + project chip + branch
  - what it wants: the command, file path, or question, in mono, in a bordered block
  - the last ~5 lines of PTY context above it, dimmed, so the user can judge without leaving
  - for permission requests: `Allow` · `Deny` · `Always` (∞), each with its key hint
  - for questions: an inline input that writes straight to that agent's PTY
- **Footer:** `↵ send · Esc close · N next · ⌘↵ allow`.
- Behind the scrim, the workspace is unchanged — that's the whole point. Make the dimming light
  enough that the user can still see what they were doing.

Also produce a **second variant**: the queue empty state — "Inbox zero", quiet, no illustration.

### S3 — Command palette (`⌘K`)

- Same elevation family as S2, ~640px wide, anchored ~15% from the top.
- Query input, then grouped results: **Agents** (status dot, name, project chip),
  **Actions** (icon, label, right-aligned key hint), **Files**, **Commits**.
- Fuzzy match highlighting on the matched characters only.
- Bottom hint row: `↑↓ navigate · ↵ run · ⌘↵ run in new pane · Esc`.

### S4 — Git graph expanded + commit inspection

- **Bottom panel expanded** to ~40% of the center height:
  - a proper commit DAG — lanes, merge/branch edges, one row per commit
  - each row: graph glyph column, message, author avatar (agents render as rounded squares,
    humans as circles), sha chip, file count, ±adds/dels, relative time
  - left rail listing branches, agent worktree branches grouped and marked
  - a filter/search field in the panel header
  - multi-select rows → a `Diff (N)` range action appears in the panel header
- **Center simultaneously shows the commit inspection view** for the selected commit: commit
  context header, a 232px file list, and the diff — with a **NEW breadcrumb** on the left of the
  header recording where the user came from:
  `Working changes  ›  Graph  ›  a3f91c2` — every crumb clickable.
- The graph and the right panel must not both show commit history. History belongs to the graph.

### S5 — Right panel, Inbox tab

- Header: `All projects · 3 pending · 12 total` with a `Clear read` affordance.
- Cards: kind icon (permission / question / done), agent name, title, relative time, the command
  or context in mono, and the resolution actions.
- Resolved cards persist, dimmed, below the pending ones — history is kept.
- One quiet primary affordance at the top: **`Work the queue (N)`** → opens S2.

### S6 — Context menu with key hints

The agent right-click menu, showing that **every item that has a binding displays it** in a right-
aligned `.kbd` chip. Items: Open workspace, Open terminal, View diff / Pause / Commit / Push /
Rebase / Merge / Rename branch / Duplicate / Discard / Delete worktree (danger).

### S7 — Light theme

Reproduce **S1** in the light theme ("Paper"). Pure neutral, editor surface at ~96% lightness,
elevation *recedes below* the editor rather than rising above it. No ambient effects.

---

## Locked decisions — do not revisit

| Decision | Rule |
|---|---|
| Ambient effects | `.bg-texture` and `.bg-glow` render **only** under the Nebula accent preset. Every other preset: flat surfaces. |
| Diff sides | No hue difference between sides. Encoded by **position** and an **A / B badge** only. |
| Deleted files | Dim + strikethrough. **Never red.** |
| `sys` log lines | `--ink-3` |
| Space Grotesk | Retired from tool chrome. Boot wordmark logotype only. Everything else is mono. |
| Boot brand triad (rose/violet/cyan) | Boot screen only. Never in the app. |
| Dark surface ramp | Graphite, chroma ≈ 0.005, JetBrains-adjacent. Chroma does **not** rise with elevation. |
| Primary buttons | Flat `--accent-fill`, inverted light/dark logic (lighter fill on dark, darker fill on light). No gradients. |
| One hue, one meaning | A hue never carries two semantic meanings in the same view. |

Additional constraints:

- **Monospace by default.** Display font only for headings and the brand.
- **Density-token driven.** Every spacing and size value comes from the scale; no hardcoded px in
  component styles. The design must survive Compact / Regular / Comfy.
- **Content ratio ≥ 55%.** In S1, the pixels rendering actual code must exceed half the viewport.
  If a rail can't justify its width, narrow it.
- **Keyboard hints are visible, not hidden.** Any surface with bindings shows them in its footer
  or as `.kbd` chips.
- **Nothing renders twice.** No screen may show the same changed-file list, commit history, or
  pending count in two places at once.
- OKLCH color space; verify WCAG AA for all ink-on-surface pairs.

---

## Deliverable

For each screen: the full frame, plus a short note on any component that is new or that changed
role from v1 (specifically: the git action bar, the peek overlay, the palette, the graph panel,
the breadcrumb, and the reduced right panel).

Do not redesign the sidebar, top bar, status bar, spawn modal, or orchestrator visualizations —
they carry over unchanged and should appear as they are so the frames read as one product.

---

## Design tokens

<!-- paste the full contents of orrery-tokens.css below this line -->
