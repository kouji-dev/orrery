# Design — Backend Foundation + Projects (Slice 1)

_Date: 2026-06-05 · Status: approved for planning_

## Source of requirements

The `docs/` checklists describe the full ORCHESTRA product (a multi-agent git
orchestrator). The frontend implements that UI today on **mock data**
(`src/app/orchestra/orchestra.store.ts`). This spec covers the **first slice** of making
it real on the Tauri/Rust backend, plus the shared framework every later slice reuses.

## Goal & scope

**This spec (Slice 1):**
1. A shared **communication + entity-state framework** (frontend) and **command/event/entity
   conventions** (backend).
2. The **Projects** domain wired end-to-end: list, create (with real git detect/init), remove —
   replacing the mock projects.

**Out of scope** for this spec (own future specs — see roadmap): agents, worktrees, git
diff/commit/merge, terminal/PTY, agent runtime, persistence/resume, inbox.

## Roadmap (full product, context only)

We build the whole product, but one slice at a time (each gets its own spec → plan →
implementation). Dependency DAG — independent branches may be built in parallel:

```
Slice1 Foundation+Projects ─▶ Slice2 Agents+Worktrees ─┬─▶ Slice3 Git ops ──┐
                                                        └─▶ Slice4 Terminal ─┴─▶ Slice5 Runtime ─▶ Slice6 Resume
                                                                                                  └─▶ Slice7 Inbox
```
Slices 3 (Git) and 4 (Terminal) are independent once worktrees exist. Within any slice, the
per-module files are independent and the implementation plan should mark them for parallel
subagent execution.

## Architecture

### Backend module layout (modular — each domain self-contained)

```
src-tauri/src/
  core/      database.rs · errors.rs · logger.rs · ids.rs · events.rs · mod.rs   # shared only
  projects/  model.rs · service.rs · commands.rs · mod.rs
  git/       service.rs · mod.rs                                                  # Slice 1: detect/init/head
  lib.rs     # builder: setup, manage services, register all commands
  (agents/, terminal/, runtime/, inbox/ added by later slices)
```
Rule: modules call *down* into others (e.g. `projects` uses `git`), never in cycles. Each
module owns a service (managed state) + its commands.

### Git integration

Use the **`git2` crate** (libgit2 bindings) — in-process, typed, no shelling out. Slice 1 needs
only `Repository::open` (detect), `Repository::init` (init), and HEAD inspection. Worktree/
diff/commit APIs (also in git2) come in later slices. CLI fallback only if a specific op isn't
exposed by libgit2.

### Backend conventions

- **Entities** serialize flat with a stable string `id` (UUID), `#[serde(rename_all = "camelCase")]`
  to match TypeScript.
- **Commands** named `<domain>_<action>` (e.g. `project_list`, `project_create`,
  `project_remove`, `project_detect_git`), returning `AppResult<T>`.
- **Events** named `<entity>://<change>`, `change ∈ created | updated | deleted`; payload is the
  entity (or `{ id }` for deleted). A `core::events::emit_entity(app, &entity, Change)` helper
  keeps emission uniform.
- **Errors**: `AppError` serializes to `{ kind, message }` so the frontend can branch on `kind`.

### Frontend framework (Angular signals — no NgRx/NGXS dependency)

Replicates the normalized-collection core of `@ngxs-labs/entity-state` (entities map + ids +
active + loading + CRUD), built from signals.

1. **`createEntityStore<T>(idSelector)` → `EntityStore<T>`**
   - State signals: `entities: Record<string,T>`, `ids: string[]`, `loading: boolean`,
     `activeId: string | null`.
   - Computed: `all: T[]`, `active: T | null`, `byId(id): Signal<T|undefined>`.
   - Mutators: `setAll`, `upsert`, `upsertMany`, `update(id, patch)`, `remove(id)`,
     `setActive(id)`, `setLoading`, `reset`.

2. **`Bridge` (command/event bus + data-source seam)**
   - `invoke<R>(command, payload?): Promise<R>` — wraps Tauri `invoke`, maps a rejected
     `{ kind, message }` into a typed `AppError`.
   - `on<T>(event, handler): () => void` — wraps `listen`, returns an unsubscribe.
   - Implementations: `TauriBridge` (real) and `MockBridge` (today's in-memory data). Selected at
     bootstrap by `window.__TAURI__` presence (browser → mock; Tauri window → real). This keeps
     the existing browser dev/verify loop alive.
   - A typed catalog (`Commands`, `Events`) so domains never hardcode strings.

3. **`EntityFacade<T>` (binds store + bridge — the reusable "state manager")**
   - Config: `{ entity, listCommand, createCommand, removeCommand, events: { created, updated, deleted } }`.
   - `load()` → `bridge.invoke(listCommand)` → `store.setAll`; subscribes the entity events to
     `upsert`/`remove`. Domain stores configure a facade instead of hand-wiring invoke/listen.

`OrchestraStore` becomes a thin composition of domain stores (facades). Existing `computed()`
selectors are preserved.

## Slice 1 detail — Projects domain

### Model (`projects/model.rs`)

```rust
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: Uuid,            // stored as TEXT
    pub name: String,
    pub path: String,        // working directory
    pub icon: String,
    pub color: String,
    pub has_git: bool,
    pub branch: Option<String>,   // from HEAD when has_git
    pub head: Option<String>,     // short sha when has_git
}
```
(`branches`, `files`, `org` are deferred to later slices.)

### `git/service.rs` (git2)

- `detect(path: &Path) -> bool` — `Repository::open(path).is_ok()`.
- `init(path: &Path) -> AppResult<()>` — `Repository::init(path)`.
- `head_info(path: &Path) -> Option<(String, String)>` — current branch name + short HEAD sha.

### `projects/service.rs` (refactor of existing)

`ProjectService { db: DB, git: GitService }` with:
- `list() -> AppResult<Vec<Project>>`
- `get(id: Uuid) -> AppResult<Project>` (`ProjectError::NotFound` if absent)
- `create(req: ProjectCreateRequest) -> AppResult<Project>` — validate (`Required`/`Exists`),
  run `git.init` when `with_git && !git.detect`, read `head_info`, insert, return the built `Project`.
- `remove(id: Uuid) -> AppResult<()>`

### `projects/commands.rs`

- `project_list() -> AppResult<Vec<Project>>`
- `project_create(req) -> AppResult<Project>` (emits `project://created`)
- `project_remove(id) -> AppResult<()>` (emits `project://deleted`)
- `project_detect_git(path: String) -> AppResult<bool>`

All inject `State<ProjectService>` (+ `AppHandle` for emitting).

### Schema + ID standardization (refactor)

Existing table uses `id INTEGER PRIMARY KEY`; `core::database::ID` is `Uuid`. Standardize on
UUID stored as TEXT:

```sql
CREATE TABLE IF NOT EXISTS projects (
  id   TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  icon TEXT NOT NULL,
  color TEXT NOT NULL,
  has_git INTEGER NOT NULL          -- 0/1
);
```
No real data exists yet, so the table is simply recreated with the new shape.

### Data flow

- **Add Project:** modal calls `project_detect_git(path)` as the path changes (real filesystem
  check, replacing the frontend regex heuristic) → submit calls `project_create` → service
  validates, optionally `git init`, reads head, inserts, returns `Project` + emits
  `project://created`. The originating window upserts the **returned** entity; the event keeps
  any **other** window in sync. Upsert is keyed by `id`, so the two paths are idempotent (no
  duplicate).
- **List (hydrate):** on boot `ProjectsStore.load()` → `project_list` → `setAll`.
- **Remove:** `project_remove(id)` → emits `project://deleted` → store `remove`.

## Refactors to existing code (explicitly in scope)

- `projects/service.rs`: change `create` to return `Project`; add `list`/`get`/`remove`; inject
  `GitService`; switch params to a UUID id.
- `projects/project.rs`: align fields with the model above (`path`, `has_git`, `branch`, `head`),
  add serde derives + camelCase.
- `core/database.rs`: schema/ID move to TEXT UUID.
- `core/errors.rs`: add `ProjectError::NotFound`; ensure `AppError` implements `Serialize` to
  `{ kind, message }`.
- `Cargo.toml`: enable `uuid` features `["v4", "serde"]` (needed to generate ids and to
  serialize/deserialize `Uuid` across the command boundary).
- `lib.rs`: build `GitService` + `ProjectService` in `setup` (needs `AppHandle` for the DB path),
  `manage` them, register the four commands in `generate_handler!`.
- Frontend: introduce `EntityStore`/`Bridge`/`EntityFacade`; refactor `OrchestraStore` projects
  to a `ProjectsStore` facade; remove mock projects (kept in `MockBridge`); wire sidebar +
  Add Project + remove to the facade.

## Error handling

Commands return `AppResult`; `AppError` → `{ kind, message }`. `Bridge.invoke` rejects with the
typed error; calling store catches and flashes the existing toast (e.g. duplicate path →
`ProjectError::Exists`).

## Testing strategy

- **Rust (`ProjectService`)** against `:memory:` SQLite: create / list / remove / duplicate-path
  rejection / empty-field validation / get-missing → NotFound.
- **Rust (`GitService`)** against temp dirs: detect false → init → detect true; head_info on a
  repo with a commit.
- **Frontend:** `MockBridge` preserves the browser verify loop; unit test that `ProjectsStore`
  hydrates from `project_list` and patches on `project://created` / `project://deleted`.

## Success criteria

- Launching the Tauri app shows real projects from SQLite (persisted across restarts).
- Add Project creates a row (and a git repo when requested), the sidebar updates live, and a
  duplicate path is rejected with a toast.
- Removing a project deletes the row and updates the UI.
- The same UI still runs in a plain browser via `MockBridge`.
- `cargo build` + `cargo test` + `ng build` pass.

## Parallelization notes (for the plan)

Independent work units that can run as parallel subagents:
- backend `git/` module · backend `projects/` module · `core` conventions (events/errors)
- frontend `EntityStore` · `Bridge` (+Mock/Tauri) · `EntityFacade`

Convergence points: `lib.rs` wiring (after services exist); `OrchestraStore`/component wiring
(after the frontend framework + commands exist).
