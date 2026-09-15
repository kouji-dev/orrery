//! Real-server integration smokes, one per v1 language server, driven through
//! the same code the app uses (`Manifest::parse` on the shipped descriptor →
//! `detect_server` → `launch_for` → `ServerHandle::start` → `route`).
//! Opt-in (they need the servers installed):
//!
//! ```text
//! ORRERY_LSP_SMOKE=1 cargo test --lib lsp::smoke_tests -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `ORRERY_LSP_<ID>_PATH` (ID = the manifest id after `server.`, upper-case,
//! `-` → `_`, e.g. `ORRERY_LSP_RUST_ANALYZER_PATH`) overrides the program;
//! jdtls is looked up in `%LOCALAPPDATA%/orrery-lsp-smoke/jdtls` (the
//! unpacked tarball) when it is not on PATH.
//!
//! PACK mode — the self-contained path the app ships:
//!
//! ```text
//! ORRERY_LSP_SMOKE=1 ORRERY_LSP_SMOKE_PACKS=<abs dist-ext> cargo test --lib lsp::smoke_tests -- --ignored --nocapture --test-threads=1
//! ```
//!
//! spins up a real [`ExtensionService`] on a scratch app-data dir with the
//! registry at `file:///<dist-ext>/index.json`, installs every `server.*`
//! pack THROUGH `run_install` (dependencies first), resolves the launch with
//! [`resolve_launch`] and runs the same drill with the process env scrubbed
//! (`PATH` = System32 only, no `JAVA_HOME` / `GOPATH` / `CARGO_HOME` /
//! `NODE_PATH`) so any reliance on the user's tooling fails loudly.
//! `ORRERY_LSP_SMOKE_TOOLCHAINS=<dir;dir>` re-adds PROJECT toolchains (go,
//! cargo) that gopls / rust-analyzer need to load a workspace — those are the
//! project's, not the server's. Point it at the COMPILER's directory, not at
//! the server binary's: gopls without `go` on PATH starts fine and then answers
//! every definition with `no views`, which reads like a broken integration and
//! is not one. On this box that is `C:/Program Files/Go/bin` (`go env GOROOT`),
//! NOT `~/go/bin` (where the gopls binary lives).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sysinfo::{Pid, System};
use uuid::Uuid;

use super::server::{launch_for, Launch, Placeholders, ProjectRef, ServerHandle, State};
use super::uri::path_to_uri;
use super::{hover_from, language_of, locations_from, prepared_manifest, resolve_launch, route, NavCtx, Routed};
use crate::extensions::detect::{detect_server, DetectEnv};
use crate::extensions::manifest::{Manifest, ServerManifest};
use crate::extensions::registry::{target_key, HttpFetcher};
use crate::extensions::{ExtProgress, ExtensionService};
use crate::settings::SettingsService;
use crate::symbols::commands::{NavKind, NavLocation};

/// One request's budget inside the retry loops (cold servers are slow).
const REQ: Duration = Duration::from_secs(20);
/// The shutdown policy: `shutdown` 2 s + `exit` 2 s, then a kill.
const SHUTDOWN_POLICY: Duration = Duration::from_millis(4500);

struct Sample {
    id: &'static str,
    files: &'static [(&'static str, &'static str)],
    def_file: &'static str,
    /// Text on the definition line (its line is the expected answer).
    def_needle: &'static str,
    use_file: &'static str,
    /// The reference token; the FIRST occurrence is asked first.
    use_needle: &'static str,
    /// The use file after the edit: a new reference at the LAST occurrence.
    edited: &'static str,
    /// `documentSymbol` name (prefix match — jdtls returns `helper() : int`).
    symbol: &'static str,
    ready_cap: Duration,
}

fn gated() -> bool {
    if std::env::var("ORRERY_LSP_SMOKE").ok().as_deref() == Some("1") {
        return true;
    }
    eprintln!("skipped: set ORRERY_LSP_SMOKE=1 to run the real-server smokes");
    false
}

fn scripts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("scripts")
        .join("extensions")
        .join("servers")
}

/// The shipped descriptor, parsed by the host's own validator. The source
/// descriptors may carry `"sha256": ""` (the build pipeline pins them before
/// publishing); the host refuses an empty pin, so pin a placeholder here.
fn manifest(id: &str) -> ServerManifest {
    let p = scripts_dir().join(format!("{id}.json"));
    let raw = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut v: Value = serde_json::from_str(&raw).unwrap();
    if let Some(d) = v.get_mut("download").and_then(Value::as_object_mut) {
        for t in d.values_mut() {
            if t.get("sha256").and_then(Value::as_str) == Some("") {
                t["sha256"] = json!("0".repeat(64));
            }
        }
    }
    match Manifest::parse(&v.to_string()).unwrap_or_else(|e| panic!("{id}.json: {e}")) {
        Manifest::Server(s) => s,
        Manifest::Grammar(_) | Manifest::Runtime(_) => panic!("{id}.json is not a server"),
    }
}

fn env_override(m: &ServerManifest) -> Option<PathBuf> {
    let key = format!(
        "ORRERY_LSP_{}_PATH",
        m.id.trim_start_matches("server.").to_ascii_uppercase().replace('-', "_")
    );
    std::env::var(&key).ok().filter(|p| !p.trim().is_empty()).map(PathBuf::from)
}

fn smoke_base() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("orrery-lsp-smoke")
}

/// Where a jdtls tarball is unpacked for the smoke (stands in for `${extDir}`).
fn jdtls_smoke_dir() -> PathBuf {
    smoke_base().join("jdtls")
}

// ---- pack mode ----------------------------------------------------------------

/// `ORRERY_LSP_SMOKE_PACKS` — the `dist-ext` dir holding `index.json`.
fn packs_dir() -> Option<PathBuf> {
    std::env::var_os("ORRERY_LSP_SMOKE_PACKS")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// The scratch extension host every pack-mode test installs into (one per
/// process: the runtimes are ~100 MB unpacked, once is enough).
struct PackHost {
    ext: ExtensionService,
}

/// Packs need `minOrreryVersion` 0.24.0; the crate may still be behind that.
const SMOKE_APP_VERSION: &str = "0.24.0";

fn pack_host() -> &'static PackHost {
    static HOST: OnceLock<PackHost> = OnceLock::new();
    HOST.get_or_init(|| {
        let dist = packs_dir().expect("ORRERY_LSP_SMOKE_PACKS");
        let index = dist.join("index.json");
        assert!(index.is_file(), "{}: not found", index.display());
        let url = tauri::Url::from_file_path(&index)
            .unwrap_or_else(|_| panic!("{}: not absolute", index.display()))
            .to_string();
        let root = smoke_base().join("packs");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let db: crate::core::database::DB =
            Arc::new(Mutex::new(rusqlite::Connection::open_in_memory().unwrap()));
        let settings = SettingsService::new(db.clone());
        let mut s = settings.get().unwrap();
        s.ext_registry_url = Some(url.clone());
        s.lsp_use_system_servers = false;
        settings.set(&s).unwrap();
        let ext = ExtensionService::new(
            db,
            settings,
            root.join("extensions"),
            SMOKE_APP_VERSION.into(),
            Box::new(HttpFetcher),
        );
        ext.startup();
        eprintln!("PACK HOST: registry {url}\n           extensions {}", ext.root().display());
        scrub_env();
        PackHost { ext }
    })
}

/// The process env a shipped Orrery cannot count on: only the OS dirs on
/// PATH, none of the toolchain homes. Done once, before any server spawns.
fn scrub_env() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let mut dirs: Vec<PathBuf> = if cfg!(windows) {
            let sr = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
            vec![Path::new(&sr).join("System32")]
        } else {
            vec![PathBuf::from("/usr/bin"), PathBuf::from("/bin")]
        };
        if let Some(extra) = std::env::var_os("ORRERY_LSP_SMOKE_TOOLCHAINS") {
            dirs.extend(std::env::split_paths(&extra));
        }
        let path = std::env::join_paths(&dirs).unwrap();
        std::env::set_var("PATH", &path);
        for var in [
            "JAVA_HOME", "JDK_HOME", "JDTLS_HOME", "GOPATH", "GOROOT", "GOBIN", "CARGO_HOME", "RUSTUP_HOME",
            "NODE_PATH", "NODE_OPTIONS", "JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS",
        ] {
            std::env::remove_var(var);
        }
        eprintln!("ENV SCRUBBED: PATH={}", path.to_string_lossy());
    });
}

/// Install `server.<id>` through the real pipeline (deps first) and resolve
/// its launch the way `LspService::spawn_start` does. `None` = not in the
/// registry for this target (reported, not failed).
fn prepare_pack(id: &str, root: &Path, storage: &Path) -> Option<Prepared> {
    let host = pack_host();
    let ext_id = format!("server.{id}");
    let t0 = Instant::now();
    let mut last = String::new();
    let mut steps: Vec<String> = Vec::new();
    let r = host.ext.run_install(&ext_id, &mut |p: ExtProgress| {
        let key = format!("{}:{}", p.dependency.clone().unwrap_or_else(|| ext_id.clone()), p.phase);
        if key != last {
            if p.phase == "download" {
                let what = p.dependency.clone().unwrap_or_else(|| ext_id.clone());
                eprintln!("  install {}/{}: {what} ({} bytes)", p.step_index, p.step_count, p.total.unwrap_or(0));
                steps.push(what);
            }
            last = key;
        }
    });
    match r {
        Ok(()) => {}
        Err(e) if e.contains("no build for") => {
            eprintln!("{ext_id}: {e}");
            return None;
        }
        Err(e) => panic!("{ext_id}: install: {e}"),
    }
    eprintln!("  installed {} in {:.2?}", steps.join(" → "), t0.elapsed());
    let pack = host.ext.server_pack(&ext_id).expect("installed pack");
    assert!(pack.enabled);
    // the panel's view says `bundled` and points at a binary under our root
    let view = host.ext.snapshot();
    let item = view.items.iter().find(|p| p.id == ext_id).expect("in view");
    assert_eq!(item.state, "installed", "{ext_id}: {item:?}");
    let d = item.detection.clone().expect("detection");
    assert_eq!(d.status, "bundled", "{ext_id}: {d:?}");
    let launch = resolve_launch(&host.ext, &pack, root, storage)
        .unwrap_or_else(|hint| panic!("{ext_id}: not launchable: {hint:?}"));
    assert!(
        launch.program.starts_with(host.ext.root()),
        "{ext_id}: program {} is NOT from a pack",
        launch.program.display()
    );
    assert_eq!(d.path.as_deref(), Some(launch.program.to_string_lossy().as_ref()));
    let manifest = prepared_manifest(&pack, root, storage);
    // the extDir substitution must have landed in initializationOptions (ts-ls tsserver.path)
    if let Some(io) = &manifest.initialization_options {
        assert!(!io.to_string().contains("${"), "{ext_id}: unresolved placeholder in {io}");
    }
    Some(Prepared {
        manifest,
        launch,
        version: pack.manifest.version.clone(),
        source: "pack",
    })
}

/// The system-mode prepare: the descriptor from `scripts/`, the program from
/// PATH / env override, `launch_for` as the app does for a system server.
fn prepare_system(s: &Sample, root: &Path, storage: &Path) -> Option<Prepared> {
    let m = manifest(s.id);
    let pack_dir = if s.id == "jdtls" { jdtls_smoke_dir() } else { std::env::temp_dir() };
    let program = program_for(&m, &pack_dir)?;
    let version = version_of(s.id, &program);
    let fwd = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let (ext_dir, storage_s, root_s) = (fwd(&pack_dir), fwd(storage), fwd(root));
    let ph = Placeholders::new(&ext_dir, &storage_s, &root_s);
    let launch = launch_for(&m, &program, &ph, root.to_path_buf(), &DetectEnv::current());
    Some(Prepared { manifest: m, launch, version, source: "system" })
}

/// One server ready to drill, whichever mode produced it.
struct Prepared {
    manifest: ServerManifest,
    launch: Launch,
    version: String,
    source: &'static str,
}

/// The program, resolved exactly as `LspService::spawn_start` does (manual
/// path → env dirs → PATH → home bins → downloaded entry under the pack dir).
fn program_for(m: &ServerManifest, pack_dir: &Path) -> Option<PathBuf> {
    if let Some(p) = env_override(m) {
        return Some(p);
    }
    let d = detect_server(m, None, Some(pack_dir), &target_key(), &DetectEnv::current());
    if d.status == "missing" {
        eprintln!("{}: cannot run here: {}", m.id, d.hint.unwrap_or_default());
        return None;
    }
    d.path.map(PathBuf::from)
}

/// `(line, utf-16 col)` of `needle` in `text` (first or last occurrence).
fn pos_of(text: &str, needle: &str, last: bool) -> (u32, u32) {
    let idx = if last { text.rfind(needle) } else { text.find(needle) }
        .unwrap_or_else(|| panic!("{needle:?} not in sample"));
    let before = &text[..idx];
    let line = before.matches('\n').count() as u32;
    let col = before.rsplit('\n').next().unwrap_or("").encode_utf16().count() as u32;
    (line, col)
}

fn version_of(id: &str, program: &Path) -> String {
    if id == "jdtls" {
        let plugins = program.parent().and_then(Path::parent).map(|d| d.join("plugins"));
        return plugins
            .and_then(|p| std::fs::read_dir(p).ok())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .find_map(|n| n.strip_prefix("org.eclipse.jdt.ls.core_").map(|r| r.trim_end_matches(".jar").to_string()))
            .unwrap_or_else(|| "?".into());
    }
    let (prog, arg) = match id {
        "gopls" => (program.to_path_buf(), "version"),
        // `pyright-langserver --version` refuses to run without a transport; its sibling answers
        "pyright" => {
            let ext = program.extension().and_then(|e| e.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
            (program.with_file_name(format!("pyright{ext}")), "--version")
        }
        _ => (program.to_path_buf(), "--version"),
    };
    let out = crate::core::proc::cmd(&prog).arg(arg).output();
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(if o.stdout.is_empty() { &o.stderr } else { &o.stdout }).to_string();
            s.lines().next().unwrap_or("?").trim().to_string()
        }
        Err(e) => format!("? ({e})"),
    }
}

fn tree_pids(sys: &System, root: u32) -> Vec<Pid> {
    let mut out = vec![Pid::from_u32(root)];
    let mut i = 0;
    while i < out.len() {
        for (pid, p) in sys.processes() {
            if p.parent() == Some(out[i]) && !out.contains(pid) {
                out.push(*pid);
            }
        }
        i += 1;
    }
    out
}

/// RSS of the server's process tree (a `.cmd` launcher's child node/python is
/// the one doing the work) + `(pid, start time)` identities so the shutdown
/// check can find orphans without being fooled by Windows' pid reuse.
fn rss_tree(root: u32) -> (f64, Vec<(Pid, u64)>) {
    let sys = System::new_all();
    let pids = tree_pids(&sys, root);
    let procs: Vec<_> = pids.iter().filter_map(|p| sys.process(*p)).collect();
    let bytes: u64 = procs.iter().map(|p| p.memory()).sum();
    (bytes as f64 / (1024.0 * 1024.0), procs.iter().map(|p| (p.pid(), p.start_time())).collect())
}

/// Members of `tree` still running (same pid AND start time), polled for up
/// to `cap`: a gracefully exiting server may still be reaping a `cargo`
/// / `tsserver` child for a moment.
fn survivors(tree: &[(Pid, u64)], cap: Duration) -> Vec<Pid> {
    let t0 = Instant::now();
    loop {
        let sys = System::new_all();
        let alive: Vec<Pid> = tree
            .iter()
            .filter(|(pid, start)| sys.process(*pid).is_some_and(|p| p.start_time() == *start))
            .map(|(pid, _)| *pid)
            .collect();
        if alive.is_empty() || t0.elapsed() > cap {
            return alive;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn write_files(root: &Path, files: &[(&str, &str)]) {
    for (rel, text) in files {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
    }
}

fn definition_until(
    h: &ServerHandle,
    abs: &Path,
    (line, col): (u32, u32),
    roots: &[(Uuid, PathBuf)],
    cap: Duration,
) -> (Vec<NavLocation>, Duration, u32) {
    let t0 = Instant::now();
    let mut attempts = 0;
    let params = json!({
        "textDocument": { "uri": path_to_uri(abs) },
        "position": { "line": line, "character": col }
    });
    loop {
        attempts += 1;
        match h.request("textDocument/definition", params.clone(), REQ) {
            Ok(v) => {
                if std::env::var("ORRERY_LSP_SMOKE_DEBUG").is_ok() {
                    eprintln!("  definition raw: {v}");
                }
                let locs = locations_from(&v, roots);
                if !locs.is_empty() {
                    return (locs, t0.elapsed(), attempts);
                }
            }
            Err(super::client::LspErr::Closed) => {
                panic!("{}: connection closed: {:?}", h.ext_id, h.snapshot().last_error)
            }
            Err(e) => eprintln!("  definition attempt {attempts}: {e}"),
        }
        assert!(t0.elapsed() < cap, "{}: no definition within {cap:?} ({attempts} attempts)", h.ext_id);
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn hover_until(h: &ServerHandle, abs: &Path, (line, col): (u32, u32), cap: Duration) -> String {
    let t0 = Instant::now();
    let params = json!({
        "textDocument": { "uri": path_to_uri(abs) },
        "position": { "line": line, "character": col }
    });
    loop {
        if let Ok(v) = h.request("textDocument/hover", params.clone(), REQ) {
            if let Some(hv) = hover_from(&v) {
                return hv.contents;
            }
        }
        assert!(t0.elapsed() < cap, "{}: no hover within {cap:?}", h.ext_id);
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn has_symbol(v: &Value, name: &str) -> bool {
    match v {
        Value::Array(a) => a.iter().any(|x| has_symbol(x, name)),
        Value::Object(o) => {
            o.get("name").and_then(Value::as_str).is_some_and(|n| n.starts_with(name))
                || o.get("children").is_some_and(|c| has_symbol(c, name))
        }
        _ => false,
    }
}

fn symbols_until(h: &ServerHandle, abs: &Path, name: &str, cap: Duration) -> Value {
    let t0 = Instant::now();
    let params = json!({ "textDocument": { "uri": path_to_uri(abs) } });
    loop {
        if let Ok(v) = h.request("textDocument/documentSymbol", params.clone(), REQ) {
            if has_symbol(&v, name) {
                return v;
            }
        }
        assert!(t0.elapsed() < cap, "{}: no documentSymbol {name:?} within {cap:?}", h.ext_id);
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// The whole drill for one server; returns the summary row.
fn run(s: &Sample) -> Option<String> {
    let proj = tempfile::tempdir().unwrap();
    let root = proj.path().to_path_buf();
    write_files(&root, s.files);
    let ws = tempfile::tempdir().unwrap();
    let storage = ws.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();

    let pack_mode = packs_dir().is_some();
    if pack_mode {
        eprintln!("== server.{} (pack mode)", s.id);
    }
    let Prepared { manifest: m, launch, version, source } = if pack_mode {
        prepare_pack(s.id, &root, &storage)?
    } else {
        prepare_system(s, &root, &storage)?
    };
    eprintln!("== {} — {} ({version}, {source})", m.id, launch.program.display());

    let project = ProjectRef {
        id: Uuid::new_v4(),
        name: "smoke".into(),
        root: root.clone(),
    };
    let roots = vec![(project.id, root.clone())];
    let handle = ServerHandle::new(m.id.clone(), m.clone(), project, Box::new(|| {}));
    eprintln!("  launch: {} {:?} (env -{:?})", launch.program.display(), launch.args, launch.env_remove);

    // 1. spawn → initialize → initialized → ready
    let t0 = Instant::now();
    handle.start(&launch);
    let spawn_ready = t0.elapsed();
    assert_eq!(
        handle.state(),
        State::Ready,
        "{}: not ready: {:?}",
        m.id,
        handle.snapshot().last_error
    );
    let pid = handle.pid().expect("pid");
    let (rss_mb, pids) = rss_tree(pid);
    eprintln!("  ready in {spawn_ready:.2?}, pid {pid} (+{} children), rss {rss_mb:.0} MB", pids.len() - 1);

    // 2. definition on the reference → the def file + line (via the router's converters)
    let lang = language_of(s.use_file).expect("language");
    let use_abs = root.join(s.use_file);
    let def_abs = root.join(s.def_file);
    let use_text = std::fs::read_to_string(&use_abs).unwrap();
    let def_text = std::fs::read_to_string(&def_abs).unwrap();
    let want_line = pos_of(&def_text, s.def_needle, false).0;
    handle.ensure_open(&use_abs, Some(&use_text), lang.lsp_id).unwrap();
    let ref_pos = pos_of(&use_text, s.use_needle, false);
    let (locs, first_def, attempts) = definition_until(&handle, &use_abs, ref_pos, &roots, s.ready_cap);
    eprintln!("  definition: {first_def:.2?} ({attempts} attempt(s)) → {:?}:{}", locs[0].path, locs[0].line);
    let def_rel = s.def_file.replace('\\', "/");
    assert!(
        locs.iter().any(|l| l.path.as_deref() == Some(def_rel.as_str()) && l.line == want_line),
        "{}: definition {:?}, want {def_rel}:{want_line}",
        m.id,
        locs.iter().map(|l| (l.uri.clone(), l.line)).collect::<Vec<_>>()
    );
    assert_eq!(locs[0].id, Some(roots[0].0), "uri mapped to the project root");

    // 3. hover
    let hover = hover_until(&handle, &use_abs, ref_pos, Duration::from_secs(30));
    eprintln!("  hover: {:?}", hover.lines().find(|l| !l.trim().is_empty()).unwrap_or(""));
    assert!(!hover.trim().is_empty());

    // 4. documentSymbol on the def file
    handle.ensure_open(&def_abs, Some(&def_text), lang.lsp_id).unwrap();
    let syms = symbols_until(&handle, &def_abs, s.symbol, Duration::from_secs(30));
    eprintln!("  symbols: {} top-level, has {:?}", syms.as_array().map(Vec::len).unwrap_or(0), s.symbol);

    // 5. didChange (a new reference appended) → definition from the new spot
    handle.change(&use_abs, s.edited.to_string(), 2, lang.lsp_id).unwrap();
    let new_pos = pos_of(s.edited, s.use_needle, true);
    assert!(new_pos.0 > ref_pos.0, "the edit adds a later reference");
    let (locs2, after_change, _) = definition_until(&handle, &use_abs, new_pos, &roots, Duration::from_secs(60));
    assert!(
        locs2.iter().any(|l| l.path.as_deref() == Some(def_rel.as_str()) && l.line == want_line),
        "{}: definition after didChange {:?}",
        m.id,
        locs2.iter().map(|l| (l.uri.clone(), l.line)).collect::<Vec<_>>()
    );
    eprintln!("  didChange → definition at {}:{} in {after_change:.2?}", new_pos.0, new_pos.1);

    // 6. the production router on a warm server (300 ms budget)
    let ctx = NavCtx {
        abs: &use_abs,
        line: new_pos.0,
        col: new_pos.1,
        text: None,
        language_id: lang.lsp_id,
        roots: &roots,
    };
    let routed = route(&handle, &NavKind::Definition, &ctx);
    let routed_s = match &routed {
        Routed::Locations(l) => format!("Locations({})", l.len()),
        other => format!("{other:?}"),
    };
    eprintln!("  route(definition, 300 ms): {routed_s}");
    assert!(!matches!(routed, Routed::Failed | Routed::Empty), "{}: route → {routed_s}", m.id);

    // 7. java only: a JDK class → jdt:// + java/classFileContents (the virtual-doc path)
    let mut extra = String::new();
    if s.id == "jdtls" {
        let jdk_pos = pos_of(s.edited, "ArrayList<String>", false);
        let (jl, jdk_ms, _) = definition_until(&handle, &use_abs, jdk_pos, &roots, Duration::from_secs(60));
        let uri = &jl[0].uri;
        eprintln!("  JDK definition in {jdk_ms:.2?}: {uri}");
        assert!(uri.starts_with("jdt://"), "{uri}");
        assert_eq!(jl[0].path, None);
        assert_eq!(super::virtual_docs::dispatch(uri), Ok(super::virtual_docs::Dispatch::Server("jdt")));
        assert!(handle.manifest.virtual_schemes.iter().any(|x| x == "jdt"));
        let method = handle.manifest.virtual_read.as_ref().map(|v| v.method.clone()).unwrap();
        let t = Instant::now();
        let v = handle.request(&method, json!({ "uri": uri }), Duration::from_secs(30)).unwrap();
        let text = v.as_str().unwrap_or("");
        eprintln!("  {method}: {} bytes in {:.2?}, title {:?}", text.len(), t.elapsed(), super::virtual_docs::title_of(uri));
        assert!(text.contains("class ArrayList"), "{}", &text[..text.len().min(200)]);
        extra = format!(" jdt://✓({} B)", text.len());
    }

    // 8. shutdown inside the policy, no zombie, no orphan
    let t = Instant::now();
    handle.stop();
    let stop = t.elapsed();
    assert_eq!(handle.state(), State::Stopped);
    assert!(stop <= SHUTDOWN_POLICY, "{}: stop took {stop:?}", m.id);
    let alive = survivors(&pids, Duration::from_secs(3));
    assert!(alive.is_empty(), "{}: still alive 3 s after stop: {alive:?}", m.id);
    eprintln!("  stopped in {stop:.2?}, tree of {} gone", pids.len());

    Some(format!(
        "SMOKE | {:<32} | {:<28} | {:<8} | ready {:>6.2}s | def {:>6}ms | rss {:>5.0}MB | definition✓ hover✓ symbols✓ didChange✓ shutdown✓({}ms){extra}",
        m.id,
        version,
        if source == "pack" { "pack ✓" } else { "system" },
        spawn_ready.as_secs_f64(),
        first_def.as_millis(),
        rss_mb,
        stop.as_millis()
    ))
}

fn go() -> Sample {
    Sample {
        id: "gopls",
        files: &[
            ("go.mod", "module smoke\n\ngo 1.24\n"),
            ("a.go", "package main\n\n// Helper returns one.\nfunc Helper() int { return 1 }\n"),
            ("b.go", "package main\n\nfunc main() {\n\tx := Helper()\n\t_ = x\n}\n"),
        ],
        def_file: "a.go",
        def_needle: "func Helper",
        use_file: "b.go",
        use_needle: "Helper()",
        edited: "package main\n\nfunc main() {\n\tx := Helper()\n\t_ = x\n}\n\nfunc again() int { return Helper() }\n",
        symbol: "Helper",
        ready_cap: Duration::from_secs(60),
    }
}

fn rust() -> Sample {
    Sample {
        id: "rust-analyzer",
        files: &[
            ("Cargo.toml", "[package]\nname = \"smoke\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n"),
            ("src/util.rs", "/// Returns one.\npub fn helper() -> i32 {\n    1\n}\n"),
            ("src/main.rs", "mod util;\n\nfn main() {\n    let x = util::helper();\n    println!(\"{x}\");\n}\n"),
        ],
        def_file: "src/util.rs",
        def_needle: "pub fn helper",
        use_file: "src/main.rs",
        use_needle: "helper()",
        edited: "mod util;\n\nfn main() {\n    let x = util::helper();\n    println!(\"{x}\");\n}\n\nfn again() -> i32 {\n    util::helper()\n}\n",
        symbol: "helper",
        ready_cap: Duration::from_secs(90),
    }
}

fn typescript() -> Sample {
    Sample {
        id: "typescript-language-server",
        files: &[
            ("package.json", "{ \"name\": \"smoke\", \"private\": true }\n"),
            ("tsconfig.json", "{ \"compilerOptions\": { \"strict\": true, \"module\": \"commonjs\", \"target\": \"es2020\" }, \"include\": [\"*.ts\"] }\n"),
            ("def.ts", "/** Returns one. */\nexport function helper(): number {\n  return 1;\n}\n"),
            ("use.ts", "import { helper } from './def';\n\nconst x = helper();\nconsole.log(x);\n"),
        ],
        def_file: "def.ts",
        def_needle: "export function helper",
        use_file: "use.ts",
        use_needle: "helper()",
        edited: "import { helper } from './def';\n\nconst x = helper();\nconsole.log(x);\n\nconst y = helper();\nconsole.log(y);\n",
        symbol: "helper",
        ready_cap: Duration::from_secs(60),
    }
}

fn python() -> Sample {
    Sample {
        id: "pyright",
        files: &[
            ("pkg/__init__.py", ""),
            ("pkg/defs.py", "def helper() -> int:\n    \"\"\"Returns one.\"\"\"\n    return 1\n"),
            ("main.py", "from pkg.defs import helper\n\nx = helper()\nprint(x)\n"),
        ],
        def_file: "pkg/defs.py",
        def_needle: "def helper",
        use_file: "main.py",
        use_needle: "helper()",
        edited: "from pkg.defs import helper\n\nx = helper()\nprint(x)\n\ny = helper()\nprint(y)\n",
        symbol: "helper",
        ready_cap: Duration::from_secs(60),
    }
}

const POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>smoke</artifactId>
  <version>0.1.0</version>
  <properties>
    <maven.compiler.release>21</maven.compiler.release>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
</project>
"#;

const USE_JAVA: &str = "package com.acme;\n\nimport java.util.ArrayList;\n\npublic class Use {\n    public static void main(String[] args) {\n        int x = Def.helper();\n        ArrayList<String> list = new ArrayList<>();\n        System.out.println(x + list.toString());\n    }\n}\n";
const USE_JAVA_EDITED: &str = "package com.acme;\n\nimport java.util.ArrayList;\n\npublic class Use {\n    public static void main(String[] args) {\n        int x = Def.helper();\n        ArrayList<String> list = new ArrayList<>();\n        System.out.println(x + list.toString());\n        int y = Def.helper();\n        System.out.println(y);\n    }\n}\n";

fn java() -> Sample {
    Sample {
        id: "jdtls",
        files: &[
            ("pom.xml", POM),
            (
                "src/main/java/com/acme/Def.java",
                "package com.acme;\n\npublic class Def {\n    public static int helper() {\n        return 1;\n    }\n}\n",
            ),
            ("src/main/java/com/acme/Use.java", USE_JAVA),
        ],
        def_file: "src/main/java/com/acme/Def.java",
        def_needle: "public static int helper",
        use_file: "src/main/java/com/acme/Use.java",
        use_needle: "helper()",
        edited: USE_JAVA_EDITED,
        symbol: "helper",
        ready_cap: Duration::from_secs(120),
    }
}

fn smoke(s: Sample) {
    if !gated() {
        return;
    }
    match run(&s) {
        Some(row) => eprintln!("{row}"),
        None => eprintln!("SMOKE | {:<32} | cannot run here (program not found / no pack for this target)", s.id),
    }
}

/// Pack mode only: the whole registry installs (grammars too), every runtime
/// resolves, and a runtime refuses to leave while a server needs it.
#[test]
#[ignore]
fn pack_registry_installs_and_guards_runtimes() {
    if !gated() || packs_dir().is_none() {
        eprintln!("skipped: set ORRERY_LSP_SMOKE_PACKS=<dist-ext>");
        return;
    }
    let host = pack_host();
    let view = host.ext.view(true);
    assert!(!view.offline, "registry {} unreachable", view.registry_url);
    let ids: Vec<&str> = view.items.iter().map(|p| p.id.as_str()).collect();
    eprintln!("registry: {ids:?}");
    for want in ["runtime.node", "server.typescript-language-server", "server.pyright", "server.jdtls", "server.gopls", "server.rust-analyzer"] {
        assert!(ids.contains(&want), "{want} missing from the registry");
    }
    for p in &view.items {
        if p.kind == "server" && p.available_for_target {
            host.ext.run_install(&p.id, &mut |_| {}).unwrap_or_else(|e| panic!("{}: {e}", p.id));
        }
    }
    let after = host.ext.snapshot();
    for p in &after.items {
        if p.installed {
            eprintln!("  {:<36} {:<8} {:>12} bytes  requires {:?}  detection {:?}", p.id, p.kind, p.bundled_size_bytes, p.requires, p.detection.as_ref().map(|d| d.status.clone()));
        }
    }
    assert!(host.ext.runtime_binary("node").is_some_and(|p| p.is_file()));
    let dependents_of_node: Vec<&str> = after
        .items
        .iter()
        .filter(|p| p.installed && p.requires.iter().any(|r| r == "runtime.node"))
        .map(|p| p.id.as_str())
        .collect();
    assert!(!dependents_of_node.is_empty());
    let err = host.ext.uninstall("runtime.node").unwrap_err().to_string();
    assert!(err.contains("required by"), "{err}");
    for p in &after.items {
        if p.kind == "server" && p.installed {
            assert_eq!(p.detection.as_ref().map(|d| d.status.as_str()), Some("bundled"), "{}", p.id);
        }
    }
    eprintln!("SMOKE | registry | {} packs installed, runtime.node guarded by {dependents_of_node:?}", after.items.iter().filter(|p| p.installed).count());
}

#[test]
#[ignore]
fn gopls() {
    smoke(go());
}

#[test]
#[ignore]
fn rust_analyzer() {
    smoke(rust());
}

#[test]
#[ignore]
fn typescript_language_server() {
    smoke(typescript());
}

#[test]
#[ignore]
fn pyright() {
    smoke(python());
}

#[test]
#[ignore]
fn jdtls() {
    smoke(java());
}

#[test]
fn sample_positions_are_sane() {
    // the needles resolve on every sample even without a server
    for s in [go(), rust(), typescript(), python(), java()] {
        let def = s.files.iter().find(|(f, _)| *f == s.def_file).unwrap().1;
        let use_ = s.files.iter().find(|(f, _)| *f == s.use_file).unwrap().1;
        let _ = pos_of(def, s.def_needle, false);
        let first = pos_of(use_, s.use_needle, false);
        let last = pos_of(s.edited, s.use_needle, true);
        assert!(last.0 > first.0, "{}: the edit adds a later reference", s.id);
        assert!(language_of(s.use_file).is_some(), "{}", s.id);
        assert!(scripts_dir().join(format!("{}.json", s.id)).is_file(), "{}", s.id);
    }
    assert_eq!(pos_of("ab\ncd ef", "ef", false), (1, 3));
    assert_eq!(pos_of("x()\ny x()", "x()", true), (1, 2));
    assert!(has_symbol(&json!([{ "name": "A", "children": [{ "name": "helper() : int" }] }]), "helper"));
    assert!(!has_symbol(&json!([{ "name": "A" }]), "helper"));
}
