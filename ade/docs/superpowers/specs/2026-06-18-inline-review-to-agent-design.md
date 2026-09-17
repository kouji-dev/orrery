# Inline review → send to agent (+ annotate popup)

**Date:** 2026-06-18
**Status:** Design — pending review
**Design source:** Claude Design project `b7cdbad2-2b62-434b-8d46-d523572055df`
(`review.jsx`, `repo-diff.jsx`, `agent-git.jsx`, surfaced via `/design-sync`).

## Summary

A GitHub-style inline code-review flow pointed at a live local agent. While reading a
diff or a file, the user drops line/range comments; they accumulate per-agent across
files; the whole batch plus a global note is delivered to the agent as one structured
message it can act on immediately. Ships alongside a second, smaller deliverable: a rich
**annotate (blame) hover popup** replacing the current native `title` tooltip.

The design is authoritative and approved (it is the user's own prototype). Implementation
**ports the prototype's unified renderer** rather than overlaying on CodeMirror — per the
explicit instruction "use same design as provided." The one engineering addition not in
the prototype is a **large-file stall guard** (§7), so we don't reintroduce the
many-second freeze already fixed in `code-diff.component.ts`.

## Scope decisions (settled)

| Decision | Choice |
|---|---|
| Surfaces (comments) | **Only** the working-changes diff (`diff-view.component.ts`) and open files (`file-view.component.ts`). **Excluded**: the multi-file commit/range inspection (`agent-git-view` → `commit-diff-view`/`range-diff-view`/`diff-or-blame`), which stays on CodeMirror untouched. Both in-scope surfaces live in an agent pane → that pane's agent is the send target |
| Comment anchor | Single line **or** a dragged range |
| Composer | Inline, GitHub-style — opens under the anchored line/range |
| Lifecycle | Per-agent batch, **in-memory only** (survives file/tab switches within the session; **not** persisted — lost on app restart), cleared on successful send |
| Delivery | Assemble one structured text message → paste into the agent PTY (bracketed-paste wrapped) |
| Renderer | Adopt the prototype's **unified** diff/file/blame renderer (replaces the CodeMirror MergeView on review-capable surfaces) |
| Annotate popup | Rich hover card (author, sha, message, "click → open commit") on the blame view |

## What already exists (verified)

- **PTY input path**: `AgentsStore.input(id, data)` → `agent_input` Tauri command →
  `rt.write(id, &data)` writes raw bytes to PTY stdin. No backend change needed; bracketed
  paste (`ESC[200~ … ESC[201~`) + a separate `\r` is achievable purely frontend-side.
  The "start-if-idle then send after a delay" pattern exists in
  `AgentActionsService.aiAction`.
- **Design tokens / classes / icons**: every token (`--bg`, `--panel*`, `--ink*`,
  `--accent*`, `--hair*`, `--code-add-bg/ink`, `--code-del-bg/ink`, `--font-mono`,
  `--r-sm/md`, `--shadow`, `--elev`, `--st-*`, `--sp-*`, `--fs-*`), utility class
  (`.btn .primary .ghost-hair .chip .kbd .up .disp .surface .rise .tnum .scroll-y
  .pane-btn`), and icon name the prototype uses already exists in `src/styles.css` and
  `icon.component.ts`. This is a port, not a re-theme.
- **Blame data**: `GitInspectStore.blameFor(agent, path, rev)` → `BlameLine[]`.
- **Diff data**: `AgentsStore.diff(id, path, oldPath?)` → `FileDiff { old, new, lang }`
  (full old/new text — **not** hunks; see §6 adaptation).

## Architecture

New/changed Angular pieces (each a focused unit; ports the matching prototype function):

1. **`ReviewStore`** (`src/app/agents/review.store.ts`) — port of `review.jsx`'s store, but
   **in-memory only** (no `localStorage`). Per-agent `comments` keyed by `agentId`,
   signal-backed so the count/cards re-render reactively. API: `list(agentId)`,
   `count(agentId)`, `add(agentId, c)`, `remove(agentId, id)`, `clear(agentId)`,
   `assemble(agentId, globalNote): string`.
   Comment shape: `{ id, file, view, lang, fromIdx, toIdx, fromLine, toLine, side,
   snippet, lines[], note }`.

2. **`UnifiedCodeComponent`** (review/diff/file renderer) — port of `ReviewCode` +
   `HunkRows`. Renders a flat row list (hunk separators + code rows), a comment gutter
   with hover "+", drag-to-select range, inline composer, persistent saved-comment cards,
   per-side `+/-` inks. Subject to the §7 stall guard. **Syntax highlighting gap**: the
   prototype calls a `highlight(s, lang)` global that does not exist in the real app (it
   highlights via CodeMirror). Options: (a) ship the unified view with plain, un-tokenized
   code (fastest, acceptable for a review surface), or (b) add a lightweight per-line
   highlighter (e.g. Lezer/`@codemirror/language` token spans extracted offscreen). Default
   to (a) for v1 unless review reads poorly without color.

3. **`InlineComposerComponent`** / **`SavedCommentCardComponent`** — ports of
   `InlineComposer` / `SavedCommentCard`. ⌘/Ctrl-↵ saves, Esc cancels; card shows
   author chip, line ref, `pending` chip, delete.

4. **`SendReviewButtonComponent`** — agent-scoped action (keyed by `agentId`), hidden at
   N=0, shows the count badge. The design places it in the **agent workspace header**
   (beside Pause/Commit/Merge). The real app's tiling pane layout has no such rich per-pane
   header, so mount it in the in-scope surfaces' header bars (`diff-view`'s `.diff-head`,
   `file-view`'s toolbar) — it appears whenever the user is on a review surface and
   reflects the agent's running total across files. Opens `SendReviewModalComponent`.

5. **`SendReviewModalComponent`** — port of `SendReviewModal`. Comments grouped by file
   (each deletable), snippet preview, global-note textarea, "Send to agent". Its body
   mirrors the structured message.

6. **`AnnotateBlameComponent`** + hover popup — port of `FileBlameGutter`: unified blame
   view (author column with age fade, line numbers, code) and the fixed-position hover
   card (author, sha, commit message, "click → open commit"). Per the design it is wired
   into the **`file-view` Annotate toggle** (the prototype's `FileViewer` has the toggle →
   swaps `ReviewCode` for `FileBlameGutter`). The working-changes `diff-view` also has an
   Annotate toggle today (blame overlay on the MergeView); it adopts the same unified blame
   view + popup for consistency, replacing the native `title` tooltip. The commit/range
   `diff-or-blame` blame is **not** changed.

7. **`AgentReviewService.sendReview(agentId, payload)`** — assembles the structured text,
   ensures the agent is running (reuse `aiAction`'s start-then-delay), pastes via
   `input(id, ESC[200~ + text + ESC[201~)` then `input(id, "\r")`, switches the pane to
   the terminal, clears the batch.

### Wiring (scoped)
Confirmed against the updated prototype (`workspace.jsx`): `FileViewer` →
`ReviewCode view="file"`; `DiffBody` → `ReviewCode view="diff"` for the selected changed
file; `AgentGitView` keeps plain `HunkRows`/`FileDiff` (no comments). Map to the real app:

- `diff-view.component.ts` — replace its `<app-code-diff>` (CodeMirror MergeView) with the
  unified renderer (one selected file at a time, as today). Its existing Annotate toggle
  swaps to the unified blame view + hover popup (§6).
- `file-view.component.ts` — replace its CodeMirror editor with the unified renderer in
  `view:"file"` mode, and **add an Annotate toggle** (new — the prototype's `FileViewer`
  has one) that swaps to the unified blame view + popup.

**Do not touch** `agent-git-view`, `commit-diff-view`, `range-diff-view`, or
`diff-or-blame.component.ts` — the commit/range "multi files diff view" keeps CodeMirror
(side-by-side, collapse-unchanged, resize, blame) exactly as-is. Net effect: two diff
renderers coexist — CodeMirror for historical inspection, unified for live review.

The `SendReviewButton` mounts in the in-scope surfaces' existing right-aligned header areas
(`diff-view`'s `.diff-head`, `file-view`'s toolbar).

## Data adaptation (§6)

- **Diff → hunks**: the prototype's renderer wants `diff.hunks: [{meta, lines:[{k,n,s}]}]`,
  but the app provides full `old`/`new` text. Add a line-diff producing unified hunks —
  reuse `@codemirror/merge`'s diff primitive (already a dependency) or a small LCS — with
  collapse of large unchanged gaps (the prototype shows full hunks; we keep context
  windows to avoid giant rows).
- **File → rows**: split working-tree `.new` into `{n, s, add:false}` lines for `fileToRows`.
- **Blame → rows**: map `BlameLine { author, sha, when, summary }` → the prototype's
  `{ author, sha, n, s, age, rel, first }` (compute `age` from min/max `when`, `rel` from
  `when`, `first` by grouping consecutive same-sha lines). Author color/avatar reuse the
  existing per-author hue.

## Structured message format (delivery)

```
Review feedback:
[global] <global note, omitted if empty>

<file>:<fromLine>[-<toLine>]  `<snippet>`  [(block, N lines)]
  → <note>
<file>:<line> …
  → <note>
```
Wrapped in `ESC[200~ … ESC[201~` and followed by `\r`. `assemble()` is pure and unit-tested
against this exact format.

## Stall guard (§7)

The prototype renders one DOM row per line with no upper bound — the same pathology
`code-diff.component.ts` documents (16s freeze on many long lines). The unified renderer
must reuse the existing `diffWouldStall(old, new)` heuristic: above threshold, render a
plain/capped view with a "Review anyway" escape, identical in spirit to the current
fallback. Normal files render pixel-identically to the mock; only pathological files
degrade.

## Testing (per global E2E rule)

- **Vitest**: `ReviewStore` add/remove/clear (in-memory, no persistence); `assemble()` output (snapshot the
  exact structured string + bracketed-paste wrapping); diff→hunks and blame→rows adapters.
- **Playwright E2E** (`e2e/`): open a diff → hover line → "+" → type note → ⌘↵ save →
  saved card appears → drag a range → second comment → "Send review (2)" → modal lists
  both grouped by file → add global note → Send → assert `agent_input` received the
  bracketed-paste-wrapped structured payload and the batch cleared. Separate E2E for the
  annotate popup hover card.

## Out of scope (YAGNI)

- Threaded/replied comments, agent replies back into the panel.
- Live anchor re-tracking when the file changes under a pending comment (we send the
  snippet so the agent can relocate; anchors are captured at comment time).
- Comments in the multi-file commit/range inspection (`agent-git-view`) — explicitly out;
  those views keep CodeMirror untouched.
- The rich annotate popup in the commit/range `diff-or-blame` blame (only `diff-view`'s
  Annotate gets it).

## Suggested implementation phases

1. **Annotate popup** — small, self-contained; unified blame view + hover card.
2. **Unified renderer + stall guard + data adapters** — the shared substrate.
3. **Inline comments** — gutter "+", drag-range, composer, saved cards, `ReviewStore`.
4. **Send** — button, modal, `assemble()`, PTY delivery.
5. **Tests** — vitest + E2E throughout (not a trailing phase).
