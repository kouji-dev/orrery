//! Tree-sitter symbol index (M2): the always-on navigation floor. One
//! in-memory [`SymbolIndex`] per root (agent worktree or project checkout),
//! filled by a below-normal-priority indexer thread that walks the root with
//! the shared ignore-aware walker, fingerprints files against `symbols.db`,
//! decodes cached blobs for known content (blake3) and parses the rest on a
//! small scoped pool. Grammars come from the extension host (`GrammarRegistry`);
//! a root only ever sees the file extensions of installed + enabled packs.
//!
//! Events: `symbols://index { root, state, files, done, total, parsed,
//! elapsedMs, error? }` at start, every 500 ms while indexing, and on done —
//! through the [`StatusSink`] `lib.rs` wires to `core::emit` (the service
//! never touches a concrete Tauri runtime, so the lib test binary stays free
//! of the windowing stack). The watcher feeds [`SymbolService::invalidate`]
//! after every settled burst so the index follows saves; deletes/renames
//! drop their file. Nothing here spawns a process.

pub mod commands;
pub mod index;
pub mod store;
pub mod tags;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tree_sitter::Parser;

use crate::extensions::loader::{GrammarRegistry, Loaded};
use crate::extensions::ExtensionService;
use crate::git::types::FileChange;

use index::SymbolIndex;
use store::{BlobRow, IndexedFile, SymbolStore};
use tags::FileSymbols;

pub const EV_INDEX: &str = "symbols://index";
/// Progress emit cadence while a full index runs.
const PROGRESS_EVERY: Duration = Duration::from_millis(500);
/// Files per sqlite transaction / per scoped-pool round.
const BATCH: usize = 200;
/// Full `FileSymbols` kept hot per root (open files, reference lookups).
const LRU_FILES: usize = 32;

/// Where grammars come from. The extension host in production; a bare
/// [`GrammarRegistry`] in tests.
pub trait GrammarSource: Send + Sync {
    fn grammar_for_extension(&self, ext: &str) -> Option<Arc<Loaded>>;
    /// Lower-case extensions (no dot) of every resident grammar.
    fn extensions(&self) -> Vec<String>;
}

impl GrammarSource for GrammarRegistry {
    fn grammar_for_extension(&self, ext: &str) -> Option<Arc<Loaded>> {
        GrammarRegistry::grammar_for_extension(self, ext)
    }

    fn extensions(&self) -> Vec<String> {
        self.loaded_ids()
            .iter()
            .filter_map(|id| self.get(id))
            .flat_map(|g| {
                g.manifest
                    .file_extensions
                    .iter()
                    .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

impl GrammarSource for ExtensionService {
    fn grammar_for_extension(&self, ext: &str) -> Option<Arc<Loaded>> {
        self.grammars().grammar_for_extension(ext)
    }

    fn extensions(&self) -> Vec<String> {
        GrammarSource::extensions(self.grammars())
    }
}

/// The `symbols://index` payload and the `symbols_index_status` answer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub root: String,
    /// `idle | indexing | ready | error`.
    pub state: String,
    /// Files walked (all types).
    pub files: usize,
    /// Grammar-backed files processed so far / in total.
    pub done: usize,
    pub total: usize,
    /// Files that needed a parse (cache misses) in the last run.
    pub parsed: usize,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct Progress {
    state: &'static str,
    files: usize,
    done: usize,
    total: usize,
    parsed: usize,
    started: Option<Instant>,
    elapsed_ms: u64,
    error: Option<String>,
    last_emit: Option<Instant>,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            state: "idle",
            files: 0,
            done: 0,
            total: 0,
            parsed: 0,
            started: None,
            elapsed_ms: 0,
            error: None,
            last_emit: None,
        }
    }
}

#[derive(Default)]
struct Pending {
    full: bool,
    force: bool,
    paths: HashSet<String>,
    removes: HashSet<String>,
}

#[derive(Default)]
struct Lru {
    map: HashMap<String, (String, Arc<FileSymbols>)>,
    order: VecDeque<String>,
}

impl Lru {
    fn get(&mut self, rel: &str, hash: &str) -> Option<Arc<FileSymbols>> {
        let (h, v) = self.map.get(rel)?;
        if h != hash {
            return None;
        }
        let v = v.clone();
        self.order.retain(|r| r != rel);
        self.order.push_back(rel.to_string());
        Some(v)
    }

    fn put(&mut self, rel: &str, hash: &str, v: Arc<FileSymbols>) {
        self.order.retain(|r| r != rel);
        self.order.push_back(rel.to_string());
        self.map.insert(rel.to_string(), (hash.to_string(), v));
        while self.order.len() > LRU_FILES {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    fn remove(&mut self, rel: &str) {
        if self.map.remove(rel).is_some() {
            self.order.retain(|r| r != rel);
        }
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

/// Everything the service keeps per root.
pub struct RootState {
    root: PathBuf,
    /// Normalized root string — the `index_files.root` key and event `root`.
    key: String,
    pub index: RwLock<SymbolIndex>,
    progress: Mutex<Progress>,
    /// Flag of the run in flight; `stop`/`start` flip it, each run gets a new one.
    cancel: Mutex<Arc<AtomicBool>>,
    pending: Mutex<Pending>,
    worker_active: AtomicBool,
    lru: Mutex<Lru>,
    /// Dirty set reported by the last watcher burst — files that LEAVE it
    /// (revert / commit) changed without being listed, so they get rechecked.
    last_dirty: Mutex<HashSet<String>>,
}

enum Work {
    Full { force: bool },
    Refresh {
        paths: HashSet<String>,
        removes: HashSet<String>,
    },
    None,
}

struct Job {
    rel: String,
    abs: PathBuf,
    ext: String,
    size: u64,
    mtime: i64,
}

struct FileResult {
    rel: String,
    meta: IndexedFile,
    syms: FileSymbols,
    parsed: bool,
    blob: Option<BlobRow>,
    /// Fingerprint differs from the stored one → write it.
    store_meta: bool,
}

/// Where `symbols://index` payloads go (`core::emit::emit_tracked` in prod).
pub type StatusSink = Box<dyn Fn(&IndexStatus) + Send + Sync>;

struct Inner {
    store: Arc<SymbolStore>,
    grammars: Arc<dyn GrammarSource>,
    roots: Mutex<HashMap<String, Arc<RootState>>>,
    sink: Option<StatusSink>,
}

#[derive(Clone)]
pub struct SymbolService(Arc<Inner>);

thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new(Parser::new());
}

/// Indexing must never compete with the UI or an agent's process tree.
pub(crate) fn lower_priority() {
    #[cfg(windows)]
    // SAFETY: plain Win32 calls on the calling thread's pseudo-handle; a
    // failure only leaves the priority unchanged.
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

fn pool_size() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get() / 2)
        .unwrap_or(2)
        .clamp(2, 8)
}

fn mtime_of(md: &std::fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Root-relative, forward-slash, no leading `./`.
pub fn norm_rel(rel: &str) -> String {
    let r = rel.trim().replace('\\', "/");
    let mut r = r.as_str();
    loop {
        let t = r.trim_start_matches("./").trim_start_matches('/');
        if t.len() == r.len() {
            break;
        }
        r = t;
    }
    r.to_string()
}

fn ext_of(rel: &str) -> Option<String> {
    let base = rel.rsplit('/').next()?;
    let (_, ext) = base.rsplit_once('.')?;
    if ext.is_empty() {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

impl SymbolService {
    /// Open (or create) `symbols.db` at `db_path`; falls back to an in-memory
    /// store when the file cannot be opened so navigation still works.
    pub fn new(grammars: Arc<dyn GrammarSource>, db_path: &Path, sink: Option<StatusSink>) -> Self {
        let store = match SymbolStore::open(db_path) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("symbols: open {}: {e} — using an in-memory cache", db_path.display());
                SymbolStore::open_in_memory()
            }
        };
        Self::with_store(grammars, Arc::new(store), sink)
    }

    pub fn with_store(
        grammars: Arc<dyn GrammarSource>,
        store: Arc<SymbolStore>,
        sink: Option<StatusSink>,
    ) -> Self {
        Self(Arc::new(Inner {
            store,
            grammars,
            roots: Mutex::new(HashMap::new()),
            sink,
        }))
    }

    /// The shared `symbols.db` handle (the library index lives in it too).
    pub fn store(&self) -> Arc<SymbolStore> {
        self.0.store.clone()
    }

    pub fn root_key(root: &Path) -> String {
        let s = root.to_string_lossy().replace('\\', "/");
        let t = s.trim_end_matches('/');
        if t.is_empty() {
            s
        } else {
            t.to_string()
        }
    }

    pub fn root_state(&self, root: &Path) -> Option<Arc<RootState>> {
        self.0.roots.lock().unwrap().get(&Self::root_key(root)).cloned()
    }

    fn root_state_or_create(&self, root: &Path) -> (Arc<RootState>, bool) {
        let key = Self::root_key(root);
        let mut roots = self.0.roots.lock().unwrap();
        if let Some(st) = roots.get(&key) {
            return (st.clone(), false);
        }
        let st = Arc::new(RootState {
            root: root.to_path_buf(),
            key: key.clone(),
            index: RwLock::new(SymbolIndex::new()),
            progress: Mutex::new(Progress::default()),
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            pending: Mutex::new(Pending::default()),
            worker_active: AtomicBool::new(false),
            lru: Mutex::new(Lru::default()),
            last_dirty: Mutex::new(HashSet::new()),
        });
        roots.insert(key, st.clone());
        (st, true)
    }

    /// The root's state, starting a full index the first time it is seen.
    pub fn ensure_indexed(&self, root: &Path) -> Arc<RootState> {
        let (st, fresh) = self.root_state_or_create(root);
        if fresh {
            st.pending.lock().unwrap().full = true;
            self.kick(st.clone());
        }
        st
    }

    /// (Re)index a root. `force` drops the stat fingerprints so every file is
    /// re-hashed (blobs still hit). A run in flight is cancelled and restarted.
    pub fn start(&self, root: &Path, force: bool) -> Arc<RootState> {
        let (st, _) = self.root_state_or_create(root);
        {
            let mut p = st.pending.lock().unwrap();
            p.full = true;
            p.force |= force;
        }
        st.cancel.lock().unwrap().store(true, Ordering::Relaxed);
        self.kick(st.clone());
        st
    }

    /// Cancel the run in flight (partial index stays usable, state `idle`).
    pub fn stop(&self, root: &Path) {
        let Some(st) = self.root_state(root) else {
            return;
        };
        *st.pending.lock().unwrap() = Pending::default();
        st.cancel.lock().unwrap().store(true, Ordering::Relaxed);
    }

    pub fn status(&self, root: &Path) -> IndexStatus {
        match self.root_state(root) {
            Some(st) => {
                let mut p = st.progress.lock().unwrap();
                Self::status_of(&st.key, &mut p)
            }
            None => IndexStatus {
                root: Self::root_key(root),
                state: "idle".into(),
                files: 0,
                done: 0,
                total: 0,
                parsed: 0,
                elapsed_ms: 0,
                error: None,
            },
        }
    }

    fn status_of(key: &str, p: &mut Progress) -> IndexStatus {
        if p.state == "indexing" {
            if let Some(s) = p.started {
                p.elapsed_ms = s.elapsed().as_millis() as u64;
            }
        }
        IndexStatus {
            root: key.to_string(),
            state: p.state.to_string(),
            files: p.files,
            done: p.done,
            total: p.total,
            parsed: p.parsed,
            elapsed_ms: p.elapsed_ms,
            error: p.error.clone(),
        }
    }

    /// Watcher hook: recheck the burst's A/M/R paths, drop D + old R paths,
    /// and recheck whatever left the dirty set since the last burst. No-op
    /// for a root nobody asked to index.
    pub fn invalidate(&self, root: &Path, changes: &[FileChange]) {
        let Some(st) = self.root_state(root) else {
            return;
        };
        let mut now_dirty: HashSet<String> = HashSet::with_capacity(changes.len());
        {
            let mut p = st.pending.lock().unwrap();
            for c in changes {
                let path = norm_rel(&c.path);
                match c.state.as_str() {
                    "D" => {
                        p.removes.insert(path.clone());
                    }
                    "R" => {
                        if let Some(old) = &c.old_path {
                            p.removes.insert(norm_rel(old));
                        }
                        p.paths.insert(path.clone());
                    }
                    _ => {
                        p.paths.insert(path.clone());
                    }
                }
                now_dirty.insert(path);
            }
            let mut last = st.last_dirty.lock().unwrap();
            for gone in last.difference(&now_dirty) {
                p.paths.insert(gone.clone());
            }
            *last = now_dirty;
        }
        self.kick(st);
    }

    pub fn grammar_for_path(&self, rel: &str) -> Option<Arc<Loaded>> {
        self.0.grammars.grammar_for_extension(&ext_of(rel)?)
    }

    /// Full symbols of one indexed file: LRU → blob cache → (last resort) a
    /// fresh parse from disk. `None` when the file is unknown to the index
    /// and unreadable.
    pub fn file_symbols(&self, st: &RootState, rel: &str) -> Option<Arc<FileSymbols>> {
        let known = st
            .index
            .read()
            .unwrap()
            .file_by_rel(rel)
            .map(|m| m.hash.clone());
        if let Some(hash) = &known {
            if let Some(v) = st.lru.lock().unwrap().get(rel, hash) {
                return Some(v);
            }
            if let Some(blob) = self.0.store.get_blob(hash) {
                if let Some(fs) = FileSymbols::decode(&blob.bytes) {
                    let v = Arc::new(fs);
                    st.lru.lock().unwrap().put(rel, hash, v.clone());
                    return Some(v);
                }
            }
        }
        let grammar = self.grammar_for_path(rel)?;
        let bytes = std::fs::read(st.root.join(rel)).ok()?;
        let hash = blake3::hash(&bytes).to_hex().to_string();
        if let Some(v) = st.lru.lock().unwrap().get(rel, &hash) {
            return Some(v);
        }
        let fs = PARSER
            .with(|p| tags::extract_with(&bytes, &grammar, &mut p.borrow_mut(), None))
            .ok()?;
        let v = Arc::new(fs);
        st.lru.lock().unwrap().put(rel, &hash, v.clone());
        Some(v)
    }

    // ---- indexer ----

    fn kick(&self, st: Arc<RootState>) {
        if st.worker_active.swap(true, Ordering::AcqRel) {
            return;
        }
        let svc = self.clone();
        let st2 = st.clone();
        let name = format!("symbols-{:08x}", blake3::hash(st.key.as_bytes()).as_bytes()[0] as u32);
        if std::thread::Builder::new()
            .name(name)
            .spawn(move || {
                lower_priority();
                svc.worker(st2);
            })
            .is_err()
        {
            log::warn!("symbols: cannot spawn the indexer thread for {}", st.key);
            st.worker_active.store(false, Ordering::Release);
        }
    }

    fn worker(&self, st: Arc<RootState>) {
        loop {
            let cancel = Arc::new(AtomicBool::new(false));
            *st.cancel.lock().unwrap() = cancel.clone();
            let work = {
                let mut p = st.pending.lock().unwrap();
                if p.full {
                    let force = p.force;
                    *p = Pending::default();
                    Work::Full { force }
                } else if !p.paths.is_empty() || !p.removes.is_empty() {
                    Work::Refresh {
                        paths: std::mem::take(&mut p.paths),
                        removes: std::mem::take(&mut p.removes),
                    }
                } else {
                    Work::None
                }
            };
            match work {
                Work::Full { force } => self.full_index(&st, force, &cancel),
                Work::Refresh { paths, removes } => self.refresh(&st, paths, removes, &cancel),
                Work::None => {
                    st.worker_active.store(false, Ordering::Release);
                    // Something may have been queued between the check and the
                    // flag drop — take the slot back if so, else leave.
                    let more = {
                        let p = st.pending.lock().unwrap();
                        p.full || !p.paths.is_empty() || !p.removes.is_empty()
                    };
                    if more && !st.worker_active.swap(true, Ordering::AcqRel) {
                        continue;
                    }
                    return;
                }
            }
        }
    }

    fn emit(&self, st: &RootState, force: bool) {
        let Some(sink) = &self.0.sink else {
            return;
        };
        let status = {
            let mut p = st.progress.lock().unwrap();
            if !force && p.last_emit.is_some_and(|t| t.elapsed() < PROGRESS_EVERY) {
                return;
            }
            p.last_emit = Some(Instant::now());
            Self::status_of(&st.key, &mut p)
        };
        sink(&status);
    }

    fn full_index(&self, st: &RootState, force: bool, cancel: &AtomicBool) {
        let started = Instant::now();
        {
            let mut p = st.progress.lock().unwrap();
            *p = Progress {
                state: "indexing",
                started: Some(started),
                ..Default::default()
            };
        }
        self.emit(st, true);
        if force {
            if let Err(e) = self.0.store.clear_root(&st.key) {
                log::warn!("symbols: clear {}: {e}", st.key);
            }
            st.index.write().unwrap().clear();
            st.lru.lock().unwrap().clear();
        }
        let known = self.0.store.index_files(&st.key);
        let exts: HashSet<String> = self.0.grammars.extensions().into_iter().collect();

        let mut jobs: Vec<Job> = Vec::new();
        let mut walked = 0usize;
        if !exts.is_empty() {
            for entry in crate::search::walker(&st.root).flatten() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    continue;
                }
                walked += 1;
                let abs = entry.path();
                let Some(ext) = abs
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .filter(|e| exts.contains(e))
                else {
                    continue;
                };
                let rel = abs
                    .strip_prefix(&st.root)
                    .unwrap_or(abs)
                    .to_string_lossy()
                    .replace('\\', "/");
                let (size, mtime) = entry
                    .metadata()
                    .map(|md| (md.len(), mtime_of(&md)))
                    .unwrap_or((0, 0));
                jobs.push(Job {
                    rel,
                    abs: abs.to_path_buf(),
                    ext,
                    size,
                    mtime,
                });
                if walked.is_multiple_of(1000) {
                    let mut p = st.progress.lock().unwrap();
                    p.files = walked;
                    p.total = jobs.len();
                    drop(p);
                    self.emit(st, false);
                }
            }
        }
        {
            let mut p = st.progress.lock().unwrap();
            p.files = walked;
            p.total = jobs.len();
        }
        self.emit(st, false);

        let mut seen: HashSet<String> = HashSet::with_capacity(jobs.len());
        let mut error: Option<String> = None;
        let lookup = |rel: &str| known.get(rel).cloned();
        for chunk in jobs.chunks(BATCH) {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let results = self.process_chunk(chunk, &lookup, cancel);
            if let Err(e) = self.apply(st, results, &mut seen) {
                error = Some(e);
                break;
            }
            {
                let mut p = st.progress.lock().unwrap();
                p.done += chunk.len();
            }
            self.emit(st, false);
        }

        let cancelled = cancel.load(Ordering::Relaxed);
        if !cancelled && error.is_none() {
            let stale: Vec<String> = known
                .keys()
                .filter(|k| !seen.contains(*k))
                .cloned()
                .collect();
            if !stale.is_empty() {
                self.remove_files(st, &stale);
            }
        }
        {
            let mut p = st.progress.lock().unwrap();
            p.state = if error.is_some() {
                "error"
            } else if cancelled {
                "idle"
            } else {
                "ready"
            };
            p.error = error;
            p.elapsed_ms = started.elapsed().as_millis() as u64;
            log::info!(
                "symbols: {} {} — {} files walked, {}/{} indexed, {} parsed, {} defs in {} ms",
                st.key,
                p.state,
                p.files,
                p.done,
                p.total,
                p.parsed,
                st.index.read().unwrap().def_count(),
                p.elapsed_ms
            );
        }
        self.emit(st, true);
        if !cancelled {
            let pruned = self.0.store.prune(store::CACHE_MAX_BYTES);
            if pruned > 0 {
                log::info!("symbols: pruned {pruned} cache rows");
            }
        }
    }

    fn refresh(
        &self,
        st: &RootState,
        paths: HashSet<String>,
        removes: HashSet<String>,
        cancel: &AtomicBool,
    ) {
        let exts: HashSet<String> = self.0.grammars.extensions().into_iter().collect();
        let mut removed: Vec<String> = removes.into_iter().collect();
        let mut jobs: Vec<Job> = Vec::new();
        let mut known: HashMap<String, IndexedFile> = HashMap::new();
        {
            let ix = st.index.read().unwrap();
            for rel in &paths {
                if let Some(m) = ix.file_by_rel(rel) {
                    known.insert(
                        rel.clone(),
                        IndexedFile {
                            hash: m.hash.clone(),
                            size: m.size,
                            mtime: m.mtime,
                            lang: m.lang.clone(),
                        },
                    );
                }
            }
        }
        for rel in paths {
            let Some(ext) = ext_of(&rel).filter(|e| exts.contains(e)) else {
                continue;
            };
            let abs = st.root.join(&rel);
            match std::fs::metadata(&abs) {
                Ok(md) if md.is_file() => jobs.push(Job {
                    rel,
                    abs,
                    ext,
                    size: md.len(),
                    mtime: mtime_of(&md),
                }),
                _ => {
                    if known.contains_key(&rel) {
                        removed.push(rel);
                    }
                }
            }
        }
        let mut changed = false;
        if !removed.is_empty() {
            changed |= self.remove_files(st, &removed);
        }
        let lookup = |rel: &str| known.get(rel).cloned();
        let mut seen = HashSet::new();
        for chunk in jobs.chunks(BATCH) {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let results = self.process_chunk(chunk, &lookup, cancel);
            changed |= !results.is_empty();
            if let Err(e) = self.apply(st, results, &mut seen) {
                let mut p = st.progress.lock().unwrap();
                p.state = "error";
                p.error = Some(e);
                break;
            }
        }
        if changed {
            let ix = st.index.read().unwrap();
            let mut p = st.progress.lock().unwrap();
            p.total = ix.file_count();
            p.done = p.total;
            drop(p);
            drop(ix);
            self.emit(st, true);
        }
    }

    fn remove_files(&self, st: &RootState, rels: &[String]) -> bool {
        let mut any = false;
        {
            let mut ix = st.index.write().unwrap();
            for r in rels {
                any |= ix.remove_file(r);
            }
        }
        {
            let mut lru = st.lru.lock().unwrap();
            for r in rels {
                lru.remove(r);
            }
        }
        if let Err(e) = self.0.store.delete_files(&st.key, rels) {
            log::warn!("symbols: delete fingerprints: {e}");
        }
        any
    }

    fn process_chunk(
        &self,
        jobs: &[Job],
        known: &(dyn Fn(&str) -> Option<IndexedFile> + Sync),
        cancel: &AtomicBool,
    ) -> Vec<FileResult> {
        let workers = pool_size().min(jobs.len()).max(1);
        let next = AtomicUsize::new(0);
        let results: Mutex<Vec<FileResult>> = Mutex::new(Vec::with_capacity(jobs.len()));
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| {
                    lower_priority();
                    loop {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= jobs.len() {
                            break;
                        }
                        let job = &jobs[i];
                        if let Some(r) = self.process_one(job, known(&job.rel), cancel) {
                            results.lock().unwrap().push(r);
                        }
                    }
                });
            }
        });
        results.into_inner().unwrap()
    }

    /// Stat-unchanged → trust the stored hash (no read). Blob hit for the
    /// hash → decode (no parse). Else read + blake3 + parse; the result is a
    /// new blob. Extraction failures (too large, timeout) index as empty so
    /// the file is not retried on every run — its next edit changes the hash.
    fn process_one(&self, job: &Job, known: Option<IndexedFile>, cancel: &AtomicBool) -> Option<FileResult> {
        let grammar = self.0.grammars.grammar_for_extension(&job.ext)?;
        let lang = grammar.manifest.language.clone();
        let mut bytes: Option<Vec<u8>> = None;
        let (hash, store_meta) = match &known {
            Some(k) if k.size == job.size && k.mtime == job.mtime && k.lang == lang => {
                (k.hash.clone(), false)
            }
            _ => {
                let b = std::fs::read(&job.abs).ok()?;
                let h = blake3::hash(&b).to_hex().to_string();
                bytes = Some(b);
                (h, true)
            }
        };
        let cached = self
            .0
            .store
            .get_blob(&hash)
            .filter(|c| c.lang == lang && c.grammar_version == grammar.version)
            .and_then(|c| FileSymbols::decode(&c.bytes));
        let (syms, parsed, blob) = match cached {
            Some(s) => (s, false, None),
            None => {
                let b = match bytes.take() {
                    Some(b) => b,
                    None => std::fs::read(&job.abs).ok()?,
                };
                let r = PARSER.with(|p| {
                    tags::extract_with(&b, &grammar, &mut p.borrow_mut(), Some(cancel))
                });
                let syms = match r {
                    Ok(s) => s,
                    Err(tags::ExtractError::Cancelled) => return None,
                    Err(e) => {
                        log::debug!("symbols: {}: {e}", job.rel);
                        FileSymbols::default()
                    }
                };
                let blob = BlobRow {
                    hash: hash.clone(),
                    lang: lang.clone(),
                    grammar_version: grammar.version.clone(),
                    bytes: syms.encode(),
                };
                (syms, true, Some(blob))
            }
        };
        Some(FileResult {
            rel: job.rel.clone(),
            meta: IndexedFile {
                hash,
                size: job.size,
                mtime: job.mtime,
                lang,
            },
            syms,
            parsed,
            blob,
            store_meta,
        })
    }

    fn apply(
        &self,
        st: &RootState,
        results: Vec<FileResult>,
        seen: &mut HashSet<String>,
    ) -> Result<(), String> {
        if results.is_empty() {
            return Ok(());
        }
        {
            let mut ix = st.index.write().unwrap();
            for r in &results {
                ix.update_file(&r.rel, &r.meta.lang, &r.meta.hash, r.meta.size, r.meta.mtime, &r.syms)
                    .map_err(|e| e.to_string())?;
            }
        }
        let mut files = Vec::new();
        let mut blobs = Vec::new();
        let mut parsed = 0usize;
        {
            let mut lru = st.lru.lock().unwrap();
            for r in results {
                seen.insert(r.rel.clone());
                lru.remove(&r.rel);
                if r.parsed {
                    parsed += 1;
                }
                if let Some(b) = r.blob {
                    blobs.push(b);
                }
                if r.store_meta {
                    files.push((r.rel, r.meta));
                }
            }
        }
        if let Err(e) = self.0.store.write_batch(&st.key, &files, &blobs) {
            log::warn!("symbols: cache write: {e}");
        }
        st.progress.lock().unwrap().parsed += parsed;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::symbols::tags::fixture;

    pub fn wait_state(svc: &SymbolService, root: &Path, want: &str) -> IndexStatus {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let s = svc.status(root);
            if s.state == want {
                return s;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {want}: {s:?}");
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn wait_idle_worker(st: &RootState) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while st.worker_active.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "worker never went idle");
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn change(path: &str, state: &str, old: Option<&str>) -> FileChange {
        FileChange {
            path: path.into(),
            add: 0,
            del: 0,
            state: state.into(),
            old_path: old.map(str::to_string),
        }
    }

    #[test]
    fn empty_registry_indexes_nothing_and_reports_ready() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("A.java"), "class A {}").unwrap();
        let reg: Arc<dyn GrammarSource> = Arc::new(GrammarRegistry::new());
        let svc = SymbolService::with_store(reg, Arc::new(SymbolStore::open_in_memory()), None);
        assert_eq!(svc.status(dir.path()).state, "idle");
        let st = svc.ensure_indexed(dir.path());
        let s = wait_state(&svc, dir.path(), "ready");
        assert_eq!((s.total, s.done, s.parsed), (0, 0, 0));
        assert_eq!(s.root, SymbolService::root_key(dir.path()));
        assert_eq!(st.index.read().unwrap().file_count(), 0);
        // invalidate on an unknown root is a no-op; on a known one it is safe
        svc.invalidate(Path::new("Z:/nowhere"), &[change("x", "M", None)]);
        svc.invalidate(dir.path(), &[change("A.java", "M", None)]);
        wait_idle_worker(&st);
        assert_eq!(svc.status(dir.path()).state, "ready");
    }

    #[test]
    fn status_serializes_camel_case() {
        let v = serde_json::to_value(IndexStatus {
            root: "r".into(),
            state: "indexing".into(),
            files: 1,
            done: 2,
            total: 3,
            parsed: 4,
            elapsed_ms: 5,
            error: None,
        })
        .unwrap();
        for k in ["root", "state", "files", "done", "total", "parsed", "elapsedMs"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        assert!(v.get("error").is_none());
    }

    #[test]
    fn norm_rel_and_root_key() {
        assert_eq!(norm_rel(".\\src\\A.java"), "src/A.java");
        assert_eq!(norm_rel("/x"), "x");
        assert_eq!(norm_rel("a/b"), "a/b");
        assert_eq!(SymbolService::root_key(Path::new("C:\\w\\t\\")), "C:/w/t");
        assert_eq!(ext_of("a/B.Java").as_deref(), Some("java"));
        assert_eq!(ext_of("Makefile"), None);
    }

    #[test]
    fn lru_evicts_oldest_and_checks_hash() {
        let mut l = Lru::default();
        for i in 0..(LRU_FILES + 2) {
            l.put(&format!("f{i}"), "h", Arc::new(FileSymbols::default()));
        }
        assert!(l.get("f0", "h").is_none());
        assert!(l.get("f1", "h").is_none());
        assert!(l.get("f2", "h").is_some());
        assert!(l.get("f2", "other").is_none(), "stale hash → miss");
        l.remove("f2");
        assert!(l.get("f2", "h").is_none());
    }

    /// Real grammar: index two files, warm reopen hits the cache (zero
    /// parses), edits/deletes flow through `invalidate`.
    #[test]
    #[ignore]
    fn java_root_indexes_caches_and_refreshes() {
        let Some(g) = fixture::java_or_skip("java_root_index") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/a")).unwrap();
        std::fs::create_dir_all(root.join("src/b")).unwrap();
        std::fs::write(root.join("src/a/Cart.java"), fixture::JAVA_SRC).unwrap();
        std::fs::write(
            root.join("src/b/Base.java"),
            "package com.acme.core;\npublic class Base { void sum(int c) {} }\n",
        )
        .unwrap();
        std::fs::write(root.join("README.md"), "# no grammar\n").unwrap();

        struct One(Arc<Loaded>);
        impl GrammarSource for One {
            fn grammar_for_extension(&self, ext: &str) -> Option<Arc<Loaded>> {
                (ext.eq_ignore_ascii_case("java")).then(|| self.0.clone())
            }
            fn extensions(&self) -> Vec<String> {
                vec!["java".into()]
            }
        }
        let reg = One(g);
        let reg: Arc<dyn GrammarSource> = Arc::new(reg);
        let store = Arc::new(SymbolStore::open_in_memory());

        let emitted: Arc<Mutex<Vec<IndexStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_log = emitted.clone();
        let sink: StatusSink = Box::new(move |s| sink_log.lock().unwrap().push(s.clone()));
        let svc = SymbolService::with_store(reg.clone(), store.clone(), Some(sink));
        let st = svc.ensure_indexed(root);
        let s = wait_state(&svc, root, "ready");
        assert_eq!((s.files, s.total, s.done, s.parsed), (3, 2, 2, 2), "{s:?}");
        {
            let e = emitted.lock().unwrap();
            assert_eq!(e.first().map(|s| s.state.as_str()), Some("indexing"), "{e:?}");
            let last = e.last().unwrap();
            assert_eq!(last.state, "ready");
            assert_eq!((last.done, last.total, last.parsed), (2, 2, 2));
            assert_eq!(last.root, SymbolService::root_key(root));
        }
        {
            let ix = st.index.read().unwrap();
            assert_eq!(ix.file_count(), 2);
            assert_eq!(ix.definitions("Cart").len(), 1);
            assert_eq!(ix.definitions("Base").len(), 1);
            let cart = ix.file_by_rel("src/a/Cart.java").unwrap();
            assert_eq!(cart.package.as_deref(), Some("com.acme.shop"));
            assert_eq!(cart.lang, "java");
            assert!(!cart.hash.is_empty());
            // reference posting: Cart.java calls sum()
            let files = ix.files_referencing("sum");
            assert_eq!(files.len(), 1);
            assert_eq!(ix.file(files[0]).unwrap().rel, "src/a/Cart.java");
        }
        assert_eq!(store.cache_rows(), 2);
        assert_eq!(store.index_files(&st.key).len(), 2);
        // full symbols come back through the blob cache
        let fs = svc.file_symbols(&st, "src/a/Cart.java").unwrap();
        assert!(fs.defs.iter().any(|d| d.name == "total"));

        // warm reopen: a fresh service on the same store parses nothing
        let svc2 = SymbolService::with_store(reg.clone(), store.clone(), None);
        svc2.ensure_indexed(root);
        let s = wait_state(&svc2, root, "ready");
        assert_eq!((s.total, s.done, s.parsed), (2, 2, 0), "{s:?}");
        assert_eq!(svc2.root_state(root).unwrap().index.read().unwrap().def_count(), st.index.read().unwrap().def_count());
        // force re-hashes every file but still hits the blob cache
        svc2.start(root, true);
        let s = wait_state(&svc2, root, "ready");
        assert_eq!(s.parsed, 0, "{s:?}");

        // incremental: edit Base.java (new class name) → invalidate → reparse
        std::thread::sleep(Duration::from_millis(20)); // distinct mtime
        std::fs::write(
            root.join("src/b/Base.java"),
            "package com.acme.core;\npublic class Base2 { }\n",
        )
        .unwrap();
        svc.invalidate(root, &[change("src/b/Base.java", "M", None)]);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let ix = st.index.read().unwrap();
            if !ix.definitions("Base2").is_empty() {
                assert!(ix.definitions("Base").is_empty());
                break;
            }
            drop(ix);
            assert!(Instant::now() < deadline, "refresh never landed");
            std::thread::sleep(Duration::from_millis(15));
        }
        assert_eq!(store.cache_rows(), 3);
        // delete → removed from index + fingerprints; a rename drops the old path
        std::fs::remove_file(root.join("src/b/Base.java")).unwrap();
        svc.invalidate(root, &[change("src/b/Base.java", "D", None)]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while st.index.read().unwrap().file_by_rel("src/b/Base.java").is_some() {
            assert!(Instant::now() < deadline, "delete never landed");
            std::thread::sleep(Duration::from_millis(15));
        }
        assert_eq!(store.index_files(&st.key).len(), 1);
        std::fs::rename(root.join("src/a/Cart.java"), root.join("src/a/Cart2.java")).unwrap();
        svc.invalidate(root, &[change("src/a/Cart2.java", "R", Some("src/a/Cart.java"))]);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let ix = st.index.read().unwrap();
            if ix.file_by_rel("src/a/Cart2.java").is_some() && ix.file_by_rel("src/a/Cart.java").is_none() {
                break;
            }
            drop(ix);
            assert!(Instant::now() < deadline, "rename never landed");
            std::thread::sleep(Duration::from_millis(15));
        }
        // stop on a finished root is harmless
        svc.stop(root);
        wait_idle_worker(&st);
        assert_eq!(svc.status(root).state, "ready");
    }
}
