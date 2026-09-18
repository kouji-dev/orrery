//! Library sources (M4): the JDK's `src.zip` and — per registered project —
//! the crates its `Cargo.lock` pins and the sources jars its `pom.xml`
//! names become searchable declarations (`lib_decls` in `symbols.db`) and
//! read-only virtual documents (`orrery-lib://<sourceId>/<entry>`).
//!
//! - [`discover`] finds sources: `jdk:<hash>` per installation,
//!   `cargo:<projectId>` / `maven:<projectId>` per project lockfile, each
//!   with its artifact set (crate dirs at the pinned version, jars).
//! - [`index`] extracts declarations; the service runs one source per
//!   below-normal std thread and walks its artifacts INCREMENTALLY: an
//!   artifact whose fingerprint the `lib_artifacts` row already carries is
//!   left alone, a removed one loses its rows, a missing one is counted,
//!   one over [`MAX_ARTIFACT_FILES`] / [`MAX_ARTIFACT_DECLS`] is skipped
//!   with its reason. `libsrc://status` every ~300 ms.
//! - [`resolve`] turns a word + the asking file's header into ordered fq
//!   candidates; the service looks them up in the asking PROJECT's sources
//!   only (rust: its cargo source; java: its maven source, then the JDKs)
//!   and falls back to a ranked simple-name query there.
//! - `read` streams one crate file / zip entry (2 MB cap, `..` rejected).
//!
//! Triggers: `libsrc_ensure{id}` (agent or project id → its project),
//! `symbols_index_start`, project registration, and the watcher when a
//! root's `Cargo.lock` / `pom.xml` changes (fingerprint = blake3 of the
//! file); `libsrc_reindex`, `libsrc_cancel`, `libsrc_remove` by hand.
//! Startup prunes sources whose project or file is gone and drops the
//! pre-M4.1 per-crate layout. Nothing here emits or spawns directly: status
//! goes through the injected sink (`core::emit::emit_tracked` in `lib.rs`),
//! the only process spawn is `discover::java_home_from_cmd` via
//! `core::proc::cmd`.

pub mod commands;
pub mod discover;
pub mod index;
pub mod resolve;

use std::cell::Cell;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::core::errors::{AppError, AppResult};
use crate::lsp::virtual_docs::{self, VirtualDoc, VirtualDocs};
use crate::symbols::store::{LibArtifactRow, LibDeclHit, LibSourceRow, SymbolStore};
use crate::symbols::tags;
use crate::symbols::GrammarSource;

use discover::{Artifact, DiscoverEnv, LibProject, LibSource};
use index::{JobEnd, Limits, Progress};
use resolve::{JavaCtx, LibHit};

pub const EV_STATUS: &str = "libsrc://status";
/// Discovery results are reused this long.
const DISCOVER_TTL: Duration = Duration::from_secs(300);
/// Entries the cheap "does this root have java files" walk looks at.
const ROOT_PROBE_ENTRIES: usize = 4000;
/// Bytes of the asking file read for its package/import header.
const HEADER_BYTES: usize = 64 * 1024;
/// Library hits returned per lookup.
const MAX_HITS: usize = 20;
/// Simple-name fallback rows fetched before ranking.
const FALLBACK_ROWS: usize = 64;
/// Rows fetched for the crate-scoped rust fallback (a common name such as
/// `Deserialize` lives in many crates; the wanted crate must be in range).
const CRATE_SCOPED_ROWS: usize = 500;
/// A crate / jar with more source files than this is skipped (a JDK's
/// `src.zip` is exempt — 15k files, ~300k declarations, and the one source
/// every java project needs).
pub const MAX_ARTIFACT_FILES: usize = 4000;
/// A crate / jar producing more declarations than this is skipped (the JDK
/// is exempt here too).
///
/// Machine-generated FFI bindings are the whole problem: over this repo's
/// 741 locked crates the declaration counts fall into two clearly separated
/// groups — `linux-raw-sys` 230k, six pinned `windows-sys` versions at
/// ~150k each, `libc` 57k, `winapi` 44k, then a 2.7× gap down to the
/// biggest hand-written API surface (`ndk-sys` 16k, `objc2-app-kit` 13k,
/// `web-sys` 5k, `tokio` 2.6k). A limit anywhere in 17k…43k skips exactly
/// those nine and keeps every real library, cutting the cargo index by 84%
/// (1,458,531 → 228,253 declarations); 25k sits in the middle of the gap.
/// The skipped artifacts keep their row and reason, and the dev console
/// reports them as "9 skipped".
pub const MAX_ARTIFACT_DECLS: usize = 25_000;
/// Minimum gap between two progress events of one run.
const STATUS_EVERY: Duration = Duration::from_millis(300);
/// Lockfile names whose change re-checks a root's sources.
const LOCKFILES: &[&str] = &["Cargo.lock", "Cargo.toml", "pom.xml"];

/// The `libsrc://status` payload.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibStatus {
    pub source_id: String,
    pub label: String,
    pub kind: String,
    /// `indexing | done | error | cancelled`.
    pub state: String,
    pub done: usize,
    pub total: usize,
    pub decls: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The `libsrc_sources` row.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibSourceView {
    pub id: String,
    pub kind: String,
    /// The src.zip / the lockfile.
    pub path: String,
    pub label: String,
    /// `pending | indexing | done | error | cancelled | removed`.
    pub state: String,
    pub files: u64,
    pub decls: u64,
    pub indexed_at: i64,
    pub size_bytes: u64,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    /// Crates / jars the lockfile names (1 for a JDK).
    pub artifacts: u64,
    /// Not on disk.
    pub missing: u64,
    /// Over a perf guard.
    pub skipped: u64,
}

pub type StatusSink = Box<dyn Fn(&LibStatus) + Send + Sync>;

/// Where the projects come from (the app's `ProjectService`; a Vec in tests).
pub trait ProjectSource: Send + Sync {
    fn projects(&self) -> Vec<LibProject>;
    fn project(&self, id: &str) -> Option<LibProject> {
        self.projects().into_iter().find(|p| p.id == id)
    }
}

impl ProjectSource for crate::projects::service::ProjectService {
    fn projects(&self) -> Vec<LibProject> {
        self.list()
            .unwrap_or_default()
            .into_iter()
            .map(|p| LibProject {
                id: p.id.to_string(),
                name: p.name,
                root: PathBuf::from(p.path),
            })
            .collect()
    }

    fn project(&self, id: &str) -> Option<LibProject> {
        let uuid = id.parse::<uuid::Uuid>().ok()?;
        let path = self.path_of(uuid).ok()?;
        Some(LibProject {
            id: id.to_string(),
            name: self.name_of(uuid).unwrap_or_default(),
            root: PathBuf::from(path),
        })
    }
}

impl ProjectSource for Mutex<Vec<LibProject>> {
    fn projects(&self) -> Vec<LibProject> {
        self.lock().unwrap().clone()
    }
}

struct Job {
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

struct Inner {
    store: Arc<SymbolStore>,
    grammars: Arc<dyn GrammarSource>,
    env: Mutex<DiscoverEnv>,
    projects: Arc<dyn ProjectSource>,
    /// `true` once the `java -XshowSettings` probe ran (spawns once per process).
    probed_java: AtomicBool,
    discovered: Mutex<Option<(Instant, Vec<LibSource>)>>,
    jobs: Mutex<HashMap<String, Job>>,
    vdocs: VirtualDocs,
    /// The last opened zip (path, archive): parsing a 50 MB `src.zip`'s
    /// central directory costs ~200 ms per open otherwise.
    archive: Mutex<Option<(String, ZipFile)>>,
    sink: Option<StatusSink>,
}

type ZipFile = zip::ZipArchive<std::io::BufReader<std::fs::File>>;

#[derive(Clone)]
pub struct LibSrcService(Arc<Inner>);

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

impl LibSrcService {
    pub fn new(
        store: Arc<SymbolStore>,
        grammars: Arc<dyn GrammarSource>,
        env: DiscoverEnv,
        projects: Arc<dyn ProjectSource>,
        sink: Option<StatusSink>,
    ) -> Self {
        Self(Arc::new(Inner {
            store,
            grammars,
            env: Mutex::new(env),
            projects,
            probed_java: AtomicBool::new(false),
            discovered: Mutex::new(None),
            jobs: Mutex::new(HashMap::new()),
            vdocs: VirtualDocs::default(),
            archive: Mutex::new(None),
            sink,
        }))
    }

    /// Prune rows whose project or file vanished; a row left `indexing` by
    /// a crash keeps its finished artifacts, loses the half-done one and goes
    /// back to `pending`. The legacy layout's space is reclaimed off-thread.
    pub fn startup(&self) {
        let projects: Vec<String> = self.0.projects.projects().into_iter().map(|p| p.id).collect();
        for row in self.0.store.lib_sources() {
            let orphan = row.project_id.as_ref().is_some_and(|p| !projects.contains(p));
            if orphan || !Path::new(&row.path).exists() {
                log::info!("libsrc: {} vanished — pruned", row.label);
                let _ = self.0.store.lib_delete_source(&row.id);
            } else if row.state == "indexing" {
                for a in self.0.store.lib_artifacts(&row.id) {
                    if a.state == "indexing" {
                        let _ = self.0.store.lib_clear_artifact(a.id);
                        let _ = self.0.store.lib_set_artifact(a.id, "pending", None, 0, 0);
                    }
                }
                let _ = self.0.store.lib_set_state(&row.id, "pending", row.files, row.decls, row.size_bytes);
            }
        }
        let store = self.0.store.clone();
        let _ = std::thread::Builder::new().name("libsrc-vacuum".into()).spawn(move || {
            if store.vacuum_if_needed() {
                log::info!("libsrc: legacy library index dropped, symbols.db vacuumed");
            }
        });
    }

    fn emit(&self, s: &LibStatus) {
        if let Some(sink) = &self.0.sink {
            sink(s);
        }
    }

    // ---- discovery ----

    fn env(&self) -> DiscoverEnv {
        let mut env = self.0.env.lock().unwrap();
        if env.java_home.is_none()
            && env.java_home_from_cmd.is_none()
            && !self.0.probed_java.swap(true, Ordering::SeqCst)
        {
            env.java_home_from_cmd = discover::java_home_from_cmd();
        }
        env.clone()
    }

    /// Discovered sources — JDKs + every project's lockfile sources (cached
    /// for `DISCOVER_TTL`); the `java` probe runs once when `JAVA_HOME` is
    /// unset.
    pub fn discover(&self, force: bool) -> Vec<LibSource> {
        {
            let d = self.0.discovered.lock().unwrap();
            if let Some((at, list)) = d.as_ref() {
                if !force && at.elapsed() < DISCOVER_TTL {
                    return list.clone();
                }
            }
        }
        let env = self.env();
        let list = discover::discover_all(&env, &self.0.projects.projects());
        *self.0.discovered.lock().unwrap() = Some((Instant::now(), list.clone()));
        list
    }

    /// One project's lockfile sources, read afresh (the cache is patched so
    /// `sources()` agrees).
    fn discover_project(&self, project: &LibProject) -> Vec<LibSource> {
        let fresh = discover::discover_project(&self.env(), project);
        let mut d = self.0.discovered.lock().unwrap();
        if let Some((_, list)) = d.as_mut() {
            list.retain(|s| s.project_id.as_deref() != Some(project.id.as_str()));
            list.extend(fresh.iter().cloned());
        }
        fresh
    }

    fn view_of_row(r: &LibSourceRow) -> LibSourceView {
        LibSourceView {
            id: r.id.clone(),
            kind: r.kind.clone(),
            path: r.path.clone(),
            label: r.label.clone(),
            state: r.state.clone(),
            files: r.files,
            decls: r.decls,
            indexed_at: r.indexed_at,
            size_bytes: r.size_bytes,
            project_id: r.project_id.clone(),
            project_name: r.project_name.clone(),
            artifacts: r.artifacts,
            missing: r.missing,
            skipped: r.skipped,
        }
    }

    fn view_of_source(s: &LibSource) -> LibSourceView {
        LibSourceView {
            id: s.id.clone(),
            kind: s.kind.clone(),
            path: norm(&s.path),
            label: s.label.clone(),
            state: "pending".into(),
            files: 0,
            decls: 0,
            indexed_at: 0,
            size_bytes: s.size_bytes,
            project_id: s.project_id.clone(),
            project_name: s.project_name.clone(),
            artifacts: s.artifacts.len() as u64,
            missing: s.missing() as u64,
            skipped: 0,
        }
    }

    /// Every known source: DB rows first (state as stored), then discovered
    /// sources not yet in the DB as `pending`.
    pub fn sources(&self) -> Vec<LibSourceView> {
        let rows = self.0.store.lib_sources();
        let mut out: Vec<LibSourceView> = rows.iter().map(Self::view_of_row).collect();
        for s in self.discover(false) {
            if rows.iter().any(|r| r.id == s.id) {
                continue;
            }
            out.push(Self::view_of_source(&s));
        }
        out
    }

    // ---- triggers ----

    /// `(java, cargo)` presence in a root: `Cargo.lock`/`Cargo.toml` at the
    /// top or one level down, java via a pom / gradle file or a bounded
    /// ignore-aware walk.
    pub fn probe_root(root: &Path) -> (bool, bool) {
        let cargo = !discover::find_cargo_locks(root).is_empty() || root.join("Cargo.toml").is_file();
        let mut java = root.join("pom.xml").is_file() || root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file();
        if !java {
            for e in crate::search::walker(root).flatten().take(ROOT_PROBE_ENTRIES) {
                if e.file_type().is_some_and(|t| t.is_file()) && e.path().extension().is_some_and(|x| x == "java") {
                    java = true;
                    break;
                }
            }
        }
        (java, cargo)
    }

    /// Index what project `project_id` needs: its `Cargo.lock` crates, its
    /// `pom.xml` jars, and the JDKs when it has java. A source whose
    /// lockfile fingerprint moved is re-run (incrementally). Returns the
    /// sources considered.
    pub fn ensure_for_project(&self, project_id: &str) -> Vec<LibSourceView> {
        let Some(project) = self.0.projects.project(project_id) else {
            return Vec::new();
        };
        let mut wanted: Vec<LibSource> = self.discover_project(&project);
        let (java, _) = Self::probe_root(&project.root);
        if java {
            wanted.extend(self.discover(false).into_iter().filter(|s| s.kind == "jdk"));
        }
        let rows = self.0.store.lib_sources();
        let mut ids = Vec::new();
        for s in wanted {
            ids.push(s.id.clone());
            let row = rows.iter().find(|r| r.id == s.id);
            match row {
                // removed by hand → never auto-indexed again
                Some(r) if r.state == "removed" => continue,
                Some(r) if r.fingerprint == s.fingerprint && matches!(r.state.as_str(), "done" | "indexing" | "cancelled") => continue,
                _ => self.start(s),
            }
        }
        self.sources().into_iter().filter(|v| ids.contains(&v.id)).collect()
    }

    /// Watcher hook: a `Cargo.lock` / `Cargo.toml` / `pom.xml` change under
    /// a project root re-checks that project's sources (no-op elsewhere —
    /// an agent worktree's lockfile is not the project's).
    pub fn on_changes(&self, root: &Path, changes: &[crate::git::types::FileChange]) {
        let touched = changes.iter().any(|c| {
            let name = c.path.rsplit(['/', '\\']).next().unwrap_or(&c.path);
            LOCKFILES.contains(&name)
        });
        if !touched {
            return;
        }
        let root_n = norm(root);
        let root_c = std::fs::canonicalize(root).map(|p| norm(&p)).unwrap_or_default();
        for p in self.0.projects.projects() {
            let pn = norm(&p.root);
            let pc = std::fs::canonicalize(&p.root).map(|x| norm(&x)).unwrap_or_default();
            if pn.eq_ignore_ascii_case(&root_n) || (!pc.is_empty() && pc.eq_ignore_ascii_case(&root_c)) {
                self.ensure_for_project(&p.id);
            }
        }
    }

    /// Manual "re-scan projects": discovery from scratch, then every
    /// project's sources ensured. Returns the full list.
    pub fn rescan(&self) -> Vec<LibSourceView> {
        self.discover(true);
        for p in self.0.projects.projects() {
            self.ensure_for_project(&p.id);
        }
        self.sources()
    }

    /// Why a java jump found nothing, when the index can tell: no JDK on
    /// this machine (`jdk-missing`), or the project's pom names jars whose
    /// `-sources.jar` is not in `~/.m2` (`sources-missing`). Rust files get
    /// no hint (v1).
    pub fn hint_for(&self, project_id: Option<&str>, rel: &str) -> Option<&'static str> {
        if !rel.rsplit('.').next().is_some_and(|e| e.eq_ignore_ascii_case("java")) {
            return None;
        }
        let rows = self.0.store.lib_sources();
        let has_jdk = rows.iter().any(|r| r.kind == "jdk" && r.state != "removed")
            || self.discover(false).iter().any(|s| s.kind == "jdk");
        if !has_jdk {
            return Some("jdk-missing");
        }
        let id = format!("maven:{}", project_id?);
        let missing = match rows.iter().find(|r| r.id == id) {
            Some(r) => r.missing > 0,
            None => self.discover(false).iter().any(|s| s.id == id && s.missing() > 0),
        };
        missing.then_some("sources-missing")
    }

    /// The source `id` denotes, read afresh (a JDK from discovery, a lockfile
    /// source from its project); falls back to the DB row for a JDK whose
    /// discovery moved.
    fn source_by_id(&self, id: &str) -> Option<LibSource> {
        let (kind, rest) = id.split_once(':')?;
        if kind == "jdk" {
            if let Some(s) = self.discover(false).into_iter().find(|s| s.id == id) {
                return Some(s);
            }
            let row = self.0.store.lib_source(id)?;
            let path = PathBuf::from(&row.path);
            let (fingerprint, size) = discover::stat_fingerprint(&path);
            return Some(LibSource {
                id: row.id,
                kind: row.kind,
                label: row.label,
                artifacts: vec![Artifact {
                    name: "src.zip".into(),
                    kind: "zip".into(),
                    path: Some(path.clone()),
                    fingerprint: fingerprint.clone(),
                }],
                path,
                fingerprint,
                size_bytes: size,
                project_id: None,
                project_name: None,
            });
        }
        let project = self.0.projects.project(rest)?;
        self.discover_project(&project).into_iter().find(|s| s.id == id)
    }

    /// Cancel a running job (if any), drop every artifact, index again.
    pub fn reindex(&self, id: &str) -> AppResult<LibSourceView> {
        let src = self
            .source_by_id(id)
            .ok_or_else(|| AppError::Other(format!("library source {id}")))?;
        self.cancel(id);
        if let Some(j) = self.0.jobs.lock().unwrap().get(id) {
            Self::wait_finished(&j.finished);
        }
        self.0.store.lib_clear_decls(id).map_err(|e| AppError::Other(e.to_string()))?;
        self.start(src);
        self.view(id)
    }

    pub fn cancel(&self, id: &str) {
        if let Some(j) = self.0.jobs.lock().unwrap().get(id) {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Cancel + drop the declarations; the row stays as `removed` so
    /// `ensure` never picks the source up again (reindex reinstates it).
    pub fn remove(&self, id: &str) -> AppResult<()> {
        self.cancel(id);
        let src = self
            .source_by_id(id)
            .ok_or_else(|| AppError::Other(format!("library source {id}")))?;
        if let Some(j) = self.0.jobs.lock().unwrap().get(id) {
            Self::wait_finished(&j.finished);
        }
        self.0.store.lib_clear_decls(id).map_err(|e| AppError::Other(e.to_string()))?;
        let mut row = Self::row_of(&src, "removed");
        row.artifacts = 0;
        row.missing = 0;
        self.0.store.lib_upsert_source(&row).map_err(|e| AppError::Other(e.to_string()))?;
        Ok(())
    }

    fn view(&self, id: &str) -> AppResult<LibSourceView> {
        self.sources()
            .into_iter()
            .find(|v| v.id == id)
            .ok_or_else(|| AppError::Other(format!("library source {id}")))
    }

    /// Wait for a job's thread to leave (bounded).
    fn wait_finished(finished: &AtomicBool) {
        let start = Instant::now();
        while !finished.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn row_of(src: &LibSource, state: &str) -> LibSourceRow {
        LibSourceRow {
            id: src.id.clone(),
            kind: src.kind.clone(),
            path: norm(&src.path),
            label: src.label.clone(),
            fingerprint: src.fingerprint.clone(),
            state: state.into(),
            files: 0,
            decls: 0,
            size_bytes: src.size_bytes,
            indexed_at: 0,
            project_id: src.project_id.clone(),
            project_name: src.project_name.clone(),
            artifacts: src.artifacts.len() as u64,
            missing: src.missing() as u64,
            skipped: 0,
        }
    }

    /// Spawn the index job for `src` on a below-normal thread. A running job
    /// for the same id is cancelled first and waited for on the new thread.
    fn start(&self, src: LibSource) {
        let cancel = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let previous = {
            let mut jobs = self.0.jobs.lock().unwrap();
            let prev = jobs.remove(&src.id);
            if let Some(p) = &prev {
                p.cancel.store(true, Ordering::Relaxed);
            }
            jobs.insert(
                src.id.clone(),
                Job {
                    cancel: cancel.clone(),
                    finished: finished.clone(),
                },
            );
            prev
        };
        let mut row = Self::row_of(&src, "indexing");
        if let Some(old) = self.0.store.lib_source(&src.id) {
            // counters of the finished artifacts stay visible while the
            // incremental run tops them up
            row.files = old.files;
            row.decls = old.decls;
            row.skipped = old.skipped;
        }
        if let Err(e) = self.0.store.lib_upsert_source(&row) {
            log::warn!("libsrc: {}: {e}", src.label);
            return;
        }
        let svc = self.clone();
        let short: String = src.id.chars().filter(|c| c.is_alphanumeric()).take(12).collect();
        let spawned = std::thread::Builder::new().name(format!("libsrc-{short}")).spawn(move || {
            crate::symbols::lower_priority();
            if let Some(p) = previous {
                Self::wait_finished(&p.finished);
            }
            svc.run(&src, &cancel);
            finished.store(true, Ordering::Relaxed);
            // only our own entry: a reindex may already have replaced it
            let mut jobs = svc.0.jobs.lock().unwrap();
            if jobs.get(&src.id).is_some_and(|j| Arc::ptr_eq(&j.cancel, &cancel)) {
                jobs.remove(&src.id);
            }
        });
        if spawned.is_err() {
            log::warn!("libsrc: cannot spawn the indexer thread for {}", row.label);
            let _ = self.0.store.lib_set_state(&row.id, "error", 0, 0, row.size_bytes);
            self.0.jobs.lock().unwrap().remove(&row.id);
        }
    }

    /// Files an artifact would read (the file guard + the progress total);
    /// `Err` when it cannot be opened.
    fn artifact_file_count(a: &Artifact, path: &Path) -> Result<usize, String> {
        if a.kind == "crate" {
            return Ok(index::rust_files(path).len());
        }
        let f = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let archive = zip::ZipArchive::new(std::io::BufReader::new(f)).map_err(|e| format!("{}: {e}", a.name))?;
        Ok(index::java_entry_count(&archive))
    }

    /// The job body: diff the artifact set against `lib_artifacts`, index
    /// what is new or changed, final state + status event.
    fn run(&self, src: &LibSource, cancel: &AtomicBool) {
        let store = self.0.store.clone();
        let status = |state: &str, p: Progress, error: Option<String>| LibStatus {
            source_id: src.id.clone(),
            label: src.label.clone(),
            kind: src.kind.clone(),
            state: state.into(),
            done: p.done,
            total: p.total,
            decls: p.decls,
            error,
        };
        self.emit(&status("indexing", Progress::default(), None));
        let started = Instant::now();
        let existing: HashMap<String, LibArtifactRow> =
            store.lib_artifacts(&src.id).into_iter().map(|a| (a.name.clone(), a)).collect();
        // gone from the lockfile → rows out
        let removed: Vec<String> = existing
            .keys()
            .filter(|n| !src.artifacts.iter().any(|a| a.name == **n))
            .cloned()
            .collect();
        if let Err(e) = store.lib_delete_artifacts(&src.id, &removed) {
            log::warn!("libsrc: {}: {e}", src.label);
        }
        // plan: what to index, with the file guard applied up front (crates
        // and jars only — the JDK is exempt from both guards)
        let guarded = src.kind != "jdk";
        let limits = Limits {
            max_decls: if guarded { MAX_ARTIFACT_DECLS } else { 0 },
        };
        let mut todo: Vec<(&Artifact, i64, usize)> = Vec::new();
        let mut total_files = 0usize;
        let mut kept_decls = 0usize;
        let mut kept_files = 0usize;
        let mut fatal: Option<String> = None;
        for a in &src.artifacts {
            let old = existing.get(&a.name);
            let Some(path) = &a.path else {
                if old.is_none_or(|o| o.state != "missing") {
                    if let Some(o) = old {
                        let _ = store.lib_clear_artifact(o.id);
                    }
                    let _ = store.lib_upsert_artifact(
                        &src.id,
                        &LibArtifactRow {
                            name: a.name.clone(),
                            kind: a.kind.clone(),
                            state: "missing".into(),
                            ..Default::default()
                        },
                    );
                }
                continue;
            };
            if let Some(o) = old {
                if o.fingerprint == a.fingerprint && o.state == "done" {
                    kept_decls += o.decls as usize;
                    kept_files += o.files as usize;
                    continue;
                }
                if o.fingerprint == a.fingerprint && o.state == "skipped" {
                    continue;
                }
            }
            let mut row = LibArtifactRow {
                name: a.name.clone(),
                kind: a.kind.clone(),
                path: norm(path),
                fingerprint: a.fingerprint.clone(),
                state: "pending".into(),
                ..Default::default()
            };
            let count = match Self::artifact_file_count(a, path) {
                Ok(n) => n,
                Err(e) => {
                    if src.kind == "jdk" {
                        fatal = Some(e.clone());
                    }
                    row.state = "error".into();
                    row.reason = Some(e);
                    if let Some(o) = old {
                        let _ = store.lib_clear_artifact(o.id);
                    }
                    let _ = store.lib_upsert_artifact(&src.id, &row);
                    continue;
                }
            };
            if guarded && count > MAX_ARTIFACT_FILES {
                row.state = "skipped".into();
                row.reason = Some(format!("{count} source files — over the {MAX_ARTIFACT_FILES} limit"));
                if let Some(o) = old {
                    let _ = store.lib_clear_artifact(o.id);
                }
                let _ = store.lib_upsert_artifact(&src.id, &row);
                continue;
            }
            match store.lib_upsert_artifact(&src.id, &row) {
                Ok(id) => {
                    if old.is_some() {
                        let _ = store.lib_clear_artifact(id);
                    }
                    todo.push((a, id, count));
                    total_files += count;
                }
                Err(e) => log::warn!("libsrc: {}: {e}", a.name),
            }
        }
        // index
        let grammar = if src.kind == "cargo" { None } else { self.0.grammars.grammar_for_extension("java") };
        let last_emit = Cell::new(Instant::now());
        let mut done_files = 0usize;
        let mut run_decls = 0usize;
        let mut cancelled = false;
        let mut errors = 0usize;
        let mut indexed = 0usize;
        for (a, id, count) in todo {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            let Some(path) = &a.path else { continue };
            let _ = store.lib_set_artifact(id, "indexing", None, 0, 0);
            let mut sink = (store.as_ref(), id);
            let base = done_files;
            let mut on_progress = |p: Progress| {
                if last_emit.get().elapsed() >= STATUS_EVERY {
                    last_emit.set(Instant::now());
                    self.emit(&status(
                        "indexing",
                        Progress {
                            done: base + p.done,
                            total: total_files,
                            decls: kept_decls + run_decls + p.decls,
                        },
                        None,
                    ));
                }
            };
            let result = if a.kind == "crate" {
                let name = discover::split_crate_dir(&a.name).map(|(c, _)| c).unwrap_or(&a.name);
                index::index_cargo_dir(path, name, cancel, &mut sink, &mut on_progress, limits)
            } else {
                std::fs::File::open(path)
                    .map_err(|e| format!("open {}: {e}", path.display()))
                    .and_then(|f| zip::ZipArchive::new(std::io::BufReader::new(f)).map_err(|e| format!("{}: {e}", a.name)))
                    .and_then(|mut z| {
                        index::index_java_zip(&mut z, src.kind == "jdk", grammar.as_deref(), cancel, &mut sink, &mut on_progress, limits)
                    })
            };
            match result {
                Ok(JobEnd::Done(s)) => {
                    let _ = store.lib_set_artifact(id, "done", None, s.files as u64, s.decls as u64);
                    run_decls += s.decls;
                    done_files += count;
                    indexed += 1;
                }
                Ok(JobEnd::Cancelled(_)) => {
                    let _ = store.lib_clear_artifact(id);
                    let _ = store.lib_set_artifact(id, "pending", None, 0, 0);
                    cancelled = true;
                    break;
                }
                Ok(JobEnd::Skipped(reason)) => {
                    log::info!("libsrc: {} skipped — {reason}", a.name);
                    let _ = store.lib_clear_artifact(id);
                    let _ = store.lib_set_artifact(id, "skipped", Some(&reason), 0, 0);
                    done_files += count;
                }
                Err(e) => {
                    log::warn!("libsrc: {}: {e}", a.name);
                    let _ = store.lib_clear_artifact(id);
                    let _ = store.lib_set_artifact(id, "error", Some(&e), 0, 0);
                    if src.kind == "jdk" {
                        fatal = Some(e);
                    }
                    errors += 1;
                    done_files += count;
                }
            }
        }
        // totals from the artifact rows (finished ones from earlier runs included)
        let rows = store.lib_artifacts(&src.id);
        let files: u64 = rows.iter().filter(|a| a.state == "done").map(|a| a.files).sum();
        let decls: u64 = rows.iter().filter(|a| a.state == "done").map(|a| a.decls).sum();
        let missing = rows.iter().filter(|a| a.state == "missing").count() as u64;
        let skipped = rows.iter().filter(|a| a.state == "skipped").count() as u64;
        let _ = store.lib_set_counts(&src.id, src.artifacts.len() as u64, missing, skipped);
        let _ = kept_files;
        if cancelled {
            let _ = store.lib_set_state(&src.id, "cancelled", files, decls, src.size_bytes);
            self.emit(&status(
                "cancelled",
                Progress {
                    done: done_files,
                    total: 0,
                    decls: decls as usize,
                },
                None,
            ));
            return;
        }
        if let Some(e) = fatal.filter(|_| indexed == 0 && files == 0) {
            let _ = store.lib_set_state(&src.id, "error", 0, 0, src.size_bytes);
            self.emit(&status("error", Progress::default(), Some(e)));
            return;
        }
        let _ = store.lib_set_state(&src.id, "done", files, decls, src.size_bytes);
        log::info!(
            "libsrc: {} — {} artifacts ({} indexed now, {} missing, {} skipped, {} errors), {} files, {} declarations in {:.1}s",
            src.label,
            src.artifacts.len(),
            indexed,
            missing,
            skipped,
            errors,
            files,
            decls,
            started.elapsed().as_secs_f64()
        );
        self.emit(&status(
            "done",
            Progress {
                done: files as usize,
                total: files as usize,
                decls: decls as usize,
            },
            None,
        ));
    }

    #[cfg(test)]
    /// `true` while a job for `id` runs.
    pub fn is_indexing(&self, id: &str) -> bool {
        self.0.jobs.lock().unwrap().contains_key(id)
    }

    // ---- lookup ----

    /// The sources a `lang` file of `project_id` resolves against, in
    /// preference order: rust → the project's cargo source; java → its maven
    /// source, then every JDK.
    fn sources_for(&self, project_id: Option<&str>, lang: &str) -> Vec<String> {
        let mut out = Vec::new();
        match lang {
            "rust" => {
                if let Some(p) = project_id {
                    out.push(format!("cargo:{p}"));
                }
            }
            _ => {
                if let Some(p) = project_id {
                    out.push(format!("maven:{p}"));
                }
                out.extend(self.0.store.lib_sources().into_iter().filter(|r| r.kind == "jdk").map(|r| r.id));
            }
        }
        out
    }

    /// Library declarations for `word` asked from `rel` under `root` (a
    /// worktree of `project_id`). The header (package/imports) comes from
    /// `text` when given, else the first 64 KB of the file on disk.
    pub fn hits_for(&self, project_id: Option<&str>, root: &Path, rel: &str, word: &str, text: Option<&str>) -> Vec<LibHit> {
        let lang = match rel.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
            Some("java") => "java",
            Some("rs") => "rust",
            _ => return Vec::new(),
        };
        let owned;
        let head: &[u8] = match text {
            Some(t) => t.as_bytes(),
            None => {
                owned = read_head(&root.join(rel));
                &owned
            }
        };
        let (package, imports) = tags::header(head, lang);
        let sources = self.sources_for(project_id, lang);
        self.resolve(lang, word, package.as_deref(), &imports, &sources)
    }

    /// Candidates in order (first fq with rows wins), else the ranked
    /// simple-name fallback — all within `sources`, in that order.
    pub fn resolve(
        &self,
        lang: &str,
        word: &str,
        package: Option<&str>,
        imports: &[String],
        sources: &[String],
    ) -> Vec<LibHit> {
        let word = word.trim();
        if word.is_empty() || sources.is_empty() {
            return Vec::new();
        }
        let store = &self.0.store;
        let scope = Some(sources);
        let mut found: Vec<LibDeclHit> = Vec::new();
        match lang {
            "java" => {
                let ctx = JavaCtx { package, imports };
                for cand in resolve::java_candidates(word, &ctx) {
                    found = store.lib_lookup_fq(&cand, scope, MAX_HITS);
                    if !found.is_empty() {
                        break;
                    }
                }
            }
            "rust" => {
                let in_crate = |h: &LibDeclHit, krate: &str| {
                    discover::split_crate_dir(&h.artifact)
                        .map(|(c, _)| index::normalise_crate(c) == krate)
                        .unwrap_or(true)
                };
                let candidates = resolve::rust_candidates(word, imports);
                for (krate, fq) in &candidates {
                    found = store
                        .lib_lookup_fq(fq, scope, MAX_HITS)
                        .into_iter()
                        .filter(|h| in_crate(h, krate))
                        .collect();
                    if !found.is_empty() {
                        break;
                    }
                }
                // `pub use de::Deserialize` re-exports are not rows: the
                // name exists in the crate under another module path
                if found.is_empty() {
                    let simple = word.rsplit("::").next().unwrap_or(word);
                    let by_name = store.lib_lookup_simple(simple, "rust", scope, CRATE_SCOPED_ROWS);
                    for (krate, _) in &candidates {
                        found = by_name.iter().filter(|h| in_crate(h, krate)).cloned().collect();
                        if !found.is_empty() {
                            break;
                        }
                    }
                }
            }
            _ => return Vec::new(),
        }
        let simple = word.rsplit('.').next().unwrap_or(word);
        if found.is_empty() {
            found = store.lib_lookup_simple(simple, lang, scope, FALLBACK_ROWS);
        }
        resolve::rank_hits(&mut found, simple, sources);
        found.dedup_by(|a, b| a.source_id == b.source_id && a.decl.entry == b.decl.entry && a.decl.line == b.decl.line);
        found.truncate(MAX_HITS);
        found.iter().map(LibHit::from_decl).collect()
    }

    // ---- virtual documents ----

    /// Text of `orrery-lib://<sourceId>/<entry>`: the crate file under its
    /// dir or the zip entry by name, 2 MB cap, LRU-cached.
    pub fn read(&self, uri: &str) -> AppResult<VirtualDoc> {
        let (source_id, entry) = parse_lib_uri(uri).map_err(AppError::Other)?;
        if let Some(doc) = self.0.vdocs.get(uri) {
            return Ok(doc);
        }
        let row = self
            .0
            .store
            .lib_source(&source_id)
            .ok_or_else(|| AppError::Other(format!("library source {source_id}")))?;
        let (artifact, rel) = split_entry(&row.kind, &entry);
        let bytes = self.read_entry(&row, artifact, rel)?;
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        let mut truncated = false;
        if text.len() > virtual_docs::MAX_TEXT {
            let mut cut = virtual_docs::MAX_TEXT;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            truncated = true;
        }
        if truncated {
            text.push_str("\n// … truncated (2 MB cap)\n");
        }
        let doc = VirtualDoc {
            uri: uri.to_string(),
            language: if row.kind == "cargo" { "rust" } else { "java" }.into(),
            text,
            title: title_for(&row, &entry),
        };
        self.0.vdocs.put(doc.clone());
        Ok(doc)
    }
}

/// First `HEADER_BYTES` of a file (empty when unreadable).
fn read_head(abs: &Path) -> Vec<u8> {
    let Ok(f) = std::fs::File::open(abs) else {
        return Vec::new();
    };
    let mut buf = Vec::with_capacity(HEADER_BYTES.min(8192));
    let _ = f.take(HEADER_BYTES as u64).read_to_end(&mut buf);
    buf
}

/// `orrery-lib://<sourceId>/<entry>` → `(sourceId, entry)`; the id is
/// `<kind>:<hash|projectId>`, the entry is validated: forward slashes, no
/// empty / `.` / `..` segments, no drive or backslash, no leading slash.
pub fn parse_lib_uri(uri: &str) -> Result<(String, String), String> {
    let rest = uri
        .strip_prefix("orrery-lib://")
        .ok_or_else(|| format!("not a library uri: {uri}"))?;
    let (id, entry) = rest
        .split_once('/')
        .ok_or_else(|| format!("library uri without an entry: {uri}"))?;
    let ok_id = match id.split_once(':') {
        Some((kind, rest)) => {
            matches!(kind, "jdk" | "cargo" | "maven")
                && !rest.is_empty()
                && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        }
        None => false,
    };
    if !ok_id {
        return Err(format!("bad library source id in {uri}"));
    }
    validate_entry(entry)?;
    Ok((id.to_string(), entry.to_string()))
}

pub fn validate_entry(entry: &str) -> Result<(), String> {
    if entry.is_empty() || entry.contains('\\') || entry.contains(':') || entry.starts_with('/') {
        return Err(format!("bad library entry: {entry}"));
    }
    if entry.split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..") {
        return Err(format!("bad library entry: {entry}"));
    }
    if entry.chars().any(|c| c.is_control()) {
        return Err(format!("bad library entry: {entry}"));
    }
    Ok(())
}

/// `(artifact, path inside it)` of an entry: `serde-1.0.219/src/lib.rs` →
/// `("serde-1.0.219", "src/lib.rs")`, `gson-2.11.0-sources.jar!/com/x/Y.java`
/// → `("gson-2.11.0-sources.jar", "com/x/Y.java")`, a JDK entry → `("src.zip", entry)`.
pub fn split_entry<'a>(source_kind: &str, entry: &'a str) -> (&'a str, &'a str) {
    match source_kind {
        "cargo" => entry.split_once('/').unwrap_or((entry, "")),
        "maven" => entry.split_once("!/").unwrap_or((entry, "")),
        _ => ("src.zip", entry),
    }
}

impl LibSrcService {
    /// The on-disk path of an artifact of `row` (the source path for a JDK).
    fn artifact_path(&self, row: &LibSourceRow, artifact: &str) -> AppResult<PathBuf> {
        if row.kind == "jdk" {
            return Ok(PathBuf::from(&row.path));
        }
        let a = self
            .0
            .store
            .lib_artifact(&row.id, artifact)
            .ok_or_else(|| AppError::Other(format!("{artifact}: not in {}", row.label)))?;
        if a.path.is_empty() {
            return Err(AppError::Other(format!("{artifact}: not on disk")));
        }
        Ok(PathBuf::from(a.path))
    }

    /// Bytes of one entry: a canonicalized prefix-checked path for crate
    /// dirs, `ZipArchive::by_name` on the (cached) archive for zips. Entries
    /// above the cap are cut at it.
    fn read_entry(&self, row: &LibSourceRow, artifact: &str, rel: &str) -> AppResult<Vec<u8>> {
        let cap = virtual_docs::MAX_TEXT as u64 + 1;
        if rel.is_empty() {
            return Err(AppError::Other(format!("{artifact}: no file in the entry")));
        }
        let path = self.artifact_path(row, artifact)?;
        if row.kind == "cargo" {
            let root = std::fs::canonicalize(&path)
                .map_err(|e| AppError::Other(format!("{}: {e}", path.display())))?;
            let abs = std::fs::canonicalize(root.join(rel))
                .map_err(|e| AppError::Other(format!("{rel}: {e}")))?;
            if !abs.starts_with(&root) {
                return Err(AppError::Other(format!("{rel}: outside the crate")));
            }
            let f = std::fs::File::open(&abs).map_err(|e| AppError::Other(format!("{rel}: {e}")))?;
            let mut buf = Vec::new();
            f.take(cap)
                .read_to_end(&mut buf)
                .map_err(|e| AppError::Other(format!("{rel}: {e}")))?;
            return Ok(buf);
        }
        let key = norm(&path);
        let mut slot = self.0.archive.lock().unwrap();
        if slot.as_ref().is_none_or(|(k, _)| *k != key) {
            let f = std::fs::File::open(&path)
                .map_err(|e| AppError::Other(format!("{}: {e}", path.display())))?;
            let archive = zip::ZipArchive::new(std::io::BufReader::new(f))
                .map_err(|e| AppError::Other(format!("{artifact}: {e}")))?;
            *slot = Some((key, archive));
        }
        let (_, archive) = slot.as_mut().expect("archive slot filled above");
        let zf = archive
            .by_name(rel)
            .map_err(|e| AppError::Other(format!("{rel}: {e}")))?;
        if !zf.is_file() {
            return Err(AppError::Other(format!("{rel}: not a file")));
        }
        let mut buf = Vec::with_capacity(zf.size().min(cap) as usize);
        zf.take(cap)
            .read_to_end(&mut buf)
            .map_err(|e| AppError::Other(format!("{rel}: {e}")))?;
        Ok(buf)
    }
}

/// `ArrayList.java — JDK 21 (java.base)` / `Gson.java — gson-2.11.0-sources.jar
/// (com.google.gson)` / `lib.rs — serde-1.0.219 (src)`.
pub fn title_for(row: &LibSourceRow, entry: &str) -> String {
    let file = virtual_docs::title_of(entry);
    let (artifact, rel) = split_entry(&row.kind, entry);
    let (head, scope) = match row.kind.as_str() {
        "jdk" => {
            let (module, package) = index::java_package_of(rel, true);
            (row.label.split(" (").next().unwrap_or(&row.label).to_string(), module.unwrap_or(package))
        }
        "cargo" => (artifact.to_string(), rel.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default()),
        _ => (artifact.to_string(), index::java_package_of(rel, false).1),
    };
    if scope.is_empty() {
        format!("{file} — {head}")
    } else {
        format!("{file} — {head} ({scope})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::loader::GrammarRegistry;
    use std::io::Write;

    type Projects = Arc<Mutex<Vec<LibProject>>>;

    fn service(store: Arc<SymbolStore>, env: DiscoverEnv) -> (LibSrcService, Arc<Mutex<Vec<LibStatus>>>, Projects) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        let projects: Projects = Arc::new(Mutex::new(Vec::new()));
        let svc = LibSrcService::new(
            store,
            Arc::new(GrammarRegistry::new()),
            env,
            projects.clone(),
            Some(Box::new(move |s| sink_events.lock().unwrap().push(s.clone()))),
        );
        // never spawn the real `java` probe from a unit test
        svc.0.probed_java.store(true, Ordering::SeqCst);
        (svc, events, projects)
    }

    /// The fixture zip written to a temp dir as a JDK `lib/src.zip`.
    fn fixture_jdk(t: &Path) -> (DiscoverEnv, LibSource) {
        let home = t.join("jdk-21");
        std::fs::create_dir_all(home.join("lib")).unwrap();
        std::fs::write(home.join("release"), "JAVA_VERSION=\"21.0.2\"\n").unwrap();
        let zip = home.join("lib").join("src.zip");
        std::fs::File::create(&zip)
            .unwrap()
            .write_all(index::fixture::jdk_zip().get_ref())
            .unwrap();
        let env = DiscoverEnv {
            java_home: Some(home),
            ..Default::default()
        };
        let src = discover::discover_jdk(&env).remove(0);
        (env, src)
    }

    /// A registry with `serde-1.0.219` (2 files) and `rkyv-0.8.0` (1 file),
    /// and a project whose lock pins serde 1.0.219 + `missing-0.1.0`.
    fn fixture_cargo(t: &Path) -> (DiscoverEnv, LibProject) {
        let idx = t.join("cargo").join("registry").join("src").join("index.crates.io-x");
        let serde = idx.join("serde-1.0.219");
        std::fs::create_dir_all(serde.join("src")).unwrap();
        std::fs::write(serde.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(serde.join("src").join("lib.rs"), "pub struct Value;\npub mod de;\npub use de::Deserialize;\n").unwrap();
        std::fs::write(serde.join("src").join("de.rs"), "pub trait Deserialize {}\n").unwrap();
        let rkyv = idx.join("rkyv-0.8.0");
        std::fs::create_dir_all(rkyv.join("src")).unwrap();
        std::fs::write(rkyv.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(rkyv.join("src").join("lib.rs"), "pub trait Deserialize {}\npub struct Archived;\n").unwrap();
        std::fs::write(t.join("secret.txt"), "no").unwrap();
        let root = t.join("proj");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Cargo.lock"),
            "[[package]]\nname = \"serde\"\nversion = \"1.0.219\"\nsource = \"registry+x\"\n\n[[package]]\nname = \"missing\"\nversion = \"0.1.0\"\nsource = \"registry+x\"\n",
        )
        .unwrap();
        let env = DiscoverEnv {
            cargo_home: Some(t.join("cargo")),
            ..Default::default()
        };
        (
            env,
            LibProject {
                id: "p-1".into(),
                name: "orrery".into(),
                root,
            },
        )
    }

    fn wait_done(svc: &LibSrcService, id: &str) -> LibSourceRow {
        let start = Instant::now();
        loop {
            if !svc.is_indexing(id) {
                if let Some(r) = svc.0.store.lib_source(id) {
                    if r.state != "indexing" {
                        return r;
                    }
                }
            }
            assert!(start.elapsed() < Duration::from_secs(20), "job never finished");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn indexes_the_fixture_jdk_resolves_and_reads() {
        let t = tempfile::tempdir().unwrap();
        let (env, src) = fixture_jdk(t.path());
        let store = Arc::new(SymbolStore::open_in_memory());
        let (svc, events, _) = service(store.clone(), env);
        assert_eq!(src.label, "JDK 21 (jdk-21)");
        assert!(src.id.starts_with("jdk:"));
        svc.start(src.clone());
        let row = wait_done(&svc, &src.id);
        assert_eq!(row.state, "done");
        assert_eq!(row.files, 3);
        assert_eq!((row.artifacts, row.missing, row.skipped), (1, 0, 0));
        assert_eq!(row.decls, store.lib_decl_count(&src.id));
        assert!(row.decls >= 20, "{}", row.decls);
        let ev = events.lock().unwrap();
        assert_eq!(ev.first().unwrap().state, "indexing");
        let last = ev.last().unwrap();
        assert_eq!((last.state.as_str(), last.done, last.total), ("done", 3, 3));
        assert_eq!(last.decls as u64, row.decls);
        drop(ev);

        let sources = vec![src.id.clone()];
        // explicit import → nested Entry
        let hits = svc.resolve("java", "Entry", Some("com.acme"), &["java.util.Map".to_string()], &sources);
        assert_eq!(hits[0].fq_name, "java.util.Map$Entry");
        assert_eq!(hits[0].kind, "interface");
        assert_eq!(hits[0].container.as_deref(), Some("java.util.Map"));
        assert_eq!(hits[0].uri, format!("orrery-lib://{}/java.base/java/util/Map.java", src.id));
        assert_eq!(hits[0].label, "JDK 21 (jdk-21)");
        assert_eq!(hits[0].artifact, "src.zip");
        assert_eq!(hits[0].line, 6);
        // no import → java.lang
        let hits = svc.resolve("java", "String", Some("com.acme"), &[], &sources);
        assert_eq!(hits[0].fq_name, "java.lang.String");
        assert_eq!(hits[0].kind, "class");
        // wildcard import
        let hits = svc.resolve("java", "ArrayList", Some("com.acme"), &["java.util.*".to_string()], &sources);
        assert_eq!(hits[0].fq_name, "java.util.ArrayList");
        // explicit import beats the fallback; the fallback still finds members
        let hits = svc.resolve("java", "ArrayList", None, &["java.util.ArrayList".to_string()], &sources);
        assert_eq!(hits[0].fq_name, "java.util.ArrayList");
        let hits = svc.resolve("java", "hasNext", None, &[], &sources);
        assert_eq!(hits[0].fq_name, "java.util.ArrayList$Itr.hasNext");
        assert_eq!(hits[0].kind, "method");
        // simple-name fallback prefers types for a capitalised word
        let hits = svc.resolve("java", "Map", None, &[], &sources);
        assert_eq!(hits[0].fq_name, "java.util.Map");
        assert!(svc.resolve("java", "Nope", None, &[], &sources).is_empty());
        assert!(svc.resolve("rust", "Map", None, &[], &sources).is_empty());
        assert!(svc.resolve("java", "", None, &[], &sources).is_empty());
        assert!(svc.resolve("java", "Map", None, &[], &[]).is_empty(), "no sources → nothing");
        assert!(svc.resolve("java", "Map", None, &[], &["maven:other".into()]).is_empty(), "scoped to the given sources");
        // hits_for: a java file of a project without a maven source still reaches the JDK
        let hits = svc.hits_for(Some("p-x"), t.path(), "A.java", "Map", Some("package a;\n"));
        assert_eq!(hits[0].fq_name, "java.util.Map");
        assert!(svc.hits_for(Some("p-x"), t.path(), "a.txt", "Map", Some("")).is_empty());

        // virtual read
        let uri = format!("orrery-lib://{}/java.base/java/util/ArrayList.java", src.id);
        let doc = svc.read(&uri).unwrap();
        assert_eq!(doc.language, "java");
        assert_eq!(doc.title, "ArrayList.java — JDK 21 (java.base)");
        assert!(doc.text.contains("public class ArrayList<E>"));
        assert_eq!(doc.uri, uri);
        assert_eq!(svc.read(&uri).unwrap(), doc, "cached");
        assert!(svc.read(&format!("orrery-lib://{}/java.base/java/util/Missing.java", src.id)).is_err());
        assert!(svc.read("orrery-lib://jdk:0000000000000000/java.base/java/util/Map.java").is_err());

        // views + remove + reindex
        let views = svc.sources();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].state, "done");
        assert_eq!(views[0].size_bytes, src.size_bytes);
        assert_eq!(views[0].artifacts, 1);
        assert!(views[0].indexed_at > 0);
        svc.remove(&src.id).unwrap();
        assert_eq!(store.lib_decl_count(&src.id), 0);
        assert_eq!(svc.sources()[0].state, "removed");
        assert!(svc.resolve("java", "Map", None, &[], &sources).is_empty());
        svc.reindex(&src.id).unwrap();
        let row = wait_done(&svc, &src.id);
        assert_eq!(row.state, "done");
        assert!(svc.reindex("jdk:ffff").is_err());
        assert!(svc.remove("jdk:ffff").is_err());
        assert!(svc.reindex("cargo:nope").is_err(), "an unknown project");
    }

    #[test]
    fn cargo_source_follows_the_lock_incrementally_and_reads_under_its_dir_only() {
        let t = tempfile::tempdir().unwrap();
        let (env, project) = fixture_cargo(t.path());
        let store = Arc::new(SymbolStore::open_in_memory());
        let (svc, events, projects) = service(store.clone(), env);
        projects.lock().unwrap().push(project.clone());
        assert!(svc.ensure_for_project("p-nope").is_empty());
        let views = svc.ensure_for_project("p-1");
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, "cargo:p-1");
        assert_eq!(views[0].label, "Cargo · orrery");
        assert_eq!(views[0].project_name.as_deref(), Some("orrery"));
        let row = wait_done(&svc, "cargo:p-1");
        assert_eq!(row.state, "done");
        assert_eq!((row.artifacts, row.missing, row.skipped), (2, 1, 0));
        assert_eq!(row.files, 2);
        assert_eq!(row.decls, 3, "Value, mod de, Deserialize");
        let arts = store.lib_artifacts("cargo:p-1");
        assert_eq!(arts.iter().map(|a| (a.name.as_str(), a.state.as_str())).collect::<Vec<_>>(), vec![("missing-0.1.0", "missing"), ("serde-1.0.219", "done")]);
        let last = events.lock().unwrap().last().cloned().unwrap();
        assert_eq!((last.state.as_str(), last.done, last.total, last.decls), ("done", 2, 2, 3));

        // resolution: the project's cargo source only — rkyv is not in the lock
        let sources = vec!["cargo:p-1".to_string()];
        let hits = svc.resolve("rust", "Deserialize", None, &["serde::de::Deserialize".to_string()], &sources);
        assert_eq!(hits[0].fq_name, "serde::de::Deserialize");
        assert_eq!(hits[0].uri, "orrery-lib://cargo:p-1/serde-1.0.219/src/de.rs");
        assert_eq!(hits[0].artifact, "serde-1.0.219");
        let hits = svc.resolve("rust", "Value", None, &["serde::*".to_string()], &sources);
        assert_eq!(hits[0].fq_name, "serde::Value");
        // `pub use de::Deserialize` re-export: the fq `serde::Deserialize` has
        // no row → the crate-scoped simple-name fallback finds `serde::de::…`
        let hits = svc.resolve("rust", "Deserialize", None, &["serde::Deserialize".to_string()], &sources);
        assert_eq!(hits[0].fq_name, "serde::de::Deserialize");
        assert_eq!(hits[0].kind, "trait");
        assert!(svc.resolve("rust", "Archived", None, &["rkyv::Archived".to_string()], &sources).is_empty(), "not in the lock");
        let hits = svc.hits_for(Some("p-1"), &project.root, "src/main.rs", "Value", Some("use serde::Value;\n"));
        assert_eq!(hits[0].fq_name, "serde::Value");
        assert!(svc.hits_for(Some("p-2"), &project.root, "src/main.rs", "Value", Some("use serde::Value;\n")).is_empty(), "another project has no cargo source");
        assert!(svc.hits_for(None, &project.root, "src/main.rs", "Value", Some("use serde::Value;\n")).is_empty());
        assert_eq!(svc.hint_for(Some("p-1"), "src/A.java"), Some("jdk-missing"), "no JDK on this (fixture) machine");
        // rescan: discovery from scratch + every project ensured — a no-op here
        assert_eq!(svc.rescan().len(), 1);

        // virtual read: artifact-prefixed entries, no escape from the crate dir
        let doc = svc.read("orrery-lib://cargo:p-1/serde-1.0.219/src/de.rs").unwrap();
        assert_eq!(doc.language, "rust");
        assert_eq!(doc.title, "de.rs — serde-1.0.219 (src)");
        assert!(doc.text.starts_with("pub trait Deserialize"));
        assert!(svc.read("orrery-lib://cargo:p-1/serde-1.0.219/../secret.txt").is_err());
        assert!(svc.read("orrery-lib://cargo:p-1/serde-1.0.219/src/../../secret.txt").is_err());
        assert!(svc.read("orrery-lib://cargo:p-1/rkyv-0.8.0/src/lib.rs").is_err(), "not an artifact of the source");
        assert!(svc.read("orrery-lib://cargo:p-1/missing-0.1.0/src/lib.rs").is_err(), "not on disk");
        assert!(svc.read("orrery-lib://cargo:p-1/serde-1.0.219").is_err(), "no file");

        // ensure again: same fingerprint → no new run
        let n = events.lock().unwrap().len();
        svc.ensure_for_project("p-1");
        assert!(!svc.is_indexing("cargo:p-1"));
        assert_eq!(events.lock().unwrap().len(), n);

        // the lock moves: rkyv in, serde out → only rkyv is indexed, serde's rows go
        std::fs::write(
            project.root.join("Cargo.lock"),
            "[[package]]\nname = \"rkyv\"\nversion = \"0.8.0\"\nsource = \"registry+x\"\n",
        )
        .unwrap();
        svc.ensure_for_project("p-1");
        let row = wait_done(&svc, "cargo:p-1");
        assert_eq!(row.state, "done");
        assert_eq!((row.artifacts, row.missing, row.files, row.decls), (1, 0, 1, 2));
        let arts = store.lib_artifacts("cargo:p-1");
        assert_eq!(arts.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["rkyv-0.8.0"]);
        assert!(svc.resolve("rust", "Value", None, &["serde::*".to_string()], &sources).is_empty());
        assert_eq!(svc.resolve("rust", "Archived", None, &["rkyv::Archived".to_string()], &sources)[0].fq_name, "rkyv::Archived");
        let done_events = events.lock().unwrap().iter().filter(|e| e.state == "done").count();
        assert_eq!(done_events, 2);

        // the watcher hook: a lockfile change under the project root re-checks it
        std::fs::write(
            project.root.join("Cargo.lock"),
            "[[package]]\nname = \"rkyv\"\nversion = \"0.8.0\"\nsource = \"registry+x\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.219\"\nsource = \"registry+x\"\n",
        )
        .unwrap();
        let change = |path: &str| crate::git::types::FileChange {
            path: path.into(),
            add: 0,
            del: 0,
            state: "M".into(),
            old_path: None,
        };
        svc.on_changes(&project.root, &[change("src/main.rs")]);
        assert!(!svc.is_indexing("cargo:p-1"), "no lockfile touched");
        svc.on_changes(&t.path().join("elsewhere"), &[change("Cargo.lock")]);
        assert!(!svc.is_indexing("cargo:p-1"), "not a project root");
        svc.on_changes(&project.root, &[change("Cargo.lock")]);
        let row = wait_done(&svc, "cargo:p-1");
        assert_eq!((row.artifacts, row.files, row.decls), (2, 3, 5));
        let arts = store.lib_artifacts("cargo:p-1");
        assert_eq!(arts.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["rkyv-0.8.0", "serde-1.0.219"]);

        // a manual reindex starts over (every artifact again)
        svc.reindex("cargo:p-1").unwrap();
        let row = wait_done(&svc, "cargo:p-1");
        assert_eq!((row.state.as_str(), row.files, row.decls), ("done", 3, 5));
        // the view of a project with no lockfile: nothing
        let empty = t.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        projects.lock().unwrap().push(LibProject {
            id: "p-2".into(),
            name: "empty".into(),
            root: empty,
        });
        assert!(svc.ensure_for_project("p-2").is_empty());
    }

    #[test]
    fn perf_guards_skip_an_artifact_and_count_it() {
        let t = tempfile::tempdir().unwrap();
        let (env, project) = fixture_cargo(t.path());
        // a crate with too many files
        let idx = t.path().join("cargo").join("registry").join("src").join("index.crates.io-x");
        let big = idx.join("big-1.0.0");
        std::fs::create_dir_all(big.join("src")).unwrap();
        std::fs::write(big.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(big.join("src").join("lib.rs"), "pub struct A;\n").unwrap();
        for i in 0..=MAX_ARTIFACT_FILES {
            std::fs::write(big.join("src").join(format!("m{i}.rs")), "").unwrap();
        }
        std::fs::write(
            project.root.join("Cargo.lock"),
            "[[package]]\nname = \"serde\"\nversion = \"1.0.219\"\nsource = \"registry+x\"\n\n[[package]]\nname = \"big\"\nversion = \"1.0.0\"\nsource = \"registry+x\"\n",
        )
        .unwrap();
        let store = Arc::new(SymbolStore::open_in_memory());
        let (svc, _, projects) = service(store.clone(), env);
        projects.lock().unwrap().push(project.clone());
        svc.ensure_for_project("p-1");
        let row = wait_done(&svc, "cargo:p-1");
        assert_eq!(row.state, "done");
        assert_eq!((row.artifacts, row.missing, row.skipped, row.files, row.decls), (2, 0, 1, 2, 3));
        let big_row = store.lib_artifact("cargo:p-1", "big-1.0.0").unwrap();
        assert_eq!(big_row.state, "skipped");
        assert!(big_row.reason.as_deref().unwrap().contains("source files"), "{:?}", big_row.reason);
        assert!(svc.resolve("rust", "A", None, &["big::A".to_string()], &["cargo:p-1".to_string()]).is_empty());
        let view = svc.sources().into_iter().find(|v| v.id == "cargo:p-1").unwrap();
        assert_eq!(view.skipped, 1);
        // a JDK over the file guard is NOT skipped (4001 entries, one declaration each)
        let home = t.path().join("jdk-big");
        std::fs::create_dir_all(home.join("lib")).unwrap();
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(home.join("lib").join("src.zip")).unwrap());
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for i in 0..=MAX_ARTIFACT_FILES {
                w.start_file(format!("java.base/p/C{i}.java"), opts).unwrap();
                w.write_all(format!("package p;\npublic class C{i} {{}}\n").as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        *svc.0.env.lock().unwrap() = DiscoverEnv {
            java_home: Some(home),
            ..DiscoverEnv::default()
        };
        let jdk = svc.discover(true).into_iter().find(|s| s.kind == "jdk").expect("the big jdk");
        svc.start(jdk.clone());
        let row = wait_done(&svc, &jdk.id);
        assert_eq!((row.state.as_str(), row.skipped, row.files as usize), ("done", 0, MAX_ARTIFACT_FILES + 1));
        assert!(row.decls as usize > MAX_ARTIFACT_FILES);
        // the decl guard, through the job driver
        let cancel = AtomicBool::new(false);
        let mut rows: Vec<crate::symbols::store::LibDecl> = Vec::new();
        let end = index::index_cargo_dir(&idx.join("serde-1.0.219"), "serde", &cancel, &mut rows, &mut |_| {}, Limits { max_decls: 2 }).unwrap();
        assert!(matches!(end, JobEnd::Skipped(_)));
    }

    #[test]
    fn maven_source_indexes_jars_with_prefixed_entries() {
        let t = tempfile::tempdir().unwrap();
        let m2 = t.path().join("m2");
        let gson = m2.join("com").join("google").join("code").join("gson").join("gson").join("2.11.0");
        std::fs::create_dir_all(&gson).unwrap();
        let jar = gson.join("gson-2.11.0-sources.jar");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&jar).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("com/google/gson/Gson.java", opts).unwrap();
            w.write_all(b"package com.google.gson;\npublic final class Gson {\n  public String toJson(Object o) { return null; }\n}\n").unwrap();
            w.finish().unwrap();
        }
        let root = t.path().join("shop");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("pom.xml"),
            "<project><dependencies>
               <dependency><groupId>com.google.code.gson</groupId><artifactId>gson</artifactId><version>2.11.0</version></dependency>
               <dependency><groupId>x</groupId><artifactId>absent</artifactId><version>1</version></dependency>
             </dependencies></project>",
        )
        .unwrap();
        let (env, jdk) = fixture_jdk(t.path());
        let env = DiscoverEnv {
            m2_repository: Some(m2),
            ..env
        };
        let store = Arc::new(SymbolStore::open_in_memory());
        let (svc, _, projects) = service(store.clone(), env);
        projects.lock().unwrap().push(LibProject {
            id: "p-9".into(),
            name: "shop".into(),
            root: root.clone(),
        });
        let views = svc.ensure_for_project("p-9");
        let mut ids: Vec<&str> = views.iter().map(|v| v.id.as_str()).collect();
        ids.sort();
        let mut want = vec!["maven:p-9", jdk.id.as_str()];
        want.sort();
        assert_eq!(ids, want, "a pom brings the JDK along");
        let row = wait_done(&svc, "maven:p-9");
        wait_done(&svc, &jdk.id);
        assert_eq!(row.state, "done");
        assert_eq!(row.label, "Maven · shop");
        assert_eq!((row.artifacts, row.missing, row.files, row.decls), (2, 1, 1, 2));
        // java resolution: the project's maven source first, then the JDK
        let hits = svc.hits_for(Some("p-9"), &root, "src/A.java", "Gson", Some("package a;\nimport com.google.gson.Gson;\n"));
        assert_eq!(hits[0].fq_name, "com.google.gson.Gson");
        assert_eq!(hits[0].uri, "orrery-lib://maven:p-9/gson-2.11.0-sources.jar!/com/google/gson/Gson.java");
        assert_eq!(hits[0].artifact, "gson-2.11.0-sources.jar");
        let hits = svc.hits_for(Some("p-9"), &root, "src/A.java", "Map", Some("package a;\nimport java.util.Map;\n"));
        assert_eq!(hits[0].source_id, jdk.id);
        let hits = svc.hits_for(Some("other"), &root, "src/A.java", "Gson", Some("package a;\nimport com.google.gson.Gson;\n"));
        assert!(hits.is_empty(), "another project does not see this pom's jars");
        // failed-jump hints: a jar the pom names is not downloaded
        assert_eq!(svc.hint_for(Some("p-9"), "src/A.java"), Some("sources-missing"));
        assert_eq!(svc.hint_for(Some("other"), "src/A.java"), None, "that project's pom is fine (there is none)");
        assert_eq!(svc.hint_for(Some("p-9"), "src/main.rs"), None, "rust: no hint (v1)");
        assert_eq!(svc.hint_for(None, "src/A.java"), None);
        // read through the jar
        let doc = svc.read("orrery-lib://maven:p-9/gson-2.11.0-sources.jar!/com/google/gson/Gson.java").unwrap();
        assert_eq!(doc.language, "java");
        assert_eq!(doc.title, "Gson.java — gson-2.11.0-sources.jar (com.google.gson)");
        assert!(doc.text.contains("class Gson"));
        assert!(svc.read("orrery-lib://maven:p-9/absent-1-sources.jar!/x/Y.java").is_err());
        assert!(svc.read("orrery-lib://maven:p-9/gson-2.11.0-sources.jar!/nope/Y.java").is_err());
    }

    #[test]
    fn uri_and_entry_validation() {
        assert_eq!(
            parse_lib_uri("orrery-lib://jdk:0a1b/java.base/java/util/Map.java").unwrap(),
            ("jdk:0a1b".into(), "java.base/java/util/Map.java".into())
        );
        assert_eq!(
            parse_lib_uri("orrery-lib://cargo:6f1a-2b/serde-1.0.219/src/lib.rs").unwrap().0,
            "cargo:6f1a-2b"
        );
        assert!(parse_lib_uri("orrery-lib://maven:p/gson-2.11.0-sources.jar!/com/x/Y.java").is_ok());
        for bad in [
            "orrery-lib://jdk:0a1b/../x",
            "orrery-lib://jdk:0a1b/a/../x",
            "orrery-lib://jdk:0a1b//x",
            "orrery-lib://jdk:0a1b/a\\b",
            "orrery-lib://jdk:0a1b/C:/x",
            "orrery-lib://jdk:0a1b/",
            "orrery-lib://jdk:0a1b",
            "orrery-lib://zz/x",
            "orrery-lib://0a1b/x",
            "orrery-lib://npm:x/y",
            "orrery-lib://jdk:/y",
            "orrery-lib://jdk:a b/y",
            "orrery://jdk:0a1b/x",
            "orrery-lib://jdk:0a1b/a/./b",
        ] {
            assert!(parse_lib_uri(bad).is_err(), "{bad}");
        }
        assert!(validate_entry("a/b.java").is_ok());
        assert_eq!(split_entry("cargo", "serde-1.0.219/src/lib.rs"), ("serde-1.0.219", "src/lib.rs"));
        assert_eq!(split_entry("maven", "g-1-sources.jar!/a/B.java"), ("g-1-sources.jar", "a/B.java"));
        assert_eq!(split_entry("jdk", "java.base/java/util/Map.java"), ("src.zip", "java.base/java/util/Map.java"));
        let row = LibSourceRow {
            id: "maven:x".into(),
            kind: "maven".into(),
            label: "Maven · shop".into(),
            state: "done".into(),
            ..Default::default()
        };
        assert_eq!(title_for(&row, "gson-2.11.0-sources.jar!/com/google/gson/Gson.java"), "Gson.java — gson-2.11.0-sources.jar (com.google.gson)");
        assert_eq!(title_for(&row, "gson-2.11.0-sources.jar!/Top.java"), "Top.java — gson-2.11.0-sources.jar");
        let row = LibSourceRow {
            kind: "cargo".into(),
            ..row
        };
        assert_eq!(title_for(&row, "serde-1.0.219/src/de/mod.rs"), "mod.rs — serde-1.0.219 (src/de)");
    }

    #[test]
    fn startup_prunes_vanished_and_orphaned_sources_and_resets_crashed_rows() {
        let t = tempfile::tempdir().unwrap();
        let store = Arc::new(SymbolStore::open_in_memory());
        let (svc, _, projects) = service(store.clone(), DiscoverEnv::default());
        projects.lock().unwrap().push(LibProject {
            id: "p-1".into(),
            name: "a".into(),
            root: t.path().to_path_buf(),
        });
        let existing = t.path().join("Cargo.lock");
        std::fs::write(&existing, b"").unwrap();
        let row = |id: &str, kind: &str, path: &str, state: &str, project: Option<&str>| LibSourceRow {
            id: id.into(),
            kind: kind.into(),
            path: path.into(),
            label: id.into(),
            fingerprint: "f".into(),
            state: state.into(),
            files: 1,
            decls: 1,
            size_bytes: 2,
            indexed_at: 1,
            project_id: project.map(str::to_string),
            project_name: project.map(str::to_string),
            artifacts: 1,
            missing: 0,
            skipped: 0,
        };
        store.lib_upsert_source(&row("jdk:gone", "jdk", &t.path().join("gone.zip").to_string_lossy(), "done", None)).unwrap();
        store.lib_upsert_source(&row("cargo:p-old", "cargo", &existing.to_string_lossy(), "done", Some("p-old"))).unwrap();
        store.lib_upsert_source(&row("cargo:p-1", "cargo", &existing.to_string_lossy(), "indexing", Some("p-1"))).unwrap();
        let done = store
            .lib_upsert_artifact("cargo:p-1", &LibArtifactRow { name: "a-1".into(), kind: "crate".into(), state: "done".into(), files: 1, decls: 1, ..Default::default() })
            .unwrap();
        let half = store
            .lib_upsert_artifact("cargo:p-1", &LibArtifactRow { name: "b-1".into(), kind: "crate".into(), state: "indexing".into(), ..Default::default() })
            .unwrap();
        let d = |fq: &str| crate::symbols::store::LibDecl {
            fq_name: fq.into(),
            simple_name: "B".into(),
            kind: "struct".into(),
            container: None,
            entry: "src/lib.rs".into(),
            line: 0,
            lang: "rust".into(),
        };
        store.lib_insert_decls(done, &[d("a::B")]).unwrap();
        store.lib_insert_decls(half, &[d("b::B")]).unwrap();
        svc.startup();
        assert!(store.lib_source("jdk:gone").is_none());
        assert!(store.lib_source("cargo:p-old").is_none(), "its project is gone");
        let c = store.lib_source("cargo:p-1").unwrap();
        assert_eq!(c.state, "pending");
        assert_eq!(store.lib_decl_count("cargo:p-1"), 1, "the finished artifact keeps its rows");
        let arts = store.lib_artifacts("cargo:p-1");
        assert_eq!(arts.iter().map(|a| (a.name.as_str(), a.state.as_str())).collect::<Vec<_>>(), vec![("a-1", "done"), ("b-1", "pending")]);
    }

    #[test]
    fn ensure_skips_finished_and_removed_sources() {
        let t = tempfile::tempdir().unwrap();
        let (env, src) = fixture_jdk(t.path());
        let store = Arc::new(SymbolStore::open_in_memory());
        let (svc, events, projects) = service(store.clone(), env);
        // probe_root: a java file under the root
        let root = t.path().join("proj");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("A.java"), "class A {}").unwrap();
        assert_eq!(LibSrcService::probe_root(&root), (true, false));
        std::fs::write(root.join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(LibSrcService::probe_root(&root), (true, true));
        projects.lock().unwrap().push(LibProject {
            id: "p-j".into(),
            name: "java".into(),
            root: root.clone(),
        });
        let views = svc.ensure_for_project("p-j");
        assert_eq!(views.len(), 1, "java files, no pom, no lock → the JDK only");
        assert_eq!(views[0].id, src.id);
        let row = wait_done(&svc, &src.id);
        assert_eq!(row.state, "done");
        let n = events.lock().unwrap().len();
        // done → ensure is a no-op
        svc.ensure_for_project("p-j");
        assert!(!svc.is_indexing(&src.id));
        assert_eq!(events.lock().unwrap().len(), n);
        // removed → still a no-op
        svc.remove(&src.id).unwrap();
        svc.ensure_for_project("p-j");
        assert!(!svc.is_indexing(&src.id));
        assert_eq!(svc.sources()[0].state, "removed");
        // an empty root triggers nothing
        let empty = t.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        projects.lock().unwrap().push(LibProject {
            id: "p-e".into(),
            name: "empty".into(),
            root: empty,
        });
        assert!(svc.ensure_for_project("p-e").is_empty());
    }

    /// Real machine smoke: `ORRERY_LIBSRC_SMOKE=1 cargo test --lib libsrc::tests::smoke -- --ignored --nocapture`
    /// — this repo's `src-tauri/Cargo.lock` (or `ORRERY_LIBSRC_SMOKE_ROOT`)
    /// as the project, plus the JDK on this box.
    #[test]
    #[ignore]
    fn smoke_real_cargo_lock_and_jdk() {
        if std::env::var("ORRERY_LIBSRC_SMOKE").as_deref() != Ok("1") {
            eprintln!("ORRERY_LIBSRC_SMOKE != 1 — skipped");
            return;
        }
        let t = tempfile::tempdir().unwrap();
        let db = t.path().join("symbols.db");
        let store = Arc::new(SymbolStore::open(&db).unwrap());
        let mut env = DiscoverEnv::from_process();
        if env.java_home.is_none() {
            env.java_home_from_cmd = discover::java_home_from_cmd();
        }
        let root = std::env::var_os("ORRERY_LIBSRC_SMOKE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf());
        let projects: Projects = Arc::new(Mutex::new(vec![LibProject {
            id: "smoke".into(),
            name: root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            root: root.clone(),
        }]));
        let svc = LibSrcService::new(store.clone(), Arc::new(GrammarRegistry::new()), env, projects, None);
        let all = svc.discover(true);
        for s in &all {
            eprintln!(
                "discovered {} — {} ({} artifacts, {} missing) at {}",
                s.id,
                s.label,
                s.artifacts.len(),
                s.missing(),
                s.path.display()
            );
        }
        let cancel = AtomicBool::new(false);
        for s in &all {
            let started = Instant::now();
            store.lib_upsert_source(&LibSrcService::row_of(s, "indexing")).unwrap();
            svc.run(s, &cancel);
            let row = store.lib_source(&s.id).unwrap();
            eprintln!(
                "{}: {} — {} artifacts, {} missing, {} skipped → {} files, {} decls in {:.1}s",
                s.kind,
                row.state,
                row.artifacts,
                row.missing,
                row.skipped,
                row.files,
                row.decls,
                started.elapsed().as_secs_f64()
            );
            for a in store.lib_artifacts(&s.id).iter().filter(|a| a.state == "skipped" || a.state == "error") {
                eprintln!("  {} {}: {}", a.state, a.name, a.reason.as_deref().unwrap_or(""));
            }
            let mut top: Vec<_> = store.lib_artifacts(&s.id).into_iter().filter(|a| a.state == "done").collect();
            top.sort_by_key(|a| std::cmp::Reverse(a.decls));
            for a in top.iter().take(30) {
                eprintln!("  top {}: {} files, {} decls", a.name, a.files, a.decls);
            }
            // what a tighter declaration guard would cost/save, from the same run
            let total: u64 = top.iter().map(|a| a.decls).sum();
            for t in [10_000u64, 15_000, 20_000, 25_000, 40_000, 50_000, 75_000, 100_000, 150_000] {
                let over: Vec<&LibArtifactRow> = top.iter().filter(|a| a.decls > t).collect();
                let kept = total - over.iter().map(|a| a.decls).sum::<u64>();
                eprintln!(
                    "  guard {t}: would skip {} of {} artifacts, {} decls kept ({:.0}% of {total}) — biggest kept {}",
                    over.len(),
                    top.len(),
                    kept,
                    100.0 * kept as f64 / total.max(1) as f64,
                    top.iter().find(|a| a.decls <= t).map(|a| format!("{} {}", a.name, a.decls)).unwrap_or_default()
                );
            }
        }
        let db_mb = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0);
        let wal_mb = std::fs::metadata(t.path().join("symbols.db-wal")).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0);
        eprintln!("symbols.db: {db_mb:.1} MB (+ wal {wal_mb:.1} MB), pragma size {:.1} MB", store.db_bytes() as f64 / (1024.0 * 1024.0));
        // resolve + read
        let started = Instant::now();
        let hits = svc.hits_for(Some("smoke"), &root, "src-tauri/src/lib.rs", "Deserialize", Some("use serde::Deserialize;\n"));
        eprintln!("resolve serde::Deserialize: {} hits in {:?}: {:?}", hits.len(), started.elapsed(), hits.first().map(|h| (&h.fq_name, &h.uri)));
        if let Some(h) = hits.first() {
            let started = Instant::now();
            let doc = svc.read(&h.uri).unwrap();
            eprintln!("read {} → {} bytes, title '{}' in {:?}", h.uri, doc.text.len(), doc.title, started.elapsed());
        }
        let started = Instant::now();
        let hits = svc.hits_for(Some("smoke"), &root, "src-tauri/src/lib.rs", "Value", Some("use serde_json::Value;\n"));
        eprintln!("resolve serde_json::Value: {:?} in {:?}", hits.first().map(|h| (&h.fq_name, &h.uri)), started.elapsed());
        if all.iter().any(|s| s.kind == "jdk") {
            let started = Instant::now();
            let hits = svc.hits_for(Some("smoke"), &root, "A.java", "ArrayList", Some("package a;\nimport java.util.ArrayList;\n"));
            eprintln!("resolve java.util.ArrayList: {} hits in {:?}: {:?}", hits.len(), started.elapsed(), hits.first().map(|h| &h.fq_name));
            let started = Instant::now();
            let hits = svc.hits_for(Some("smoke"), &root, "A.java", "size", Some("package a;\n"));
            eprintln!("fallback 'size': {} hits in {:?}", hits.len(), started.elapsed());
        }
    }
}
