//! Extension host (M1): tree-sitter grammar packs + language-server packs,
//! fetched from a release registry, sha256-verified, unpacked into versioned
//! dirs under app-data `extensions/<id>/<version>/`, and (for grammars)
//! dlopen'ed into a process-wide [`GrammarRegistry`].
//!
//! Storage: `ext_installed` + `ext_registry_cache` in the shared sqlite DB.
//! Events: `ext://status` (full snapshot) after every mutation, `ext://progress`
//! while an install job runs — both through `core::emit` only.
//!
//! Replacing a mapped dylib is impossible on Windows (file lock) and unsafe
//! everywhere (live `Language` pointers), so an upgrade of a loaded grammar is
//! staged next to it and flagged `pending_version`; `startup()` promotes it
//! before anything loads and prunes every other version dir + `tmp/`.

pub mod commands;
pub mod detect;
pub mod install;
pub mod loader;
pub mod manifest;
pub mod registry;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::core::database::DB;
use crate::core::errors::{AppError, AppResult, DbError};
use crate::settings::SettingsService;

use detect::{resolve_server, DetectEnv, Detection, ResolveOpts, Resolved};
use install::{Activation, InstalledRow};
use loader::{GrammarRegistry, Loaded};
use manifest::{Manifest, RegistryIndex, RegistryPack};
use registry::Fetcher;

pub const EV_STATUS: &str = "ext://status";
pub const EV_PROGRESS: &str = "ext://progress";

/// One catalog row as the UI sees it (registry entry merged with local state).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtPack {
    pub id: String,
    /// `"grammar" | "server" | "runtime"`.
    pub kind: String,
    pub name: String,
    pub description: String,
    /// Latest version in the registry (the installed one when offline-only).
    pub version: String,
    pub installed_version: Option<String>,
    pub languages: Vec<String>,
    pub size_bytes: u64,
    /// Own artifact + every NOT-yet-installed dependency's artifact — what an
    /// install of this pack actually downloads.
    pub bundled_size_bytes: u64,
    /// Pack ids installed automatically with this one.
    pub requires: Vec<String>,
    pub installed: bool,
    pub enabled: bool,
    pub compatible: bool,
    /// The registry's `minOrreryVersion` for this pack — the panel's
    /// "needs Orrery ≥ x" copy when `compatible` is false.
    pub min_app_version: Option<String>,
    pub available_for_target: bool,
    /// `available | downloading | installed | pendingRestart | error | incompatible`.
    pub state: String,
    pub error: Option<String>,
    pub detection: Option<Detection>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtRegistryView {
    pub registry_url: String,
    pub fetched_at: Option<i64>,
    pub offline: bool,
    pub items: Vec<ExtPack>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtProgress {
    /// The pack the user asked for (the job).
    pub id: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    /// `download | verify | unpack | activate`.
    pub phase: String,
    /// The dependency being fetched right now; `None` = the pack itself.
    pub dependency: Option<String>,
    /// 1-based position of the current step among `step_count` packs
    /// (missing deps first, the pack itself last).
    pub step_index: usize,
    pub step_count: usize,
}

/// An installed language-server pack as the LSP host launches it.
#[derive(Debug, Clone)]
pub struct ServerPack {
    pub id: String,
    pub manifest: manifest::ServerManifest,
    pub enabled: bool,
    /// The versioned pack dir (`${extDir}`).
    pub dir: PathBuf,
}

#[derive(Debug, Clone)]
enum Job {
    Downloading,
    Error(String),
}

/// Everything one `pack_view` reads besides its own row.
#[derive(Clone, Copy)]
struct ViewCtx<'a> {
    index: Option<&'a RegistryIndex>,
    rows: &'a HashMap<String, InstalledRow>,
    settings: &'a crate::settings::Settings,
    env: &'a DetectEnv,
    /// runtime kind → binary of the installed runtime pack.
    runtimes: &'a HashMap<String, PathBuf>,
}

struct Inner {
    db: DB,
    settings: SettingsService,
    /// app-data `extensions/`.
    root: PathBuf,
    target: String,
    app_version: String,
    fetcher: Box<dyn Fetcher>,
    grammars: GrammarRegistry,
    /// Per-pack transient state: an install in flight, or the last failure.
    jobs: Mutex<HashMap<String, Job>>,
}

#[derive(Clone)]
pub struct ExtensionService(Arc<Inner>);

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn other(e: impl std::fmt::Display) -> AppError {
    AppError::Other(e.to_string())
}

impl ExtensionService {
    pub fn new(
        db: DB,
        settings: SettingsService,
        root: PathBuf,
        app_version: String,
        fetcher: Box<dyn Fetcher>,
    ) -> Self {
        for d in [root.clone(), root.join("tmp")] {
            if let Err(e) = fs::create_dir_all(&d) {
                log::warn!("extensions: create {}: {e}", d.display());
            }
        }
        let svc = Self(Arc::new(Inner {
            db,
            settings,
            root,
            target: registry::target_key(),
            app_version,
            fetcher,
            grammars: GrammarRegistry::new(),
            jobs: Mutex::new(HashMap::new()),
        }));
        svc.init_schema();
        svc
    }

    fn init_schema(&self) {
        let c = self.0.db.lock().unwrap();
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS ext_installed (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                version TEXT NOT NULL,
                target TEXT NOT NULL,
                dir TEXT NOT NULL,
                sha256 TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                manifest_json TEXT NOT NULL,
                installed_at INTEGER NOT NULL,
                pending_version TEXT NULL
            );
            CREATE TABLE IF NOT EXISTS ext_registry_cache (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                url TEXT NOT NULL DEFAULT '',
                json TEXT NOT NULL,
                fetched_at INTEGER NOT NULL
            );",
        )
        .unwrap();
    }

    // The read surface for the M2 symbols indexer (grammar lookup by file
    // extension) — no production consumer yet, so allow the dead code rather
    // than churn the API when the first caller lands.
    #[allow(dead_code)]
    pub fn root(&self) -> &Path {
        &self.0.root
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.0.root.join("tmp")
    }

    #[allow(dead_code)]
    pub fn target(&self) -> &str {
        &self.0.target
    }

    #[allow(dead_code)]
    pub fn grammars(&self) -> &GrammarRegistry {
        &self.0.grammars
    }

    /// Resident grammar for a file extension (`"java"` / `".java"`).
    #[allow(dead_code)]
    pub fn grammar_for_extension(&self, ext: &str) -> Option<Arc<Loaded>> {
        self.0.grammars.grammar_for_extension(ext)
    }

    // ---- installed rows ----

    fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<InstalledRow> {
        Ok(InstalledRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            version: r.get(2)?,
            target: r.get(3)?,
            dir: r.get(4)?,
            sha256: r.get(5)?,
            enabled: r.get::<_, i64>(6)? != 0,
            manifest_json: r.get(7)?,
            installed_at: r.get(8)?,
            pending_version: r.get(9)?,
        })
    }

    const ROW_COLS: &'static str =
        "id, kind, version, target, dir, sha256, enabled, manifest_json, installed_at, pending_version";

    pub fn rows(&self) -> AppResult<Vec<InstalledRow>> {
        let c = self.0.db.lock().unwrap();
        let mut st = c
            .prepare(&format!(
                "SELECT {} FROM ext_installed ORDER BY id",
                Self::ROW_COLS
            ))
            .map_err(DbError::Sqlite)?;
        let rows = st
            .query_map([], Self::row_from)
            .map_err(DbError::Sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(DbError::Sqlite)?;
        Ok(rows)
    }

    pub fn row(&self, id: &str) -> AppResult<Option<InstalledRow>> {
        let c = self.0.db.lock().unwrap();
        c.query_row(
            &format!("SELECT {} FROM ext_installed WHERE id = ?1", Self::ROW_COLS),
            [id],
            Self::row_from,
        )
        .optional()
        .map_err(|e| DbError::Sqlite(e).into())
    }

    fn upsert_row(&self, r: &InstalledRow) -> AppResult<()> {
        let c = self.0.db.lock().unwrap();
        c.execute(
            "INSERT INTO ext_installed (id, kind, version, target, dir, sha256, enabled, manifest_json, installed_at, pending_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, version = excluded.version,
               target = excluded.target, dir = excluded.dir, sha256 = excluded.sha256,
               enabled = excluded.enabled, manifest_json = excluded.manifest_json,
               installed_at = excluded.installed_at, pending_version = excluded.pending_version",
            params![
                r.id,
                r.kind,
                r.version,
                r.target,
                r.dir,
                r.sha256,
                r.enabled as i64,
                r.manifest_json,
                r.installed_at,
                r.pending_version
            ],
        )
        .map_err(DbError::Sqlite)?;
        Ok(())
    }

    fn delete_row(&self, id: &str) -> AppResult<()> {
        let c = self.0.db.lock().unwrap();
        c.execute("DELETE FROM ext_installed WHERE id = ?1", [id])
            .map_err(DbError::Sqlite)?;
        Ok(())
    }

    // ---- registry cache ----

    fn read_cache(&self) -> Option<(String, String, i64)> {
        let c = self.0.db.lock().unwrap();
        c.query_row(
            "SELECT url, json, fetched_at FROM ext_registry_cache WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .ok()
        .flatten()
    }

    fn write_cache(&self, url: &str, json: &str, at: i64) {
        let c = self.0.db.lock().unwrap();
        if let Err(e) = c.execute(
            "INSERT INTO ext_registry_cache (id, url, json, fetched_at) VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET url = excluded.url, json = excluded.json, fetched_at = excluded.fetched_at",
            params![url, json, at],
        ) {
            log::warn!("extensions: cache write: {e}");
        }
    }

    /// The settings override when it is a valid registry URL, else the default.
    pub fn registry_url(&self) -> String {
        let custom = self
            .0
            .settings
            .get()
            .ok()
            .and_then(|s| s.ext_registry_url)
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty());
        match custom {
            Some(u) => match registry::validate_registry_url(&u) {
                Ok(()) => u,
                Err(e) => {
                    log::warn!("extensions: ignoring extRegistryUrl: {e}");
                    registry::DEFAULT_REGISTRY_URL.to_string()
                }
            },
            // dev: a freshly built dist-ext/ next to the repo wins over the
            // built-in registry (release builds never take this branch)
            None => registry::local_dev_registry_url().unwrap_or_else(|| registry::DEFAULT_REGISTRY_URL.to_string()),
        }
    }

    fn fetch_index(&self, url: &str) -> Result<String, String> {
        registry::validate_registry_url(url)?;
        let bytes = registry::fetch_bytes(self.0.fetcher.as_ref(), url, registry::MAX_INDEX_BYTES)?;
        let json = String::from_utf8(bytes).map_err(|_| "index: not utf-8".to_string())?;
        RegistryIndex::parse(&json)?;
        Ok(json)
    }

    /// `(index, fetched_at, offline)`. Fresh cache → no network. `refresh` or a
    /// stale cache → fetch (when `allow_fetch`); a failed fetch falls back to
    /// the cache at any age and reports `offline`.
    fn index(&self, refresh: bool, allow_fetch: bool) -> (Option<RegistryIndex>, Option<i64>, bool) {
        let url = self.registry_url();
        let cached = self.read_cache().filter(|(u, _, _)| *u == url);
        let now = now();
        let fresh = cached
            .as_ref()
            .is_some_and(|(_, _, at)| now - at < registry::CACHE_TTL_SECS);
        let mut offline = false;
        if allow_fetch && (refresh || !fresh) {
            match self.fetch_index(&url) {
                Ok(json) => {
                    self.write_cache(&url, &json, now);
                    return (RegistryIndex::parse(&json).ok(), Some(now), false);
                }
                Err(e) => {
                    log::warn!("extensions: registry fetch failed: {e}");
                    offline = true;
                }
            }
        }
        match cached {
            Some((_, json, at)) => (RegistryIndex::parse(&json).ok(), Some(at), offline),
            None => (None, None, true),
        }
    }

    // ---- startup ----

    /// Promote pending versions, prune `tmp/` + stale version dirs, load every
    /// enabled grammar. Runs once, before any grammar consumer exists.
    pub fn startup(&self) {
        let rows = match self.rows() {
            Ok(r) => r,
            Err(e) => {
                log::error!("extensions: startup: {e}");
                return;
            }
        };
        let mut live = Vec::with_capacity(rows.len());
        for row in rows {
            let had_pending = row.pending_version.is_some();
            let row = install::activate_pending(row);
            if had_pending {
                if let Err(e) = self.upsert_row(&row) {
                    log::warn!("extensions: activate {}: {e}", row.id);
                }
            }
            live.push(row);
        }
        self.prune(&live);
        for row in &live {
            if row.kind == "grammar" && row.enabled {
                if let Err(e) = self.load_grammar_row(row) {
                    log::warn!("extensions: {}: {e}", row.id);
                    self.0.jobs.lock().unwrap().insert(row.id.clone(), Job::Error(e));
                }
            }
        }
        log::info!(
            "extensions: {} pack(s) installed, grammars loaded: {:?}",
            live.len(),
            self.0.grammars.loaded_ids()
        );
    }

    /// Delete everything in `tmp/`, every dir for an id that has no row, and
    /// every version dir that is neither live nor pending. Best-effort: a
    /// locked file (mapped dylib) is retried at the next startup.
    fn prune(&self, rows: &[InstalledRow]) {
        let tmp = self.tmp_dir();
        if let Ok(rd) = fs::read_dir(&tmp) {
            for e in rd.flatten() {
                let p = e.path();
                let _ = if p.is_dir() {
                    fs::remove_dir_all(&p)
                } else {
                    fs::remove_file(&p)
                };
            }
        }
        let by_id: HashMap<&str, &InstalledRow> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
        let Ok(rd) = fs::read_dir(&self.0.root) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name == "tmp" || !p.is_dir() {
                continue;
            }
            match by_id.get(name.as_str()) {
                None => {
                    if let Err(err) = fs::remove_dir_all(&p) {
                        log::debug!("extensions: prune {}: {err}", p.display());
                    }
                }
                Some(row) => self.prune_versions(row),
            }
        }
    }

    fn prune_versions(&self, row: &InstalledRow) {
        let keep: HashSet<&str> = [Some(row.version.as_str()), row.pending_version.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        let Ok(rd) = fs::read_dir(self.0.root.join(&row.id)) else {
            return;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() && !keep.contains(name.as_str()) {
                if let Err(err) = fs::remove_dir_all(e.path()) {
                    log::debug!("extensions: prune {}: {err}", e.path().display());
                }
            }
        }
    }

    /// sha256 re-check of the dylib, then dlopen + register.
    fn load_grammar_row(&self, row: &InstalledRow) -> Result<(), String> {
        if self.0.grammars.loaded_version(&row.id).as_deref() == Some(row.version.as_str()) {
            return Ok(());
        }
        let Manifest::Grammar(g) = Manifest::parse(&row.manifest_json)? else {
            return Err(format!("{}: row is not a grammar", row.id));
        };
        let dir = PathBuf::from(&row.dir);
        install::verify_sha256(&dir.join(&g.library), &row.sha256)
            .map_err(|e| format!("library changed since install ({e})"))?;
        let loaded = loader::load(&dir, &g)?;
        self.0.grammars.insert(loaded);
        Ok(())
    }

    // ---- view ----

    /// Catalog merged with local state; hits the network when the cache is
    /// stale (or `refresh`).
    pub fn view(&self, refresh: bool) -> ExtRegistryView {
        self.view_opts(refresh, true)
    }

    /// Same, cache-only (what `ext://status` carries).
    pub fn snapshot(&self) -> ExtRegistryView {
        self.view_opts(false, false)
    }

    fn view_opts(&self, refresh: bool, allow_fetch: bool) -> ExtRegistryView {
        let (index, fetched_at, offline) = self.index(refresh, allow_fetch);
        let rows: HashMap<String, InstalledRow> = self
            .rows()
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();
        let jobs = self.0.jobs.lock().unwrap().clone();
        let settings = self.0.settings.get().unwrap_or_default();
        let env = DetectEnv::current();
        let runtimes = self.runtime_binaries(&rows);
        let ctx = ViewCtx {
            index: index.as_ref(),
            rows: &rows,
            settings: &settings,
            env: &env,
            runtimes: &runtimes,
        };
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        if let Some(idx) = &index {
            for pack in &idx.packs {
                seen.insert(pack.id.clone());
                items.push(self.pack_view(Some(pack), rows.get(&pack.id), jobs.get(&pack.id), &ctx));
            }
        }
        for row in rows.values() {
            if !seen.contains(&row.id) {
                items.push(self.pack_view(None, Some(row), jobs.get(&row.id), &ctx));
            }
        }
        items.sort_by(|a, b| a.id.cmp(&b.id));
        ExtRegistryView {
            registry_url: self.registry_url(),
            fetched_at,
            offline,
            items,
        }
    }

    fn pack_view(
        &self,
        pack: Option<&RegistryPack>,
        row: Option<&InstalledRow>,
        job: Option<&Job>,
        ctx: &ViewCtx<'_>,
    ) -> ExtPack {
        let ViewCtx { index, rows, settings, env, runtimes } = *ctx;
        let manifest = row.and_then(|r| Manifest::parse(&r.manifest_json).ok());
        let id = pack.map(|p| p.id.clone()).or_else(|| row.map(|r| r.id.clone())).unwrap_or_default();
        let kind = pack.map(|p| p.kind.clone()).or_else(|| row.map(|r| r.kind.clone())).unwrap_or_default();
        let name = pack
            .map(RegistryPack::display_name)
            .or_else(|| manifest.as_ref().map(|m| m.label().to_string()).filter(|l| !l.is_empty()))
            .unwrap_or_else(|| id.clone());
        let description = pack.map(|p| p.description.clone()).unwrap_or_default();
        let version = pack.map(|p| p.version.clone()).or_else(|| row.map(|r| r.version.clone())).unwrap_or_default();
        let languages = pack
            .map(|p| p.languages.clone())
            .filter(|l| !l.is_empty())
            .or_else(|| manifest.as_ref().map(Manifest::languages))
            .unwrap_or_default();
        let picked = pack.and_then(|p| registry::pick_target(&p.targets, &self.0.target));
        let size_bytes = picked.map(|(_, t)| t.size).unwrap_or(0);
        let requires: Vec<String> = pack
            .map(|p| p.requires.clone())
            .filter(|r| !r.is_empty())
            .or_else(|| manifest.as_ref().map(|m| m.requires().to_vec()))
            .unwrap_or_default();
        // Own artifact + the artifacts of deps not installed yet (transitive).
        let bundled_size_bytes = size_bytes
            + index
                .and_then(|idx| manifest::resolve_requires(idx, &id).ok())
                .unwrap_or_default()
                .iter()
                .filter(|dep| !rows.contains_key(*dep))
                .filter_map(|dep| index?.packs.iter().find(|p| &p.id == dep))
                .filter_map(|p| registry::pick_target(&p.targets, &self.0.target))
                .map(|(_, t)| t.size)
                .sum::<u64>();
        let (min_ver, abi) = match (pack, &manifest) {
            (Some(p), _) => (p.min_orrery_version.clone(), p.abi),
            (None, Some(m)) => (m.min_orrery_version().map(str::to_string), m.abi()),
            (None, None) => (None, None),
        };
        let compatible = registry::app_version_ok(min_ver.as_deref(), &self.0.app_version)
            && registry::abi_ok(abi);
        let installed = row.is_some();
        let available_for_target = match pack {
            Some(_) => picked.is_some(),
            None => installed,
        };
        let (state, error) = match (job, row) {
            (Some(Job::Downloading), _) => ("downloading", None),
            (Some(Job::Error(e)), _) => ("error", Some(e.clone())),
            (None, Some(r)) if r.pending_version.is_some() => ("pendingRestart", None),
            (None, Some(_)) => ("installed", None),
            (None, None) if !compatible || !available_for_target => ("incompatible", None),
            (None, None) => ("available", None),
        };
        let detection = if kind == "server" {
            let configured = settings.lsp_paths.get(&id).map(String::as_str);
            match &manifest {
                Some(Manifest::Server(s)) => {
                    let runtime_bin = s
                        .launch
                        .as_ref()
                        .and_then(|l| l.runtime.as_deref())
                        .and_then(|rt| runtimes.get(rt));
                    Some(
                        resolve_server(
                            s,
                            &ResolveOpts {
                                pack_dir: row.map(|r| Path::new(&r.dir)),
                                runtime_bin: runtime_bin.map(PathBuf::as_path),
                                configured,
                                use_system: settings.lsp_use_system_servers,
                                target: &self.0.target,
                            },
                            env,
                        )
                        .detection(),
                    )
                }
                _ => configured.filter(|c| !c.trim().is_empty()).map(|c| Detection {
                    status: "configured".into(),
                    path: Some(c.to_string()),
                    hint: None,
                }),
            }
        } else {
            None
        };
        ExtPack {
            id,
            kind,
            name,
            description,
            version,
            installed_version: row.map(|r| r.version.clone()),
            languages,
            size_bytes,
            bundled_size_bytes,
            requires,
            installed,
            enabled: row.map(|r| r.enabled).unwrap_or(false),
            compatible,
            min_app_version: min_ver.clone(),
            available_for_target,
            state: state.into(),
            error,
            detection,
        }
    }

    pub fn emit_status(&self, app: &tauri::AppHandle) {
        if let Err(e) = crate::core::emit::emit_tracked(app, EV_STATUS, &self.snapshot()) {
            log::warn!("extensions: emit status: {e}");
        }
    }

    // ---- server packs (the LSP host's read surface) ----

    fn server_pack_from(row: InstalledRow) -> Option<ServerPack> {
        if row.kind != "server" {
            return None;
        }
        let Ok(Manifest::Server(manifest)) = Manifest::parse(&row.manifest_json) else {
            return None;
        };
        Some(ServerPack {
            id: row.id,
            manifest,
            enabled: row.enabled,
            dir: PathBuf::from(row.dir),
        })
    }

    /// The installed pack `id` (any enabled state), manifest parsed.
    pub fn server_pack(&self, id: &str) -> Option<ServerPack> {
        self.row(id).ok().flatten().and_then(Self::server_pack_from)
    }

    /// The installed + enabled server pack whose `languages` lists `lang`.
    pub fn server_for_language(&self, lang: &str) -> Option<ServerPack> {
        self.rows()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.kind == "server" && r.enabled)
            .filter_map(Self::server_pack_from)
            .find(|p| p.manifest.languages.iter().any(|l| l == lang))
    }

    /// Where `pack` launches from right now (bundled → manual path → system
    /// when enabled → missing). Probes the filesystem: call at spawn time only.
    pub fn resolve_server_pack(&self, pack: &ServerPack) -> Resolved {
        let settings = self.0.settings.get().unwrap_or_default();
        let runtime_bin = pack
            .manifest
            .launch
            .as_ref()
            .and_then(|l| l.runtime.as_deref())
            .and_then(|rt| self.runtime_binary(rt));
        resolve_server(
            &pack.manifest,
            &ResolveOpts {
                pack_dir: Some(&pack.dir),
                runtime_bin: runtime_bin.as_deref(),
                configured: settings.lsp_paths.get(&pack.id).map(String::as_str),
                use_system: settings.lsp_use_system_servers,
                target: &self.0.target,
            },
            &DetectEnv::current(),
        )
    }

    // ---- runtime packs ----

    /// `runtime kind` (`"node"` / `"java"`) → the installed + enabled runtime
    /// pack's binary, whether or not the file exists (the resolver reports a
    /// vanished binary as a hint rather than silently falling through).
    fn runtime_binaries(&self, rows: &HashMap<String, InstalledRow>) -> HashMap<String, PathBuf> {
        rows.values()
            .filter(|r| r.kind == "runtime" && r.enabled)
            .filter_map(|r| match Manifest::parse(&r.manifest_json) {
                Ok(Manifest::Runtime(m)) => Some((m.runtime, Path::new(&r.dir).join(m.binary))),
                _ => None,
            })
            .collect()
    }

    /// The binary of the installed runtime pack for `runtime` (`node` / `java`).
    pub fn runtime_binary(&self, runtime: &str) -> Option<PathBuf> {
        let rows: HashMap<String, InstalledRow> = self
            .rows()
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();
        self.runtime_binaries(&rows).remove(runtime)
    }

    /// Installed server packs whose `requires` names `id` (uninstall guard).
    fn dependents_of(&self, id: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .rows()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.id != id)
            .filter(|r| {
                Manifest::parse(&r.manifest_json)
                    .map(|m| m.requires().iter().any(|d| d == id))
                    .unwrap_or(false)
            })
            .map(|r| r.id)
            .collect();
        out.sort();
        out
    }

    // ---- mutations ----

    /// Start an install job; returns once the job thread is running. Progress
    /// and the final snapshot arrive as events.
    pub fn install(&self, app: tauri::AppHandle, id: String) -> AppResult<()> {
        manifest::validate_id(&id).map_err(other)?;
        {
            let mut jobs = self.0.jobs.lock().unwrap();
            if matches!(jobs.get(&id), Some(Job::Downloading)) {
                return Err(other(format!("{id}: install already in progress")));
            }
            jobs.insert(id.clone(), Job::Downloading);
        }
        let svc = self.clone();
        let job_id = id.clone();
        std::thread::Builder::new()
            .name(format!("ext-install-{id}"))
            .spawn(move || {
                let id = job_id;
                let mut progress = |p: ExtProgress| {
                    let _ = crate::core::emit::emit_tracked(&app, EV_PROGRESS, &p);
                };
                let result = svc.run_install(&id, &mut progress);
                {
                    let mut jobs = svc.0.jobs.lock().unwrap();
                    match &result {
                        Ok(()) => {
                            jobs.remove(&id);
                        }
                        Err(e) => {
                            log::warn!("extensions: install {id}: {e}");
                            jobs.insert(id.clone(), Job::Error(e.clone()));
                        }
                    }
                }
                svc.emit_status(&app);
            })
            .map_err(|e| {
                self.0.jobs.lock().unwrap().remove(&id);
                other(format!("spawn install thread: {e}"))
            })?;
        Ok(())
    }

    /// The install job for `id`: every dependency it `requires` (transitively)
    /// that is not installed yet, deepest first, then the pack itself — all on
    /// this thread, one `ExtProgress` stream tagged with the current step. A
    /// failing dependency aborts before the pack is touched; the deps already
    /// installed stay (they are complete packs on their own).
    pub fn run_install(&self, id: &str, progress: &mut dyn FnMut(ExtProgress)) -> Result<(), String> {
        let (index, _, _) = self.index(false, true);
        let index = index.ok_or_else(|| "registry unavailable (offline?)".to_string())?;
        let deps = manifest::resolve_requires(&index, id)?;
        let missing: Vec<String> = deps
            .into_iter()
            .filter(|d| self.row(d).ok().flatten().is_none())
            .collect();
        let step_count = missing.len() + 1;
        for (i, dep) in missing.iter().enumerate() {
            log::info!("extensions: {id} requires {dep} — installing it first ({}/{step_count})", i + 1);
            let mut tagged = |mut p: ExtProgress| {
                p.id = id.to_string();
                p.dependency = Some(dep.clone());
                p.step_index = i + 1;
                p.step_count = step_count;
                progress(p);
            };
            self.run_install_one(&index, dep, &mut tagged)
                .map_err(|e| format!("{id}: dependency {dep}: {e}"))?;
        }
        let mut tagged = |mut p: ExtProgress| {
            p.step_index = step_count;
            p.step_count = step_count;
            progress(p);
        };
        self.run_install_one(&index, id, &mut tagged)
    }

    /// One pack, synchronous: registry lookup → gates → download to `tmp/` →
    /// sha256 → unzip → manifest check → probe-load → move into
    /// `<id>/<version>/` → DB row → (grammar) load. The staging dir is always
    /// removed afterwards; on failure nothing installed changes.
    fn run_install_one(
        &self,
        index: &RegistryIndex,
        id: &str,
        progress: &mut dyn FnMut(ExtProgress),
    ) -> Result<(), String> {
        let pack = index
            .packs
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("{id}: not in the registry"))?;
        if !registry::app_version_ok(pack.min_orrery_version.as_deref(), &self.0.app_version) {
            return Err(format!(
                "{id}: needs Orrery {}+ (this is {})",
                pack.min_orrery_version.as_deref().unwrap_or("?"),
                self.0.app_version
            ));
        }
        if !registry::abi_ok(pack.abi) {
            return Err(format!("{id}: grammar abi {:?} not supported by this build", pack.abi));
        }
        let (tkey, art) = registry::pick_target(&pack.targets, &self.0.target)
            .ok_or_else(|| format!("{id}: no build for {}", self.0.target))?;
        if art.size > registry::MAX_ARTIFACT_BYTES {
            return Err(format!("{id}: artifact too large ({} bytes)", art.size));
        }
        if pack.kind == "grammar"
            && self.0.grammars.loaded_version(id).as_deref() == Some(pack.version.as_str())
        {
            return Err(format!(
                "{id} {} is already loaded — restart Orrery to reinstall it",
                pack.version
            ));
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let stage = self.tmp_dir().join(format!("{id}-{}-{stamp}", pack.version));
        fs::create_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
        let result = self.stage_and_activate(pack, tkey, art, &stage, progress);
        if let Err(e) = fs::remove_dir_all(&stage) {
            log::debug!("extensions: stage cleanup {}: {e}", stage.display());
        }
        result
    }

    fn stage_and_activate(
        &self,
        pack: &RegistryPack,
        tkey: &str,
        art: &manifest::RegistryTarget,
        stage: &Path,
        progress: &mut dyn FnMut(ExtProgress),
    ) -> Result<(), String> {
        let id = pack.id.as_str();
        let zip_path = stage.join("pack.zip");
        {
            let mut f = fs::File::create(&zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
            let mut downloaded: u64 = 0;
            self.0.fetcher.get(&art.url, &mut |chunk, total| {
                downloaded += chunk.len() as u64;
                if downloaded > registry::MAX_ARTIFACT_BYTES {
                    return Err(format!("{id}: download exceeds size cap"));
                }
                f.write_all(chunk).map_err(|e| format!("{id}: write: {e}"))?;
                progress(ExtProgress {
                    id: id.into(),
                    downloaded,
                    total: total.or(if art.size > 0 { Some(art.size) } else { None }),
                    phase: "download".into(),
                    dependency: None,
                    step_index: 1,
                    step_count: 1,
                });
                Ok(())
            })?;
            f.flush().map_err(|e| format!("{id}: flush: {e}"))?;
        }
        let phase = |progress: &mut dyn FnMut(ExtProgress), phase: &str| {
            progress(ExtProgress {
                id: id.into(),
                downloaded: art.size,
                total: Some(art.size),
                phase: phase.into(),
                dependency: None,
                step_index: 1,
                step_count: 1,
            })
        };
        phase(progress, "verify");
        install::verify_sha256(&zip_path, &art.sha256)?;
        phase(progress, "unpack");
        let unpacked = stage.join("unpacked");
        install::unzip_guarded(&zip_path, &unpacked)?;
        let (root, manifest, raw) = install::read_manifest(&unpacked)?;
        if manifest.id() != id {
            return Err(format!("{id}: pack manifest is for {:?}", manifest.id()));
        }
        if manifest.kind() != pack.kind {
            return Err(format!("{id}: manifest kind {:?} ≠ registry {:?}", manifest.kind(), pack.kind));
        }
        if manifest.version() != pack.version {
            return Err(format!(
                "{id}: manifest version {} ≠ registry {}",
                manifest.version(),
                pack.version
            ));
        }
        if !registry::app_version_ok(manifest.min_orrery_version(), &self.0.app_version) {
            return Err(format!(
                "{id}: needs Orrery {}+",
                manifest.min_orrery_version().unwrap_or("?")
            ));
        }
        phase(progress, "activate");
        let claimed = manifest.target();
        if !claimed.is_empty() && claimed != self.0.target {
            return Err(format!("{id}: built for {claimed}, this is {}", self.0.target));
        }
        let sha256 = match &manifest {
            Manifest::Grammar(g) => {
                loader::probe(&root, g)?;
                install::sha256_file(&root.join(&g.library))?
            }
            Manifest::Runtime(r) => {
                if !root.join(&r.binary).is_file() {
                    return Err(format!("{id}: runtime binary {} missing from the pack", r.binary));
                }
                art.sha256.to_ascii_lowercase()
            }
            Manifest::Server(s) => {
                if let Some(l) = &s.launch {
                    detect::resolve_entry(&root, &l.entry)
                        .map_err(|e| format!("{id}: launch entry: {e}"))?;
                }
                art.sha256.to_ascii_lowercase()
            }
        };
        let final_dir = self.0.root.join(id).join(&pack.version);
        if final_dir.exists() {
            fs::remove_dir_all(&final_dir).map_err(|e| {
                format!(
                    "{id}: cannot replace {} ({e}) — restart Orrery and retry",
                    final_dir.display()
                )
            })?;
        }
        if let Some(parent) = final_dir.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::rename(&root, &final_dir)
            .map_err(|e| format!("{id}: move into {}: {e}", final_dir.display()))?;
        let current = self.row(id).map_err(|e| e.to_string())?;
        let loaded = pack.kind == "grammar" && self.0.grammars.is_loaded(id);
        let new_row = InstalledRow {
            id: id.into(),
            kind: pack.kind.clone(),
            version: pack.version.clone(),
            target: tkey.into(),
            dir: final_dir.to_string_lossy().into_owned(),
            sha256,
            enabled: current.as_ref().map(|r| r.enabled).unwrap_or(true),
            manifest_json: raw,
            installed_at: now(),
            pending_version: None,
        };
        let (row, activation) = install::apply_install(current, new_row, loaded);
        self.upsert_row(&row).map_err(|e| e.to_string())?;
        self.prune_versions(&row);
        if activation == Activation::Immediate && row.enabled {
            if let Manifest::Grammar(g) = &manifest {
                self.0.grammars.insert(loader::load(&final_dir, g)?);
            }
        }
        log::info!(
            "extensions: installed {id} {} ({:?})",
            pack.version,
            activation
        );
        Ok(())
    }

    /// Forget the pack: registry entry, DB row, dir. A mapped dylib keeps its
    /// dir locked on Windows — the leftover is pruned at the next startup.
    pub fn uninstall(&self, id: &str) -> AppResult<()> {
        manifest::validate_id(id).map_err(other)?;
        let Some(row) = self.row(id)? else {
            return Err(other(format!("{id}: not installed")));
        };
        if row.kind == "runtime" {
            let dependents = self.dependents_of(id);
            if !dependents.is_empty() {
                return Err(other(format!(
                    "{id}: required by {} — uninstall those first",
                    dependents.join(", ")
                )));
            }
        }
        self.0.grammars.remove(id);
        self.delete_row(id)?;
        self.0.jobs.lock().unwrap().remove(id);
        let dir = self.0.root.join(id);
        if let Err(e) = fs::remove_dir_all(&dir) {
            log::info!("extensions: {} kept until restart: {e}", dir.display());
        }
        Ok(())
    }

    /// Toggle a pack. Grammars load/unregister immediately (the image stays
    /// mapped when disabled; nothing consults it).
    pub fn set_enabled(&self, id: &str, enabled: bool) -> AppResult<()> {
        let Some(mut row) = self.row(id)? else {
            return Err(other(format!("{id}: not installed")));
        };
        row.enabled = enabled;
        self.upsert_row(&row)?;
        self.0.jobs.lock().unwrap().remove(id);
        if row.kind == "grammar" {
            if enabled {
                if let Err(e) = self.load_grammar_row(&row) {
                    self.0.jobs.lock().unwrap().insert(id.to_string(), Job::Error(e.clone()));
                    return Err(other(e));
                }
            } else {
                self.0.grammars.remove(id);
            }
        }
        Ok(())
    }

    /// Manual server path (`settings.lspPaths[id]`); `None`/empty clears it.
    pub fn set_path(&self, id: &str, path: Option<String>) -> AppResult<()> {
        manifest::validate_id(id).map_err(other)?;
        let mut s = self.0.settings.get()?;
        match path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
            Some(p) => {
                s.lsp_paths.insert(id.to_string(), p);
            }
            None => {
                s.lsp_paths.remove(id);
            }
        }
        self.0.settings.set(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::install::testutil::write_zip;
    use crate::extensions::manifest::fixtures;
    use crate::extensions::registry::Sink;
    use rusqlite::Connection;
    use std::collections::BTreeMap;

    /// In-memory registry + artifacts; `fail` simulates being offline.
    struct MemFetcher {
        files: Mutex<BTreeMap<String, Vec<u8>>>,
        fail: Mutex<bool>,
        hits: Mutex<Vec<String>>,
    }

    impl MemFetcher {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                files: Mutex::new(BTreeMap::new()),
                fail: Mutex::new(false),
                hits: Mutex::new(Vec::new()),
            })
        }
        fn put(&self, url: &str, bytes: Vec<u8>) {
            self.files.lock().unwrap().insert(url.into(), bytes);
        }
        fn set_fail(&self, f: bool) {
            *self.fail.lock().unwrap() = f;
        }
        fn hits(&self) -> usize {
            self.hits.lock().unwrap().len()
        }
    }

    struct Shared(Arc<MemFetcher>);
    impl Fetcher for Shared {
        fn get(&self, url: &str, sink: &mut Sink<'_>) -> Result<(), String> {
            self.0.hits.lock().unwrap().push(url.into());
            if *self.0.fail.lock().unwrap() {
                return Err("simulated offline".into());
            }
            let bytes = self
                .0
                .files
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| format!("404 {url}"))?;
            for c in bytes.chunks(1024) {
                sink(c, Some(bytes.len() as u64))?;
            }
            Ok(())
        }
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        svc: ExtensionService,
        fetcher: Arc<MemFetcher>,
        settings: SettingsService,
    }

    const URL: &str = "https://registry.test/index.json";

    fn fixture(app_version: &str) -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let db: DB = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        let settings = SettingsService::new(db.clone());
        let mut s = settings.get().unwrap();
        s.ext_registry_url = Some(URL.into());
        settings.set(&s).unwrap();
        let fetcher = MemFetcher::new();
        let svc = ExtensionService::new(
            db,
            settings.clone(),
            tmp.path().join("extensions"),
            app_version.into(),
            Box::new(Shared(fetcher.clone())),
        );
        Fixture {
            _tmp: tmp,
            svc,
            fetcher,
            settings,
        }
    }

    fn server_zip(version: &str) -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.zip");
        let manifest = fixtures::SERVER
            .replace("1.40.0-1", version)
            .replace("\"minOrreryVersion\": \"0.24.0\"", "\"minOrreryVersion\": \"0.1.0\"");
        write_zip(&p, &[("manifest.json", manifest.as_bytes())]);
        fs::read(p).unwrap()
    }

    /// A self-contained ts-ls pack: manifest + the launch entry file.
    fn bundled_server_zip() -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.zip");
        let manifest = fixtures::SERVER_BUNDLED
            .replace("\"minOrreryVersion\": \"0.24.0\"", "\"minOrreryVersion\": \"0.1.0\"");
        write_zip(
            &p,
            &[
                ("manifest.json", manifest.as_bytes()),
                ("server/node_modules/typescript-language-server/lib/cli.mjs", b"// cli"),
                ("server/node_modules/typescript/lib/tsserver.js", b"// tsserver"),
            ],
        );
        fs::read(p).unwrap()
    }

    /// A runtime.node pack for THIS target with a fake `node.exe`.
    fn runtime_zip() -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("r.zip");
        let manifest = fixtures::RUNTIME_NODE
            .replace("\"minOrreryVersion\": \"0.24.0\"", "\"minOrreryVersion\": \"0.1.0\"")
            .replace("\"target\": \"windows-x86_64\"", &format!("\"target\": \"{}\"", registry::target_key()));
        write_zip(&p, &[("manifest.json", manifest.as_bytes()), ("node.exe", b"MZ")]);
        fs::read(p).unwrap()
    }

    /// Index with `requires` edges: `(id, kind, version, bytes, target, requires)`.
    fn index_json_with_requires(packs: &[(&str, &str, &str, &[u8], &str, &[&str])]) -> String {
        let plain: Vec<(&str, &str, &str, &[u8], &str)> =
            packs.iter().map(|(a, b, c, d, e, _)| (*a, *b, *c, *d, *e)).collect();
        let mut v: serde_json::Value = serde_json::from_str(&index_json(&plain)).unwrap();
        for (i, (_, _, _, _, _, req)) in packs.iter().enumerate() {
            v["packs"][i]["requires"] = serde_json::json!(req);
        }
        v.to_string()
    }

    fn publish_with_requires(f: &Fixture, packs: &[(&str, &str, &str, &[u8], &str, &[&str])]) {
        f.fetcher.put(URL, index_json_with_requires(packs).into_bytes());
        for (id, _, version, bytes, _, _) in packs {
            f.fetcher
                .put(&format!("https://registry.test/{id}-{version}.zip"), bytes.to_vec());
        }
    }

    const TS: &str = "server.typescript-language-server";

    #[test]
    fn install_pulls_missing_dependencies_first_with_tagged_progress() {
        let f = fixture("0.30.0");
        let (szip, rzip) = (bundled_server_zip(), runtime_zip());
        let target = registry::target_key();
        publish_with_requires(
            &f,
            &[
                (TS, "server", "6.0.0-1", &szip, "any", &["runtime.node"]),
                ("runtime.node", "runtime", "22.12.0-1", &rzip, target.as_str(), &[]),
            ],
        );
        // the catalog reports what an install would download
        let v = f.svc.view(false);
        let p = item(&v, TS);
        assert_eq!(p.requires, vec!["runtime.node"]);
        assert_eq!(p.size_bytes, szip.len() as u64);
        assert_eq!(p.bundled_size_bytes, (szip.len() + rzip.len()) as u64, "own + missing dep");
        let r = item(&v, "runtime.node");
        assert_eq!(r.kind, "runtime");
        assert_eq!(r.bundled_size_bytes, rzip.len() as u64);
        assert_eq!(r.detection, None, "runtimes carry no detection");
        // install the server: runtime first, then the server, one progress stream
        let mut seen: Vec<(Option<String>, usize, usize, String)> = Vec::new();
        f.svc
            .run_install(TS, &mut |p| {
                assert_eq!(p.id, TS, "every event is tagged with the job id");
                seen.push((p.dependency, p.step_index, p.step_count, p.phase));
            })
            .unwrap();
        assert!(seen.iter().all(|(_, _, n, _)| *n == 2));
        let dep_steps: Vec<&(Option<String>, usize, usize, String)> =
            seen.iter().filter(|(d, _, _, _)| d.is_some()).collect();
        assert!(!dep_steps.is_empty());
        assert!(dep_steps.iter().all(|(d, i, _, _)| d.as_deref() == Some("runtime.node") && *i == 1));
        let own: Vec<_> = seen.iter().filter(|(d, _, _, _)| d.is_none()).collect();
        assert!(own.iter().all(|(_, i, _, _)| *i == 2));
        assert_eq!(own.last().unwrap().3, "activate");
        let first_own = seen.iter().position(|(d, _, _, _)| d.is_none()).unwrap();
        assert!(seen[..first_own].iter().all(|(d, _, _, _)| d.is_some()), "deps strictly before the pack");
        // both installed; the runtime binary resolves; the server is `bundled`
        assert!(f.svc.row("runtime.node").unwrap().is_some());
        assert!(f.svc.row(TS).unwrap().is_some());
        let node = f.svc.runtime_binary("node").unwrap();
        assert!(node.ends_with("node.exe") && node.is_file());
        assert_eq!(f.svc.runtime_binary("java"), None);
        let v = f.svc.snapshot();
        let p = item(&v, TS);
        assert_eq!(p.state, "installed");
        assert_eq!(p.bundled_size_bytes, szip.len() as u64, "dep installed → own size only");
        let d = p.detection.clone().unwrap();
        assert_eq!(d.status, "bundled");
        assert_eq!(d.path.as_deref(), Some(node.to_string_lossy().as_ref()));
        // bundled beats a manual path in the detection view
        f.svc.set_path(TS, Some("C:/manual/tls.cmd".into())).unwrap();
        assert_eq!(item(&f.svc.snapshot(), TS).detection.clone().unwrap().status, "bundled");
        f.svc.set_path(TS, None).unwrap();
        // the LSP read surface resolves the same way
        let pack = f.svc.server_pack(TS).unwrap();
        let Resolved::Bundled(b) = f.svc.resolve_server_pack(&pack) else {
            panic!("bundled")
        };
        assert_eq!(b.program, node);
        assert!(b.entry.ends_with("cli.mjs"));
        // a second install of the server does not re-download the runtime
        let hits_before = f.fetcher.hits();
        f.svc.run_install(TS, &mut |p| assert_eq!(p.step_count, 1)).unwrap();
        assert_eq!(f.fetcher.hits(), hits_before + 1, "only the server zip");
        // the runtime refuses to go while the server needs it
        let err = f.svc.uninstall("runtime.node").unwrap_err().to_string();
        assert!(err.contains("required by server.typescript-language-server"), "{err}");
        assert!(f.svc.row("runtime.node").unwrap().is_some());
        // server gone → the runtime is `missing`-hinted, then removable
        f.svc.uninstall(TS).unwrap();
        f.svc.uninstall("runtime.node").unwrap();
        assert!(f.svc.row("runtime.node").unwrap().is_none());
    }

    #[test]
    fn dependency_failure_aborts_before_the_pack_and_missing_runtime_is_hinted() {
        let f = fixture("0.30.0");
        let (szip, rzip) = (bundled_server_zip(), runtime_zip());
        let target = registry::target_key();
        publish_with_requires(
            &f,
            &[
                (TS, "server", "6.0.0-1", &szip, "any", &["runtime.node"]),
                ("runtime.node", "runtime", "22.12.0-1", &rzip, target.as_str(), &[]),
            ],
        );
        // corrupt the runtime artifact → the server never lands
        f.fetcher.put("https://registry.test/runtime.node-22.12.0-1.zip", b"garbage".to_vec());
        let err = f.svc.run_install(TS, &mut |_| {}).unwrap_err();
        assert!(err.contains("dependency runtime.node"), "{err}");
        assert!(f.svc.row(TS).unwrap().is_none());
        assert!(f.svc.row("runtime.node").unwrap().is_none());
        // unknown dependency → refused before any download
        let mut v: serde_json::Value = serde_json::from_str(&index_json_with_requires(&[(
            TS, "server", "6.0.0-1", &szip, "any", &["runtime.deno"],
        )]))
        .unwrap();
        v["packs"][0]["requires"] = serde_json::json!(["runtime.deno"]);
        f.fetcher.put(URL, v.to_string().into_bytes());
        f.svc.view(true);
        let hits = f.fetcher.hits();
        let err = f.svc.run_install(TS, &mut |_| {}).unwrap_err();
        assert!(err.contains("runtime.deno"), "{err}");
        assert_eq!(f.fetcher.hits(), hits, "nothing fetched");
        // a server pack installed by hand without its runtime → missing + hint naming the runtime
        publish_with_requires(&f, &[(TS, "server", "6.0.0-1", &szip, "any", &[])]);
        f.svc.view(true);
        f.svc.run_install(TS, &mut |_| {}).unwrap();
        let d = item(&f.svc.snapshot(), TS).detection.clone().unwrap();
        assert_eq!(d.status, "missing");
        assert!(d.hint.unwrap().contains("runtime.node is not installed"));
        let pack = f.svc.server_pack(TS).unwrap();
        assert!(matches!(f.svc.resolve_server_pack(&pack), Resolved::Missing { .. }));
        // …the manual path still wins as the explicit choice
        f.svc.set_path(TS, Some("C:/manual/tls.cmd".into())).unwrap();
        assert_eq!(item(&f.svc.snapshot(), TS).detection.clone().unwrap().status, "configured");
    }

    #[test]
    fn runtime_pack_for_another_target_or_without_its_binary_is_refused() {
        let f = fixture("0.30.0");
        let rzip = runtime_zip();
        publish(&f, &[("runtime.node", "runtime", "22.12.0-1", &rzip, "any")]);
        // manifest claims THIS target: fine
        f.svc.run_install("runtime.node", &mut |_| {}).unwrap();
        f.svc.uninstall("runtime.node").unwrap();
        // binary missing from the zip → refused, nothing installed
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("r.zip");
        let manifest = fixtures::RUNTIME_NODE
            .replace("\"minOrreryVersion\": \"0.24.0\"", "\"minOrreryVersion\": \"0.1.0\"")
            .replace("\"target\": \"windows-x86_64\"", &format!("\"target\": \"{}\"", registry::target_key()));
        write_zip(&p, &[("manifest.json", manifest.as_bytes())]);
        let bad = fs::read(p).unwrap();
        publish(&f, &[("runtime.node", "runtime", "22.12.0-1", &bad, "any")]);
        f.svc.view(true);
        let err = f.svc.run_install("runtime.node", &mut |_| {}).unwrap_err();
        assert!(err.contains("runtime binary"), "{err}");
        assert!(f.svc.row("runtime.node").unwrap().is_none());
        // built for another target → refused
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("r.zip");
        let manifest = fixtures::RUNTIME_NODE
            .replace("\"minOrreryVersion\": \"0.24.0\"", "\"minOrreryVersion\": \"0.1.0\"")
            .replace("\"target\": \"windows-x86_64\"", "\"target\": \"nope-arch\"");
        write_zip(&p, &[("manifest.json", manifest.as_bytes()), ("node.exe", b"MZ")]);
        let foreign = fs::read(p).unwrap();
        publish(&f, &[("runtime.node", "runtime", "22.12.0-1", &foreign, "any")]);
        f.svc.view(true);
        let err = f.svc.run_install("runtime.node", &mut |_| {}).unwrap_err();
        assert!(err.contains("built for nope-arch"), "{err}");
        // a bundled server pack whose launch entry is absent → refused
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.zip");
        let manifest = fixtures::SERVER_BUNDLED
            .replace("\"minOrreryVersion\": \"0.24.0\"", "\"minOrreryVersion\": \"0.1.0\"");
        write_zip(&p, &[("manifest.json", manifest.as_bytes())]);
        let no_entry = fs::read(p).unwrap();
        publish(&f, &[(TS, "server", "6.0.0-1", &no_entry, "any")]);
        f.svc.view(true);
        let err = f.svc.run_install(TS, &mut |_| {}).unwrap_err();
        assert!(err.contains("launch entry"), "{err}");
        assert!(f.svc.row(TS).unwrap().is_none());
    }

    fn grammar_zip() -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("g.zip");
        let manifest = fixtures::GRAMMAR
            .replace("\"minOrreryVersion\": \"0.24.0\"", "\"minOrreryVersion\": \"0.1.0\"")
            .replace("\"target\": \"windows-x86_64\"", &format!("\"target\": \"{}\"", registry::target_key()));
        write_zip(
            &p,
            &[
                ("manifest.json", manifest.as_bytes()),
                ("tree_sitter_java.dll", b"not a dylib"),
                ("queries/tags.scm", b"(x) @y"),
            ],
        );
        fs::read(p).unwrap()
    }

    fn index_json(packs: &[(&str, &str, &str, &[u8], &str)]) -> String {
        // (id, kind, version, artifact bytes, target key)
        let packs: Vec<serde_json::Value> = packs
            .iter()
            .map(|(id, kind, version, bytes, target)| {
                serde_json::json!({
                    "id": id, "kind": kind, "version": version, "description": format!("{id} pack"),
                    "languages": ["java"], "abi": if *kind == "grammar" { Some(tree_sitter::LANGUAGE_VERSION) } else { None },
                    "minOrreryVersion": "0.1.0",
                    "targets": { *target: { "url": format!("https://registry.test/{id}-{version}.zip"),
                                            "sha256": install::sha256_hex(bytes), "size": bytes.len() } }
                })
            })
            .collect();
        serde_json::json!({ "schema": 1, "generated": "now", "packs": packs }).to_string()
    }

    fn publish(f: &Fixture, packs: &[(&str, &str, &str, &[u8], &str)]) {
        f.fetcher.put(URL, index_json(packs).into_bytes());
        for (id, _, version, bytes, _) in packs {
            f.fetcher
                .put(&format!("https://registry.test/{id}-{version}.zip"), bytes.to_vec());
        }
    }

    fn item<'a>(v: &'a ExtRegistryView, id: &str) -> &'a ExtPack {
        v.items.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn view_lists_registry_with_states_and_caches_the_index() {
        let f = fixture("0.30.0");
        let zip = server_zip("1.40.0-1");
        publish(&f, &[("server.jdtls", "server", "1.40.0-1", &zip, "any")]);
        let v = f.svc.view(false);
        assert!(!v.offline);
        assert_eq!(v.registry_url, URL);
        assert!(v.fetched_at.is_some());
        let p = item(&v, "server.jdtls");
        assert_eq!(p.state, "available");
        assert!(!p.installed && !p.enabled);
        assert!(p.available_for_target && p.compatible);
        assert_eq!(p.size_bytes, zip.len() as u64);
        assert_eq!(p.detection, None, "no manifest yet → no detection");
        assert_eq!(f.fetcher.hits(), 1);
        // fresh cache → second view is network-free; refresh forces a fetch
        f.svc.view(false);
        assert_eq!(f.fetcher.hits(), 1);
        f.svc.view(true);
        assert_eq!(f.fetcher.hits(), 2);
        // snapshot never fetches
        f.svc.snapshot();
        assert_eq!(f.fetcher.hits(), 2);
    }

    #[test]
    fn offline_falls_back_to_the_cached_index() {
        let f = fixture("0.30.0");
        let zip = server_zip("1.40.0-1");
        publish(&f, &[("server.jdtls", "server", "1.40.0-1", &zip, "any")]);
        f.svc.view(false);
        f.fetcher.set_fail(true);
        let v = f.svc.view(true);
        assert!(v.offline);
        assert_eq!(v.items.len(), 1, "cached catalog still listed");
        // no cache at all → offline with an empty catalog
        let g = fixture("0.30.0");
        g.fetcher.set_fail(true);
        let v = g.svc.view(false);
        assert!(v.offline);
        assert!(v.items.is_empty());
        assert_eq!(v.fetched_at, None);
    }

    #[test]
    fn gates_mark_packs_incompatible_or_unavailable() {
        let f = fixture("0.23.1");
        let zip = server_zip("1.40.0-1");
        let mut idx: serde_json::Value =
            serde_json::from_str(&index_json(&[("server.jdtls", "server", "1.40.0-1", &zip, "any")]))
                .unwrap();
        idx["packs"][0]["minOrreryVersion"] = "0.24.0".into();
        idx["packs"].as_array_mut().unwrap().push(serde_json::json!({
            "id": "grammar.java", "kind": "grammar", "version": "1", "abi": tree_sitter::LANGUAGE_VERSION,
            "targets": { "nope-arch": { "url": "https://registry.test/x.zip", "sha256": "0".repeat(64), "size": 1 } }
        }));
        idx["packs"].as_array_mut().unwrap().push(serde_json::json!({
            "id": "grammar.old", "kind": "grammar", "version": "1", "abi": 1,
            "targets": { "any": { "url": "https://registry.test/y.zip", "sha256": "0".repeat(64), "size": 1 } }
        }));
        f.fetcher.put(URL, idx.to_string().into_bytes());
        let v = f.svc.view(false);
        let p = item(&v, "server.jdtls");
        assert!(!p.compatible);
        assert_eq!(p.state, "incompatible");
        let p = item(&v, "grammar.java");
        assert!(!p.available_for_target);
        assert_eq!(p.state, "incompatible");
        assert_eq!(p.size_bytes, 0);
        let p = item(&v, "grammar.old");
        assert!(!p.compatible, "abi 1 is below the runtime floor");
        let err = f.svc.run_install("server.jdtls", &mut |_| {}).unwrap_err();
        assert!(err.contains("needs Orrery 0.24.0"), "{err}");
        let err = f.svc.run_install("grammar.java", &mut |_| {}).unwrap_err();
        assert!(err.contains("no build for"), "{err}");
    }

    #[test]
    fn installs_a_server_pack_end_to_end() {
        let f = fixture("0.30.0");
        let zip = server_zip("1.40.0-1");
        publish(&f, &[("server.jdtls", "server", "1.40.0-1", &zip, "any")]);
        let mut phases = Vec::new();
        f.svc
            .run_install("server.jdtls", &mut |p| phases.push(p.phase))
            .unwrap();
        assert_eq!(phases.first().map(String::as_str), Some("download"));
        assert!(phases.iter().any(|p| p == "verify"));
        assert!(phases.iter().any(|p| p == "unpack"));
        assert_eq!(phases.last().map(String::as_str), Some("activate"));
        let row = f.svc.row("server.jdtls").unwrap().unwrap();
        assert_eq!(row.version, "1.40.0-1");
        assert_eq!(row.target, "any");
        assert!(row.enabled);
        assert_eq!(row.sha256, install::sha256_hex(&zip));
        let dir = PathBuf::from(&row.dir);
        assert!(dir.join("manifest.json").is_file());
        assert!(dir.ends_with(Path::new("server.jdtls").join("1.40.0-1")));
        assert!(
            fs::read_dir(f.svc.tmp_dir()).unwrap().next().is_none(),
            "staging dir cleaned"
        );
        let v = f.svc.snapshot();
        let p = item(&v, "server.jdtls");
        assert_eq!(p.state, "installed");
        assert_eq!(p.installed_version.as_deref(), Some("1.40.0-1"));
        let d = p.detection.as_ref().expect("server → detection");
        assert_eq!(d.status, "missing");
        assert!(
            d.hint.as_deref().unwrap().contains("not self-contained"),
            "legacy pack, system servers off: {:?}",
            d.hint
        );
        // opting into system servers surfaces the legacy requires-derived hint
        let mut s = f.settings.get().unwrap();
        s.lsp_use_system_servers = true;
        f.settings.set(&s).unwrap();
        let d = item(&f.svc.snapshot(), "server.jdtls").detection.clone().unwrap();
        assert!(d.hint.as_deref().unwrap().contains("java 17+"), "{:?}", d.hint);
        s.lsp_use_system_servers = false;
        f.settings.set(&s).unwrap();
        // upgrade: a newer registry version replaces the dir immediately
        let zip2 = server_zip("1.41.0-1");
        publish(&f, &[("server.jdtls", "server", "1.41.0-1", &zip2, "any")]);
        f.svc.view(true); // the cache is fresh — pick up the new catalog
        f.svc.run_install("server.jdtls", &mut |_| {}).unwrap();
        let row = f.svc.row("server.jdtls").unwrap().unwrap();
        assert_eq!(row.version, "1.41.0-1");
        assert!(!dir.exists(), "old version dir pruned");
        // uninstall
        f.svc.uninstall("server.jdtls").unwrap();
        assert!(f.svc.row("server.jdtls").unwrap().is_none());
        assert!(!f.svc.root().join("server.jdtls").exists());
        assert!(f.svc.uninstall("server.jdtls").is_err(), "not installed");
        let after = f.svc.snapshot();
        assert_eq!(item(&after, "server.jdtls").state, "available");
    }

    #[test]
    fn sha256_mismatch_and_manifest_mismatch_install_nothing() {
        let f = fixture("0.30.0");
        let zip = server_zip("1.40.0-1");
        publish(&f, &[("server.jdtls", "server", "1.40.0-1", &zip, "any")]);
        f.fetcher
            .put("https://registry.test/server.jdtls-1.40.0-1.zip", server_zip("1.40.0-2"));
        let err = f.svc.run_install("server.jdtls", &mut |_| {}).unwrap_err();
        assert!(err.contains("sha256 mismatch"), "{err}");
        assert!(f.svc.row("server.jdtls").unwrap().is_none());
        // right bytes for the index but the manifest inside says another version
        let wrong = server_zip("9.9.9-1");
        publish(&f, &[("server.jdtls", "server", "1.40.0-1", &wrong, "any")]);
        f.svc.view(true);
        let err = f.svc.run_install("server.jdtls", &mut |_| {}).unwrap_err();
        assert!(err.contains("manifest version"), "{err}");
        assert!(f.svc.row("server.jdtls").unwrap().is_none());
        assert!(!f.svc.root().join("server.jdtls").exists());
        assert!(fs::read_dir(f.svc.tmp_dir()).unwrap().next().is_none());
        let err = f.svc.run_install("server.unknown", &mut |_| {}).unwrap_err();
        assert!(err.contains("not in the registry"), "{err}");
    }

    #[test]
    fn grammar_that_fails_probe_is_not_activated() {
        let f = fixture("0.30.0");
        let zip = grammar_zip();
        publish(&f, &[("grammar.java", "grammar", "0.23.4-1", &zip, registry::target_key().as_str())]);
        let err = f.svc.run_install("grammar.java", &mut |_| {}).unwrap_err();
        assert!(err.contains("grammar.java"), "{err}");
        assert!(f.svc.row("grammar.java").unwrap().is_none());
        assert!(!f.svc.root().join("grammar.java").exists());
        assert!(!f.svc.grammars().is_loaded("grammar.java"));
        assert!(f.svc.grammar_for_extension("java").is_none());
    }

    #[test]
    fn enable_toggle_and_manual_path() {
        let f = fixture("0.30.0");
        let zip = server_zip("1.40.0-1");
        publish(&f, &[("server.jdtls", "server", "1.40.0-1", &zip, "any")]);
        f.svc.run_install("server.jdtls", &mut |_| {}).unwrap();
        f.svc.set_enabled("server.jdtls", false).unwrap();
        assert!(!f.svc.row("server.jdtls").unwrap().unwrap().enabled);
        assert!(!item(&f.svc.snapshot(), "server.jdtls").enabled);
        f.svc.set_enabled("server.jdtls", true).unwrap();
        assert!(f.svc.row("server.jdtls").unwrap().unwrap().enabled);
        assert!(f.svc.set_enabled("server.nope", true).is_err());
        // manual path lands in settings and shows as `configured`
        f.svc
            .set_path("server.jdtls", Some(" C:/tools/jdtls.bat ".into()))
            .unwrap();
        assert_eq!(
            f.settings.get().unwrap().lsp_paths.get("server.jdtls").map(String::as_str),
            Some("C:/tools/jdtls.bat")
        );
        let d = item(&f.svc.snapshot(), "server.jdtls").detection.clone().unwrap();
        assert_eq!(d.status, "configured");
        assert_eq!(d.path.as_deref(), Some("C:/tools/jdtls.bat"));
        f.svc.set_path("server.jdtls", None).unwrap();
        assert!(f.settings.get().unwrap().lsp_paths.is_empty());
        assert!(f.svc.set_path("../x", None).is_err());
    }

    #[test]
    fn job_state_shows_downloading_and_error() {
        let f = fixture("0.30.0");
        let zip = server_zip("1.40.0-1");
        publish(&f, &[("server.jdtls", "server", "1.40.0-1", &zip, "any")]);
        f.svc.view(false);
        f.svc
            .0
            .jobs
            .lock()
            .unwrap()
            .insert("server.jdtls".into(), Job::Downloading);
        assert_eq!(item(&f.svc.snapshot(), "server.jdtls").state, "downloading");
        f.svc
            .0
            .jobs
            .lock()
            .unwrap()
            .insert("server.jdtls".into(), Job::Error("boom".into()));
        let p = item(&f.svc.snapshot(), "server.jdtls").clone();
        assert_eq!(p.state, "error");
        assert_eq!(p.error.as_deref(), Some("boom"));
    }

    #[test]
    fn startup_activates_pending_and_prunes_stale_dirs() {
        let f = fixture("0.30.0");
        let root = f.svc.root().to_path_buf();
        // installed 1.40.0-1 with 1.40.0-2 pending, plus junk everywhere
        let old = root.join("server.jdtls").join("1.40.0-1");
        let new = root.join("server.jdtls").join("1.40.0-2");
        let stale = root.join("server.jdtls").join("1.39.0-1");
        let orphan = root.join("grammar.orphan").join("1");
        for d in [&old, &new, &stale, &orphan] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(
            new.join("manifest.json"),
            fixtures::SERVER.replace("1.40.0-1", "1.40.0-2"),
        )
        .unwrap();
        fs::write(f.svc.tmp_dir().join("leftover.zip"), b"x").unwrap();
        fs::create_dir_all(f.svc.tmp_dir().join("stage-1")).unwrap();
        f.svc
            .upsert_row(&InstalledRow {
                id: "server.jdtls".into(),
                kind: "server".into(),
                version: "1.40.0-1".into(),
                target: "any".into(),
                dir: old.to_string_lossy().into_owned(),
                sha256: "0".repeat(64),
                enabled: true,
                manifest_json: fixtures::SERVER.into(),
                installed_at: 1,
                pending_version: Some("1.40.0-2".into()),
            })
            .unwrap();
        assert_eq!(item(&f.svc.snapshot(), "server.jdtls").state, "pendingRestart");
        f.svc.startup();
        let row = f.svc.row("server.jdtls").unwrap().unwrap();
        assert_eq!(row.version, "1.40.0-2");
        assert_eq!(row.pending_version, None);
        assert_eq!(Path::new(&row.dir), new);
        assert!(new.exists());
        assert!(!old.exists(), "superseded version pruned");
        assert!(!stale.exists());
        assert!(!root.join("grammar.orphan").exists(), "no row → gone");
        assert!(fs::read_dir(f.svc.tmp_dir()).unwrap().next().is_none());
        assert_eq!(item(&f.svc.snapshot(), "server.jdtls").state, "installed");
    }

    #[test]
    fn invalid_registry_override_falls_back_to_default() {
        let f = fixture("0.30.0");
        let mut s = f.settings.get().unwrap();
        s.ext_registry_url = Some("http://insecure/index.json".into());
        f.settings.set(&s).unwrap();
        assert_eq!(f.svc.registry_url(), registry::DEFAULT_REGISTRY_URL);
        s.ext_registry_url = None;
        f.settings.set(&s).unwrap();
        assert_eq!(f.svc.registry_url(), registry::DEFAULT_REGISTRY_URL);
    }

    #[test]
    fn view_serializes_the_contract_shape() {
        let f = fixture("0.30.0");
        let zip = server_zip("1.40.0-1");
        publish(&f, &[("server.jdtls", "server", "1.40.0-1", &zip, "any")]);
        f.svc.run_install("server.jdtls", &mut |_| {}).unwrap();
        let v = serde_json::to_value(f.svc.snapshot()).unwrap();
        for k in ["registryUrl", "fetchedAt", "offline", "items"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        let p = &v["items"][0];
        for k in [
            "id", "kind", "name", "description", "version", "installedVersion", "languages",
            "sizeBytes", "installed", "enabled", "compatible", "availableForTarget", "state",
            "error", "detection",
        ] {
            assert!(p.get(k).is_some(), "{k} in {p}");
        }
        assert_eq!(p["detection"]["status"], "missing");
        for k in ["bundledSizeBytes", "requires"] {
            assert!(p.get(k).is_some(), "{k} in {p}");
        }
        let prog = serde_json::to_value(ExtProgress {
            id: "x".into(),
            downloaded: 1,
            total: None,
            phase: "download".into(),
            dependency: None,
            step_index: 1,
            step_count: 1,
        })
        .unwrap();
        assert_eq!(prog["total"], serde_json::Value::Null);
        assert_eq!(prog["dependency"], serde_json::Value::Null);
        assert_eq!(prog["stepIndex"], 1);
        assert_eq!(prog["stepCount"], 1);
    }
}
