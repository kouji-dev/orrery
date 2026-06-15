# Ticket Backlog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the full ticket-backlog feature from the Claude Design handoff into Orrery: a persistent backlog board (To do / In progress / Done), dedicated inline ticket pages with a comments thread and a Lexical rich-text editor, and a "Ticket" field on the spawn modal — wired to the real agent fleet (dispatch → In progress, agent done → Done).

**Architecture:** A new Rust `tickets` module (mirrors `projects`) persists tickets + comments and emits `ticket://*` / `comment://*` events. A frontend `TicketsStore` (mirrors `ProjectsStore` via `createEntityStore`/`bindFacade`) consumes them. New tab kinds `backlog` and `ticket` extend `UiStore`; the shell renders `<app-backlog>` / `<app-ticket-page>` for them. The rich editor is an Angular wrapper around Lexical's framework-agnostic core. Status auto-transitions live in the backend (spawn-with-ticket → InProgress; agent status `done` → Done); manual moves go through `ticket_set_status`.

**Tech Stack:** Rust/Tauri (SQLite via `core/database.rs`), Angular 20 (signals, standalone components), Lexical (`lexical` + `@lexical/{rich-text,list,link,code,html,utils,selection}`), Vitest.

**Design source of truth (in-repo):** `docs/design/backlog/` — `backlog.jsx` (board + cards + AgentStrip), `ticketpage.jsx` (ticket page + comments), `richeditor.jsx` (editor feature set/toolbar), `data.js` (shapes), `styles.css` (tokens), `components.jsx` (atoms), `app.jsx` (wiring/handlers/lifecycle). Recreate the **visual output** of these in Angular using the app's existing tokens/atoms (`StatusDotComponent`, `ToolBadgeComponent`, `IconComponent`, `status-pill`, `.btn`/`.chip`/`.surface`/`.field-label` classes); do **not** transcribe the JSX structure literally.

---

## Data shapes (canonical — used across phases)

```
Ticket {
  id: string (uuid)
  title: string
  notes: string            // rich HTML (Lexical-exported)
  status: "todo" | "inprogress" | "done"
  projectId: string | null
  agentId: string | null   // the attached agent, when dispatched
  createdAt: number        // unix ms
  updatedAt: number
}
Comment {
  id: string (uuid)
  ticketId: string
  author: string           // "You" for human; agent name for agent
  role: "user" | "agent"
  tool: string | null      // agent tool id when role === "agent"
  body: string             // rich HTML
  createdAt: number
}
```
Rust enums use serde camelCase rename to match (`#[serde(rename_all = "camelCase")]`), `status` lower-cased (`todo`/`inprogress`/`done`). `Agent` gains `ticketId: Option<Uuid>` (Rust) / `ticketId?: string` (TS).

---

## Phase 1 — Backend: `tickets` Rust module (mirror `projects`)

**Files:**
- Create: `src-tauri/src/tickets/{mod,model,service,commands}.rs`
- Modify: `src-tauri/src/core/database.rs` (tickets + comments tables), `src-tauri/src/lib.rs` (register service + commands)

- [ ] **Step 1: Read the template before writing.** Read `src-tauri/src/projects/{mod,model,service,commands}.rs`, `src-tauri/src/core/database.rs`, `src-tauri/src/core/events.rs`, and how `lib.rs` registers `ProjectService` + the `project_*` commands in `invoke_handler`. The `tickets` module must follow this structure exactly (same error type `AppResult`, same `emit_entity(&app, "ticket", Change::…, payload)` pattern, same `State<'_, TicketService>` injection).

- [ ] **Step 2: `model.rs`** — `Ticket`, `TicketStatus` (enum, serde `todo`/`inprogress`/`done`), `Comment`, `CommentRole`, and request structs `TicketCreateRequest { title, notes?, project_id? }`, `TicketUpdateRequest { title?, notes?, project_id? }`, `CommentCreateRequest { body, author?, role?, tool? }`. All `#[serde(rename_all = "camelCase")]`. Shapes per the canonical block above.

- [ ] **Step 3: `core/database.rs`** — add a `tickets` table and a `comments` table (FK ticket_id, ON DELETE CASCADE) following the existing projects-table migration/setup. Provide CRUD helpers consistent with how projects rows are read/written.

- [ ] **Step 4: `service.rs`** — `TicketService` (clonable, DB-backed like `ProjectService`): `list()`, `create(req)`, `update(id, req)`, `remove(id)`, `set_status(id, status)`, `attach_agent(id, agent_id)` (sets agentId + status=inprogress), `complete_for_agent(agent_id)` (find ticket by agentId, set status=done), `comments(ticket_id)`, `add_comment(ticket_id, req)`. `remove` cascades comments.

- [ ] **Step 5: `commands.rs`** — `ticket_list`, `ticket_create`, `ticket_update`, `ticket_remove`, `ticket_set_status`, `comment_list`, `comment_add`, each emitting the matching `ticket://*` / `comment://*` event via `emit_entity` (mirror `project_*`).

- [ ] **Step 6: `mod.rs` + `lib.rs`** — expose the module; in `lib.rs` `.manage(TicketService::new(...))` and add every `ticket_*`/`comment_*` to `invoke_handler![...]`, exactly as projects are registered.

- [ ] **Step 7: Build + (if projects has service tests) a Rust unit test** for `TicketService` create/list/update/set_status/remove + comment add/list, mirroring any `projects/service.rs` test. Run `rtk cargo test` (and `rtk cargo build`). Expected: green.

- [ ] **Step 8: Commit** — `feat(tickets): backend tickets+comments module (persist + events)`

---

## Phase 2 — Backend: wire spawn + lifecycle

**Files:**
- Modify: `src-tauri/src/agents/{model,commands,service}.rs`

- [ ] **Step 1:** Add `ticket_id: Option<Uuid>` to the agent model + `AgentSpawnRequest`. On `agent_spawn`, persist it on the agent; if present, call `tickets.attach_agent(ticket_id, agent_id)` and `emit_entity(&app, "ticket", Change::Updated, ticket)` so the board flips the ticket to In progress.

- [ ] **Step 2:** Where an agent's status becomes `done` (the same place that emits `agent://updated` with `status:"done"` — find it in `agents/service.rs`/runtime), call `tickets.complete_for_agent(agent_id)`; if a ticket transitioned, emit `ticket://updated`. Guard: only auto-complete if the ticket isn't already `done` (so manual moves aren't fought).

- [ ] **Step 3:** Build + test (`rtk cargo build`, `rtk cargo test`). Commit — `feat(tickets): attach on spawn + auto-complete on agent done`.

---

## Phase 3 — Frontend data layer

**Files:**
- Modify: `src/app/models.ts`, `src/app/data-source/bridge.ts`
- Create: `src/app/stores/tickets.store.ts`, `src/app/stores/tickets.store.spec.ts`

- [ ] **Step 1:** `models.ts` — add `TicketStatus`, `Ticket`, `CommentRole`, `Comment` (canonical shapes); add `ticketId?: string` to `Agent`; extend `Tab` → `kind?: "orchestrator" | "agent" | "backlog" | "ticket"` and add `ticketId?: string` (for ticket tabs).

- [ ] **Step 2:** `bridge.ts` — add Commands `TicketList/Create/Update/Remove/SetStatus`, `CommentList/Add` and Events `TicketCreated/Updated/Deleted`, `CommentCreated`, matching the Rust command/event strings.

- [ ] **Step 3 (TDD):** Write `tickets.store.spec.ts` mirroring `stores/agents.store.spec.ts`/`metrics` specs — assert list load, create/update/remove, setStatus, addComment go through the bridge and the store upserts on events. Run, see it fail.

- [ ] **Step 4:** `tickets.store.ts` — copy `ProjectsStore` structure (`createEntityStore`/`bindFacade` on the ticket events); add `create/update/remove/setStatus/addComment` invokers + a `comments(ticketId)` accessor (comments can ride on the ticket record or a small per-ticket Loadable map — choose the simplest that the design needs). Run the spec, green. Commit — `feat(tickets): models + bridge + TicketsStore`.

---

## Phase 4 — Lexical rich-text editor (Angular)

**Files:**
- Modify: `package.json` (add `lexical`, `@lexical/rich-text`, `@lexical/list`, `@lexical/link`, `@lexical/code`, `@lexical/html`, `@lexical/utils`, `@lexical/selection`)
- Create: `src/app/shared/rich-editor/rich-editor.component.ts`, `rich-view.component.ts`, `rich-editor.component.spec.ts`

- [ ] **Step 1:** `pnpm add` the Lexical packages. Reference `docs/design/backlog/richeditor.jsx` for the exact toolbar/feature set: headings (H1/H2), bold/italic/strike, inline code, bullet + numbered lists, quote, code block, link popover; ⌘B/⌘I/⌘K shortcuts; `compact` mode for the comment composer.

- [ ] **Step 2 (TDD):** Spec — mount the editor, assert: HTML round-trips (`value` HTML in → rendered → `valueChange` HTML out via `@lexical/html` `$generateHtmlFromNodes`/`$generateNodesFromDOM`); bold command wraps selection (`<strong>`/`<b>`); a heading command emits `<h2>`. Run, fail.

- [ ] **Step 3:** Build `RichEditorComponent` — a standalone Angular component wrapping `createEditor()` with `RichTextPlugin`-equivalent manual wiring (register `HeadingNode`, `QuoteNode`, `ListNode`, `ListItemNode`, `LinkNode`, `CodeNode`, `CodeHighlightNode`), a contentEditable host, a toolbar driving `FORMAT_TEXT_COMMAND`/`INSERT_ORDERED_LIST_COMMAND`/etc., a link popover, and `@Input() value`/`@Output() valueChange` as sanitized HTML. Add `RichViewComponent` (read-only: sanitized `innerHTML` of stored HTML, styled like the editor output). Match the design's toolbar visuals using existing tokens. Run spec, green.

- [ ] **Step 4:** Commit — `feat(rich-editor): Lexical-based RichEditor + RichView`.

---

## Phase 5 — Backlog board view

**Files:**
- Create: `src/app/backlog/backlog.component.ts`, `ticket-card.component.ts`, `agent-strip.component.ts`
- Modify: `src/app/ui/ui.store.ts` (openBacklog), `src/app/shell/shell.component.ts` (render backlog tab), `src/app/top-bar/top-bar.component.ts` + `src/app/sidebar/{sidebar,compact-rail}.component.ts` (nav entry)

- [ ] **Step 1:** `UiStore` — add `openBacklog()` (pinned singleton `{id:"backlog", kind:"backlog"}` tab, reused if present) and a computed `activeTabKind()` (look up `activeTab` in `tabs`). Pin the backlog tab so it isn't closable.

- [ ] **Step 2:** `shell.component.ts` — replace the `activeTab()==='orchestrator'` check with a switch on `ui.activeTabKind()`: `orchestrator`→overview, `backlog`→`<app-backlog>`, `ticket`→`<app-ticket-page [ticketId]>` (Phase 6), else→pane-manager.

- [ ] **Step 3:** Build `BacklogComponent` + `TicketCardComponent` + `AgentStripComponent` matching `docs/design/backlog/backlog.jsx` (3 columns w/ counts + colored header border, project filter dropdown, "+ New ticket", empty state, drag-and-drop between columns calling `ticketsStore.setStatus`, the three card variants, AgentStrip resolving the live agent from `AgentsStore`). Clicking a card → `openTicket(id)`; Dispatch → `openSpawn` prefilled+linked to the ticket; agent strip click → `ui.openAgent(agentId)`.

- [ ] **Step 4:** Nav — topbar renders the pinned Backlog tab; sidebar + compact-rail get a Backlog entry with an open-ticket count badge (`tickets where status!=='done'`).

- [ ] **Step 5:** Vitest for the card status→variant mapping + open-count badge. Run green. Commit — `feat(backlog): board view + cards + nav + shell wiring`.

---

## Phase 6 — Ticket page (dedicated inline tab) + comments

**Files:**
- Create: `src/app/backlog/ticket-page.component.ts` (+ small `comment.component.ts`)
- Modify: `src/app/ui/ui.store.ts` (openTicket / openTicketDraft / closeTab handles ticket tabs), `src/app/top-bar/top-bar.component.ts` (ticket tab labels, closable)

- [ ] **Step 1:** `UiStore` — `openTicket(ticketId)` (reuse an existing ticket tab for that id, else push `{id, kind:"ticket", ticketId}`) and `openTicketDraft()` (a `{kind:"ticket", ticketId:"draft"}` tab → page opens in create mode). Ticket tabs are closable.

- [ ] **Step 2:** Build `TicketPageComponent` matching `docs/design/backlog/ticketpage.jsx`: header (breadcrumb Backlog ▸ #id, status pill, actions — Dispatch agent / Open agent / Edit / Delete, or Create/Cancel in draft/edit), 2-column body — main = rich notes (`RichViewComponent` view, `RichEditorComponent` edit) + comments thread (`Comment` list with human/agent avatars + AGENT badge + `RichViewComponent` bodies, and a `RichEditorComponent` composer posting via `ticketsStore.addComment`); side rail = status segmented control (`ticketsStore.setStatus`), project select, attached-agent card (→ openAgent), branch, created. Draft mode hides comments and calls `ticketsStore.create` then opens the created ticket.

- [ ] **Step 3:** topbar — ticket tabs labelled by ticket title (truncated) / "New ticket" for draft, with a close affordance.

- [ ] **Step 4:** Manual verification in the running app (per global E2E rule): create a ticket, edit notes (Lexical), post a comment, dispatch from it (spawn modal prefilled), drag/segment status, confirm persistence across restart. Commit — `feat(backlog): dedicated ticket page + comments + lifecycle`.

---

## Phase 7 — Spawn modal: Ticket field

**Files:**
- Modify: `src/app/modals/spawn-modal.component.ts`, `src/app/agents/agent-actions.service.ts` (+ runtime/spawn payload)

- [ ] **Step 1:** Add an optional **Ticket** field to the spawn modal (a picker of open `todo`/`inprogress` tickets + "None"), placed per the design. Selecting one prefills `name` + `prompt` from the ticket (title → name slug, notes plain-text → prompt) and shows the "linked" accent cue. Default-select the ticket when spawn was opened from a ticket's Dispatch.

- [ ] **Step 2:** Thread `ticketId` through `submit()` → `agentActions.spawn({…, ticketId})` → the `AgentSpawn` invoke payload (Phase 2 backend consumes it). Confirm dispatch flips the ticket to In progress and attaches the agent (event-driven).

- [ ] **Step 3:** Commit — `feat(spawn): optional Ticket attach (prefill + link)`.

---

## Phase 8 — Final review + verification

- [ ] **Step 1:** `rtk vitest run` (frontend) + `rtk cargo test` (backend) — all green.
- [ ] **Step 2:** Run the app; walk the full loop: Backlog tab → New ticket (Lexical notes) → Dispatch → ticket In progress + live AgentStrip → open agent → agent finishes → ticket auto-Done → comments thread + editor work → restart app, data persists.
- [ ] **Step 3:** Final code review across the branch; address findings. Commit any fixes.

---

## Notes / decisions baked in
- **Auto-Done** on agent finish, manual drag/segment overrides (backend guards already-done).
- **Dedicated Backlog view** + dedicated ticket **tabs** (not modals), per the design's second iteration.
- Agent-authored comments are **human-postable now**; live agent→comment plumbing is out of scope (the design seeds agent comments as mock) — `comment_add` supports `role:"agent"` for future use but nothing auto-posts.
- Visual fidelity defers to `docs/design/backlog/*`; reuse existing Orrery atoms/tokens rather than the design's own `components.jsx`/`styles.css`.
