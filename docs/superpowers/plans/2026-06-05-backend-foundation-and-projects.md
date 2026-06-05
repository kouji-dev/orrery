# Backend Foundation + Projects (Slice 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the ORCHESTRA Projects feature real — persisted in SQLite via the Tauri backend with `git2` detection/init — and stand up the shared comms/entity-state framework every later slice reuses, while keeping the UI runnable in a plain browser via a mock data source.

**Architecture:** Modular Rust backend (`core/`, `git/`, `projects/`) exposing `domain_action` commands and `entity://change` events over normalized camelCase entities. The Angular frontend gets three reusable units — a signal-based `EntityStore`, a `Bridge` (Tauri vs Mock, the data-source seam), and an `EntityFacade` that binds them — and `OrchestraStore` becomes a thin composition. SQLite is the source of truth; the frontend store is its live projection.

**Tech Stack:** Rust + Tauri v2, `rusqlite` (bundled), `git2`, `uuid`, `thiserror`, `tauri-plugin-log`; Angular 20 signals, `@tauri-apps/api`, Vitest for pure-TS unit tests.

**Parallel tracks:** Track A (backend) and Track B (frontend framework) are independent and can run as parallel subagents. **Convergence** tasks (C1 `lib.rs` wiring; C2 frontend Projects wiring; C3 end-to-end verify) require both tracks finished. Within Track A, the `git/` module (A4) is independent of `core` (A2/A3); within Track B, `EntityStore` (B2), `Bridge` (B3) are independent and both feed `EntityFacade` (B4).

---

## File Structure

**Backend (`src-tauri/src/`):**
- `core/mod.rs` — add `pub mod events;` (modify)
- `core/errors.rs` — add `ProjectError::NotFound`, `Serialize for AppError` (modify)
- `core/events.rs` — `Change` enum + `event_name` + `emit_entity` (create)
- `core/database.rs` — keep `DB`/`ID`/`Database::get`; no project schema here (unchanged)
- `git/mod.rs` — `pub mod service;` (create)
- `git/service.rs` — `GitService` detect/init/head_info (create)
- `projects/mod.rs` — `pub mod model; pub mod service; pub mod commands;` (modify)
- `projects/model.rs` — `Project`, `ProjectCreateRequest` (create; replaces `project.rs`)
- `projects/project.rs` — delete
- `projects/service.rs` — refactor: `list/get/create/remove/detect_git`, TEXT-UUID schema (modify)
- `projects/commands.rs` — 4 Tauri commands (create)
- `lib.rs` — register `git`/`projects` modules; build+manage services in `setup`; register handlers (modify)
- `Cargo.toml` — add `git2`, `uuid` features, dev-dep `tempfile` (modify)

**Frontend (`src/`):**
- `app/orchestra/state/entity-store.ts` — `createEntityStore` (create)
- `app/orchestra/state/entity-store.spec.ts` — tests (create)
- `app/orchestra/state/entity-facade.ts` — `bindFacade` (create)
- `app/orchestra/data-source/bridge.ts` — `Bridge`, `BridgeError`, `Commands`, `Events`, `BRIDGE` token (create)
- `app/orchestra/data-source/tauri-bridge.ts` — `TauriBridge` (create)
- `app/orchestra/data-source/mock-bridge.ts` — `MockBridge` + mock project seed (create)
- `app/orchestra/data-source/mock-bridge.spec.ts` — tests (create)
- `app/orchestra/stores/projects.store.ts` — `ProjectsStore` (create)
- `app/orchestra/orchestra.store.ts` — projects delegate to `ProjectsStore` (modify)
- `app/orchestra/sidebar/sidebar.component.ts` — Add Project / remove via store (modify)
- `app/orchestra/modals/add-project-modal.component.ts` — real `detectGit`, async submit (modify)
- `app/orchestra/modals/spawn-modal.component.ts` — guard `proj.branches ?? []` (modify)
- `app/app.config.ts` — provide `BRIDGE` (modify)
- `vitest.config.ts` — Vitest config (create)
- `package.json` — `test` script + devDeps (modify)

---

# TRACK A — Backend (Rust)

## Task A1: Dependencies

**Files:** Modify `src-tauri/Cargo.toml`

- [ ] **Step 1: Add crates**

Run (from `src-tauri/`):
```bash
cargo add git2
cargo add uuid --features v4,serde
cargo add --dev tempfile
```
Expected: `Cargo.toml` shows `git2`, `uuid = { version = "1", features = ["v4","serde"] }`, and `[dev-dependencies] tempfile`.

- [ ] **Step 2: Verify it resolves**

Run: `cargo build`
Expected: compiles (downloads + builds libgit2 via the bundled `git2` build; first build is slow).

- [ ] **Step 3: Commit**
```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "build: add git2, uuid (v4+serde), tempfile dev-dep"
```

---

## Task A2: Core errors — `NotFound` + serializable `{kind,message}`

**Files:** Modify `src-tauri/src/core/errors.rs`; Test: same file (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/core/errors.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_kind_and_message() {
        let err = AppError::Project(ProjectError::NotFound("p1".into()));
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, r#"{"kind":"notFound","message":"p1"}"#);
    }

    #[test]
    fn db_error_serializes_with_db_kind() {
        let err = AppError::Other("boom".into());
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(v["kind"], "other");
        assert_eq!(v["message"], "boom");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p katrix core::errors`
Expected: FAIL — `NotFound` variant missing and `AppError` does not implement `Serialize`.

- [ ] **Step 3: Implement**

Add the variant to `ProjectError`:
```rust
    #[error("not found: {0}")]
    NotFound(String),
```
Add below the enums (do NOT `#[derive(Serialize)]` — write it by hand to control the shape):
```rust
impl serde::Serialize for AppError {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (kind, message) = match self {
            AppError::Project(ProjectError::NotFound(m)) => ("notFound", m.clone()),
            AppError::Project(e) => ("project", e.to_string()),
            AppError::Db(e) => ("db", e.to_string()),
            AppError::Other(m) => ("other", m.clone()),
        };
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("kind", kind)?;
        st.serialize_field("message", &message)?;
        st.end()
    }
}
```
Add `use serde;` is unnecessary (serde is a dep; reference via full path as above).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p katrix core::errors`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**
```bash
git add src-tauri/src/core/errors.rs
git commit -m "feat(core): ProjectError::NotFound + AppError serializes to {kind,message}"
```

---

## Task A3: Core events helper

**Files:** Create `src-tauri/src/core/events.rs`; Modify `src-tauri/src/core/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/core/events.rs`:
```rust
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

#[derive(Clone, Copy, Debug)]
pub enum Change {
    Created,
    Updated,
    Deleted,
}

impl Change {
    pub fn as_str(self) -> &'static str {
        match self {
            Change::Created => "created",
            Change::Updated => "updated",
            Change::Deleted => "deleted",
        }
    }
}

pub fn event_name(entity: &str, change: Change) -> String {
    format!("{entity}://{}", change.as_str())
}

pub fn emit_entity<R: Runtime, T: Serialize + Clone>(
    app: &AppHandle<R>,
    entity: &str,
    change: Change,
    payload: T,
) {
    let _ = app.emit(&event_name(entity, change), payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_event_names() {
        assert_eq!(event_name("project", Change::Created), "project://created");
        assert_eq!(event_name("agent", Change::Deleted), "agent://deleted");
    }
}
```
Add to `src-tauri/src/core/mod.rs`:
```rust
pub mod events;
```

- [ ] **Step 2: Run test to verify it fails, then passes**

Run: `cargo test -p katrix core::events`
Expected: compiles and PASS (1 test). If it fails to compile first, that's the red; re-run after the file is saved for green.

- [ ] **Step 3: Commit**
```bash
git add src-tauri/src/core/events.rs src-tauri/src/core/mod.rs
git commit -m "feat(core): entity event-name + emit_entity helper"
```

---

## Task A4: `git/` module — detect / init / head_info  *(independent — parallelizable)*

**Files:** Create `src-tauri/src/git/mod.rs`, `src-tauri/src/git/service.rs`; Modify `src-tauri/src/lib.rs` (add `mod git;`)

- [ ] **Step 1: Register the module**

Create `src-tauri/src/git/mod.rs`:
```rust
pub mod service;
```
Add to the top of `src-tauri/src/lib.rs` (with the other `mod` lines):
```rust
mod git;
```

- [ ] **Step 2: Write the failing test + service skeleton**

Create `src-tauri/src/git/service.rs`:
```rust
use std::path::Path;

use git2::Repository;

use crate::core::errors::{AppError, AppResult};

#[derive(Clone, Default)]
pub struct GitService;

impl GitService {
    pub fn new() -> Self {
        Self
    }

    /// True if `path` is (inside) a git repository.
    pub fn detect(&self, path: &Path) -> bool {
        Repository::open(path).is_ok()
    }

    /// Initialize a new repository at `path`.
    pub fn init(&self, path: &Path) -> AppResult<()> {
        Repository::init(path).map_err(|e| AppError::Other(format!("git init: {e}")))?;
        Ok(())
    }

    /// (branch, short-sha) of HEAD, or None for a repo with no commits / no repo.
    pub fn head_info(&self, path: &Path) -> Option<(String, String)> {
        let repo = Repository::open(path).ok()?;
        let head = repo.head().ok()?;
        let branch = head.shorthand()?.to_string();
        let short = head.target()?.to_string().chars().take(7).collect::<String>();
        Some((branch, short))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_then_init_then_detect() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        assert!(!svc.detect(dir.path()), "fresh dir is not a repo");
        svc.init(dir.path()).unwrap();
        assert!(svc.detect(dir.path()), "after init it is a repo");
    }

    #[test]
    fn head_info_is_none_without_commits() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        assert!(svc.head_info(dir.path()).is_none());
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p katrix git::service`
Expected: PASS (2 tests).

- [ ] **Step 4: Commit**
```bash
git add src-tauri/src/git/ src-tauri/src/lib.rs
git commit -m "feat(git): GitService detect/init/head_info via git2"
```

---

## Task A5: `projects` model

**Files:** Create `src-tauri/src/projects/model.rs`; Delete `src-tauri/src/projects/project.rs`; Modify `src-tauri/src/projects/mod.rs`

- [ ] **Step 1: Create the model**

Create `src-tauri/src/projects/model.rs`:
```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub icon: String,
    pub color: String,
    pub has_git: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCreateRequest {
    pub name: String,
    pub path: String,
    pub icon: String,
    pub color: String,
    pub with_git: bool,
}
```

- [ ] **Step 2: Swap the module wiring**

Delete `src-tauri/src/projects/project.rs`. Replace `src-tauri/src/projects/mod.rs` with:
```rust
pub mod commands;
pub mod model;
pub mod service;
```

- [ ] **Step 3: Commit** (will not compile until A6 — that's fine for an isolated commit of the model only if it compiles; otherwise commit together with A6. Prefer committing A5+A6 together.)

Skip committing here; commit at the end of A6.

---

## Task A6: `ProjectService` refactor + commands

**Files:** Modify `src-tauri/src/projects/service.rs`; Create `src-tauri/src/projects/commands.rs`; Test: in `service.rs`

- [ ] **Step 1: Write the failing tests**

Replace `src-tauri/src/projects/service.rs` entirely with:
```rust
use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use uuid::Uuid;

use crate::core::database::DB;
use crate::core::errors::{AppError, AppResult, DbError, ProjectError};
use crate::git::service::GitService;

use super::model::{Project, ProjectCreateRequest};

pub struct ProjectService {
    db: DB,
    git: GitService,
}

impl ProjectService {
    pub fn new(db: DB, git: GitService) -> Self {
        let svc = Self { db, git };
        svc.init_schema();
        svc
    }

    fn init_schema(&self) {
        let c = self.db.lock().unwrap();
        c.execute(
            "CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                icon TEXT NOT NULL,
                color TEXT NOT NULL,
                has_git INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();
    }

    pub fn detect_git(&self, path: &str) -> bool {
        self.git.detect(Path::new(path))
    }

    pub fn list(&self) -> AppResult<Vec<Project>> {
        // collect raw rows, then drop the lock before touching the filesystem (git)
        let raw: Vec<(String, String, String, String, String, i64)> = {
            let c = self.db.lock().unwrap();
            let mut stmt = c
                .prepare("SELECT id, name, path, icon, color, has_git FROM projects")
                .map_err(DbError::Sqlite)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                })
                .map_err(DbError::Sqlite)?;
            rows.collect::<Result<_, _>>().map_err(DbError::Sqlite)?
        };

        raw.into_iter()
            .map(|(id, name, path, icon, color, has_git)| {
                let id = Uuid::parse_str(&id).map_err(|e| AppError::Other(e.to_string()))?;
                let has_git = has_git != 0;
                let (branch, head) = self.head(&path, has_git);
                Ok(Project { id, name, path, icon, color, has_git, branch, head })
            })
            .collect()
    }

    pub fn get(&self, id: Uuid) -> AppResult<Project> {
        let row: Option<(String, String, String, String, i64)> = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT name, path, icon, color, has_git FROM projects WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .ok()
        };
        match row {
            Some((name, path, icon, color, has_git)) => {
                let has_git = has_git != 0;
                let (branch, head) = self.head(&path, has_git);
                Ok(Project { id, name, path, icon, color, has_git, branch, head })
            }
            None => Err(ProjectError::NotFound(id.to_string()).into()),
        }
    }

    pub fn create(&self, req: ProjectCreateRequest) -> AppResult<Project> {
        if req.name.trim().is_empty() || req.path.trim().is_empty() {
            return Err(ProjectError::Required("name or path is empty".into()).into());
        }
        if self.exists_by_path(&req.path)? {
            return Err(ProjectError::Exists(req.path.clone()).into());
        }

        let path = Path::new(&req.path);
        let detected = self.git.detect(path);
        if req.with_git && !detected {
            self.git.init(path)?;
        }
        let has_git = req.with_git || detected;
        let (branch, head) = self.head(&req.path, has_git);

        let id = Uuid::new_v4();
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "INSERT INTO projects (id, name, path, icon, color, has_git) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    id.to_string(),
                    req.name,
                    req.path,
                    req.icon,
                    req.color,
                    has_git as i64
                ],
            )
            .map_err(DbError::Sqlite)?;
        }

        Ok(Project {
            id,
            name: req.name,
            path: req.path,
            icon: req.icon,
            color: req.color,
            has_git,
            branch,
            head,
        })
    }

    pub fn remove(&self, id: Uuid) -> AppResult<()> {
        let c = self.db.lock().unwrap();
        let n = c
            .execute("DELETE FROM projects WHERE id = ?1", [id.to_string()])
            .map_err(DbError::Sqlite)?;
        if n == 0 {
            return Err(ProjectError::NotFound(id.to_string()).into());
        }
        Ok(())
    }

    fn exists_by_path(&self, path: &str) -> AppResult<bool> {
        let c = self.db.lock().unwrap();
        let count: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE path = ?1",
                [path],
                |r| r.get(0),
            )
            .map_err(DbError::Sqlite)?;
        Ok(count > 0)
    }

    fn head(&self, path: &str, has_git: bool) -> (Option<String>, Option<String>) {
        if !has_git {
            return (None, None);
        }
        match self.git.head_info(Path::new(path)) {
            Some((b, h)) => (Some(b), Some(h)),
            None => (None, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> ProjectService {
        let db: DB = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        ProjectService::new(db, GitService::new())
    }

    fn req(name: &str, path: &str, with_git: bool) -> ProjectCreateRequest {
        ProjectCreateRequest {
            name: name.into(),
            path: path.into(),
            icon: "box".into(),
            color: "#a855f7".into(),
            with_git,
        }
    }

    #[test]
    fn create_and_list() {
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let p = s.create(req("pay", dir.path().to_str().unwrap(), false)).unwrap();
        assert_eq!(p.name, "pay");
        let all = s.list().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, p.id);
    }

    #[test]
    fn rejects_empty_fields() {
        let s = svc();
        let err = s.create(req("", "", false)).unwrap_err();
        assert!(matches!(err, AppError::Project(ProjectError::Required(_))));
    }

    #[test]
    fn rejects_duplicate_path() {
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        s.create(req("a", path, false)).unwrap();
        let err = s.create(req("b", path, false)).unwrap_err();
        assert!(matches!(err, AppError::Project(ProjectError::Exists(_))));
    }

    #[test]
    fn with_git_initializes_repo() {
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let p = s.create(req("g", dir.path().to_str().unwrap(), true)).unwrap();
        assert!(p.has_git);
        assert!(dir.path().join(".git").exists());
    }

    #[test]
    fn get_missing_is_not_found() {
        let s = svc();
        let err = s.get(Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, AppError::Project(ProjectError::NotFound(_))));
    }

    #[test]
    fn remove_deletes() {
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let p = s.create(req("x", dir.path().to_str().unwrap(), false)).unwrap();
        s.remove(p.id).unwrap();
        assert_eq!(s.list().unwrap().len(), 0);
    }
}
```

- [ ] **Step 2: Create the commands**

Create `src-tauri/src/projects/commands.rs`:
```rust
use tauri::{AppHandle, Runtime, State};
use uuid::Uuid;

use crate::core::errors::AppResult;
use crate::core::events::{emit_entity, Change};

use super::model::{Project, ProjectCreateRequest};
use super::service::ProjectService;

#[tauri::command]
pub fn project_list(svc: State<'_, ProjectService>) -> AppResult<Vec<Project>> {
    svc.list()
}

#[tauri::command]
pub fn project_create<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, ProjectService>,
    req: ProjectCreateRequest,
) -> AppResult<Project> {
    let project = svc.create(req)?;
    emit_entity(&app, "project", Change::Created, project.clone());
    Ok(project)
}

#[tauri::command]
pub fn project_remove<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, ProjectService>,
    id: Uuid,
) -> AppResult<()> {
    svc.remove(id)?;
    emit_entity(&app, "project", Change::Deleted, serde_json::json!({ "id": id }));
    Ok(())
}

#[tauri::command]
pub fn project_detect_git(svc: State<'_, ProjectService>, path: String) -> AppResult<bool> {
    Ok(svc.detect_git(&path))
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p katrix projects::service`
Expected: PASS (6 tests). (Commands aren't unit-tested here; they're thin wrappers verified at C3.)

- [ ] **Step 4: Commit**
```bash
git add src-tauri/src/projects/
git commit -m "feat(projects): UUID-TEXT schema + list/get/create/remove + commands"
```

---

# TRACK B — Frontend framework (Angular)  *(parallel with Track A)*

## Task B1: Vitest setup

**Files:** Create `vitest.config.ts`; Modify `package.json`

- [ ] **Step 1: Add dev dependencies**

Run: `pnpm add -D vitest jsdom`
Expected: `vitest` + `jsdom` in `devDependencies`.

- [ ] **Step 2: Create config**

Create `vitest.config.ts`:
```ts
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'jsdom',
    globals: true,
    include: ['src/**/*.spec.ts'],
  },
});
```

- [ ] **Step 3: Add the script**

In `package.json` `scripts`, add:
```json
"test": "vitest run"
```

- [ ] **Step 4: Sanity test**

Create `src/app/orchestra/state/_sanity.spec.ts`:
```ts
import { describe, it, expect } from 'vitest';
describe('vitest', () => it('runs', () => expect(1 + 1).toBe(2)));
```
Run: `pnpm test`
Expected: PASS. Then delete `_sanity.spec.ts`.

- [ ] **Step 5: Commit**
```bash
git add package.json pnpm-lock.yaml vitest.config.ts
git commit -m "test: add vitest for pure-TS unit tests"
```

---

## Task B2: `EntityStore`  *(independent — parallelizable)*

**Files:** Create `src/app/orchestra/state/entity-store.ts`, `entity-store.spec.ts`

- [ ] **Step 1: Write the failing tests**

Create `src/app/orchestra/state/entity-store.spec.ts`:
```ts
import { describe, it, expect } from 'vitest';
import { createEntityStore } from './entity-store';

interface P { id: string; name: string; }

describe('createEntityStore', () => {
  it('setAll establishes entities + order', () => {
    const s = createEntityStore<P>(p => p.id);
    s.setAll([{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }]);
    expect(s.ids()).toEqual(['a', 'b']);
    expect(s.all().map(p => p.name)).toEqual(['A', 'B']);
  });

  it('upsert adds new and replaces existing without duplicating ids', () => {
    const s = createEntityStore<P>(p => p.id);
    s.upsert({ id: 'a', name: 'A' });
    s.upsert({ id: 'a', name: 'A2' });
    expect(s.ids()).toEqual(['a']);
    expect(s.all()[0].name).toBe('A2');
  });

  it('update patches a field', () => {
    const s = createEntityStore<P>(p => p.id);
    s.upsert({ id: 'a', name: 'A' });
    s.update('a', { name: 'Z' });
    expect(s.all()[0].name).toBe('Z');
  });

  it('remove drops entity and id', () => {
    const s = createEntityStore<P>(p => p.id);
    s.setAll([{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }]);
    s.remove('a');
    expect(s.ids()).toEqual(['b']);
  });

  it('active reflects setActive', () => {
    const s = createEntityStore<P>(p => p.id);
    s.setAll([{ id: 'a', name: 'A' }]);
    expect(s.active()).toBeNull();
    s.setActive('a');
    expect(s.active()?.name).toBe('A');
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `pnpm test`
Expected: FAIL — `entity-store.ts` not found.

- [ ] **Step 3: Implement**

Create `src/app/orchestra/state/entity-store.ts`:
```ts
import { computed, signal, Signal } from '@angular/core';

export interface EntityStore<T> {
  entities: Signal<Record<string, T>>;
  ids: Signal<string[]>;
  all: Signal<T[]>;
  loading: Signal<boolean>;
  active: Signal<T | null>;
  byId(id: string): Signal<T | undefined>;
  setAll(list: T[]): void;
  upsert(e: T): void;
  upsertMany(list: T[]): void;
  update(id: string, patch: Partial<T>): void;
  remove(id: string): void;
  setActive(id: string | null): void;
  setLoading(v: boolean): void;
  reset(): void;
}

export function createEntityStore<T>(idOf: (e: T) => string): EntityStore<T> {
  const entities = signal<Record<string, T>>({});
  const ids = signal<string[]>([]);
  const loading = signal(false);
  const activeId = signal<string | null>(null);

  const all = computed(() => ids().map(id => entities()[id]));
  const active = computed(() => {
    const id = activeId();
    return id ? entities()[id] ?? null : null;
  });

  return {
    entities, ids, all, loading, active,
    byId: (id: string) => computed(() => entities()[id]),
    setAll(list) {
      const map: Record<string, T> = {};
      const order: string[] = [];
      for (const e of list) { const id = idOf(e); map[id] = e; order.push(id); }
      entities.set(map); ids.set(order);
    },
    upsert(e) {
      const id = idOf(e);
      entities.update(m => ({ ...m, [id]: e }));
      ids.update(arr => (arr.includes(id) ? arr : [...arr, id]));
    },
    upsertMany(list) { for (const e of list) this.upsert(e); },
    update(id, patch) {
      entities.update(m => (m[id] ? { ...m, [id]: { ...m[id], ...patch } } : m));
    },
    remove(id) {
      entities.update(m => { const { [id]: _drop, ...rest } = m; return rest; });
      ids.update(arr => arr.filter(x => x !== id));
    },
    setActive(id) { activeId.set(id); },
    setLoading(v) { loading.set(v); },
    reset() { entities.set({}); ids.set([]); activeId.set(null); loading.set(false); },
  };
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `pnpm test`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**
```bash
git add src/app/orchestra/state/entity-store.ts src/app/orchestra/state/entity-store.spec.ts
git commit -m "feat(state): signal-based normalized EntityStore"
```

---

## Task B3: `Bridge` + Mock/Tauri implementations  *(independent — parallelizable)*

**Files:** Create `bridge.ts`, `tauri-bridge.ts`, `mock-bridge.ts`, `mock-bridge.spec.ts` under `src/app/orchestra/data-source/`

- [ ] **Step 1: Define the contract + catalog + token**

Create `src/app/orchestra/data-source/bridge.ts`:
```ts
import { InjectionToken } from '@angular/core';

export interface AppErrorShape { kind: string; message: string; }

export class BridgeError extends Error {
  constructor(public kind: string, message: string) {
    super(message);
    this.name = 'BridgeError';
  }
}

export interface Bridge {
  invoke<R>(command: string, payload?: Record<string, unknown>): Promise<R>;
  on<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

export const Commands = {
  ProjectList: 'project_list',
  ProjectCreate: 'project_create',
  ProjectRemove: 'project_remove',
  ProjectDetectGit: 'project_detect_git',
} as const;

export const Events = {
  ProjectCreated: 'project://created',
  ProjectUpdated: 'project://updated',
  ProjectDeleted: 'project://deleted',
} as const;

export const BRIDGE = new InjectionToken<Bridge>('BRIDGE');
```

- [ ] **Step 2: TauriBridge**

Create `src/app/orchestra/data-source/tauri-bridge.ts`:
```ts
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { AppErrorShape, Bridge, BridgeError } from './bridge';

export class TauriBridge implements Bridge {
  async invoke<R>(command: string, payload?: Record<string, unknown>): Promise<R> {
    try {
      return await invoke<R>(command, payload);
    } catch (e) {
      const err = e as AppErrorShape;
      if (err && typeof err === 'object' && 'kind' in err) {
        throw new BridgeError(err.kind, err.message);
      }
      throw new BridgeError('unknown', String(e));
    }
  }

  async on<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    return listen<T>(event, e => handler(e.payload));
  }
}
```

- [ ] **Step 3: Write the failing MockBridge test**

Create `src/app/orchestra/data-source/mock-bridge.spec.ts`:
```ts
import { describe, it, expect, vi } from 'vitest';
import { MockBridge } from './mock-bridge';
import { Commands, Events } from './bridge';

describe('MockBridge', () => {
  it('lists seeded projects', async () => {
    const b = new MockBridge();
    const list = await b.invoke<any[]>(Commands.ProjectList);
    expect(list.length).toBeGreaterThan(0);
    expect(list[0]).toHaveProperty('id');
  });

  it('create adds a project and emits created', async () => {
    const b = new MockBridge();
    const seen: any[] = [];
    await b.on(Events.ProjectCreated, p => seen.push(p));
    const created = await b.invoke<any>(Commands.ProjectCreate, {
      req: { name: 'new', path: '~/x', icon: 'box', color: '#fff', withGit: false },
    });
    expect(created.id).toBeTruthy();
    expect(seen).toHaveLength(1);
    const list = await b.invoke<any[]>(Commands.ProjectList);
    expect(list.find(p => p.id === created.id)).toBeTruthy();
  });

  it('remove deletes and emits deleted', async () => {
    const b = new MockBridge();
    const created = await b.invoke<any>(Commands.ProjectCreate, {
      req: { name: 'tmp', path: '~/y', icon: 'box', color: '#fff', withGit: false },
    });
    const seen: any[] = [];
    await b.on(Events.ProjectDeleted, p => seen.push(p));
    await b.invoke(Commands.ProjectRemove, { id: created.id });
    expect(seen[0].id).toBe(created.id);
  });
});
```

- [ ] **Step 4: Implement MockBridge**

Create `src/app/orchestra/data-source/mock-bridge.ts`:
```ts
import { Bridge, Commands, Events } from './bridge';

// Seed mirrors the frontend Project shape (extra fields like branches/files
// are kept here only for the browser/mock so the rest of the UI still works).
const SEED = [
  { id: 'p_pay', name: 'payments-service', path: '~/code/northwind/payments-service',
    icon: 'box', color: '#a855f7', hasGit: true, branch: 'main', head: 'a3f91c2',
    branches: ['main', 'develop', 'release/2.4', 'hotfix/refund-rounding'], files: [] },
  { id: 'p_web', name: 'web-dashboard', path: '~/code/northwind/web-dashboard',
    icon: 'globe', color: '#22d3ee', hasGit: true, branch: 'main', head: '7d10b4e',
    branches: ['main', 'develop', 'feat/new-settings'], files: [] },
  { id: 'p_infra', name: 'infra-terraform', path: '~/code/northwind/infra-terraform',
    icon: 'server', color: '#34e0a1', hasGit: true, branch: 'main', head: 'f02ce91',
    branches: ['main', 'staging'], files: [] },
];

export class MockBridge implements Bridge {
  private projects: any[] = SEED.map(p => ({ ...p }));
  private listeners = new Map<string, Set<(p: any) => void>>();

  async invoke<R>(command: string, payload: any = {}): Promise<R> {
    switch (command) {
      case Commands.ProjectList:
        return this.projects.slice() as R;
      case Commands.ProjectDetectGit:
        return (/(\/code\/|github|\.git)/i.test(payload.path) && payload.path.length > 4) as unknown as R;
      case Commands.ProjectCreate: {
        const r = payload.req;
        const detected = /(\/code\/|github|\.git)/i.test(r.path);
        const hasGit = r.withGit || detected;
        const p = {
          id: crypto.randomUUID(),
          name: r.name, path: r.path, icon: r.icon, color: r.color,
          hasGit, branch: hasGit ? 'main' : null, head: hasGit ? '0000000' : null,
          branches: ['main'], files: [],
        };
        this.projects.push(p);
        this.emit(Events.ProjectCreated, p);
        return p as R;
      }
      case Commands.ProjectRemove: {
        this.projects = this.projects.filter(p => p.id !== payload.id);
        this.emit(Events.ProjectDeleted, { id: payload.id });
        return undefined as R;
      }
      default:
        throw new Error(`MockBridge: unhandled command ${command}`);
    }
  }

  async on<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    const set = this.listeners.get(event) ?? new Set();
    set.add(handler as (p: any) => void);
    this.listeners.set(event, set);
    return () => set.delete(handler as (p: any) => void);
  }

  private emit(event: string, payload: any) {
    this.listeners.get(event)?.forEach(h => h(payload));
  }
}
```

- [ ] **Step 5: Run tests**

Run: `pnpm test`
Expected: PASS (MockBridge: 3 tests; EntityStore still green).

- [ ] **Step 6: Commit**
```bash
git add src/app/orchestra/data-source/
git commit -m "feat(data-source): Bridge contract + Tauri/Mock implementations"
```

---

## Task B4: `EntityFacade`

**Files:** Create `src/app/orchestra/state/entity-facade.ts`

- [ ] **Step 1: Implement (verified via ProjectsStore at C2)**

Create `src/app/orchestra/state/entity-facade.ts`:
```ts
import { Bridge } from '../data-source/bridge';
import { EntityStore } from './entity-store';

export interface FacadeConfig {
  listCommand: string;
  events: { created: string; updated?: string; deleted: string };
}

export interface EntityFacade {
  load(): Promise<void>;
  listen(): Promise<() => void>;
}

export function bindFacade<T extends { id: string }>(
  store: EntityStore<T>,
  bridge: Bridge,
  cfg: FacadeConfig,
): EntityFacade {
  return {
    async load() {
      store.setLoading(true);
      try {
        store.setAll(await bridge.invoke<T[]>(cfg.listCommand));
      } finally {
        store.setLoading(false);
      }
    },
    async listen() {
      const unsubs: Array<() => void> = [
        await bridge.on<T>(cfg.events.created, e => store.upsert(e)),
        await bridge.on<{ id: string }>(cfg.events.deleted, e => store.remove(e.id)),
      ];
      if (cfg.events.updated) {
        unsubs.push(await bridge.on<T>(cfg.events.updated, e => store.upsert(e)));
      }
      return () => unsubs.forEach(u => u());
    },
  };
}
```

- [ ] **Step 2: Commit**
```bash
git add src/app/orchestra/state/entity-facade.ts
git commit -m "feat(state): EntityFacade binding store <-> bridge"
```

---

# CONVERGENCE  *(requires Track A + Track B complete)*

## Task C1: Backend `lib.rs` wiring

**Files:** Modify `src-tauri/src/lib.rs`

- [ ] **Step 1: Rewrite `run()`**

Replace `src-tauri/src/lib.rs` with:
```rust
mod core;
mod git;
mod projects;

use crate::core::database::Database;
use crate::git::service::GitService;
use crate::projects::service::ProjectService;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(core::logger::plugin())
        .setup(|app| {
            let db = Database::get(app);
            let git = GitService::new();
            app.manage(ProjectService::new(db, git));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            projects::commands::project_list,
            projects::commands::project_create,
            projects::commands::project_remove,
            projects::commands::project_detect_git,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 2: Build + test the whole backend**

Run: `cd src-tauri && cargo build && cargo test`
Expected: compiles; all unit tests PASS (errors, events, git::service, projects::service).

- [ ] **Step 3: Commit**
```bash
git add src-tauri/src/lib.rs
git commit -m "feat: wire git + projects services and commands in setup"
```

---

## Task C2: Frontend Projects wiring

**Files:** Create `src/app/orchestra/stores/projects.store.ts`; Modify `app.config.ts`, `orchestra.store.ts`, `sidebar.component.ts`, `add-project-modal.component.ts`, `spawn-modal.component.ts`

- [ ] **Step 1: Provide the bridge**

In `src/app/app.config.ts`, add to the `providers` array:
```ts
import { BRIDGE } from './orchestra/data-source/bridge';
import { TauriBridge } from './orchestra/data-source/tauri-bridge';
import { MockBridge } from './orchestra/data-source/mock-bridge';

// ...inside providers:
{
  provide: BRIDGE,
  useFactory: () =>
    (window as unknown as { __TAURI__?: unknown }).__TAURI__ ? new TauriBridge() : new MockBridge(),
},
```

- [ ] **Step 2: Create `ProjectsStore`**

Create `src/app/orchestra/stores/projects.store.ts`:
```ts
import { inject, Injectable } from '@angular/core';
import { BRIDGE, Commands, Events } from '../data-source/bridge';
import { Project } from '../models';
import { bindFacade } from '../state/entity-facade';
import { createEntityStore } from '../state/entity-store';

@Injectable({ providedIn: 'root' })
export class ProjectsStore {
  private bridge = inject(BRIDGE);
  private store = createEntityStore<Project>(p => p.id);

  readonly all = this.store.all;
  readonly loading = this.store.loading;

  private facade = bindFacade(this.store, this.bridge, {
    listCommand: Commands.ProjectList,
    events: { created: Events.ProjectCreated, deleted: Events.ProjectDeleted },
  });

  constructor() {
    void this.init();
  }
  private async init() {
    await this.facade.listen();
    await this.facade.load();
  }

  byId(id: string): Project | undefined {
    return this.all().find(p => p.id === id);
  }
  async create(req: {
    name: string; path: string; icon: string; color: string; withGit: boolean;
  }): Promise<Project> {
    const p = await this.bridge.invoke<Project>(Commands.ProjectCreate, { req });
    this.store.upsert(p);
    return p;
  }
  async remove(id: string): Promise<void> {
    await this.bridge.invoke(Commands.ProjectRemove, { id });
    this.store.remove(id);
  }
  detectGit(path: string): Promise<boolean> {
    return this.bridge.invoke<boolean>(Commands.ProjectDetectGit, { path });
  }
}
```

- [ ] **Step 2b: Align the `Project` model**

In `src/app/orchestra/models.ts`, make the fields not guaranteed by the backend optional so backend rows type-check:
```ts
export interface Project {
  id: string;
  name: string;
  path: string;
  icon: string;
  color: string;
  hasGit?: boolean;
  org?: string;
  branch?: string;
  head?: string;
  repo?: string;
  branches?: string[];
  files?: string[];
}
```

- [ ] **Step 3: Delegate projects in `OrchestraStore`**

In `src/app/orchestra/orchestra.store.ts`:
- Inject the store: add `private projectsStore = inject(ProjectsStore);` (import it).
- Replace the `projects` signal with a delegate: `readonly projects = this.projectsStore.all;`
- Remove the `PROJECTS` seed usage for `projects` and the `projCount` id logic.
- Change `addProject(req)` body to:
```ts
addProject(req: { path: string; name: string; icon: string; color: string; gitInit: boolean }) {
  void this.projectsStore
    .create({ name: req.name, path: req.path, icon: req.icon, color: req.color, withGit: req.gitInit })
    .then(p => this.flash('added project ' + p.name))
    .catch((e: { kind?: string; message?: string }) =>
      this.flash(e?.kind === 'project' || e?.kind === 'notFound' ? e.message ?? 'failed' : 'add failed'));
  this.addingProject.set(false);
}
```
- Change `removeProject(id)` body to:
```ts
removeProject(id: string) {
  const p = this.projectOf(id);
  void this.projectsStore.remove(id).then(() => this.flash('removed project ' + (p ? p.name : id)));
  this.agents.update(prev => prev.filter(a => a.projectId !== id));
}
```
- `projectOf(id)` stays (`this.projects().find(...)`).

- [ ] **Step 4: Guard the spawn modal against missing branches**

In `src/app/orchestra/modals/spawn-modal.component.ts`, the branch `<select>` iterates `proj.branches`. Replace that loop's source with `(proj.branches ?? [proj.branch ?? 'main'])` and initialize `branch` from `this.project().branch ?? 'main'`. Concretely, change the `@for` in the template:
```html
@for (b of (proj.branches ?? [proj.branch ?? 'main']); track b) { <option [value]="b">{{ b }}</option> }
```
and the `branch` signal init:
```ts
readonly branch = signal<string>(this.project().branch ?? 'main');
```
and `setProject`:
```ts
setProject(id: string) {
  this.projectId.set(id);
  this.branch.set(this.project().branch ?? 'main');
}
```

- [ ] **Step 5: Wire Add Project detection to the real backend**

In `src/app/orchestra/modals/add-project-modal.component.ts`:
- Inject `ProjectsStore`: `private projects = inject(ProjectsStore);`
- Replace the regex-based `detectedGit` computed with a signal updated from the backend. Add `readonly detectedGit = signal(false);` and react to `dir` changes:
```ts
constructor() {
  effect(() => {
    const d = this.dir().trim();
    if (!d) { this.detectedGit.set(false); this.gitInit.set(true); return; }
    void this.projects.detectGit(d).then(found => {
      this.detectedGit.set(found);
      this.gitInit.set(!found);
    });
  });
}
```
(Remove the old `detectedGit` computed and its `gitInit` effect.) Submit still calls `store.addProject(...)` (unchanged) — `OrchestraStore.addProject` now routes to the backend.

- [ ] **Step 6: Type-check / build**

Run: `pnpm build`
Expected: Angular build succeeds (no type errors).

- [ ] **Step 7: Commit**
```bash
git add src/app
git commit -m "feat(frontend): wire Projects to backend via ProjectsStore + bridge"
```

---

## Task C3: End-to-end verification

**Files:** none (verification only)

- [ ] **Step 1: Backend green**

Run: `cd src-tauri && cargo test`
Expected: all tests PASS.

- [ ] **Step 2: Frontend unit + build green**

Run: `pnpm test && pnpm build`
Expected: Vitest PASS; Angular build succeeds.

- [ ] **Step 3: Browser smoke (MockBridge path)**

Run: `pnpm start` and open `http://localhost:1420`.
Expected: sidebar shows the 3 seeded projects; "Add project" creates a new one (appears live); removing a project (project context menu → Remove) drops it. Confirms the framework + components work without a backend.

- [ ] **Step 4: Real backend (TauriBridge path)**

Run: `pnpm tauri dev`
Expected: app window opens; sidebar lists projects from SQLite (empty on first run); Add Project (with "Run git init" on a fresh folder) creates a row + a `.git`; relaunch → the project is still there (persisted); duplicate path → error toast; remove → row gone. Check the log file / stdout for the `project_create` activity.

- [ ] **Step 5: Final commit (if any verification tweaks were needed)**
```bash
git add -A
git commit -m "chore: slice 1 verification fixes"
```

---

## Self-Review (completed by plan author)

- **Spec coverage:** framework (EntityStore B2 / Bridge B3 / EntityFacade B4 / conventions A2-A3-A6); Projects domain (model A5, service+commands A6, git A4, schema/ID A6, lib wiring C1); frontend wiring + mock fallback (C2); error handling ({kind,message} A2, toast C2 step 3); testing (cargo tests A2/A3/A4/A6, vitest B2/B3, E2E C3). All spec sections map to a task.
- **Placeholder scan:** no TBD/TODO; every code step has complete code; commands/events referenced (`project_list`, `project://created`, etc.) are defined in A6/A3/B3.
- **Type consistency:** Rust `Project`/`ProjectCreateRequest` (A5) used identically in A6 + commands; `GitService::{detect,init,head_info}` (A4) used in A6; TS `EntityStore` methods (B2) used by `bindFacade` (B4) and `ProjectsStore` (C2); `Commands`/`Events`/`BRIDGE` (B3) used in MockBridge (B3) and ProjectsStore (C2); command payload keys (`req`, `id`, `path`) consistent between commands (A6) and MockBridge/ProjectsStore.
- **Scope:** single slice (foundation + Projects); agents/git-ops/terminal/etc. explicitly deferred.
