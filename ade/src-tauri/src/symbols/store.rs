//! `app_data/symbols.db` — the symbol index's persistent side (M2). Its own
//! connection (WAL, never the app DB mutex: bulk writes must not stall the
//! agents/projects tables), two tables:
//!
//! - `symbol_cache(hash PK, lang, grammar_version, blob, created_at)` —
//!   content-addressed by blake3 of the file bytes, so identical files across
//!   worktrees share one row and a reopened project never re-parses. Pruned
//!   above `CACHE_MAX_BYTES` oldest-first.
//! - `index_files(root, path, hash, size, mtime, lang, PK(root, path))` —
//!   the stat fingerprint per indexed file: unchanged size+mtime → the stored
//!   hash is trusted without reading the file.
//!
//! M4 adds the library-sources side (`libsrc/`), same connection, sized for
//! a million declarations under 100 MB:
//! - `lib_sources(seq, id UNIQUE, kind, path, label, fingerprint, state, files,
//!   decls, size_bytes, indexed_at, project_id, project_name, artifacts,
//!   missing, skipped)` — one row per JDK `src.zip` (`jdk:<hash>`) or per
//!   project lockfile (`cargo:<projectId>` / `maven:<projectId>`).
//! - `lib_artifacts(id, src, name, kind, path, fingerprint, state, reason,
//!   files, decls)` — one row per crate dir / sources jar / src.zip under a
//!   source; the unit of incremental re-indexing.
//! - `lib_entries(id, artifact, path)` — every indexed file, so a declaration
//!   row carries a 1-3 byte entry id instead of a 40-byte path.
//! - `lib_names(id, name UNIQUE)` — every distinct name string ONCE: the
//!   container path (`java.util.Map`, `serde::de`), the simple name (`get`,
//!   `Entry`) and the kind (`method`). A million declarations share a few
//!   tens of thousands of them, so a row shrinks from ~90 to ~30 bytes.
//! - `lib_decls(entry, container, flags, name, kind, line)` — every
//!   declaration as five small integers: `container`/`name`/`kind` are
//!   `lib_names` ids (container `0` = none) and `flags` packs the separator
//!   that rebuilds the fully-qualified name (`.` / `$` / `::`) with the
//!   "has a semantic container" bit. Indexed by `name` (a fully-qualified
//!   lookup adds `container`) and `entry`; the language comes from the
//!   source kind. Written 2k rows per transaction.
//!
//! `PRAGMA user_version` carries [`LIB_SCHEMA`]: a database written by any
//! earlier library layout (the per-crate one, or the pre-interning tables)
//! has its `lib_*` tables dropped on open and rebuilt on demand — the
//! `symbol_cache` / `index_files` side is never touched.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

/// Prune threshold for `symbol_cache` (sum of blob bytes).
pub const CACHE_MAX_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndexedFile {
    pub hash: String,
    pub size: u64,
    pub mtime: i64,
    pub lang: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedBlob {
    pub lang: String,
    pub grammar_version: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct BlobRow {
    pub hash: String,
    pub lang: String,
    pub grammar_version: String,
    pub bytes: Vec<u8>,
}

/// One `lib_sources` row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LibSourceRow {
    /// `jdk:<hash>` / `cargo:<projectId>` / `maven:<projectId>`.
    pub id: String,
    /// `jdk | maven | cargo`.
    pub kind: String,
    /// The src.zip / the lockfile (`Cargo.lock`, `pom.xml`) the row follows.
    pub path: String,
    pub label: String,
    pub fingerprint: String,
    /// `pending | indexing | done | error | cancelled | removed`.
    pub state: String,
    pub files: u64,
    pub decls: u64,
    pub size_bytes: u64,
    pub indexed_at: i64,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    /// Artifacts the lockfile names (crates / jars; 1 for a JDK).
    pub artifacts: u64,
    /// Named but not on disk (`cargo fetch` / `mvn dependency:sources`).
    pub missing: u64,
    /// Over the perf guard (`lib_artifacts.reason` says why).
    pub skipped: u64,
}

/// One `lib_artifacts` row: a crate dir, a sources jar, or the src.zip.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LibArtifactRow {
    /// Row id (0 before the first upsert).
    pub id: i64,
    /// `serde-1.0.219` / `gson-2.11.0-sources.jar` / `src.zip`.
    pub name: String,
    /// `crate | jar | zip`.
    pub kind: String,
    /// Absolute path on disk (empty when missing).
    pub path: String,
    /// Change ⇒ re-index this artifact only.
    pub fingerprint: String,
    /// `pending | indexing | done | missing | skipped | error`.
    pub state: String,
    pub reason: Option<String>,
    pub files: u64,
    pub decls: u64,
}

/// One declaration as extracted (the store derives `container` and `lang`
/// on the way back out).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LibDecl {
    pub fq_name: String,
    pub simple_name: String,
    /// `class | interface | enum | record | annotation | method | field` (java);
    /// `fn | struct | enum | trait | type | mod | const | static | macro | union` (rust).
    pub kind: String,
    /// The enclosing type / owner: always `fq_name` minus its last segment.
    pub container: Option<String>,
    /// Zip entry name / crate-dir-relative path with forward slashes. On a
    /// hit the artifact prefix is added (`<crate>/src/lib.rs`,
    /// `<jar>!/com/x/Y.java`); a JDK entry is the bare zip path.
    pub entry: String,
    /// 0-based.
    pub line: u32,
    pub lang: String,
}

/// A declaration hit joined with its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibDeclHit {
    pub source_id: String,
    pub source_kind: String,
    pub source_label: String,
    /// The artifact the entry lives in (`serde-1.0.219`, `gson-2.11.0-sources.jar`).
    pub artifact: String,
    pub decl: LibDecl,
}

/// Rows per `lib_decls` transaction.
pub const LIB_BATCH: usize = 2000;
/// Library-table layout stamped in `PRAGMA user_version`; a mismatch drops
/// and rebuilds the `lib_*` tables.
pub const LIB_SCHEMA: i32 = 2;
/// Name→id memo entries kept in memory (a few MB; cleared wholesale above).
const NAME_CACHE_MAX: usize = 300_000;

/// `flags` bits of a `lib_decls` row: the separator that rebuilds the
/// fully-qualified name, plus the "has a semantic container" bit.
const SEP_NONE: u8 = 0;
const SEP_DOT: u8 = 1;
const SEP_DOLLAR: u8 = 2;
const SEP_COLONS: u8 = 3;
const NESTED_BIT: u8 = 1 << 2;

fn sep_str(code: u8) -> &'static str {
    match code {
        SEP_DOT => ".",
        SEP_DOLLAR => "$",
        SEP_COLONS => "::",
        _ => "",
    }
}

pub struct SymbolStore {
    conn: Mutex<Connection>,
    /// An earlier library layout was dropped on open — a `VACUUM` gives the
    /// space back (the caller runs it off the startup path).
    needs_vacuum: AtomicBool,
    /// `lib_names` memo, so a batch does not re-query the ids it just wrote.
    names: Mutex<HashMap<String, i64>>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `(container path, separator code, simple name)` of a fully-qualified name:
/// `java.util.Map$Entry` → `(Some("java.util.Map"), $, "Entry")`,
/// `serde::de::Deserialize` → `(Some("serde::de"), ::, "Deserialize")`,
/// `Top` → `(None, -, "Top")`. The three parts rebuild the name exactly.
pub fn split_fq(fq: &str) -> (Option<&str>, u8, &str) {
    let rs = fq.rfind("::");
    let jv = fq.rfind(['.', '$']);
    let (cut, sep, width) = match (rs, jv) {
        (Some(a), Some(b)) if a > b => (a, SEP_COLONS, 2),
        (Some(_), Some(b)) | (None, Some(b)) => {
            (b, if fq.as_bytes()[b] == b'$' { SEP_DOLLAR } else { SEP_DOT }, 1)
        }
        (Some(a), None) => (a, SEP_COLONS, 2),
        (None, None) => return (None, SEP_NONE, fq),
    };
    if cut == 0 || cut + width >= fq.len() {
        return (None, SEP_NONE, fq);
    }
    (Some(&fq[..cut]), sep, &fq[cut + width..])
}

/// `fq_name` minus its last segment (`java.util.Map$Entry` → `java.util.Map`,
/// `serde::de::Deserialize` → `serde::de`).
pub fn container_of(fq: &str) -> Option<String> {
    split_fq(fq).0.map(str::to_string)
}

/// Last segment of a fully-qualified name — the `lib_names` lookup key.
pub fn simple_of(fq: &str) -> &str {
    split_fq(fq).2
}

fn lang_of_kind(kind: &str) -> &'static str {
    if kind == "cargo" {
        "rust"
    } else {
        "java"
    }
}

/// The hit's `entry`: the artifact prefix + the file path (JDK: bare).
pub fn entry_of(source_kind: &str, artifact: &str, path: &str) -> String {
    match source_kind {
        "cargo" => format!("{artifact}/{path}"),
        "maven" => format!("{artifact}!/{path}"),
        _ => path.to_string(),
    }
}

impl SymbolStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");
        let _ = conn.pragma_update(None, "cache_size", -8000);
        let legacy = Self::init(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            needs_vacuum: AtomicBool::new(legacy),
            names: Mutex::new(HashMap::new()),
        })
    }

    pub fn open_in_memory() -> Self {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        Self::init(&conn).expect("schema");
        Self {
            conn: Mutex::new(conn),
            needs_vacuum: AtomicBool::new(false),
            names: Mutex::new(HashMap::new()),
        }
    }

    /// `true` when library tables of an earlier layout were dropped.
    fn init(conn: &Connection) -> rusqlite::Result<bool> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS symbol_cache (
                hash TEXT PRIMARY KEY,
                lang TEXT NOT NULL,
                grammar_version TEXT NOT NULL,
                blob BLOB NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS symbol_cache_created ON symbol_cache(created_at);
            CREATE TABLE IF NOT EXISTS index_files (
                root TEXT NOT NULL,
                path TEXT NOT NULL,
                hash TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                lang TEXT NOT NULL,
                PRIMARY KEY (root, path)
            );",
        )?;
        // any earlier library layout (the per-crate one keyed by a text id, or
        // the pre-interning decl rows) is dropped wholesale and rebuilt on
        // demand — indexing a source again is cheaper than migrating it
        let has_lib: bool = conn
            .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name IN ('lib_sources', 'lib_decls')")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);
        let version: i32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap_or(0);
        let legacy = has_lib && version != LIB_SCHEMA;
        if legacy {
            conn.execute_batch(
                "DROP TABLE IF EXISTS lib_decls;
                 DROP TABLE IF EXISTS lib_names;
                 DROP TABLE IF EXISTS lib_entries;
                 DROP TABLE IF EXISTS lib_artifacts;
                 DROP TABLE IF EXISTS lib_sources;",
            )?;
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS lib_sources (
                seq INTEGER PRIMARY KEY,
                id TEXT NOT NULL UNIQUE,
                kind TEXT NOT NULL,
                path TEXT NOT NULL,
                label TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                state TEXT NOT NULL,
                files INTEGER NOT NULL DEFAULT 0,
                decls INTEGER NOT NULL DEFAULT 0,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                indexed_at INTEGER NOT NULL DEFAULT 0,
                project_id TEXT,
                project_name TEXT,
                artifacts INTEGER NOT NULL DEFAULT 0,
                missing INTEGER NOT NULL DEFAULT 0,
                skipped INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS lib_artifacts (
                id INTEGER PRIMARY KEY,
                src INTEGER NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                path TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                state TEXT NOT NULL,
                reason TEXT,
                files INTEGER NOT NULL DEFAULT 0,
                decls INTEGER NOT NULL DEFAULT 0,
                UNIQUE (src, name)
            );
            CREATE TABLE IF NOT EXISTS lib_entries (
                id INTEGER PRIMARY KEY,
                artifact INTEGER NOT NULL,
                path TEXT NOT NULL,
                UNIQUE (artifact, path)
            );
            CREATE TABLE IF NOT EXISTS lib_names (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE
            );
            CREATE TABLE IF NOT EXISTS lib_decls (
                entry INTEGER NOT NULL,
                container INTEGER NOT NULL DEFAULT 0,
                flags INTEGER NOT NULL DEFAULT 0,
                name INTEGER NOT NULL,
                kind INTEGER NOT NULL,
                line INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS lib_decls_name ON lib_decls(name);
            CREATE INDEX IF NOT EXISTS lib_decls_entry ON lib_decls(entry);",
        )?;
        conn.pragma_update(None, "user_version", LIB_SCHEMA)?;
        Ok(legacy)
    }

    /// The `lib_names` id of `name`, or `None` when it was never written.
    fn name_id(&self, name: &str) -> Option<i64> {
        if let Some(id) = self.names.lock().unwrap().get(name) {
            return Some(*id);
        }
        let c = self.conn.lock().unwrap();
        let id: i64 = c
            .query_row("SELECT id FROM lib_names WHERE name = ?1", [name], |r| r.get(0))
            .optional()
            .ok()
            .flatten()?;
        drop(c);
        let mut names = self.names.lock().unwrap();
        if names.len() >= NAME_CACHE_MAX {
            names.clear();
        }
        names.insert(name.to_string(), id);
        Some(id)
    }

    /// `VACUUM` once when the legacy layout was dropped on open (the file
    /// keeps its size otherwise). Cheap no-op every other time.
    pub fn vacuum_if_needed(&self) -> bool {
        if !self.needs_vacuum.swap(false, Ordering::SeqCst) {
            return false;
        }
        let c = self.conn.lock().unwrap();
        c.execute_batch("VACUUM").is_ok()
    }

    // ---- library sources (M4) ------------------------------------------------

    const SOURCE_COLS: &'static str = "id, kind, path, label, fingerprint, state, files, decls, size_bytes, indexed_at, project_id, project_name, artifacts, missing, skipped";

    fn row_to_source(r: &rusqlite::Row<'_>) -> rusqlite::Result<LibSourceRow> {
        Ok(LibSourceRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            path: r.get(2)?,
            label: r.get(3)?,
            fingerprint: r.get(4)?,
            state: r.get(5)?,
            files: r.get::<_, i64>(6)?.max(0) as u64,
            decls: r.get::<_, i64>(7)?.max(0) as u64,
            size_bytes: r.get::<_, i64>(8)?.max(0) as u64,
            indexed_at: r.get(9)?,
            project_id: r.get(10)?,
            project_name: r.get(11)?,
            artifacts: r.get::<_, i64>(12)?.max(0) as u64,
            missing: r.get::<_, i64>(13)?.max(0) as u64,
            skipped: r.get::<_, i64>(14)?.max(0) as u64,
        })
    }

    pub fn lib_sources(&self) -> Vec<LibSourceRow> {
        let c = self.conn.lock().unwrap();
        let Ok(mut st) = c.prepare(&format!(
            "SELECT {} FROM lib_sources ORDER BY kind, label",
            Self::SOURCE_COLS
        )) else {
            return Vec::new();
        };
        st.query_map([], Self::row_to_source)
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn lib_source(&self, id: &str) -> Option<LibSourceRow> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            &format!("SELECT {} FROM lib_sources WHERE id = ?1", Self::SOURCE_COLS),
            [id],
            Self::row_to_source,
        )
        .optional()
        .ok()
        .flatten()
    }

    fn seq_of(c: &Connection, id: &str) -> rusqlite::Result<Option<i64>> {
        c.query_row("SELECT seq FROM lib_sources WHERE id = ?1", [id], |r| r.get(0))
            .optional()
    }

    /// Insert or replace the source row (state/counters included).
    pub fn lib_upsert_source(&self, s: &LibSourceRow) -> rusqlite::Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO lib_sources (id, kind, path, label, fingerprint, state, files, decls, size_bytes, indexed_at,
               project_id, project_name, artifacts, missing, skipped)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, path = excluded.path,
               label = excluded.label, fingerprint = excluded.fingerprint, state = excluded.state,
               files = excluded.files, decls = excluded.decls, size_bytes = excluded.size_bytes,
               indexed_at = excluded.indexed_at, project_id = excluded.project_id,
               project_name = excluded.project_name, artifacts = excluded.artifacts,
               missing = excluded.missing, skipped = excluded.skipped",
            params![
                s.id,
                s.kind,
                s.path,
                s.label,
                s.fingerprint,
                s.state,
                s.files as i64,
                s.decls as i64,
                s.size_bytes as i64,
                s.indexed_at,
                s.project_id,
                s.project_name,
                s.artifacts as i64,
                s.missing as i64,
                s.skipped as i64,
            ],
        )?;
        Ok(())
    }

    /// Update the lifecycle columns of a source.
    pub fn lib_set_state(
        &self,
        id: &str,
        state: &str,
        files: u64,
        decls: u64,
        size_bytes: u64,
    ) -> rusqlite::Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE lib_sources SET state = ?2, files = ?3, decls = ?4, size_bytes = ?5, indexed_at = ?6
             WHERE id = ?1",
            params![id, state, files as i64, decls as i64, size_bytes as i64, now()],
        )?;
        Ok(())
    }

    /// Update the artifact counters of a source.
    pub fn lib_set_counts(&self, id: &str, artifacts: u64, missing: u64, skipped: u64) -> rusqlite::Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE lib_sources SET artifacts = ?2, missing = ?3, skipped = ?4 WHERE id = ?1",
            params![id, artifacts as i64, missing as i64, skipped as i64],
        )?;
        Ok(())
    }

    fn delete_artifact_rows(tx: &Connection, artifact_id: i64, keep_row: bool) -> rusqlite::Result<()> {
        tx.execute(
            "DELETE FROM lib_decls WHERE entry IN (SELECT id FROM lib_entries WHERE artifact = ?1)",
            [artifact_id],
        )?;
        tx.execute("DELETE FROM lib_entries WHERE artifact = ?1", [artifact_id])?;
        if !keep_row {
            tx.execute("DELETE FROM lib_artifacts WHERE id = ?1", [artifact_id])?;
        }
        Ok(())
    }

    fn artifact_ids(c: &Connection, seq: i64) -> rusqlite::Result<Vec<i64>> {
        let mut st = c.prepare("SELECT id FROM lib_artifacts WHERE src = ?1")?;
        let ids = st.query_map([seq], |r| r.get::<_, i64>(0))?.flatten().collect();
        Ok(ids)
    }

    /// Drop every artifact of a source with its entries + declarations (the
    /// source row stays).
    pub fn lib_clear_decls(&self, source_id: &str) -> rusqlite::Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        if let Some(seq) = Self::seq_of(&tx, source_id)? {
            for a in Self::artifact_ids(&tx, seq)? {
                Self::delete_artifact_rows(&tx, a, false)?;
            }
        }
        tx.commit()
    }

    /// Drop the source row, its artifacts and their declarations.
    pub fn lib_delete_source(&self, source_id: &str) -> rusqlite::Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        if let Some(seq) = Self::seq_of(&tx, source_id)? {
            for a in Self::artifact_ids(&tx, seq)? {
                Self::delete_artifact_rows(&tx, a, false)?;
            }
            tx.execute("DELETE FROM lib_sources WHERE seq = ?1", [seq])?;
        }
        tx.commit()
    }

    // ---- artifacts ----

    const ARTIFACT_COLS: &'static str = "id, name, kind, path, fingerprint, state, reason, files, decls";

    fn row_to_artifact(r: &rusqlite::Row<'_>) -> rusqlite::Result<LibArtifactRow> {
        Ok(LibArtifactRow {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            path: r.get(3)?,
            fingerprint: r.get(4)?,
            state: r.get(5)?,
            reason: r.get(6)?,
            files: r.get::<_, i64>(7)?.max(0) as u64,
            decls: r.get::<_, i64>(8)?.max(0) as u64,
        })
    }

    /// Every artifact of a source, by name.
    pub fn lib_artifacts(&self, source_id: &str) -> Vec<LibArtifactRow> {
        let c = self.conn.lock().unwrap();
        let Ok(Some(seq)) = Self::seq_of(&c, source_id) else {
            return Vec::new();
        };
        let Ok(mut st) = c.prepare(&format!(
            "SELECT {} FROM lib_artifacts WHERE src = ?1 ORDER BY name",
            Self::ARTIFACT_COLS
        )) else {
            return Vec::new();
        };
        st.query_map([seq], Self::row_to_artifact)
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// One artifact of a source by name.
    pub fn lib_artifact(&self, source_id: &str, name: &str) -> Option<LibArtifactRow> {
        let c = self.conn.lock().unwrap();
        let seq = Self::seq_of(&c, source_id).ok().flatten()?;
        c.query_row(
            &format!("SELECT {} FROM lib_artifacts WHERE src = ?1 AND name = ?2", Self::ARTIFACT_COLS),
            params![seq, name],
            Self::row_to_artifact,
        )
        .optional()
        .ok()
        .flatten()
    }

    /// Insert or update an artifact row (matched by `(source, name)`);
    /// returns its id. Declarations are untouched.
    pub fn lib_upsert_artifact(&self, source_id: &str, a: &LibArtifactRow) -> rusqlite::Result<i64> {
        let c = self.conn.lock().unwrap();
        let seq = Self::seq_of(&c, source_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        c.execute(
            "INSERT INTO lib_artifacts (src, name, kind, path, fingerprint, state, reason, files, decls)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(src, name) DO UPDATE SET kind = excluded.kind, path = excluded.path,
               fingerprint = excluded.fingerprint, state = excluded.state, reason = excluded.reason,
               files = excluded.files, decls = excluded.decls",
            params![
                seq,
                a.name,
                a.kind,
                a.path,
                a.fingerprint,
                a.state,
                a.reason,
                a.files as i64,
                a.decls as i64
            ],
        )?;
        c.query_row(
            "SELECT id FROM lib_artifacts WHERE src = ?1 AND name = ?2",
            params![seq, a.name],
            |r| r.get(0),
        )
    }

    /// Update the lifecycle columns of one artifact.
    pub fn lib_set_artifact(
        &self,
        artifact_id: i64,
        state: &str,
        reason: Option<&str>,
        files: u64,
        decls: u64,
    ) -> rusqlite::Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE lib_artifacts SET state = ?2, reason = ?3, files = ?4, decls = ?5 WHERE id = ?1",
            params![artifact_id, state, reason, files as i64, decls as i64],
        )?;
        Ok(())
    }

    /// Drop one artifact's entries + declarations (its row stays).
    pub fn lib_clear_artifact(&self, artifact_id: i64) -> rusqlite::Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        Self::delete_artifact_rows(&tx, artifact_id, true)?;
        tx.commit()
    }

    /// Drop these artifacts of a source with everything under them.
    pub fn lib_delete_artifacts(&self, source_id: &str, names: &[String]) -> rusqlite::Result<()> {
        if names.is_empty() {
            return Ok(());
        }
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        if let Some(seq) = Self::seq_of(&tx, source_id)? {
            for n in names {
                let id: Option<i64> = tx
                    .query_row(
                        "SELECT id FROM lib_artifacts WHERE src = ?1 AND name = ?2",
                        params![seq, n],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(id) = id {
                    Self::delete_artifact_rows(&tx, id, false)?;
                }
            }
        }
        tx.commit()
    }

    /// One transaction of declaration rows under an artifact; entry paths go
    /// to `lib_entries`, every name string to `lib_names` (memoised, so a
    /// batch writes each distinct name once and reuses it afterwards).
    pub fn lib_insert_decls(&self, artifact_id: i64, decls: &[LibDecl]) -> rusqlite::Result<()> {
        if decls.is_empty() {
            return Ok(());
        }
        let mut c = self.conn.lock().unwrap();
        let mut names = self.names.lock().unwrap();
        let result = (|| -> rusqlite::Result<()> {
            let tx = c.transaction()?;
            {
                let mut entries: HashMap<&str, i64> = HashMap::new();
                let mut ins_entry = tx.prepare_cached(
                    "INSERT OR IGNORE INTO lib_entries (artifact, path) VALUES (?1, ?2)",
                )?;
                let mut sel_entry =
                    tx.prepare_cached("SELECT id FROM lib_entries WHERE artifact = ?1 AND path = ?2")?;
                let mut ins_name =
                    tx.prepare_cached("INSERT OR IGNORE INTO lib_names (name) VALUES (?1)")?;
                let mut sel_name = tx.prepare_cached("SELECT id FROM lib_names WHERE name = ?1")?;
                let mut ins = tx.prepare_cached(
                    "INSERT INTO lib_decls (entry, container, flags, name, kind, line)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?;
                let mut intern = |s: &str, names: &mut HashMap<String, i64>| -> rusqlite::Result<i64> {
                    if let Some(id) = names.get(s) {
                        return Ok(*id);
                    }
                    ins_name.execute([s])?;
                    let id: i64 = sel_name.query_row([s], |r| r.get(0))?;
                    if names.len() >= NAME_CACHE_MAX {
                        names.clear();
                    }
                    names.insert(s.to_string(), id);
                    Ok(id)
                };
                for d in decls {
                    let entry = match entries.get(d.entry.as_str()) {
                        Some(id) => *id,
                        None => {
                            ins_entry.execute(params![artifact_id, d.entry])?;
                            let id: i64 = sel_entry.query_row(params![artifact_id, d.entry], |r| r.get(0))?;
                            entries.insert(d.entry.as_str(), id);
                            id
                        }
                    };
                    // the PATH prefix always goes in (it rebuilds the fq name);
                    // the semantic container is the extra `nested` bit
                    let (prefix, sep, simple) = split_fq(&d.fq_name);
                    let container = match prefix {
                        Some(p) => intern(p, &mut names)?,
                        None => 0,
                    };
                    let flags = sep | if d.container.is_some() { NESTED_BIT } else { 0 };
                    let name = intern(simple, &mut names)?;
                    let kind = intern(&d.kind, &mut names)?;
                    ins.execute(params![entry, container, flags as i64, name, kind, d.line as i64])?;
                }
            }
            tx.commit()
        })();
        if result.is_err() {
            // a rolled-back batch takes its lib_names rows with it
            names.clear();
        }
        result
    }

    const HIT_SQL: &'static str =
        "SELECT s.id, s.kind, s.label, a.name, e.path, cn.name, d.flags, sn.name, kn.name, d.line
         FROM lib_decls d
         JOIN lib_entries e ON e.id = d.entry
         JOIN lib_artifacts a ON a.id = e.artifact
         JOIN lib_sources s ON s.seq = a.src
         JOIN lib_names sn ON sn.id = d.name
         JOIN lib_names kn ON kn.id = d.kind
         LEFT JOIN lib_names cn ON cn.id = d.container";

    fn row_to_hit(r: &rusqlite::Row<'_>) -> rusqlite::Result<LibDeclHit> {
        let source_kind: String = r.get(1)?;
        let artifact: String = r.get(3)?;
        let path: String = r.get(4)?;
        let prefix: Option<String> = r.get(5)?;
        let flags: i64 = r.get(6)?;
        let simple_name: String = r.get(7)?;
        let fq_name = match &prefix {
            Some(p) => format!("{p}{}{simple_name}", sep_str(flags as u8 & 0b11)),
            None => simple_name.clone(),
        };
        Ok(LibDeclHit {
            source_id: r.get(0)?,
            source_label: r.get(2)?,
            decl: LibDecl {
                container: prefix.filter(|_| flags as u8 & NESTED_BIT != 0),
                fq_name,
                simple_name,
                kind: r.get(8)?,
                entry: entry_of(&source_kind, &artifact, &path),
                line: r.get::<_, i64>(9)?.max(0) as u32,
                lang: lang_of_kind(&source_kind).into(),
            },
            artifact,
            source_kind,
        })
    }

    /// `AND s.id IN (?k, …)` for a source filter (`None` = every source).
    fn source_clause(sources: Option<&[String]>, first_param: usize) -> String {
        match sources {
            None => String::new(),
            Some(ids) if ids.is_empty() => " AND 0".into(),
            Some(ids) => {
                let ph: Vec<String> = (0..ids.len()).map(|i| format!("?{}", first_param + i)).collect();
                format!(" AND s.id IN ({})", ph.join(", "))
            }
        }
    }

    fn hits(
        &self,
        where_sql: &str,
        head: &[&dyn rusqlite::ToSql],
        sources: Option<&[String]>,
        limit: usize,
    ) -> Vec<LibDeclHit> {
        let c = self.conn.lock().unwrap();
        let clause = Self::source_clause(sources, head.len() + 1);
        let limit_param = head.len() + 1 + sources.map(|s| s.len()).unwrap_or(0);
        let sql = format!(
            "{} WHERE {where_sql} AND s.state = 'done'{clause} LIMIT ?{limit_param}",
            Self::HIT_SQL
        );
        let Ok(mut st) = c.prepare_cached(&sql) else {
            return Vec::new();
        };
        let limit = limit as i64;
        let mut params: Vec<&dyn rusqlite::ToSql> = head.to_vec();
        if let Some(ids) = sources {
            for id in ids {
                params.push(id);
            }
        }
        params.push(&limit);
        st.query_map(params_from_iter(params), Self::row_to_hit)
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Declarations with exactly this fully-qualified name, in `sources`
    /// (`None` = any). Both halves are resolved through `lib_names` first —
    /// a name nothing ever wrote cannot have rows.
    pub fn lib_lookup_fq(&self, fq_name: &str, sources: Option<&[String]>, limit: usize) -> Vec<LibDeclHit> {
        let (prefix, _, simple) = split_fq(fq_name);
        let Some(name) = self.name_id(simple) else {
            return Vec::new();
        };
        let container = match prefix {
            Some(p) => match self.name_id(p) {
                Some(id) => id,
                None => return Vec::new(),
            },
            None => 0,
        };
        self.hits("d.name = ?1 AND d.container = ?2", &[&name, &container], sources, limit)
    }

    /// Declarations named `simple_name` in `lang`, in `sources` (`None` = any).
    pub fn lib_lookup_simple(
        &self,
        simple_name: &str,
        lang: &str,
        sources: Option<&[String]>,
        limit: usize,
    ) -> Vec<LibDeclHit> {
        let Some(name) = self.name_id(simple_name) else {
            return Vec::new();
        };
        let kinds = if lang == "rust" { "s.kind = 'cargo'" } else { "s.kind IN ('jdk', 'maven')" };
        self.hits(&format!("d.name = ?1 AND {kinds}"), &[&name], sources, limit)
    }

    #[cfg(test)]
    /// Declaration count of one source.
    pub fn lib_decl_count(&self, source_id: &str) -> u64 {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT COUNT(*) FROM lib_decls d JOIN lib_entries e ON e.id = d.entry
             JOIN lib_artifacts a ON a.id = e.artifact JOIN lib_sources s ON s.seq = a.src
             WHERE s.id = ?1",
            [source_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n.max(0) as u64)
        .unwrap_or(0)
    }

    #[cfg(test)]
    /// Database size (page_count × page_size).
    pub fn db_bytes(&self) -> u64 {
        let c = self.conn.lock().unwrap();
        let pages: i64 = c.pragma_query_value(None, "page_count", |r| r.get(0)).unwrap_or(0);
        let size: i64 = c.pragma_query_value(None, "page_size", |r| r.get(0)).unwrap_or(0);
        (pages.max(0) as u64) * (size.max(0) as u64)
    }

    pub fn get_blob(&self, hash: &str) -> Option<CachedBlob> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT lang, grammar_version, blob FROM symbol_cache WHERE hash = ?1",
            [hash],
            |r| {
                Ok(CachedBlob {
                    lang: r.get(0)?,
                    grammar_version: r.get(1)?,
                    bytes: r.get(2)?,
                })
            },
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn index_files(&self, root: &str) -> HashMap<String, IndexedFile> {
        let c = self.conn.lock().unwrap();
        let mut out = HashMap::new();
        let Ok(mut st) =
            c.prepare("SELECT path, hash, size, mtime, lang FROM index_files WHERE root = ?1")
        else {
            return out;
        };
        let rows = st.query_map([root], |r| {
            Ok((
                r.get::<_, String>(0)?,
                IndexedFile {
                    hash: r.get(1)?,
                    size: r.get::<_, i64>(2)? as u64,
                    mtime: r.get(3)?,
                    lang: r.get(4)?,
                },
            ))
        });
        if let Ok(rows) = rows {
            for (p, f) in rows.flatten() {
                out.insert(p, f);
            }
        }
        out
    }

    /// One transaction: new cache blobs + fingerprint upserts for `root`.
    pub fn write_batch(
        &self,
        root: &str,
        files: &[(String, IndexedFile)],
        blobs: &[BlobRow],
    ) -> rusqlite::Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        {
            let at = now();
            let mut ins = tx.prepare_cached(
                "INSERT INTO symbol_cache (hash, lang, grammar_version, blob, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(hash) DO UPDATE SET lang = excluded.lang,
                   grammar_version = excluded.grammar_version, blob = excluded.blob,
                   created_at = excluded.created_at",
            )?;
            for b in blobs {
                ins.execute(params![b.hash, b.lang, b.grammar_version, b.bytes, at])?;
            }
            let mut up = tx.prepare_cached(
                "INSERT INTO index_files (root, path, hash, size, mtime, lang)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(root, path) DO UPDATE SET hash = excluded.hash,
                   size = excluded.size, mtime = excluded.mtime, lang = excluded.lang",
            )?;
            for (p, f) in files {
                up.execute(params![root, p, f.hash, f.size as i64, f.mtime, f.lang])?;
            }
        }
        tx.commit()
    }

    pub fn delete_files(&self, root: &str, paths: &[String]) -> rusqlite::Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        {
            let mut del =
                tx.prepare_cached("DELETE FROM index_files WHERE root = ?1 AND path = ?2")?;
            for p in paths {
                del.execute(params![root, p])?;
            }
        }
        tx.commit()
    }

    pub fn clear_root(&self, root: &str) -> rusqlite::Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM index_files WHERE root = ?1", [root])?;
        Ok(())
    }

    pub fn cache_bytes(&self) -> u64 {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT COALESCE(SUM(LENGTH(blob)), 0) FROM symbol_cache",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n.max(0) as u64)
        .unwrap_or(0)
    }

    #[cfg(test)]
    pub fn cache_rows(&self) -> usize {
        let c = self.conn.lock().unwrap();
        c.query_row("SELECT COUNT(*) FROM symbol_cache", [], |r| r.get::<_, i64>(0))
            .map(|n| n.max(0) as usize)
            .unwrap_or(0)
    }

    /// Drop the oldest blobs until the cache is at most `max_bytes`. Returns
    /// the number of rows removed.
    pub fn prune(&self, max_bytes: u64) -> usize {
        let mut removed = 0usize;
        loop {
            if self.cache_bytes() <= max_bytes {
                return removed;
            }
            let c = self.conn.lock().unwrap();
            let n = c
                .execute(
                    "DELETE FROM symbol_cache WHERE hash IN
                       (SELECT hash FROM symbol_cache ORDER BY created_at ASC, hash ASC LIMIT 256)",
                    [],
                )
                .unwrap_or(0);
            if n == 0 {
                return removed;
            }
            removed += n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::tags::FileSymbols;

    fn blob(hash: &str, bytes: Vec<u8>) -> BlobRow {
        BlobRow {
            hash: hash.into(),
            lang: "java".into(),
            grammar_version: "1".into(),
            bytes,
        }
    }

    #[test]
    fn blob_and_fingerprint_round_trip() {
        let s = SymbolStore::open_in_memory();
        let fs = FileSymbols {
            imports: vec!["a.b".into()],
            package: Some("p".into()),
            ..Default::default()
        };
        let f = IndexedFile {
            hash: "h1".into(),
            size: 10,
            mtime: 20,
            lang: "java".into(),
        };
        s.write_batch("/r", &[("a/B.java".into(), f.clone())], &[blob("h1", fs.encode())])
            .unwrap();
        let got = s.get_blob("h1").unwrap();
        assert_eq!(got.lang, "java");
        assert_eq!(got.grammar_version, "1");
        assert_eq!(FileSymbols::decode(&got.bytes).unwrap(), fs);
        assert!(s.get_blob("nope").is_none());
        let files = s.index_files("/r");
        assert_eq!(files.get("a/B.java"), Some(&f));
        assert!(s.index_files("/other").is_empty());
        // upsert replaces the fingerprint
        let f2 = IndexedFile {
            hash: "h2".into(),
            ..f.clone()
        };
        s.write_batch("/r", &[("a/B.java".into(), f2.clone())], &[]).unwrap();
        assert_eq!(s.index_files("/r").get("a/B.java"), Some(&f2));
        s.delete_files("/r", &["a/B.java".into()]).unwrap();
        assert!(s.index_files("/r").is_empty());
        assert!(s.get_blob("h1").is_some(), "blobs are content-addressed, not per root");
        s.write_batch("/r", &[("x".into(), f.clone())], &[]).unwrap();
        s.clear_root("/r").unwrap();
        assert!(s.index_files("/r").is_empty());
    }

    #[test]
    fn prune_drops_oldest_first() {
        let s = SymbolStore::open_in_memory();
        // three 1 KB blobs with distinct created_at
        for (i, h) in ["old", "mid", "new"].iter().enumerate() {
            s.write_batch("/r", &[], &[blob(h, vec![b'x'; 1024])]).unwrap();
            let c = s.conn.lock().unwrap();
            c.execute(
                "UPDATE symbol_cache SET created_at = ?1 WHERE hash = ?2",
                params![100 + i as i64, h],
            )
            .unwrap();
        }
        assert_eq!(s.cache_bytes(), 3 * 1024);
        assert_eq!(s.cache_rows(), 3);
        assert_eq!(s.prune(10 * 1024), 0, "under the cap → nothing pruned");
        // LIMIT 256 per round: all three go in one round when over the cap
        assert_eq!(s.prune(1024), 3);
        assert!(s.get_blob("new").is_none());
        // ordering check with a finer cap needs >256 rows; verify the
        // ORDER BY directly instead
        for (i, h) in ["a", "b"].iter().enumerate() {
            s.write_batch("/r", &[], &[blob(h, vec![b'x'; 10])]).unwrap();
            let c = s.conn.lock().unwrap();
            c.execute(
                "UPDATE symbol_cache SET created_at = ?1 WHERE hash = ?2",
                params![i as i64, h],
            )
            .unwrap();
        }
        let c = s.conn.lock().unwrap();
        let first: String = c
            .query_row(
                "SELECT hash FROM symbol_cache ORDER BY created_at ASC, hash ASC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(first, "a");
    }

    fn decl(fq: &str, kind: &str, entry: &str, lang: &str) -> LibDecl {
        LibDecl {
            fq_name: fq.into(),
            simple_name: simple_of(fq).into(),
            kind: kind.into(),
            container: if fq.contains('$') || fq.contains("::") || matches!(kind, "method" | "field") { container_of(fq) } else { None },
            entry: entry.into(),
            line: 3,
            lang: lang.into(),
        }
    }

    #[test]
    fn name_helpers() {
        assert_eq!(split_fq("java.util.Map$Entry"), (Some("java.util.Map"), SEP_DOLLAR, "Entry"));
        assert_eq!(split_fq("java.util.Map"), (Some("java.util"), SEP_DOT, "Map"));
        assert_eq!(split_fq("serde::de::Deserialize"), (Some("serde::de"), SEP_COLONS, "Deserialize"));
        assert_eq!(split_fq("Top"), (None, SEP_NONE, "Top"));
        assert_eq!(split_fq(".lead"), (None, SEP_NONE, ".lead"), "a leading separator is not a container");
        assert_eq!(split_fq("trailing."), (None, SEP_NONE, "trailing."), "nor a trailing one");
        // every split rebuilds its input exactly
        for fq in ["java.util.Map$Entry.getKey", "serde::de::Deserialize", "Top", "a.b$C::d"] {
            let (p, sep, simple) = split_fq(fq);
            assert_eq!(format!("{}{}{simple}", p.unwrap_or(""), sep_str(sep)), fq);
        }
        assert_eq!(container_of("java.util.Map$Entry").as_deref(), Some("java.util.Map"));
        assert_eq!(container_of("java.util.Map$Entry.getKey").as_deref(), Some("java.util.Map$Entry"));
        assert_eq!(container_of("serde::de::Deserialize").as_deref(), Some("serde::de"));
        assert_eq!(container_of("Top"), None);
        assert_eq!(simple_of("java.util.Map$Entry"), "Entry");
        assert_eq!(simple_of("serde::de::Deserialize"), "Deserialize");
        assert_eq!(simple_of("Top"), "Top");
        assert_eq!(entry_of("cargo", "serde-1.0.219", "src/lib.rs"), "serde-1.0.219/src/lib.rs");
        assert_eq!(entry_of("maven", "gson-2.11.0-sources.jar", "com/google/gson/Gson.java"), "gson-2.11.0-sources.jar!/com/google/gson/Gson.java");
        assert_eq!(entry_of("jdk", "src.zip", "java.base/java/util/Map.java"), "java.base/java/util/Map.java");
    }

    #[test]
    fn lib_sources_artifacts_and_decls_round_trip() {
        let s = SymbolStore::open_in_memory();
        let src = LibSourceRow {
            id: "jdk:abc".into(),
            kind: "jdk".into(),
            path: "C:/jdk/lib/src.zip".into(),
            label: "JDK 21".into(),
            fingerprint: "fp1".into(),
            state: "indexing".into(),
            size_bytes: 10,
            artifacts: 1,
            ..Default::default()
        };
        s.lib_upsert_source(&src).unwrap();
        assert_eq!(s.lib_source("jdk:abc"), Some(src.clone()));
        assert!(s.lib_source("nope").is_none());
        let zip = s
            .lib_upsert_artifact(
                "jdk:abc",
                &LibArtifactRow {
                    name: "src.zip".into(),
                    kind: "zip".into(),
                    path: "C:/jdk/lib/src.zip".into(),
                    fingerprint: "fp1".into(),
                    state: "indexing".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(zip > 0);
        assert!(s.lib_upsert_artifact("nope", &LibArtifactRow::default()).is_err());
        let e = "java.base/java/util/Map.java";
        s.lib_insert_decls(
            zip,
            &[
                decl("java.util.Map", "interface", e, "java"),
                decl("java.util.Map$Entry", "interface", e, "java"),
                decl("java.util.Map$Entry.getKey", "method", e, "java"),
            ],
        )
        .unwrap();
        assert_eq!(s.lib_decl_count("jdk:abc"), 3);
        // not visible until the source is done
        assert!(s.lib_lookup_fq("java.util.Map", None, 10).is_empty());
        s.lib_set_state("jdk:abc", "done", 1, 3, 10).unwrap();
        s.lib_set_artifact(zip, "done", None, 1, 3).unwrap();
        let hit = &s.lib_lookup_fq("java.util.Map", None, 10)[0];
        assert_eq!(hit.source_label, "JDK 21");
        assert_eq!(hit.artifact, "src.zip");
        assert_eq!(hit.decl.simple_name, "Map");
        assert_eq!(hit.decl.entry, e, "a JDK entry is the bare zip path");
        assert_eq!(hit.decl.lang, "java");
        assert_eq!(hit.decl.container, None);
        let entry = &s.lib_lookup_simple("Entry", "java", None, 10);
        assert_eq!(entry.len(), 1);
        assert_eq!(entry[0].decl.container.as_deref(), Some("java.util.Map"), "derived from the fq name");
        let get_key = &s.lib_lookup_fq("java.util.Map$Entry.getKey", None, 10)[0];
        assert_eq!(get_key.decl.container.as_deref(), Some("java.util.Map$Entry"));
        assert!(s.lib_lookup_simple("Entry", "rust", None, 10).is_empty());
        // the source filter
        assert!(s.lib_lookup_simple("Entry", "java", Some(&[]), 10).is_empty());
        assert!(s.lib_lookup_simple("Entry", "java", Some(&["maven:x".into()]), 10).is_empty());
        assert_eq!(s.lib_lookup_simple("Entry", "java", Some(&["maven:x".into(), "jdk:abc".into()]), 10).len(), 1);
        let row = s.lib_source("jdk:abc").unwrap();
        assert_eq!((row.state.as_str(), row.files, row.decls), ("done", 1, 3));
        assert!(row.indexed_at > 0);
        s.lib_set_counts("jdk:abc", 1, 0, 0).unwrap();
        assert_eq!(s.lib_source("jdk:abc").unwrap().artifacts, 1);
        let arts = s.lib_artifacts("jdk:abc");
        assert_eq!(arts.len(), 1);
        assert_eq!((arts[0].id, arts[0].state.as_str(), arts[0].files, arts[0].decls), (zip, "done", 1, 3));
        assert_eq!(s.lib_artifact("jdk:abc", "src.zip").map(|a| a.id), Some(zip));
        assert_eq!(s.lib_sources().len(), 1);

        // a cargo source: artifact-prefixed entries, per-artifact deletes
        s.lib_upsert_source(&LibSourceRow {
            id: "cargo:p1".into(),
            kind: "cargo".into(),
            label: "Cargo · p1".into(),
            state: "done".into(),
            ..Default::default()
        })
        .unwrap();
        let serde = s
            .lib_upsert_artifact("cargo:p1", &LibArtifactRow { name: "serde-1.0.219".into(), kind: "crate".into(), state: "done".into(), ..Default::default() })
            .unwrap();
        let rkyv = s
            .lib_upsert_artifact("cargo:p1", &LibArtifactRow { name: "rkyv-0.8.0".into(), kind: "crate".into(), state: "done".into(), ..Default::default() })
            .unwrap();
        s.lib_insert_decls(serde, &[decl("serde::de::Deserialize", "trait", "src/de.rs", "rust")]).unwrap();
        s.lib_insert_decls(rkyv, &[decl("rkyv::Deserialize", "trait", "src/lib.rs", "rust")]).unwrap();
        let hits = s.lib_lookup_simple("Deserialize", "rust", Some(&["cargo:p1".into()]), 10);
        assert_eq!(hits.len(), 2);
        let serde_hit = hits.iter().find(|h| h.artifact == "serde-1.0.219").unwrap();
        assert_eq!(serde_hit.decl.entry, "serde-1.0.219/src/de.rs");
        assert_eq!(serde_hit.decl.lang, "rust");
        assert_eq!(serde_hit.decl.container.as_deref(), Some("serde::de"));
        assert!(s.lib_lookup_simple("Deserialize", "java", None, 10).is_empty());
        // upsert keeps the id and the declarations
        let again = s
            .lib_upsert_artifact("cargo:p1", &LibArtifactRow { name: "rkyv-0.8.0".into(), kind: "crate".into(), state: "done".into(), fingerprint: "v2".into(), ..Default::default() })
            .unwrap();
        assert_eq!(again, rkyv);
        assert_eq!(s.lib_decl_count("cargo:p1"), 2);
        s.lib_delete_artifacts("cargo:p1", &["rkyv-0.8.0".into(), "ghost".into()]).unwrap();
        assert_eq!(s.lib_decl_count("cargo:p1"), 1);
        assert_eq!(s.lib_artifacts("cargo:p1").len(), 1);
        // names are written once and shared by every row that uses them —
        // both artifacts declared a `Deserialize`, both rows point at one
        // `lib_names` entry, and the kind strings are shared the same way
        {
            let c = s.conn.lock().unwrap();
            let rows: i64 = c.query_row("SELECT COUNT(*) FROM lib_decls", [], |r| r.get(0)).unwrap();
            assert_eq!(rows, 4, "3 jdk + serde (rkyv's row went with its artifact)");
            let one = |name: &str| -> i64 {
                c.query_row("SELECT COUNT(*) FROM lib_names WHERE name = ?1", [name], |r| r.get(0))
                    .unwrap()
            };
            assert_eq!(one("Deserialize"), 1, "declared by serde AND rkyv");
            assert_eq!(one("trait"), 1);
            assert_eq!(one("interface"), 1);
            let dupes: i64 = c
                .query_row("SELECT COUNT(*) FROM (SELECT name FROM lib_names GROUP BY name HAVING COUNT(*) > 1)", [], |r| r.get(0))
                .unwrap();
            assert_eq!(dupes, 0);
            // a declaration row is five integers — no strings of its own
            let cols: i64 = c
                .query_row("SELECT COUNT(*) FROM pragma_table_info('lib_decls') WHERE type = 'TEXT'", [], |r| r.get(0))
                .unwrap();
            assert_eq!(cols, 0);
        }
        s.lib_clear_artifact(serde).unwrap();
        assert_eq!(s.lib_decl_count("cargo:p1"), 0);
        assert_eq!(s.lib_artifacts("cargo:p1").len(), 1, "the row stays");
        s.lib_clear_decls("jdk:abc").unwrap();
        assert_eq!(s.lib_decl_count("jdk:abc"), 0);
        assert!(s.lib_artifacts("jdk:abc").is_empty(), "clearing a source drops its artifacts");
        s.lib_delete_source("jdk:abc").unwrap();
        s.lib_delete_source("cargo:p1").unwrap();
        assert!(s.lib_sources().is_empty());
        assert!(s.db_bytes() > 0);
        assert!(!s.vacuum_if_needed());
    }

    #[test]
    fn open_creates_the_file_and_schema() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub").join("symbols.db");
        let s = SymbolStore::open(&p).unwrap();
        assert!(p.is_file());
        assert_eq!(s.cache_rows(), 0);
        drop(s);
        // reopen is idempotent
        SymbolStore::open(&p).unwrap();
    }

    #[test]
    fn open_drops_any_earlier_library_layout() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("symbols.db");
        {
            let c = Connection::open(&p).unwrap();
            c.execute_batch(
                "CREATE TABLE lib_sources (id TEXT PRIMARY KEY, kind TEXT NOT NULL, path TEXT NOT NULL,
                   label TEXT NOT NULL, fingerprint TEXT NOT NULL, state TEXT NOT NULL, files INTEGER,
                   decls INTEGER, size_bytes INTEGER, indexed_at INTEGER);
                 CREATE TABLE lib_decls (source_id TEXT NOT NULL, fq_name TEXT NOT NULL, simple_name TEXT,
                   kind TEXT, container TEXT, entry TEXT, line INTEGER, lang TEXT);
                 CREATE TABLE symbol_cache (hash TEXT PRIMARY KEY, lang TEXT NOT NULL,
                   grammar_version TEXT NOT NULL, blob BLOB NOT NULL, created_at INTEGER NOT NULL);
                 INSERT INTO lib_sources VALUES ('0a1b', 'cargo', 'x', 'serde-1.0.1', 'f', 'done', 1, 1, 0, 0);
                 INSERT INTO lib_decls VALUES ('0a1b', 'serde::X', 'X', 'struct', NULL, 'src/lib.rs', 0, 'rust');
                 INSERT INTO symbol_cache VALUES ('h1', 'java', '1', x'00', 1);",
            )
            .unwrap();
        }
        let s = SymbolStore::open(&p).unwrap();
        assert!(s.lib_sources().is_empty(), "library rows are gone");
        assert!(s.lib_lookup_simple("X", "rust", None, 10).is_empty());
        assert_eq!(s.cache_rows(), 1, "the parse cache is NOT part of the library layout");
        assert!(s.vacuum_if_needed(), "one vacuum after the drop");
        assert!(!s.vacuum_if_needed());
        drop(s);
        let s = SymbolStore::open(&p).unwrap();
        assert!(!s.vacuum_if_needed(), "the current layout is kept");
        drop(s);
        // a database stamped with another library version is dropped again
        {
            let c = Connection::open(&p).unwrap();
            c.pragma_update(None, "user_version", LIB_SCHEMA + 1).unwrap();
        }
        assert!(SymbolStore::open(&p).unwrap().vacuum_if_needed());
    }
}
