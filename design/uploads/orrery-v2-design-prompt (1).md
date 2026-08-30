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

## The three problems v2 solves

1. **Unblocking an agent destroys the user's context.** Today a notification click replaces the
   entire active workspace tab with that agent's terminal. Answering three blocked agents means
   three full context switches, all mouse-driven, with no way back but the tab bar. Unblocking is
   the single most frequent action in the product and it has no queue.

2. **Git is scattered across four surfaces that overlap.** A right-panel Git tab, the center diff,
   a center commit-inspection view, and a bottom commit graph — between them, the changed-file
   list is implemented five times and commit history appears three times. Meanwhile git *actions*
   (commit / push / merge) sit ~1000px away from the diff the user is judging.

3. **A 312px right panel is spending a quarter of the width on two things that belong
   elsewhere.** Its Inbox tab renders the same notification cards as the top-bar bell — a second
   view of a feed that already has a home. Its Files tab is a repo tree, which is a *navigation*
   surface and belongs with the other navigation surface. **The right panel is deleted entirely in
   v2.** The center gains ~312px, taking the diff body from ~716px to ~1028px.

**The organising principle for v2 — one home per verb:**

| Verb | Home |
|---|---|
| **Navigate** — agents and files | Sidebar (two stacked sections) |
| **Read** — judge a change, read a file | Center |
| **Navigate history** | Bottom graph panel |
| **Act** — commit / push / merge | An action bar docked directly under the diff |
| **Decide** — answer a blocked agent | Peek overlay (act) + top-bar bell (browse) |

---

## The v2 shell

```
┌──────────────────────────────────────────────────────────────────────┐
│ top bar  44px   brand(252) │ tab strip │ bell · run/theme/settings   │
├──────────┬───────────────────────────────────────────────────────────┤
│ AGENTS   │ center — pane tree                                        │
│  tree    │                                                           │
│  252px   │                                                           │
│  (or 54  │                                                           │
│   rail)  │                                                           │
├──────────┤                                                           │
│ FILES    │                                                           │
│  tree    ├───────────────────────────────────────────────────────────┤
│ resizable│ bottom git graph — resizable, collapsible                 │
├──────────┴───────────────────────────────────────────────────────────┤
│ status bar 24px                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

- **No right panel.** It is removed in v2. Do not draw one in any frame.
- **Sidebar is two stacked, independently resizable sections**: agents on top, files below, split
  by a drag handle. Both are *navigation*, so they share one home. The whole sidebar still
  collapses to a 54px icon rail; the files section can also collapse to its header strip alone.
- Center: a pane tree — leaves can be split/tiled, each showing `terminal`, `diff`, or an open
  `file`. There is **no chat pane**; agents are talked to through the PTY.
- Bottom graph spans the center column only (not under the sidebar).
- The top-bar right action cluster previously mirrored its width into the right panel so the two
  read as one column. With the panel gone that alignment is dead — right-align the cluster to the
  window edge.

### Tab model — new in v2

Tabs today are `orchestrator · backlog · ticket · agent`. v2 adds a **project tab**, because
*an agent is just a worktree with a process attached* and file browsing must work with no agent
running at all.

| | Project tab | Agent tab |
|---|---|---|
| Worktree root | main | `agent/<name>` |
| Pane tree | ✓ | ✓ |
| Diff view | main's working changes | that agent's changes |
| Terminal leaf | plain shell | the agent PTY |
| Graph | repo-wide | repo-wide, agent branch highlighted |

Same components, different root. The project tab uses a project chip where the agent tab uses a
status dot; otherwise the two must be visually indistinguishable in structure.

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
- **Sidebar:** agents section on top, **files section below** — see S5 for its full spec.
- **Bottom graph:** collapsed to a ~28px title strip showing branch name + "N commits" + a
  chevron. It must be obvious it expands.
- **Status bar:** running count, "N need attention", project/agent totals, worktree root,
  cpu/mem gauge, cost readout.

**Width budget for this frame — treat as a spec, not a suggestion:**

| Region | px |
|---|---|
| Sidebar | 252 |
| Changed-file list | 232 |
| **Diff body** | **~1028** |

**Critical:** the changed-file list appears **once** on screen. The center owns the *changed*
files (with ± counts and state badges); the sidebar owns the *full worktree tree* (quiet, no ±
counts). Different scopes, visibly different treatment — they must never read as the same list
drawn twice.

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
- Commit history exists **only** in the graph. No other surface in any frame lists commits.
- The sidebar files section stays visible and usable while inspecting a commit — its root chip
  still reads the active tab's worktree.

### S5 — Sidebar, both sections *(replaces the deleted right panel)*

A detail study of the sidebar at 252px, plus its 54px rail state.

**Agents section (top):** unchanged from v1 — projects as collapsible groups, agent rows with
status dot, name, branch, and an attention marker.

**Files section (bottom) — NEW:**

```
┌──────────────────┐
│ ⌄ agents         │
│   ● auth-refactor│
│   ○ migrate-db   │
├══════════════════┤  ← drag handle
│ [orrery ▾]   ⌕   │  ← root chip + fuzzy finder
│  src/            │
│   app/           │
│    shell.ts      │
│    ui.store.ts   │
└──────────────────┘
```

- **Root chip** — the critical element. It names the worktree the tree is rooted at and is a
  dropdown: `main` or any agent worktree. It defaults to the active tab's worktree so it follows
  the user, but the scope is always *visible*, never implicit. This is what makes "compare the
  agent's version against main" expressible: open the same path from two roots into split panes.
- `⌕` opens the fuzzy finder scoped to the current root.
- Tree rows are **quiet**: name, folder chevron, muted type glyph. No ± counts, no state badges —
  those belong to the center's changed-file list.
- Show a modified-file marker (a small dot, not a letter badge) so the tree hints at change
  without duplicating the changed-file list.
- Draw the **preview vs. pinned tab distinction**: single click opens a *preview* tab rendered in
  italic that the next preview replaces; double-click or `Enter` pins it upright. Show one of
  each in the center's file tab strip so the difference is legible.
- Also draw: `⌥`+click opening into a split, with the diff still visible in the other leaf.

**Rail state:** at 54px, agents become project icons with attention badges and the files section
becomes a single folder icon that expands the sidebar on click.

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
  Deleting the right panel gets you there (~38% → ~55%); do not spend the gain on new chrome.
- **Keyboard hints are visible, not hidden.** Any surface with bindings shows them in its footer
  or as `.kbd` chips.
- **Nothing renders twice.** No screen may show the same changed-file list, commit history, or
  pending count in two places at once.
- OKLCH color space; verify WCAG AA for all ink-on-surface pairs.

---

## Deliverable

For each screen: the full frame, plus a short note on any component that is new or that changed
role from v1 — specifically the git action bar, the peek overlay, the command palette, the graph
panel, the origin breadcrumb, the sidebar files section with its root chip, the project tab, and
the preview/pinned tab distinction.

Do not redesign the top bar, status bar, spawn modal, agents tree, or orchestrator visualizations
— they carry over unchanged and should appear as they are so the frames read as one product.

**Three things that are easy to quietly drop and must not be:**

1. The peek overlay must **not** replace the active tab. The dimmed workspace behind it is the
   entire point of that screen.
2. No frame may contain a right panel.
3. The sidebar's root chip must be visible in every frame that shows the files section. An
   implicit scope is the failure mode this whole revision exists to fix.

---

## Design tokens

<!-- paste the full contents of orrery-tokens.css below this line -->
