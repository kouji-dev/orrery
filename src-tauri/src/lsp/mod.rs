//! Language-server host (M3): one generic JSON-RPC client per
//! `(server extension, project)`, started lazily on the first navigation
//! request for a file of its language, reaped when idle, restarted with a
//! backoff after a crash. Every worktree of a project shares the project's
//! server (the project checkout is the LSP root); opened documents keep
//! their own worktree `file://` uris.
//!
//! The nav router ([`route`]) asks a live server with a small budget and lets
//! the tree-sitter index answer when the server is starting, slow or empty —
//! one IPC per lookup, late replies dropped by the client.
//!
//! Events: `lsp://status { servers }` (full snapshot) on every state change
//! and every metrics tick, through the [`StatusSink`] `lib.rs` wires to
//! `core::emit`. Processes are spawned only through `core::proc::cmd`.

pub mod client;
pub mod commands;
pub mod server;
pub mod transport;
pub mod uri;
pub mod virtual_docs;
#[cfg(test)]
mod smoke_tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use lsp_types::{GotoDefinitionResponse, Hover, HoverContents, Location, MarkedString, Range};
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::core::errors::{AppError, AppResult};
use crate::extensions::detect::Resolved;
use crate::extensions::manifest::ServerManifest;
use crate::extensions::{ExtensionService, ServerPack};
use crate::metrics::ProcMetric;
use crate::settings::SettingsService;
use crate::symbols::commands::{NavHover, NavKind, NavLocation, NavRange};

use client::LspErr;
use server::{Begin, Launch, Placeholders, ProjectRef, Reap, ServerHandle, State};
use uri::{path_to_uri, relative_to, uri_to_path};
use virtual_docs::{Dispatch, VirtualDoc, VirtualDocs};

pub const EV_STATUS: &str = "lsp://status";
/// Router budgets.
pub const BUDGET_DEFINITION: Duration = Duration::from_millis(300);
pub const BUDGET_HOVER: Duration = Duration::from_millis(300);
pub const BUDGET_REFERENCES: Duration = Duration::from_millis(1000);
/// Virtual document reads (class-file decompilation can be slow).
const BUDGET_VIRTUAL_READ: Duration = Duration::from_secs(5);
/// Idle reaper cadence.
const REAP_EVERY: Duration = Duration::from_secs(30);
/// Locations kept from one LSP answer.
const MAX_LOCATIONS: usize = 2000;

/// The `lsp://status` payload and the `lsp_status` answer.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LspStatus {
    pub servers: Vec<server::LspServer>,
}

/// Where `lsp://status` payloads go (`core::emit::emit_tracked` in prod).
pub type StatusSink = Box<dyn Fn(&LspStatus) + Send + Sync>;

/// A file's language as the extension host + LSP name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lang {
    /// Extension-pack language (`server.*.languages`).
    pub id: &'static str,
    /// LSP `languageId` for `didOpen`.
    pub lsp_id: &'static str,
}

/// Language of a root-relative path by extension (`None` = no server pack
/// could ever claim it).
pub fn language_of(rel: &str) -> Option<Lang> {
    let base = rel.rsplit(['/', '\\']).next()?;
    let ext = base.rsplit_once('.')?.1.to_ascii_lowercase();
    let (id, lsp_id) = match ext.as_str() {
        "java" => ("java", "java"),
        "rs" => ("rust", "rust"),
        "ts" | "mts" | "cts" => ("typescript", "typescript"),
        "tsx" => ("tsx", "typescriptreact"),
        "js" | "mjs" | "cjs" => ("javascript", "javascript"),
        "jsx" => ("javascript", "javascriptreact"),
        "py" | "pyi" => ("python", "python"),
        "go" => ("go", "go"),
        _ => return None,
    };
    Some(Lang { id, lsp_id })
}

/// Monaco language id of a virtual document served by a server for `lang`.
fn monaco_language(lang: &str) -> &str {
    match lang {
        "tsx" => "typescript",
        other => other,
    }
}

// ---- router (pure over a handle) ----------------------------------------------

/// What the router needs to know about the asking file.
pub struct NavCtx<'a> {
    pub abs: &'a Path,
    pub line: u32,
    pub col: u32,
    /// Live buffer for the first `didOpen` (else disk).
    pub text: Option<&'a str>,
    pub language_id: &'a str,
    /// Known worktree roots `(owner id, root)` for `file://` → `orrery://`.
    pub roots: &'a [(Uuid, PathBuf)],
}

#[derive(Debug, PartialEq)]
pub enum Routed {
    Locations(Vec<NavLocation>),
    Hover(Option<NavHover>),
    /// The server answered with nothing — let the index try.
    Empty,
    Timeout,
    /// RPC error / connection gone.
    Failed,
}

/// Ask `handle` within the kind's budget.
pub fn route(handle: &ServerHandle, kind: &NavKind, ctx: &NavCtx<'_>) -> Routed {
    if handle.ensure_open(ctx.abs, ctx.text, ctx.language_id).is_err() {
        return Routed::Failed;
    }
    let doc = json!({
        "textDocument": { "uri": path_to_uri(ctx.abs) },
        "position": { "line": ctx.line, "character": ctx.col }
    });
    let (method, params, budget) = match kind {
        NavKind::Definition => ("textDocument/definition", doc, BUDGET_DEFINITION),
        NavKind::References { include_declaration } => {
            let mut p = doc;
            p["context"] = json!({ "includeDeclaration": include_declaration });
            ("textDocument/references", p, BUDGET_REFERENCES)
        }
        NavKind::Hover => ("textDocument/hover", doc, BUDGET_HOVER),
    };
    match handle.request(method, params, budget) {
        Ok(v) => match kind {
            NavKind::Hover => match hover_from(&v) {
                Some(h) => Routed::Hover(Some(h)),
                None => Routed::Empty,
            },
            _ => {
                let locs = locations_from(&v, ctx.roots);
                if locs.is_empty() {
                    Routed::Empty
                } else {
                    Routed::Locations(locs)
                }
            }
        },
        Err(LspErr::Timeout) => Routed::Timeout,
        Err(e) => {
            log::debug!("lsp {method}: {e}");
            Routed::Failed
        }
    }
}

/// `Location | Location[] | LocationLink[] | null` → nav locations.
pub fn locations_from(v: &Value, roots: &[(Uuid, PathBuf)]) -> Vec<NavLocation> {
    if v.is_null() {
        return Vec::new();
    }
    let mut out = Vec::new();
    match serde_json::from_value::<GotoDefinitionResponse>(v.clone()) {
        Ok(GotoDefinitionResponse::Scalar(l)) => out.push(to_nav(l.uri.as_str(), &l.range, roots)),
        Ok(GotoDefinitionResponse::Array(ls)) => {
            out.extend(ls.iter().map(|l| to_nav(l.uri.as_str(), &l.range, roots)));
        }
        Ok(GotoDefinitionResponse::Link(ls)) => out.extend(
            ls.iter()
                .map(|l| to_nav(l.target_uri.as_str(), &l.target_selection_range, roots)),
        ),
        Err(_) => {
            if let Ok(ls) = serde_json::from_value::<Vec<Location>>(v.clone()) {
                out.extend(ls.iter().map(|l| to_nav(l.uri.as_str(), &l.range, roots)));
            }
        }
    }
    out.truncate(MAX_LOCATIONS);
    out
}

/// One LSP location → `orrery://<id>/<rel>` when the file sits under a known
/// root (longest root wins), else the raw uri with no path (a virtual doc).
pub fn to_nav(uri: &str, range: &Range, roots: &[(Uuid, PathBuf)]) -> NavLocation {
    let mut best: Option<(Uuid, String, usize)> = None;
    if let Some(abs) = uri_to_path(uri) {
        for (id, root) in roots {
            if let Some(rel) = relative_to(root, &abs) {
                let depth = root.to_string_lossy().len();
                if best.as_ref().is_none_or(|b| depth > b.2) {
                    best = Some((*id, rel, depth));
                }
            }
        }
    }
    let (uri, id, path) = match best {
        Some((id, rel, _)) => (format!("orrery://{id}/{rel}"), Some(id), Some(rel)),
        None => (uri.to_string(), None, None),
    };
    NavLocation {
        uri,
        id,
        path,
        line: range.start.line,
        col: range.start.character,
        end_line: range.end.line,
        end_col: range.end.character,
        kind: None,
        container: None,
        preview: None,
    }
}

fn marked(m: &MarkedString) -> String {
    match m {
        MarkedString::String(s) => s.clone(),
        MarkedString::LanguageString(l) => format!("```{}\n{}\n```", l.language, l.value),
    }
}

/// `Hover | null` → markdown + range.
pub fn hover_from(v: &Value) -> Option<NavHover> {
    if v.is_null() {
        return None;
    }
    let h: Hover = serde_json::from_value(v.clone()).ok()?;
    let contents = match &h.contents {
        HoverContents::Scalar(m) => marked(m),
        HoverContents::Array(ms) => ms.iter().map(marked).collect::<Vec<_>>().join("\n\n"),
        HoverContents::Markup(m) => m.value.clone(),
    };
    if contents.trim().is_empty() {
        return None;
    }
    Some(NavHover {
        source: "lsp".into(),
        contents,
        range: h.range.map(|r| NavRange {
            line: r.start.line,
            col: r.start.character,
            end_line: r.end.line,
            end_col: r.end.character,
        }),
    })
}

// ---- launch resolution --------------------------------------------------------

fn fwd(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The manifest with `${extDir}` / `${workspaceStorage}` / `${projectRoot}` /
/// `${jdtlsConfig}` resolved for one `(pack, project)` — what the handle's
/// `initialize` + `workspace/configuration` carry.
pub fn prepared_manifest(pack: &ServerPack, root: &Path, storage: &Path) -> ServerManifest {
    let (e, w, r) = (fwd(&pack.dir), fwd(storage), fwd(root));
    server::substitute_manifest(&pack.manifest, &Placeholders::new(&e, &w, &r))
}

/// The launch for `pack` in `root`: the installed self-contained pack (its
/// downloaded runtime, entry glob resolved) → the user's manual path → a
/// system server when `lsp_use_system_servers` is on → `Err(hint)`. The
/// ONE place the order lives; the smoke tests go through it too.
pub fn resolve_launch(
    ext: &ExtensionService,
    pack: &ServerPack,
    root: &Path,
    storage: &Path,
) -> Result<Launch, Option<String>> {
    let (e, w, r) = (fwd(&pack.dir), fwd(storage), fwd(root));
    let ph = Placeholders::new(&e, &w, &r);
    match ext.resolve_server_pack(pack) {
        Resolved::Bundled(b) => Ok(server::launch_bundled(&b, &ph, root.to_path_buf())),
        Resolved::Configured(p) | Resolved::System(p) => Ok(server::launch_for(
            &pack.manifest,
            &p,
            &ph,
            root.to_path_buf(),
            &crate::extensions::detect::DetectEnv::current(),
        )),
        Resolved::Missing { hint } => Err(hint),
    }
}

// ---- service ------------------------------------------------------------------

/// Outcome of asking for a server for `(lang, project)`.
pub enum Acquire {
    /// No installed + enabled server pack claims the language.
    NotInstalled,
    /// Spawning / initializing — answer from the index now.
    Starting,
    Ready(Arc<ServerHandle>),
    /// Missing program, manual stop, exhausted backoff, or waiting one out.
    Unavailable,
}

struct Inner {
    ext: ExtensionService,
    settings: SettingsService,
    /// app-data `lsp-workspaces/`.
    workspaces_dir: PathBuf,
    /// `"<extId>:<projectId>"` → handle.
    servers: Mutex<HashMap<String, Arc<ServerHandle>>>,
    /// Last metrics tick per server id: `(memBytes, cpu)`.
    metrics: Mutex<HashMap<String, (u64, f32)>>,
    vdocs: VirtualDocs,
    sink: Option<StatusSink>,
}

#[derive(Clone)]
pub struct LspService(Arc<Inner>);

impl Inner {
    fn snapshot(&self) -> LspStatus {
        let handles: Vec<Arc<ServerHandle>> = self.servers.lock().unwrap().values().cloned().collect();
        let metrics = self.metrics.lock().unwrap();
        let mut servers: Vec<server::LspServer> = handles
            .iter()
            .map(|h| {
                let mut s = h.snapshot();
                if let Some((mem, cpu)) = metrics.get(&s.id) {
                    s.mem_bytes = *mem;
                    s.cpu = *cpu;
                }
                s
            })
            .collect();
        servers.sort_by(|a, b| a.id.cmp(&b.id));
        LspStatus { servers }
    }

    fn emit(&self) {
        if let Some(sink) = &self.sink {
            sink(&self.snapshot());
        }
    }
}

impl LspService {
    pub fn new(
        ext: ExtensionService,
        settings: SettingsService,
        workspaces_dir: PathBuf,
        sink: Option<StatusSink>,
    ) -> Self {
        if let Err(e) = std::fs::create_dir_all(&workspaces_dir) {
            log::warn!("lsp: create {}: {e}", workspaces_dir.display());
        }
        let inner = Arc::new(Inner {
            ext,
            settings,
            workspaces_dir,
            servers: Mutex::new(HashMap::new()),
            metrics: Mutex::new(HashMap::new()),
            vdocs: VirtualDocs::default(),
            sink,
        });
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("lsp-reaper".into())
            .spawn(move || loop {
                std::thread::sleep(REAP_EVERY);
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                Self::reap(&inner);
            })
            .expect("spawn lsp reaper");
        Self(inner)
    }

    /// One reaper tick: poll children, flip `ready` → `idle` at half the
    /// window, shut down (and forget) servers idle past the whole window.
    fn reap(inner: &Arc<Inner>) {
        let minutes = inner.settings.get().map(|s| s.lsp_idle_minutes).unwrap_or(10);
        let window = Duration::from_secs(u64::from(minutes) * 60);
        let handles: Vec<Arc<ServerHandle>> = inner.servers.lock().unwrap().values().cloned().collect();
        let mut changed = false;
        for h in handles {
            h.check_alive();
            match server::reap_decision(h.state(), h.idle_for(), window) {
                Reap::Keep => {}
                Reap::MarkIdle => h.mark_idle(),
                Reap::Stop => {
                    log::info!("lsp {}: idle for {minutes} min, stopping", h.ext_id);
                    h.stop();
                    inner.servers.lock().unwrap().remove(&h.key());
                    inner.metrics.lock().unwrap().remove(&h.key());
                    changed = true;
                }
            }
        }
        if changed {
            inner.emit();
        }
    }

    fn key(ext_id: &str, project_id: Uuid) -> String {
        format!("{ext_id}:{project_id}")
    }

    fn handle(&self, key: &str) -> Option<Arc<ServerHandle>> {
        self.0.servers.lock().unwrap().get(key).cloned()
    }

    /// `${workspaceStorage}` for a project root.
    fn workspace_storage(&self, root: &Path) -> PathBuf {
        let key = crate::symbols::SymbolService::root_key(root);
        let hash = blake3::hash(key.as_bytes()).to_hex();
        self.0.workspaces_dir.join(&hash.as_str()[..16])
    }

    /// Resolve + spawn on a worker thread; the handle must be `starting`.
    fn spawn_start(&self, pack: ServerPack, handle: Arc<ServerHandle>) {
        let inner = self.0.clone();
        let storage = self.workspace_storage(&handle.project.root);
        std::thread::Builder::new()
            .name(format!("lsp-start:{}", pack.id))
            .spawn(move || {
                if let Err(e) = std::fs::create_dir_all(&storage) {
                    log::warn!("lsp: create {}: {e}", storage.display());
                }
                let launch = match resolve_launch(&inner.ext, &pack, &handle.project.root, &storage) {
                    Ok(l) => l,
                    Err(hint) => {
                        handle.set_missing(hint);
                        return;
                    }
                };
                log::info!("lsp {}: starting {} {:?}", pack.id, launch.program.display(), launch.args);
                handle.start(&launch);
            })
            .expect("spawn lsp start thread");
    }

    /// A fresh handle for `(pack, project)` with the manifest's placeholders resolved.
    fn new_handle(&self, pack: &ServerPack, project: ProjectRef) -> Arc<ServerHandle> {
        let storage = self.workspace_storage(&project.root);
        let manifest = prepared_manifest(pack, &project.root, &storage);
        let weak: Weak<Inner> = Arc::downgrade(&self.0);
        ServerHandle::new(
            pack.id.clone(),
            manifest,
            project,
            Box::new(move || {
                if let Some(i) = weak.upgrade() {
                    i.emit();
                }
            }),
        )
    }

    /// The server for `lang` in `project`, starting it when needed.
    /// `name` is only consulted when a handle is created.
    pub fn acquire(
        &self,
        lang: &str,
        project_id: Uuid,
        root: &Path,
        name: &dyn Fn() -> String,
    ) -> Acquire {
        let Some(pack) = self.0.ext.server_for_language(lang) else {
            return Acquire::NotInstalled;
        };
        let key = Self::key(&pack.id, project_id);
        let (handle, fresh) = {
            let mut servers = self.0.servers.lock().unwrap();
            match servers.get(&key) {
                Some(h) => (h.clone(), false),
                None => {
                    let h = self.new_handle(
                        &pack,
                        ProjectRef {
                            id: project_id,
                            name: name(),
                            root: root.to_path_buf(),
                        },
                    );
                    servers.insert(key.clone(), h.clone());
                    (h, true)
                }
            }
        };
        if fresh {
            self.0.emit();
            self.spawn_start(pack, handle);
            return Acquire::Starting;
        }
        match handle.state() {
            State::Ready | State::Idle => Acquire::Ready(handle),
            State::Starting => Acquire::Starting,
            State::Crashed => match handle.begin_start(false) {
                Begin::Start => {
                    self.spawn_start(pack, handle);
                    Acquire::Starting
                }
                Begin::Busy => Acquire::Starting,
                Begin::Wait => Acquire::Unavailable,
            },
            State::Stopped | State::Missing => Acquire::Unavailable,
        }
    }

    /// Start (or leave running) the server `ext_id` for a project — the
    /// manual `lsp_start`; forces past `stopped`/`missing`/backoff.
    pub fn start(&self, ext_id: &str, project: ProjectRef) -> AppResult<()> {
        let pack = self
            .0
            .ext
            .server_pack(ext_id)
            .ok_or_else(|| AppError::Other(format!("{ext_id}: not installed")))?;
        if !pack.enabled {
            return Err(AppError::Other(format!("{ext_id}: disabled")));
        }
        let key = Self::key(ext_id, project.id);
        let (handle, fresh) = {
            let mut servers = self.0.servers.lock().unwrap();
            match servers.get(&key) {
                Some(h) => (h.clone(), false),
                None => {
                    let h = self.new_handle(&pack, project);
                    servers.insert(key, h.clone());
                    (h, true)
                }
            }
        };
        if fresh {
            self.0.emit();
            self.spawn_start(pack, handle);
        } else if handle.begin_start(true) == Begin::Start {
            self.spawn_start(pack, handle);
        }
        Ok(())
    }

    /// Manual stop: graceful shutdown, stays listed as `stopped`.
    pub fn stop(&self, ext_id: &str, project_id: Uuid) -> AppResult<()> {
        let handle = self
            .handle(&Self::key(ext_id, project_id))
            .ok_or_else(|| AppError::Other(format!("{ext_id}: not running for this project")))?;
        handle.stop();
        Ok(())
    }

    /// Manual restart: stop, reset the crash history, start.
    pub fn restart(&self, ext_id: &str, project_id: Uuid) -> AppResult<()> {
        let handle = self
            .handle(&Self::key(ext_id, project_id))
            .ok_or_else(|| AppError::Other(format!("{ext_id}: not running for this project")))?;
        if handle.state().is_live() || handle.state() == State::Starting {
            handle.stop();
        }
        let project = handle.project.clone();
        self.start(ext_id, project)
    }

    /// Teardown: shut every server down.
    pub fn stop_all(&self) {
        let handles: Vec<Arc<ServerHandle>> = self.0.servers.lock().unwrap().values().cloned().collect();
        for h in handles {
            if h.state().is_live() || h.state() == State::Starting {
                h.stop();
            }
        }
        self.0.servers.lock().unwrap().clear();
        self.0.metrics.lock().unwrap().clear();
    }

    /// A live server for `(lang, project)` — `None` never starts one.
    pub fn get_ready(&self, lang: &str, project_id: Uuid) -> Option<Arc<ServerHandle>> {
        let pack = self.0.ext.server_for_language(lang)?;
        let h = self.handle(&Self::key(&pack.id, project_id))?;
        h.state().is_live().then_some(h)
    }

    /// Any server process alive (drives the metrics cadence).
    pub fn any_running(&self) -> bool {
        self.0
            .servers
            .lock()
            .unwrap()
            .values()
            .any(|h| h.pid().is_some())
    }

    /// `("lsp:<extId>:<projectId>", "lsp:<extId>", pid)` rows for the metrics sampler.
    pub fn pids(&self) -> Vec<(String, String, u32)> {
        self.0
            .servers
            .lock()
            .unwrap()
            .values()
            .filter_map(|h| {
                h.pid()
                    .map(|pid| (format!("lsp:{}", h.key()), format!("lsp:{}", h.ext_id), pid))
            })
            .collect()
    }

    /// Take `memBytes`/`cpu` from a metrics snapshot's `lsp:` rows.
    pub fn record_metrics(&self, procs: &[ProcMetric]) {
        let mut m = self.0.metrics.lock().unwrap();
        for p in procs {
            if let Some(key) = p.id.strip_prefix("lsp:") {
                m.insert(key.to_string(), (p.mem_bytes, p.cpu));
            }
        }
    }

    pub fn status_snapshot(&self) -> LspStatus {
        self.0.snapshot()
    }

    pub fn emit_status(&self) {
        self.0.emit();
    }

    // ---- document sync (no-ops without a live server) ----

    pub fn doc_open(&self, lang: &Lang, project_id: Uuid, abs: &Path, text: String, version: i64) {
        if let Some(h) = self.get_ready(lang.id, project_id) {
            let _ = h.open(path_to_uri(abs), text, lang.lsp_id, version);
        }
    }

    pub fn doc_change(&self, lang: &Lang, project_id: Uuid, abs: &Path, text: String, version: i64) {
        if let Some(h) = self.get_ready(lang.id, project_id) {
            let _ = h.change(abs, text, version, lang.lsp_id);
        }
    }

    pub fn doc_close(&self, lang: &Lang, project_id: Uuid, abs: &Path) {
        if let Some(h) = self.get_ready(lang.id, project_id) {
            let _ = h.close(abs);
        }
    }

    // ---- virtual documents ----

    /// Text of a server-owned virtual uri (`jdt://…`), LRU-cached.
    pub fn virtual_read(&self, uri: &str) -> AppResult<VirtualDoc> {
        let scheme = match virtual_docs::dispatch(uri).map_err(AppError::Other)? {
            Dispatch::Library => {
                // `nav_virtual_read` routes orrery-lib:// to LibSrcService before us
                return Err(AppError::Other("orrery-lib:// is served by the library index".into()));
            }
            Dispatch::Server(s) => s.to_string(),
        };
        if let Some(doc) = self.0.vdocs.get(uri) {
            return Ok(doc);
        }
        let handle = self
            .0
            .servers
            .lock()
            .unwrap()
            .values()
            .find(|h| h.state().is_live() && h.manifest.virtual_schemes.iter().any(|s| s == &scheme))
            .cloned()
            .ok_or_else(|| AppError::Other(format!("no running language server serves {scheme}://")))?;
        let method = handle
            .manifest
            .virtual_read
            .as_ref()
            .map(|v| v.method.clone())
            .filter(|m| !m.is_empty())
            .ok_or_else(|| AppError::Other(format!("{}: no virtualRead method", handle.ext_id)))?;
        let v = handle
            .request(&method, json!({ "uri": uri }), BUDGET_VIRTUAL_READ)
            .map_err(|e| AppError::Other(format!("{method}: {e}")))?;
        let mut text = v.as_str().map(str::to_string).unwrap_or_default();
        if text.len() > virtual_docs::MAX_TEXT {
            let mut cut = virtual_docs::MAX_TEXT;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            text.push_str("\n// … truncated (2 MB cap)\n");
        }
        let doc = VirtualDoc {
            uri: uri.to_string(),
            language: monaco_language(&handle.language).to_string(),
            text,
            title: virtual_docs::title_of(uri),
        };
        self.0.vdocs.put(doc.clone());
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::manifest::{fixtures, Manifest};
    use crate::lsp::client::fake::delayed_server;

    fn manifest() -> crate::extensions::manifest::ServerManifest {
        let Manifest::Server(s) = Manifest::parse(fixtures::SERVER).unwrap() else {
            unreachable!()
        };
        s
    }

    fn root() -> PathBuf {
        PathBuf::from(if cfg!(windows) { "C:/w/proj" } else { "/w/proj" })
    }

    fn project() -> ProjectRef {
        ProjectRef {
            id: Uuid::nil(),
            name: "proj".into(),
            root: root(),
        }
    }

    fn range(l: u32, c: u32, el: u32, ec: u32) -> Range {
        Range {
            start: lsp_types::Position { line: l, character: c },
            end: lsp_types::Position { line: el, character: ec },
        }
    }

    #[test]
    fn language_table() {
        assert_eq!(language_of("src/A.java").map(|l| l.id), Some("java"));
        assert_eq!(language_of("src\\main.RS").map(|l| l.lsp_id), Some("rust"));
        assert_eq!(language_of("app/x.tsx").map(|l| (l.id, l.lsp_id)), Some(("tsx", "typescriptreact")));
        assert_eq!(language_of("a.jsx").map(|l| (l.id, l.lsp_id)), Some(("javascript", "javascriptreact")));
        assert_eq!(language_of("x.py").map(|l| l.id), Some("python"));
        assert_eq!(language_of("README.md"), None);
        assert_eq!(language_of("Makefile"), None);
    }

    #[test]
    fn location_conversion_known_root_vs_foreign_scheme() {
        let agent = Uuid::from_u128(7);
        let roots = vec![
            (Uuid::nil(), root()),
            (agent, root().parent().unwrap().join("wt").join("feat")),
        ];
        let file = root().join("src").join("A.java");
        let loc = to_nav(&path_to_uri(&file), &range(3, 4, 3, 9), &roots);
        assert_eq!(loc.uri, format!("orrery://{}/src/A.java", Uuid::nil()));
        assert_eq!(loc.id, Some(Uuid::nil()));
        assert_eq!(loc.path.as_deref(), Some("src/A.java"));
        assert_eq!((loc.line, loc.col, loc.end_line, loc.end_col), (3, 4, 3, 9));
        // a worktree file maps to the agent id
        let wt_file = roots[1].1.join("b.rs");
        let loc = to_nav(&path_to_uri(&wt_file), &range(0, 0, 0, 1), &roots);
        assert_eq!(loc.id, Some(agent));
        assert_eq!(loc.path.as_deref(), Some("b.rs"));
        // foreign scheme → raw uri, no id/path
        let loc = to_nav("jdt://contents/rt.jar/java.lang/String.class", &range(1, 1, 1, 2), &roots);
        assert_eq!(loc.uri, "jdt://contents/rt.jar/java.lang/String.class");
        assert_eq!(loc.id, None);
        assert_eq!(loc.path, None);
        let v = serde_json::to_value(&loc).unwrap();
        assert_eq!(v["id"], Value::Null);
        assert_eq!(v["path"], Value::Null);
        // a file outside every root stays a raw file:// uri
        let loc = to_nav("file:///D:/elsewhere/x.java", &range(0, 0, 0, 0), &roots);
        assert!(loc.uri.starts_with("file://"));
        assert_eq!(loc.id, None);
    }

    #[test]
    fn locations_from_every_response_shape() {
        let roots = vec![(Uuid::nil(), root())];
        let f = path_to_uri(&root().join("a.java"));
        assert!(locations_from(&Value::Null, &roots).is_empty());
        let one = json!({ "uri": f, "range": { "start": { "line": 1, "character": 2 }, "end": { "line": 1, "character": 5 } } });
        assert_eq!(locations_from(&one, &roots).len(), 1);
        assert_eq!(locations_from(&json!([one, one]), &roots).len(), 2);
        let link = json!([{ "targetUri": f, "targetRange": { "start": { "line": 0, "character": 0 }, "end": { "line": 9, "character": 0 } },
            "targetSelectionRange": { "start": { "line": 2, "character": 6 }, "end": { "line": 2, "character": 9 } } }]);
        let l = locations_from(&link, &roots);
        assert_eq!((l[0].line, l[0].col, l[0].end_col), (2, 6, 9), "selection range wins");
        assert!(locations_from(&json!({ "garbage": true }), &roots).is_empty());
    }

    #[test]
    fn hover_markdown_shapes() {
        assert!(hover_from(&Value::Null).is_none());
        let h = hover_from(&json!({ "contents": { "kind": "markdown", "value": "**A**" }, "range": { "start": { "line": 1, "character": 2 }, "end": { "line": 1, "character": 3 } } })).unwrap();
        assert_eq!(h.contents, "**A**");
        assert_eq!(h.source, "lsp");
        let r = h.range.unwrap();
        assert_eq!((r.line, r.col, r.end_line, r.end_col), (1, 2, 1, 3));
        let h = hover_from(&json!({ "contents": { "language": "java", "value": "class A" } })).unwrap();
        assert_eq!(h.contents, "```java\nclass A\n```");
        assert!(h.range.is_none());
        let h = hover_from(&json!({ "contents": ["plain", { "language": "rust", "value": "fn x()" }] })).unwrap();
        assert_eq!(h.contents, "plain\n\n```rust\nfn x()\n```");
        assert!(hover_from(&json!({ "contents": "   " })).is_none());
    }

    #[test]
    fn router_budget_lsp_wins_at_50ms_index_at_500ms() {
        let file = root().join("src").join("A.java");
        let roots = vec![(Uuid::nil(), root())];
        let answer = json!([{ "uri": path_to_uri(&file), "range": { "start": { "line": 4, "character": 1 }, "end": { "line": 4, "character": 2 } } }]);
        let ctx = NavCtx {
            abs: &file,
            line: 0,
            col: 0,
            text: Some("class A {}"),
            language_id: "java",
            roots: &roots,
        };
        let fast = ServerHandle::with_client(
            "server.jdtls",
            manifest(),
            project(),
            delayed_server(Duration::from_millis(50), answer.clone()),
        );
        match route(&fast, &NavKind::Definition, &ctx) {
            Routed::Locations(l) => {
                assert_eq!(l.len(), 1);
                assert_eq!(l[0].path.as_deref(), Some("src/A.java"));
                assert_eq!(l[0].line, 4);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(fast.open_docs().len(), 1, "didOpen once");
        let slow = ServerHandle::with_client(
            "server.jdtls",
            manifest(),
            project(),
            delayed_server(Duration::from_millis(500), answer),
        );
        assert_eq!(route(&slow, &NavKind::Definition, &ctx), Routed::Timeout);
        assert_eq!(route(&slow, &NavKind::Hover, &ctx), Routed::Timeout);
        // empty answers fall through to the index
        let empty = ServerHandle::with_client(
            "server.jdtls",
            manifest(),
            project(),
            delayed_server(Duration::from_millis(5), Value::Null),
        );
        assert_eq!(route(&empty, &NavKind::References { include_declaration: true }, &ctx), Routed::Empty);
        assert_eq!(route(&empty, &NavKind::Hover, &ctx), Routed::Empty);
    }

    #[test]
    fn status_shape() {
        let v = serde_json::to_value(LspStatus::default()).unwrap();
        assert_eq!(v["servers"], json!([]));
    }
}
