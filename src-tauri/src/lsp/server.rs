//! One language-server process for one `(extension, project)` pair: spawn
//! (through `core::proc::cmd`, the only sanctioned spawn path), the
//! `initialize` handshake, document sync, graceful shutdown, crash
//! bookkeeping with a restart backoff. Every state change reports through
//! `on_change` so the service can push an `lsp://status` snapshot; the
//! callback always runs with the handle's lock RELEASED.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::extensions::detect::{find_java, Bundled, DetectEnv};
use crate::extensions::manifest::ServerManifest;

use super::client::{Client, LspErr, RpcError};
use super::uri::path_to_uri;

/// `shutdown` request budget, then `exit` + this long before a kill.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);
/// `initialize` budget (jdtls on a cold workspace is slow).
const INIT_TIMEOUT: Duration = Duration::from_secs(60);
/// Crashes inside this window count toward the backoff table.
pub const CRASH_WINDOW: Duration = Duration::from_secs(600);

/// Restart delay after the `n`th crash inside [`CRASH_WINDOW`] (1-based);
/// `None` = give up (`stopped` until a manual restart).
pub fn backoff_for(crashes_in_window: usize) -> Option<Duration> {
    match crashes_in_window {
        0 | 1 => Some(Duration::ZERO),
        2 => Some(Duration::from_secs(5)),
        3 => Some(Duration::from_secs(30)),
        _ => None,
    }
}

/// `${extDir}`, `${workspaceStorage}`, `${projectRoot}`, `${jdtlsConfig}` in
/// manifest args / jvmArgs / initializationOptions / settings.
pub struct Placeholders<'a> {
    pub ext_dir: &'a str,
    pub workspace_storage: &'a str,
    pub project_root: &'a str,
    /// jdtls' equinox config dir for this platform (`config_win`, …).
    pub jdtls_config: &'a str,
}

impl<'a> Placeholders<'a> {
    /// The three path placeholders, `${jdtlsConfig}` for THIS platform.
    pub fn new(ext_dir: &'a str, workspace_storage: &'a str, project_root: &'a str) -> Self {
        Self {
            ext_dir,
            workspace_storage,
            project_root,
            jdtls_config: jdtls_config_dir(std::env::consts::OS, std::env::consts::ARCH),
        }
    }
}

pub fn substitute(arg: &str, p: &Placeholders<'_>) -> String {
    arg.replace("${extDir}", p.ext_dir)
        .replace("${workspaceStorage}", p.workspace_storage)
        .replace("${projectRoot}", p.project_root)
        .replace("${jdtlsConfig}", p.jdtls_config)
}

/// [`substitute`] over every string inside a JSON document (the manifest's
/// `initializationOptions` / `settings` — `tsserver.path` lives there).
pub fn substitute_value(v: &Value, p: &Placeholders<'_>) -> Value {
    match v {
        Value::String(s) => Value::String(substitute(s, p)),
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute_value(x, p)).collect()),
        Value::Object(o) => Value::Object(
            o.iter().map(|(k, x)| (k.clone(), substitute_value(x, p))).collect(),
        ),
        other => other.clone(),
    }
}

/// The manifest with every placeholder resolved for one `(pack, project)`:
/// what a [`ServerHandle`] is built from, so the `initialize` handshake and
/// `workspace/configuration` answers carry real paths.
pub fn substitute_manifest(m: &ServerManifest, p: &Placeholders<'_>) -> ServerManifest {
    let mut out = m.clone();
    out.args = m.args.iter().map(|a| substitute(a, p)).collect();
    if let Some(l) = out.launch.as_mut() {
        l.args = l.args.iter().map(|a| substitute(a, p)).collect();
        l.jvm_args = l.jvm_args.iter().map(|a| substitute(a, p)).collect();
    }
    out.initialization_options = m.initialization_options.as_ref().map(|v| substitute_value(v, p));
    out.settings = m.settings.as_ref().map(|v| substitute_value(v, p));
    out
}

/// Env vars a user's shell can leak into a bundled runtime: unset so the pack
/// runs the same everywhere (`NODE_OPTIONS=--inspect`, a stray `NODE_PATH`
/// pointing at a global typescript, `JAVA_TOOL_OPTIONS` agents…).
fn scrubbed_env_for(runtime: Option<&str>) -> Vec<String> {
    match runtime {
        Some("node") => ["NODE_OPTIONS", "NODE_PATH", "NODE_REPL_EXTERNAL_MODULE", "NODE_EXTRA_CA_CERTS"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        Some("java") => ["JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS", "CLASSPATH"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// The launch of a self-contained pack (see [`Bundled`]):
/// `java` → `[java, jvmArgs…, -jar, entry, args…]`, `node` → `[node, entry,
/// args…]`, no runtime → `[entry, args…]`. Placeholders substituted, cwd =
/// the project root, the runtime's env scrubbed.
pub fn launch_bundled(b: &Bundled, ph: &Placeholders<'_>, cwd: PathBuf) -> Launch {
    let s = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let tail: Vec<String> = b.launch.args.iter().map(|a| substitute(a, ph)).collect();
    let mut args: Vec<String> = Vec::new();
    match b.launch.runtime.as_deref() {
        Some("java") => {
            args.extend(b.launch.jvm_args.iter().map(|a| substitute(a, ph)));
            args.push("-jar".into());
            args.push(s(&b.entry));
        }
        Some(_) => args.push(s(&b.entry)),
        None => {}
    }
    args.extend(tail);
    Launch {
        program: b.program.clone(),
        args,
        cwd,
        env_remove: scrubbed_env_for(b.launch.runtime.as_deref()),
    }
}

/// The launch for a SYSTEM `program` (manual path / PATH fallback): manifest
/// args substituted, cwd = the project root. A jdtls launcher script
/// (`bin/jdtls`, `.bat`, `.py`) is replaced by a direct JVM launch — see
/// [`jdtls_direct_launch`].
pub fn launch_for(
    manifest: &ServerManifest,
    program: &Path,
    ph: &Placeholders<'_>,
    cwd: PathBuf,
    env: &DetectEnv,
) -> Launch {
    // a self-contained (v2) pack keeps its arguments under `launch` and may
    // omit the top-level `args` — a system copy of that server needs the
    // same `--stdio` (typescript-language-server and pyright refuse without)
    let source: &[String] = if manifest.args.is_empty() {
        manifest.launch.as_ref().map(|l| l.args.as_slice()).unwrap_or(&[])
    } else {
        manifest.args.as_slice()
    };
    let args: Vec<String> = source.iter().map(|a| substitute(a, ph)).collect();
    if is_jdtls_script(program) {
        match find_java(env) {
            Some(java) => {
                if let Some(l) = jdtls_direct_launch(program, &java, &args, cwd.clone()) {
                    return l;
                }
            }
            None => log::warn!(
                "lsp: no java runtime found (JAVA_HOME / PATH); falling back to {}",
                program.display()
            ),
        }
    }
    Launch {
        program: program.to_path_buf(),
        args,
        cwd,
        env_remove: Vec::new(),
    }
}

fn is_jdtls_script(program: &Path) -> bool {
    matches!(
        program.file_name().and_then(|n| n.to_str()).map(str::to_ascii_lowercase).as_deref(),
        Some("jdtls" | "jdtls.bat" | "jdtls.py")
    )
}

/// Equinox `config_*` dir jdtls ships for this platform (`${jdtlsConfig}`).
pub fn jdtls_config_dir(os: &str, arch: &str) -> &'static str {
    match (os, arch) {
        ("windows", _) => "config_win",
        ("macos", "aarch64") => "config_mac_arm",
        ("macos", _) => "config_mac",
        (_, "aarch64") => "config_linux_arm",
        _ => "config_linux",
    }
}

/// jdtls ships `bin/jdtls` (a python script) and, on Windows, a `jdtls.bat`
/// that runs `python … %*` then `pause`s. Launching that means: a python
/// dependency the pack never declared, a cmd.exe → python → java chain whose
/// pid (metrics, kill) is the wrapper — a kill orphans the JVM — and a
/// wrapper that never exits after `exit` because `pause` waits on stdin.
/// So the host runs the JVM itself, the way the jdtls README does:
/// `java <flags> -jar plugins/org.eclipse.equinox.launcher_*.jar
///  -configuration <dist>/config_<os> <manifest args (-data …)>`.
/// `None` when `script` is not inside a jdtls distribution.
pub fn jdtls_direct_launch(script: &Path, java: &Path, args: &[String], cwd: PathBuf) -> Option<Launch> {
    let dist = script.parent()?.parent()?;
    let plugins = dist.join("plugins");
    let mut launchers: Vec<PathBuf> = std::fs::read_dir(&plugins)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("org.eclipse.equinox.launcher_") && n.ends_with(".jar"))
        })
        .collect();
    launchers.sort();
    let launcher = launchers.pop()?;
    let config = dist.join(jdtls_config_dir(std::env::consts::OS, std::env::consts::ARCH));
    if !config.is_dir() {
        return None;
    }
    let s = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let mut out: Vec<String> = [
        "-Declipse.application=org.eclipse.jdt.ls.core.id1",
        "-Dosgi.bundles.defaultStartLevel=4",
        "-Declipse.product=org.eclipse.jdt.ls.core.product",
        "-Dosgi.checkConfiguration=true",
        "-Djdk.xml.maxGeneralEntitySizeLimit=0",
        "-Djdk.xml.totalEntitySizeLimit=0",
        "-Xms1G",
        "--add-modules=ALL-SYSTEM",
        "--add-opens",
        "java.base/java.util=ALL-UNNAMED",
        "--add-opens",
        "java.base/java.lang=ALL-UNNAMED",
        "-jar",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    out.push(s(&launcher));
    out.push("-configuration".into());
    out.push(s(&config));
    out.extend(args.iter().cloned());
    Some(Launch {
        program: java.to_path_buf(),
        args: out,
        cwd,
        env_remove: Vec::new(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Starting,
    Ready,
    Idle,
    Crashed,
    Stopped,
    Missing,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Starting => "starting",
            State::Ready => "ready",
            State::Idle => "idle",
            State::Crashed => "crashed",
            State::Stopped => "stopped",
            State::Missing => "missing",
        }
    }

    /// Accepting requests (or one `touch` away from it).
    pub fn is_live(self) -> bool {
        matches!(self, State::Ready | State::Idle)
    }
}

/// Idle-reaper verdict for one server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reap {
    Keep,
    /// Past 50 % of the idle window: flip `ready` → `idle`.
    MarkIdle,
    /// Past the whole window: shut it down.
    Stop,
}

/// Pure state machine of the reaper: `idle_for` since the last request,
/// `window` = `settings.lsp_idle_minutes`. A zero window disables reaping.
pub fn reap_decision(state: State, idle_for: Duration, window: Duration) -> Reap {
    if window.is_zero() {
        return Reap::Keep;
    }
    match state {
        State::Ready | State::Idle if idle_for >= window => Reap::Stop,
        State::Ready if idle_for >= window / 2 => Reap::MarkIdle,
        _ => Reap::Keep,
    }
}

/// The project a server belongs to (its main checkout is the LSP root).
#[derive(Debug, Clone)]
pub struct ProjectRef {
    pub id: Uuid,
    pub name: String,
    pub root: PathBuf,
}

/// How to spawn the server (already resolved + substituted).
#[derive(Debug, Clone)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Env vars removed from the child (bundled runtimes: the user's
    /// `NODE_OPTIONS` / `NODE_PATH` / `JAVA_TOOL_OPTIONS` must not leak in).
    pub env_remove: Vec<String>,
}

/// The `lsp://status` row.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LspServer {
    /// `"<extId>:<projectId>"`.
    pub id: String,
    pub ext_id: String,
    pub label: String,
    pub language: String,
    pub root: String,
    pub project_id: Uuid,
    pub project_name: String,
    pub pid: Option<u32>,
    pub state: String,
    pub mem_bytes: u64,
    pub cpu: f32,
    pub restarts: u32,
    /// ms epoch.
    pub started_at: Option<i64>,
    pub last_error: Option<String>,
}

/// An editor document the server should know about. `sent` = the server
/// has had its `didOpen`; a doc opened while the server is still `starting`
/// (or re-queued by a restart) waits for [`ServerHandle::flush_docs`].
struct Doc {
    version: i64,
    text: String,
    language_id: String,
    sent: bool,
}

struct Inner {
    state: State,
    client: Option<Arc<Client>>,
    child: Option<Child>,
    pid: Option<u32>,
    started_at: Option<i64>,
    restarts: u32,
    last_error: Option<String>,
    /// uri → open document.
    docs: HashMap<String, Doc>,
    /// Crash instants (pruned to [`CRASH_WINDOW`]).
    crashes: Vec<Instant>,
    /// Earliest instant a restart is allowed after the last crash.
    retry_at: Option<Instant>,
    last_used: Instant,
    /// Bumped per spawn so a stale reader's close callback is ignored.
    generation: u64,
    /// Set for the duration of a deliberate shutdown.
    stopping: bool,
}

pub struct ServerHandle {
    pub ext_id: String,
    pub label: String,
    pub language: String,
    pub manifest: ServerManifest,
    pub project: ProjectRef,
    inner: Mutex<Inner>,
    on_change: Box<dyn Fn() + Send + Sync>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// What `ensure_started` may do with a handle in its current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Begin {
    /// Flipped to `starting`; the caller owns the start.
    Start,
    /// Already starting / live — nothing to do.
    Busy,
    /// Backoff not elapsed yet (or exhausted / manual stop): wait for the
    /// next request or a manual restart.
    Wait,
}

/// `workspace/configuration` answer for one `section` of the manifest
/// `settings`: dotted sections (`python.analysis`) walk the tree; an unknown
/// section gets `null` (what the server reads from an absent setting).
fn config_section(settings: &Value, section: &str) -> Value {
    let mut cur = settings;
    for seg in section.split('.') {
        match cur.get(seg) {
            Some(v) => cur = v,
            None => return Value::Null,
        }
    }
    cur.clone()
}

impl ServerHandle {
    /// A fresh handle is `starting`: the creator spawns its start thread.
    pub fn new(
        ext_id: String,
        manifest: ServerManifest,
        project: ProjectRef,
        on_change: Box<dyn Fn() + Send + Sync>,
    ) -> Arc<Self> {
        let label = if manifest.label.trim().is_empty() {
            ext_id.clone()
        } else {
            manifest.label.clone()
        };
        let language = manifest.languages.first().cloned().unwrap_or_default();
        Arc::new(Self {
            ext_id,
            label,
            language,
            manifest,
            project,
            inner: Mutex::new(Inner {
                state: State::Starting,
                client: None,
                child: None,
                pid: None,
                started_at: None,
                restarts: 0,
                last_error: None,
                docs: HashMap::new(),
                crashes: Vec::new(),
                retry_at: None,
                last_used: Instant::now(),
                generation: 0,
                stopping: false,
            }),
            on_change,
        })
    }

    /// Test-only: a handle already `ready` on an in-memory client.
    #[cfg(test)]
    pub fn with_client(
        ext_id: &str,
        manifest: ServerManifest,
        project: ProjectRef,
        client: Arc<Client>,
    ) -> Arc<Self> {
        let h = Self::new(ext_id.to_string(), manifest, project, Box::new(|| {}));
        {
            let mut g = h.inner.lock().unwrap();
            g.state = State::Ready;
            g.client = Some(client);
            g.started_at = Some(now_ms());
        }
        h
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.ext_id, self.project.id)
    }

    pub fn state(&self) -> State {
        self.inner.lock().unwrap().state
    }

    pub fn pid(&self) -> Option<u32> {
        self.inner.lock().unwrap().pid
    }

    pub fn idle_for(&self) -> Duration {
        self.inner.lock().unwrap().last_used.elapsed()
    }

    pub fn snapshot(&self) -> LspServer {
        let g = self.inner.lock().unwrap();
        LspServer {
            id: self.key(),
            ext_id: self.ext_id.clone(),
            label: self.label.clone(),
            language: self.language.clone(),
            root: self.project.root.to_string_lossy().into_owned(),
            project_id: self.project.id,
            project_name: self.project.name.clone(),
            pid: g.pid,
            state: g.state.as_str().into(),
            mem_bytes: 0,
            cpu: 0.0,
            restarts: g.restarts,
            started_at: g.started_at,
            last_error: g.last_error.clone(),
        }
    }

    /// A request is about to go out: bump `last_used`, wake an idle server.
    pub fn touch(&self) {
        let changed = {
            let mut g = self.inner.lock().unwrap();
            g.last_used = Instant::now();
            if g.state == State::Idle {
                g.state = State::Ready;
                true
            } else {
                false
            }
        };
        if changed {
            (self.on_change)();
        }
    }

    /// The reaper flips `ready` → `idle`.
    pub fn mark_idle(&self) {
        let changed = {
            let mut g = self.inner.lock().unwrap();
            if g.state == State::Ready {
                g.state = State::Idle;
                true
            } else {
                false
            }
        };
        if changed {
            (self.on_change)();
        }
    }

    /// Claim the right to (re)start. `force` = manual restart: clears the
    /// crash history and leaves `stopped`/`missing` too.
    pub fn begin_start(&self, force: bool) -> Begin {
        let out = {
            let mut g = self.inner.lock().unwrap();
            match g.state {
                State::Starting | State::Ready | State::Idle => Begin::Busy,
                State::Crashed if force || g.retry_at.is_none_or(|t| Instant::now() >= t) => {
                    g.state = State::Starting;
                    g.restarts += 1;
                    if force {
                        g.crashes.clear();
                    }
                    g.retry_at = None;
                    g.last_error = None;
                    Begin::Start
                }
                State::Crashed => Begin::Wait,
                State::Stopped | State::Missing if force => {
                    g.state = State::Starting;
                    g.restarts += 1;
                    g.crashes.clear();
                    g.retry_at = None;
                    g.last_error = None;
                    Begin::Start
                }
                State::Stopped | State::Missing => Begin::Wait,
            }
        };
        if out == Begin::Start {
            (self.on_change)();
        }
        out
    }

    /// The program could not be resolved: park as `missing` with the hint.
    pub fn set_missing(&self, hint: Option<String>) {
        {
            let mut g = self.inner.lock().unwrap();
            g.state = State::Missing;
            g.last_error = hint.or_else(|| Some("language server not found".into()));
            g.pid = None;
        }
        (self.on_change)();
    }

    fn fail_start(&self, err: String) {
        log::warn!("lsp {}: start failed: {err}", self.ext_id);
        {
            let mut g = self.inner.lock().unwrap();
            g.state = State::Crashed;
            g.last_error = Some(err);
            g.pid = None;
            g.client = None;
            if let Some(mut c) = g.child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
            Self::note_crash(&mut g);
        }
        (self.on_change)();
    }

    /// Record a crash instant; past the table → `stopped` with the reason.
    fn note_crash(g: &mut Inner) {
        let now = Instant::now();
        g.crashes.retain(|t| now.duration_since(*t) < CRASH_WINDOW);
        g.crashes.push(now);
        match backoff_for(g.crashes.len()) {
            Some(d) => g.retry_at = Some(now + d),
            None => {
                g.state = State::Stopped;
                g.retry_at = None;
                let why = format!(
                    "crashed {} times in {} min; restart manually",
                    g.crashes.len(),
                    CRASH_WINDOW.as_secs() / 60
                );
                g.last_error = Some(match g.last_error.take() {
                    Some(e) if !e.is_empty() => format!("{why}\n{e}"),
                    _ => why,
                });
            }
        }
    }

    /// Spawn + handshake. Blocking (call from a worker thread); the handle
    /// must be `starting` (see [`Self::begin_start`]).
    pub fn start(self: &Arc<Self>, launch: &Launch) {
        let generation = {
            let mut g = self.inner.lock().unwrap();
            g.generation += 1;
            g.stopping = false;
            // the editor's buffers survive a (re)start: they go out again
            // after the handshake
            Self::requeue_docs(&mut g);
            g.generation
        };
        let mut cmd = crate::core::proc::cmd(&launch.program);
        cmd.args(&launch.args)
            .current_dir(&launch.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for var in &launch.env_remove {
            cmd.env_remove(var);
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.fail_start(format!("spawn {}: {e}", launch.program.display()));
                return;
            }
        };
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            self.fail_start("spawn: stdio not piped".into());
            return;
        };
        let stderr = child.stderr.take().map(|e| Box::new(e) as Box<dyn std::io::Read + Send>);
        let pid = child.id();
        let (ready_tx, ready_rx) = mpsc::channel::<()>();
        let weak: Weak<ServerHandle> = Arc::downgrade(self);
        let close_weak = weak.clone();
        let client = Client::start(
            Box::new(stdout),
            Box::new(stdin),
            stderr,
            Self::request_handler(weak.clone()),
            Self::notify_handler(weak, ready_tx),
            Box::new(move || {
                if let Some(h) = close_weak.upgrade() {
                    h.on_closed(generation);
                }
            }),
        );
        {
            let mut g = self.inner.lock().unwrap();
            g.child = Some(child);
            g.pid = Some(pid);
            g.client = Some(client.clone());
            g.started_at = Some(now_ms());
        }
        (self.on_change)();
        if let Err(e) = self.handshake(&client, ready_rx) {
            let tail = client.last_error();
            let msg = if tail.is_empty() {
                format!("initialize: {e}")
            } else {
                format!("initialize: {e}\n{tail}")
            };
            log::warn!("lsp {}: {msg}", self.ext_id);
            self.fail_start(msg);
            return;
        }
        {
            let mut g = self.inner.lock().unwrap();
            if g.state == State::Starting && g.generation == generation {
                g.state = State::Ready;
                g.last_used = Instant::now();
            }
        }
        self.flush_docs();
        (self.on_change)();
    }

    fn handshake(&self, client: &Client, ready_rx: Receiver<()>) -> Result<(), LspErr> {
        let root_uri = path_to_uri(&self.project.root);
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "orrery" },
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": self.project.name }],
            "initializationOptions": self.manifest.initialization_options.clone().unwrap_or(Value::Null),
            "capabilities": {
                "textDocument": {
                    "definition": { "linkSupport": true },
                    "references": {},
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                    "synchronization": { "didSave": false, "willSave": false }
                },
                "workspace": {
                    "configuration": true,
                    "workspaceFolders": true
                },
                "window": { "workDoneProgress": true },
                "general": { "positionEncodings": ["utf-16"] }
            }
        });
        client.request("initialize", params, INIT_TIMEOUT)?;
        client.notify("initialized", json!({}))?;
        if let Some(settings) = &self.manifest.settings {
            if settings.is_object() && settings.as_object().is_some_and(|o| !o.is_empty()) {
                let _ = client.notify(
                    "workspace/didChangeConfiguration",
                    json!({ "settings": settings }),
                );
            }
        }
        if self.ext_id == "server.jdtls" {
            // jdtls answers navigation with nothing until its `ServiceReady`
            // status (or the import progress end) — wait, capped.
            let _ = ready_rx.recv_timeout(INIT_TIMEOUT);
        }
        Ok(())
    }

    fn request_handler(weak: Weak<ServerHandle>) -> super::client::RequestHandler {
        Box::new(move |method, params| match method {
            "client/registerCapability" | "client/unregisterCapability" => Ok(Value::Null),
            "window/workDoneProgress/create" => Ok(Value::Null),
            "workspace/configuration" => {
                let settings = weak
                    .upgrade()
                    .and_then(|h| h.manifest.settings.clone())
                    .unwrap_or(Value::Null);
                let n = params
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|a| a.len())
                    .unwrap_or(0);
                let items = params.get("items").and_then(Value::as_array);
                let out: Vec<Value> = (0..n)
                    .map(|i| {
                        let section = items
                            .and_then(|a| a.get(i))
                            .and_then(|it| it.get("section"))
                            .and_then(Value::as_str);
                        match section {
                            Some(s) if !s.is_empty() => config_section(&settings, s),
                            _ => settings.clone(),
                        }
                    })
                    .collect();
                Ok(Value::Array(out))
            }
            "workspace/workspaceFolders" => Ok(weak
                .upgrade()
                .map(|h| {
                    json!([{ "uri": path_to_uri(&h.project.root), "name": h.project.name }])
                })
                .unwrap_or(Value::Null)),
            other => Err(RpcError::method_not_found(other)),
        })
    }

    fn notify_handler(weak: Weak<ServerHandle>, ready_tx: Sender<()>) -> super::client::NotifyHandler {
        Box::new(move |method, params| match method {
            "language/status" => {
                // jdtls: { type: "Started" | "ServiceReady" | "Error", message }
                let ty = params.get("type").and_then(Value::as_str).unwrap_or("");
                if ty == "ServiceReady" {
                    let _ = ready_tx.send(());
                } else if ty == "Error" {
                    if let Some(h) = weak.upgrade() {
                        let msg = params.get("message").and_then(Value::as_str).unwrap_or("").to_string();
                        h.inner.lock().unwrap().last_error = Some(msg);
                    }
                }
            }
            "$/progress" => {
                let kind = params
                    .get("value")
                    .and_then(|v| v.get("kind"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if kind == "end" {
                    let _ = ready_tx.send(());
                }
            }
            "window/showMessage" | "window/logMessage" => {
                // type 1 = Error
                let is_error = params.get("type").and_then(Value::as_i64) == Some(1);
                if let Some(h) = weak.upgrade().filter(|_| is_error) {
                    let msg = params.get("message").and_then(Value::as_str).unwrap_or("").to_string();
                    log::warn!("lsp {}: {msg}", h.ext_id);
                    if method == "window/showMessage" {
                        h.inner.lock().unwrap().last_error = Some(msg);
                    }
                }
            }
            _ => {} // diagnostics etc. — ignored
        })
    }

    /// The reader hit EOF: the server died (unless we are stopping it).
    fn on_closed(&self, generation: u64) {
        let changed = {
            let mut g = self.inner.lock().unwrap();
            if g.generation != generation || g.stopping {
                false
            } else {
                let tail = g.client.as_ref().map(|c| c.last_error()).unwrap_or_default();
                g.client = None;
                g.pid = None;
                Self::requeue_docs(&mut g);
                if let Some(mut c) = g.child.take() {
                    if c.try_wait().ok().flatten().is_none() {
                        let _ = c.kill();
                    }
                    let _ = c.wait();
                }
                g.state = State::Crashed;
                g.last_error = Some(if tail.is_empty() {
                    "server exited".into()
                } else {
                    tail
                });
                Self::note_crash(&mut g);
                true
            }
        };
        if changed {
            log::warn!("lsp {}: exited unexpectedly", self.ext_id);
            (self.on_change)();
        }
    }

    /// Poll the child (reaper / metrics tick): an exit without a reader EOF
    /// yet is still a crash.
    pub fn check_alive(&self) {
        let (closed, generation) = {
            let mut g = self.inner.lock().unwrap();
            if !g.state.is_live() {
                return;
            }
            let exited = g
                .child
                .as_mut()
                .and_then(|c| c.try_wait().ok().flatten())
                .is_some();
            (exited, g.generation)
        };
        if closed {
            self.on_closed(generation);
        }
    }

    /// Graceful stop → `stopped` (manual; no auto-restart).
    pub fn stop(&self) {
        let (client, child) = {
            let mut g = self.inner.lock().unwrap();
            g.stopping = true;
            g.generation += 1; // the reader's close callback becomes stale
            (g.client.take(), g.child.take())
        };
        if let Some(client) = &client {
            let _ = client.request("shutdown", Value::Null, SHUTDOWN_WAIT);
            let _ = client.notify("exit", Value::Null);
        }
        if let Some(mut child) = child {
            let deadline = Instant::now() + SHUTDOWN_WAIT;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
        {
            let mut g = self.inner.lock().unwrap();
            g.state = State::Stopped;
            g.pid = None;
            g.docs.clear();
            g.stopping = false;
        }
        (self.on_change)();
    }

    /// The client while the server is up (`ready` / `idle`).
    fn live_client(g: &Inner) -> Option<Arc<Client>> {
        match (&g.client, g.state) {
            (Some(c), State::Ready | State::Idle) => Some(c.clone()),
            _ => None,
        }
    }

    fn client(&self) -> Result<Arc<Client>, LspErr> {
        Self::live_client(&self.inner.lock().unwrap()).ok_or(LspErr::Closed)
    }

    /// A request with a budget; bumps `last_used`.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, LspErr> {
        self.touch();
        self.client()?.request(method, params, timeout)
    }

    // ---- document sync ----

    /// `didOpen` unless already open. `text` = the live buffer, else disk.
    pub fn ensure_open(&self, abs: &Path, text: Option<&str>, language_id: &str) -> Result<(), LspErr> {
        let uri = path_to_uri(abs);
        if self.inner.lock().unwrap().docs.contains_key(&uri) {
            return Ok(());
        }
        let text = match text {
            Some(t) => t.to_string(),
            None => std::fs::read_to_string(abs).unwrap_or_default(),
        };
        self.open(uri, text, language_id, 1)
    }

    /// Explicit open from the editor (replaces a disk-sourced open). While
    /// the server is still `starting` the document is queued and goes out
    /// with [`Self::flush_docs`] after the handshake; a uri the server already
    /// has becomes a full-text `didChange` rather than a second `didOpen`.
    /// Editor traffic counts as use: an edited buffer keeps the server alive.
    pub fn open(&self, uri: String, text: String, language_id: &str, version: i64) -> Result<(), LspErr> {
        self.touch();
        let (client, was_sent, version) = {
            let mut g = self.inner.lock().unwrap();
            let (was_sent, version) = match g.docs.get(&uri) {
                Some(d) if d.sent => (true, version.max(d.version + 1)),
                _ => (false, version),
            };
            g.docs.insert(
                uri.clone(),
                Doc { version, text: text.clone(), language_id: language_id.to_string(), sent: false },
            );
            (Self::live_client(&g), was_sent, version)
        };
        let Some(client) = client else {
            return Ok(()); // queued: the flush after the handshake sends it
        };
        let r = if was_sent {
            client.notify(
                "textDocument/didChange",
                json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }] }),
            )
        } else {
            client.notify(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": uri, "languageId": language_id, "version": version, "text": text } }),
            )
        };
        if r.is_ok() {
            self.mark_sent(&uri);
        }
        r
    }

    /// Full-text `didChange` (opens the doc first when it is unknown). A
    /// queued doc just takes the new text — the flush carries the latest.
    pub fn change(&self, abs: &Path, text: String, version: i64, language_id: &str) -> Result<(), LspErr> {
        let uri = path_to_uri(abs);
        let known = {
            let mut g = self.inner.lock().unwrap();
            match g.docs.get_mut(&uri) {
                Some(d) => {
                    d.version = version.max(d.version + 1);
                    d.text = text.clone();
                    Some((d.version, d.sent))
                }
                None => None,
            }
        };
        match known {
            Some((v, true)) => {
                self.touch();
                self.client()?.notify(
                    "textDocument/didChange",
                    json!({ "textDocument": { "uri": uri, "version": v }, "contentChanges": [{ "text": text }] }),
                )
            }
            Some((_, false)) => {
                self.touch();
                Ok(())
            }
            None => self.open(uri, text, language_id, version),
        }
    }

    pub fn close(&self, abs: &Path) -> Result<(), LspErr> {
        let uri = path_to_uri(abs);
        let sent = match self.inner.lock().unwrap().docs.remove(&uri) {
            Some(d) => d.sent,
            None => return Ok(()),
        };
        if !sent {
            return Ok(()); // the server never heard of it
        }
        self.client()?
            .notify("textDocument/didClose", json!({ "textDocument": { "uri": uri } }))
    }

    /// `didOpen` for every queued document — after the handshake, and after
    /// a restart re-queued the buffers the editor still has open.
    pub(crate) fn flush_docs(&self) {
        let (client, pending) = {
            let g = self.inner.lock().unwrap();
            let pending: Vec<(String, String, String, i64)> = g
                .docs
                .iter()
                .filter(|(_, d)| !d.sent)
                .map(|(u, d)| (u.clone(), d.text.clone(), d.language_id.clone(), d.version))
                .collect();
            (Self::live_client(&g), pending)
        };
        let Some(client) = client else {
            return;
        };
        for (uri, text, language_id, version) in pending {
            let r = client.notify(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": uri, "languageId": language_id, "version": version, "text": text } }),
            );
            if r.is_ok() {
                self.mark_sent(&uri);
            }
        }
    }

    fn mark_sent(&self, uri: &str) {
        if let Some(d) = self.inner.lock().unwrap().docs.get_mut(uri) {
            d.sent = true;
        }
    }

    /// Every known doc goes out again on the next handshake.
    fn requeue_docs(g: &mut Inner) {
        for d in g.docs.values_mut() {
            d.sent = false;
        }
    }

    #[cfg(test)]
    pub(crate) fn queued_docs(&self) -> Vec<String> {
        let g = self.inner.lock().unwrap();
        let mut v: Vec<String> = g.docs.iter().filter(|(_, d)| !d.sent).map(|(u, _)| u.clone()).collect();
        v.sort();
        v
    }

    #[cfg(test)]
    pub(crate) fn open_docs(&self) -> Vec<String> {
        self.inner.lock().unwrap().docs.keys().cloned().collect()
    }

    #[cfg(test)]
    pub(crate) fn force_state(&self, state: State) {
        self.inner.lock().unwrap().state = state;
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if let Ok(mut g) = self.inner.lock() {
            if let Some(mut c) = g.child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_table() {
        assert_eq!(backoff_for(1), Some(Duration::ZERO));
        assert_eq!(backoff_for(2), Some(Duration::from_secs(5)));
        assert_eq!(backoff_for(3), Some(Duration::from_secs(30)));
        assert_eq!(backoff_for(4), None);
        assert_eq!(backoff_for(9), None);
    }

    #[test]
    fn placeholder_substitution() {
        let p = Placeholders {
            ext_dir: "C:/app/extensions/server.jdtls/1.40.0-1",
            workspace_storage: "C:/app/lsp-workspaces/0123456789abcdef",
            project_root: "C:/w/proj",
            jdtls_config: "config_win",
        };
        assert_eq!(
            substitute("${workspaceStorage}/jdtls", &p),
            "C:/app/lsp-workspaces/0123456789abcdef/jdtls"
        );
        assert_eq!(substitute("--root=${projectRoot}", &p), "--root=C:/w/proj");
        assert_eq!(
            substitute("${extDir}/bin/x ${extDir}", &p),
            "C:/app/extensions/server.jdtls/1.40.0-1/bin/x C:/app/extensions/server.jdtls/1.40.0-1"
        );
        assert_eq!(
            substitute("${extDir}/${jdtlsConfig}", &p),
            "C:/app/extensions/server.jdtls/1.40.0-1/config_win"
        );
        assert_eq!(substitute("--stdio", &p), "--stdio");
        assert_eq!(substitute("${unknown}", &p), "${unknown}");
        // ::new picks this platform's jdtls config dir
        let n = Placeholders::new("E", "W", "P");
        assert_eq!(n.jdtls_config, jdtls_config_dir(std::env::consts::OS, std::env::consts::ARCH));
        // deep substitution over json
        let v = json!({ "tsserver": { "path": "${extDir}/server/tsserver.js", "n": 1, "list": ["${projectRoot}", true] } });
        let out = substitute_value(&v, &p);
        assert_eq!(out["tsserver"]["path"], "C:/app/extensions/server.jdtls/1.40.0-1/server/tsserver.js");
        assert_eq!(out["tsserver"]["n"], 1);
        assert_eq!(out["tsserver"]["list"][0], "C:/w/proj");
        assert_eq!(out["tsserver"]["list"][1], true);
        // whole manifest: args, launch args/jvmArgs, initializationOptions, settings
        let m = bundled_ts();
        let s = substitute_manifest(&m, &p);
        assert_eq!(
            s.initialization_options.unwrap()["tsserver"]["path"],
            "C:/app/extensions/server.jdtls/1.40.0-1/server/node_modules/typescript/lib/tsserver.js"
        );
        let j = substitute_manifest(&bundled_jdtls(), &p);
        let l = j.launch.unwrap();
        assert_eq!(l.args, vec!["-configuration", "C:/app/extensions/server.jdtls/1.40.0-1/config_win", "-data", "C:/app/lsp-workspaces/0123456789abcdef"]);
        assert_eq!(j.args, vec!["-data", "C:/app/lsp-workspaces/0123456789abcdef/jdtls"]);
    }

    fn bundled_ts() -> ServerManifest {
        let crate::extensions::manifest::Manifest::Server(s) =
            crate::extensions::manifest::Manifest::parse(crate::extensions::manifest::fixtures::SERVER_BUNDLED).unwrap()
        else {
            unreachable!()
        };
        s
    }

    fn bundled_jdtls() -> ServerManifest {
        let crate::extensions::manifest::Manifest::Server(s) =
            crate::extensions::manifest::Manifest::parse(crate::extensions::manifest::fixtures::SERVER_JDTLS_BUNDLED).unwrap()
        else {
            unreachable!()
        };
        s
    }

    #[test]
    fn bundled_launch_assembly_per_runtime_kind() {
        let ph = Placeholders {
            ext_dir: "C:/e/server.x/1",
            workspace_storage: "C:/w/abc",
            project_root: "C:/p",
            jdtls_config: "config_mac_arm",
        };
        let cwd = PathBuf::from("C:/p");
        // node: [node, entry, args…], NODE_* scrubbed
        let ts = bundled_ts();
        let b = Bundled {
            program: PathBuf::from("C:/e/runtime.node/22/node.exe"),
            entry: PathBuf::from("C:/e/server.x/1/server/node_modules/typescript-language-server/lib/cli.mjs"),
            launch: ts.launch.clone().unwrap(),
        };
        let l = launch_bundled(&b, &ph, cwd.clone());
        assert_eq!(l.program, PathBuf::from("C:/e/runtime.node/22/node.exe"));
        assert_eq!(l.args, vec!["C:/e/server.x/1/server/node_modules/typescript-language-server/lib/cli.mjs", "--stdio"]);
        assert_eq!(l.cwd, cwd);
        assert!(l.env_remove.iter().any(|v| v == "NODE_OPTIONS"));
        assert!(l.env_remove.iter().any(|v| v == "NODE_PATH"));
        // java: [java, jvmArgs…, -jar, entry, args…] with ${jdtlsConfig} + ${extDir} resolved
        let jd = bundled_jdtls();
        let b = Bundled {
            program: PathBuf::from("C:/e/runtime.java/21/bin/java.exe"),
            entry: PathBuf::from("C:/e/server.x/1/plugins/org.eclipse.equinox.launcher_1.7.0.jar"),
            launch: jd.launch.clone().unwrap(),
        };
        let l = launch_bundled(&b, &ph, cwd.clone());
        assert_eq!(l.program, PathBuf::from("C:/e/runtime.java/21/bin/java.exe"));
        assert_eq!(
            l.args,
            vec![
                "-Declipse.application=org.eclipse.jdt.ls.core.id1",
                "-Xms1G",
                "--add-modules=ALL-SYSTEM",
                "-jar",
                "C:/e/server.x/1/plugins/org.eclipse.equinox.launcher_1.7.0.jar",
                "-configuration",
                "C:/e/server.x/1/config_mac_arm",
                "-data",
                "C:/w/abc",
            ]
        );
        assert!(l.env_remove.iter().any(|v| v == "JAVA_TOOL_OPTIONS"));
        // no runtime: [entry, args…], nothing scrubbed
        let mut raw = ts.launch.clone().unwrap();
        raw.runtime = None;
        raw.args = vec!["--log=${projectRoot}/ra.log".into()];
        let b = Bundled {
            program: PathBuf::from("C:/e/server.ra/1/rust-analyzer.exe"),
            entry: PathBuf::from("C:/e/server.ra/1/rust-analyzer.exe"),
            launch: raw,
        };
        let l = launch_bundled(&b, &ph, cwd);
        assert_eq!(l.program, PathBuf::from("C:/e/server.ra/1/rust-analyzer.exe"));
        assert_eq!(l.args, vec!["--log=C:/p/ra.log"]);
        assert!(l.env_remove.is_empty());
        // the system path keeps its shape (no scrubbing — the user's own install)
        let l = launch_for(&ts, Path::new("C:/x/tls.cmd"), &ph, PathBuf::from("C:/p"), &DetectEnv { path_env: None, home: None, vars: Default::default(), windows: true });
        assert_eq!(l.args, vec!["--stdio"]);
        assert!(l.env_remove.is_empty());
    }

    #[test]
    fn config_sections_walk_dotted_paths() {
        let s = json!({ "python": { "analysis": { "autoSearchPaths": true } }, "java": {} });
        assert_eq!(config_section(&s, "python.analysis"), json!({ "autoSearchPaths": true }));
        assert_eq!(config_section(&s, "python"), json!({ "analysis": { "autoSearchPaths": true } }));
        assert_eq!(config_section(&s, "java"), json!({}));
        assert_eq!(config_section(&s, "rust-analyzer"), Value::Null);
        assert_eq!(config_section(&Value::Null, "x"), Value::Null);
    }

    #[test]
    fn jdtls_script_becomes_a_direct_jvm_launch() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path().join("jdtls");
        let cfg = jdtls_config_dir(std::env::consts::OS, std::env::consts::ARCH);
        std::fs::create_dir_all(dist.join("bin")).unwrap();
        std::fs::create_dir_all(dist.join("plugins")).unwrap();
        std::fs::create_dir_all(dist.join(cfg)).unwrap();
        std::fs::write(dist.join("bin").join("jdtls.bat"), b"").unwrap();
        std::fs::write(dist.join("plugins").join("org.eclipse.equinox.launcher_1.6.0.jar"), b"").unwrap();
        std::fs::write(dist.join("plugins").join("org.eclipse.equinox.launcher_1.8.0.jar"), b"").unwrap();
        std::fs::write(dist.join("plugins").join("org.eclipse.equinox.launcher.win32_1.2.jar"), b"").unwrap();
        let java = tmp.path().join("jdk").join("bin").join("java.exe");
        let args = vec!["-data".to_string(), "C:/ws/jdtls".to_string()];
        let l = jdtls_direct_launch(&dist.join("bin").join("jdtls.bat"), &java, &args, dist.clone()).unwrap();
        assert_eq!(l.program, java);
        let jar = l.args.iter().position(|a| a == "-jar").unwrap();
        assert!(l.args[jar + 1].ends_with("org.eclipse.equinox.launcher_1.8.0.jar"), "newest launcher: {:?}", l.args);
        assert_eq!(l.args[jar + 2], "-configuration");
        assert!(l.args[jar + 3].ends_with(cfg));
        assert_eq!(&l.args[jar + 4..], &args[..], "manifest args trail");
        assert!(l.args.iter().any(|a| a == "--add-modules=ALL-SYSTEM"));
        // not a jdtls layout → None
        assert!(jdtls_direct_launch(&tmp.path().join("bin").join("jdtls"), &java, &args, dist.clone()).is_none());
        assert!(!is_jdtls_script(Path::new("C:/x/gopls.exe")));
        assert!(is_jdtls_script(Path::new("/opt/jdtls/bin/jdtls")));
        assert!(is_jdtls_script(Path::new("C:\\x\\JDTLS.BAT")));
        assert_eq!(jdtls_config_dir("windows", "x86_64"), "config_win");
        assert_eq!(jdtls_config_dir("macos", "aarch64"), "config_mac_arm");
        assert_eq!(jdtls_config_dir("linux", "x86_64"), "config_linux");
        // launch_for: a jdtls script + a jdk → the JVM; anything else passes through
        let m = manifest();
        let ph = Placeholders::new("C:/e", "C:/w", "C:/p");
        std::fs::create_dir_all(java.parent().unwrap()).unwrap();
        std::fs::write(&java, b"").unwrap();
        let mut env = DetectEnv { path_env: None, home: None, vars: Default::default(), windows: true };
        env.vars.insert("JAVA_HOME".into(), tmp.path().join("jdk").to_string_lossy().into_owned());
        let l = launch_for(&m, &dist.join("bin").join("jdtls.bat"), &ph, PathBuf::from("C:/p"), &env);
        assert_eq!(l.program, java);
        assert!(l.args.ends_with(&["-data".to_string(), "C:/w/jdtls".to_string()]));
        let l = launch_for(&m, Path::new("C:/x/gopls.exe"), &ph, PathBuf::from("C:/p"), &env);
        assert_eq!(l.program, PathBuf::from("C:/x/gopls.exe"));
        assert_eq!(l.args, vec!["-data", "C:/w/jdtls"]);
        // no jdk at all → the script itself (the user sees the hint, not a silent no-op)
        let env = DetectEnv { path_env: None, home: None, vars: Default::default(), windows: true };
        let l = launch_for(&m, &dist.join("bin").join("jdtls.bat"), &ph, PathBuf::from("C:/p"), &env);
        assert!(l.program.ends_with("jdtls.bat"));
    }

    #[test]
    fn system_launch_of_a_v2_pack_takes_the_launch_block_args() {
        let crate::extensions::manifest::Manifest::Server(m) =
            crate::extensions::manifest::Manifest::parse(crate::extensions::manifest::fixtures::SERVER_BUNDLED).unwrap()
        else {
            unreachable!()
        };
        let m = ServerManifest { args: Vec::new(), ..m };
        let ph = Placeholders::new("C:/e", "C:/w", "C:/p");
        let env = DetectEnv { path_env: None, home: None, vars: Default::default(), windows: true };
        let l = launch_for(&m, Path::new("C:/x/typescript-language-server.cmd"), &ph, PathBuf::from("C:/p"), &env);
        assert_eq!(l.args, vec!["--stdio"], "a system ts-ls without --stdio refuses to start");
        // explicit top-level args still win
        let m = ServerManifest { args: vec!["--socket=1".into()], ..m };
        let l = launch_for(&m, Path::new("C:/x/typescript-language-server.cmd"), &ph, PathBuf::from("C:/p"), &env);
        assert_eq!(l.args, vec!["--socket=1"]);
    }

    #[test]
    fn idle_reaper_state_machine() {
        let w = Duration::from_secs(600);
        assert_eq!(reap_decision(State::Ready, Duration::from_secs(10), w), Reap::Keep);
        assert_eq!(reap_decision(State::Ready, Duration::from_secs(299), w), Reap::Keep);
        assert_eq!(reap_decision(State::Ready, Duration::from_secs(300), w), Reap::MarkIdle);
        assert_eq!(reap_decision(State::Ready, Duration::from_secs(600), w), Reap::Stop);
        assert_eq!(reap_decision(State::Idle, Duration::from_secs(400), w), Reap::Keep);
        assert_eq!(reap_decision(State::Idle, Duration::from_secs(600), w), Reap::Stop);
        for s in [State::Starting, State::Crashed, State::Stopped, State::Missing] {
            assert_eq!(reap_decision(s, Duration::from_secs(9999), w), Reap::Keep, "{s:?}");
        }
        assert_eq!(reap_decision(State::Ready, Duration::from_secs(9999), Duration::ZERO), Reap::Keep);
    }

    fn manifest() -> ServerManifest {
        let crate::extensions::manifest::Manifest::Server(s) =
            crate::extensions::manifest::Manifest::parse(crate::extensions::manifest::fixtures::SERVER).unwrap()
        else {
            unreachable!()
        };
        s
    }

    fn project() -> ProjectRef {
        ProjectRef {
            id: Uuid::nil(),
            name: "proj".into(),
            root: PathBuf::from(if cfg!(windows) { "C:/w/proj" } else { "/w/proj" }),
        }
    }

    #[test]
    fn crash_bookkeeping_and_begin_start() {
        let h = ServerHandle::new("server.jdtls".into(), manifest(), project(), Box::new(|| {}));
        assert_eq!(h.state(), State::Starting);
        assert_eq!(h.begin_start(false), Begin::Busy);
        // first crash: immediate restart allowed
        h.fail_start("boom".into());
        assert_eq!(h.state(), State::Crashed);
        assert_eq!(h.snapshot().last_error.as_deref(), Some("boom"));
        assert_eq!(h.begin_start(false), Begin::Start);
        assert_eq!(h.snapshot().restarts, 1);
        // second crash: 5 s backoff → wait; force skips it
        h.fail_start("boom2".into());
        assert_eq!(h.begin_start(false), Begin::Wait);
        assert_eq!(h.begin_start(true), Begin::Start);
        // manual restart cleared the history: crash again = first crash
        h.fail_start("x".into());
        assert_eq!(h.begin_start(false), Begin::Start);
        // three more crashes inside the window → stopped, no auto restart
        h.fail_start("a".into());
        h.begin_start(true);
        h.force_state(State::Starting);
        {
            let mut g = h.inner.lock().unwrap();
            g.crashes = vec![Instant::now(); 3];
        }
        h.fail_start("final".into());
        assert_eq!(h.state(), State::Stopped);
        assert!(h.snapshot().last_error.unwrap().contains("crashed 4 times"));
        assert_eq!(h.begin_start(false), Begin::Wait);
        assert_eq!(h.begin_start(true), Begin::Start);
        // missing: manual only
        h.set_missing(Some("needs java 17+".into()));
        assert_eq!(h.state(), State::Missing);
        assert_eq!(h.begin_start(false), Begin::Wait);
        assert_eq!(h.begin_start(true), Begin::Start);
    }

    #[test]
    fn touch_and_idle_flip_and_snapshot_shape() {
        let h = ServerHandle::new("server.jdtls".into(), manifest(), project(), Box::new(|| {}));
        h.force_state(State::Ready);
        h.mark_idle();
        assert_eq!(h.state(), State::Idle);
        h.touch();
        assert_eq!(h.state(), State::Ready);
        let v = serde_json::to_value(h.snapshot()).unwrap();
        assert_eq!(v["id"], format!("server.jdtls:{}", Uuid::nil()));
        assert_eq!(v["label"], "Eclipse JDT LS");
        assert_eq!(v["language"], "java");
        assert_eq!(v["state"], "ready");
        for k in ["extId", "root", "projectId", "projectName", "pid", "memBytes", "cpu", "restarts", "startedAt", "lastError"] {
            assert!(v.get(k).is_some(), "{k}");
        }
    }

    #[test]
    fn doc_sync_tracks_versions_over_a_fake_server() {
        use crate::lsp::client::fake::client_with;
        let seen: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let client = client_with(
            Box::new(move |msg, _| {
                seen2.lock().unwrap().push(msg.clone());
            }),
            Box::new(|m, _| Err(RpcError::method_not_found(m))),
            Box::new(|_, _| {}),
            Box::new(|| {}),
        );
        let h = ServerHandle::with_client("server.jdtls", manifest(), project(), client);
        let abs = project().root.join("src").join("A.java");
        h.ensure_open(&abs, Some("class A {}"), "java").unwrap();
        h.ensure_open(&abs, Some("ignored"), "java").unwrap(); // already open
        h.change(&abs, "class A { int x; }".into(), 2, "java").unwrap();
        h.change(&abs, "class A { int y; }".into(), 2, "java").unwrap(); // stale version bumps
        h.close(&abs).unwrap();
        h.close(&abs).unwrap(); // second close is a no-op
        std::thread::sleep(Duration::from_millis(80));
        let seen = seen.lock().unwrap().clone();
        let methods: Vec<&str> = seen.iter().map(|m| m["method"].as_str().unwrap()).collect();
        assert_eq!(
            methods,
            vec!["textDocument/didOpen", "textDocument/didChange", "textDocument/didChange", "textDocument/didClose"]
        );
        assert_eq!(seen[0]["params"]["textDocument"]["version"], 1);
        assert_eq!(seen[0]["params"]["textDocument"]["languageId"], "java");
        assert!(seen[0]["params"]["textDocument"]["uri"].as_str().unwrap().starts_with("file:///"));
        assert_eq!(seen[1]["params"]["textDocument"]["version"], 2);
        assert_eq!(seen[2]["params"]["textDocument"]["version"], 3);
        assert_eq!(seen[2]["params"]["contentChanges"][0]["text"], "class A { int y; }");
        assert!(h.open_docs().is_empty());
    }

    fn recording_client() -> (Arc<Client>, Arc<Mutex<Vec<Value>>>) {
        use crate::lsp::client::fake::client_with;
        let seen: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let client = client_with(
            Box::new(move |msg, _| {
                seen2.lock().unwrap().push(msg.clone());
            }),
            Box::new(|m, _| Err(RpcError::method_not_found(m))),
            Box::new(|_, _| {}),
            Box::new(|| {}),
        );
        (client, seen)
    }

    fn methods(seen: &Arc<Mutex<Vec<Value>>>) -> Vec<String> {
        std::thread::sleep(Duration::from_millis(80));
        seen.lock().unwrap().iter().map(|m| m["method"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn docs_opened_while_starting_are_queued_and_flushed_once_ready() {
        let (client, seen) = recording_client();
        let h = ServerHandle::with_client("server.jdtls", manifest(), project(), client);
        h.force_state(State::Starting);
        let abs = project().root.join("src").join("A.java");
        // the editor opens + edits before the handshake is done: nothing goes out yet
        h.open(path_to_uri(&abs), "class A {}".into(), "java", 1).unwrap();
        h.change(&abs, "class A { int x; }".into(), 2, "java").unwrap();
        assert_eq!(h.queued_docs().len(), 1);
        assert!(methods(&seen).is_empty(), "nothing before the handshake");
        // ready → one didOpen carrying the LATEST text; a second flush is a no-op
        h.force_state(State::Ready);
        h.flush_docs();
        h.flush_docs();
        assert_eq!(methods(&seen), vec!["textDocument/didOpen"]);
        assert!(h.queued_docs().is_empty());
        {
            let s = seen.lock().unwrap();
            assert_eq!(s[0]["params"]["textDocument"]["text"], "class A { int x; }");
            assert_eq!(s[0]["params"]["textDocument"]["version"], 2);
            assert_eq!(s[0]["params"]["textDocument"]["languageId"], "java");
        }
        // an explicit re-open of a uri the server has = full-text change, not a 2nd didOpen
        h.open(path_to_uri(&abs), "class A { int y; }".into(), "java", 1).unwrap();
        assert_eq!(methods(&seen), vec!["textDocument/didOpen", "textDocument/didChange"]);
        assert_eq!(seen.lock().unwrap()[1]["params"]["textDocument"]["version"], 3);
        // a crash re-queues the buffer for the next handshake; closing an
        // unsent doc tells nobody
        let generation = h.inner.lock().unwrap().generation;
        h.on_closed(generation);
        assert_eq!(h.state(), State::Crashed);
        assert_eq!(h.queued_docs().len(), 1);
        h.close(&abs).unwrap();
        assert!(h.open_docs().is_empty());
        assert_eq!(methods(&seen).len(), 2, "no didClose for a doc the server never got");
    }

    #[test]
    fn editor_traffic_keeps_an_idle_server_awake() {
        let (client, _seen) = recording_client();
        let h = ServerHandle::with_client("server.jdtls", manifest(), project(), client);
        h.mark_idle();
        assert_eq!(h.state(), State::Idle);
        let before = h.idle_for();
        std::thread::sleep(Duration::from_millis(5));
        let abs = project().root.join("src").join("B.java");
        h.change(&abs, "class B {}".into(), 1, "java").unwrap();
        assert_eq!(h.state(), State::Ready, "a didChange counts as use");
        assert!(h.idle_for() < before + Duration::from_millis(5));
    }
}
