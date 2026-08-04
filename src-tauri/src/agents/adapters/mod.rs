//! Per-agent abstraction: one `AgentAdapter` impl per CLI coding tool
//! (claude / codex / cursor / gemini). Everything that differs between tools —
//! how it launches, where its hook config lives, the JSON it expects back for an
//! allow/deny decision, and how we detect it on the machine — lives behind the
//! trait so the runtime + hook bridge stay tool-agnostic.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use portable_pty::CommandBuilder;
use serde::Serialize;
use serde_json::{Map, Value};

mod claude;
mod codex;
mod cursor;
mod gemini;

pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;
pub use cursor::CursorAdapter;
pub use gemini::GeminiAdapter;

/// Identity + connection a orrery-launched agent's hook needs to broker with the
/// bridge. Stamped onto the agent process env at launch (the `ORRERY_*` vars);
/// the globally-installed hook only brokers when these are present. The hook
/// executable itself is baked into the global hook config at install time, so it
/// is not part of this per-launch env.
#[derive(Debug, Clone)]
pub struct HookEnv {
    pub agent_id: String,
    pub tool: String,
    /// Loopback base, e.g. `http://127.0.0.1:54321`.
    pub endpoint: String,
    /// Shared secret the hook echoes so the server only honours our own hooks.
    pub token: String,
}

/// Per-tool detection report surfaced to the frontend (`detect_tools` /
/// `verify_tool_path`). Three outcomes, mirroring the design:
///   `ok`      — binary resolved AND `--version` ran → we know its path + version
///   `error`   — binary resolved but launching it failed (perm/arch/wrapper) →
///               the user must point Orrery at a working path (`reason` explains)
///   `missing` — nothing found on PATH (and no manual override)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub id: String,
    /// "ok" | "error" | "missing".
    pub status: String,
    /// Convenience mirror of `status == "ok"` (kept for existing callers).
    pub available: bool,
    /// Resolved executable path (PATH lookup or a manual override). `None` when missing.
    pub path: Option<String>,
    /// Version string parsed from `<bin> --version`, when it ran.
    pub version: Option<String>,
    /// Where `path` came from: "path" (auto-detected) | "manual" (user override).
    pub source: Option<String>,
    /// Why an `error` tool couldn't run — shown to the user.
    pub reason: Option<String>,
    /// True when the resolved install is a script shim (`.cmd`/`.bat`/`.ps1` or
    /// npm's extensionless launcher) rather than a native executable. The
    /// Settings tool tile shows a one-line "native installer" hint — a shim
    /// install may need a shell wrapper process per agent (A0.1).
    pub shim: bool,
}

impl ToolStatus {
    fn missing(id: String) -> Self {
        Self { id, status: "missing".into(), available: false, path: None, version: None, source: None, reason: None, shim: false }
    }
    fn ok(id: String, path: String, source: &str, version: Option<String>) -> Self {
        let shim = is_shim_path(&path);
        Self { id, status: "ok".into(), available: true, path: Some(path), version, source: Some(source.into()), reason: None, shim }
    }
    fn error(id: String, path: String, source: &str, reason: String) -> Self {
        Self { id, status: "error".into(), available: false, path: Some(path), version: None, source: Some(source.into()), reason: Some(reason), shim: false }
    }
}

/// Script-shim heuristic for the detection payload: on Windows anything that is
/// not a PE image (`.exe`/`.com`) — npm installs `.cmd`/`.ps1`/extensionless
/// launchers, all of which cost a wrapper process (or at best a parsed re-spawn)
/// per agent. Never a shim off Windows (Unix npm shims are exec-replaced shell
/// scripts — no lingering wrapper, so no hint to show).
fn is_shim_path(path: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    !matches!(ext.as_deref(), Some("exe") | Some("com"))
}

pub trait AgentAdapter: Send + Sync {
    /// Stable tool id used everywhere (db `tool`, frontend, event payloads).
    fn id(&self) -> &str;

    /// Executable name for `which`-detection and spawning.
    fn binary(&self) -> &str;

    /// Is this tool installed on the user's machine? (PATH lookup, never spawns.)
    fn is_installed(&self) -> bool {
        which(self.binary())
    }

    /// Program + args to launch the tool. The initial task prompt is included
    /// only when `task` is `Some` (first launch); resumes pass `None`.
    fn argv(&self, task: Option<&str>) -> Vec<String>;

    /// Program + args to RESUME a prior CLI session by its captured session id —
    /// e.g. claude's `claude --resume <id>`. `None` (the default) means the tool
    /// has no resume-by-id flow, so the runtime falls back to a normal launch.
    fn resume_argv(&self, _session_id: &str) -> Option<Vec<String>> {
        None
    }

    /// Build the PTY launch command (program + args from `argv`). The runtime
    /// adds cwd + env stamps; this stays pure so it is trivially testable.
    fn build_command(&self, task: Option<&str>) -> CommandBuilder {
        command_from(self.argv(task))
    }

    /// Build the PTY launch command to RESUME a session (program + args from
    /// `resume_argv`). `None` when the tool has no resume-by-id flow.
    fn build_resume_command(&self, session_id: &str) -> Option<CommandBuilder> {
        self.resume_argv(session_id).map(command_from)
    }

    /// Extra env to set on the agent process so its (globally-installed) hook
    /// recognises this run as orrery-launched and brokers with the bridge: the
    /// `ORRERY_*` identity + connection stamps. Default suits every tool now that
    /// hooks live in the user's real config (no per-tool managed home).
    fn env(&self, worktree: &Path, env: &HookEnv) -> Vec<(String, String)> {
        let _ = worktree;
        vec![
            ("ORRERY_AGENT_ID".into(), env.agent_id.clone()),
            ("ORRERY_TOOL".into(), env.tool.clone()),
            ("ORRERY_ENDPOINT".into(), env.endpoint.clone()),
            ("ORRERY_TOKEN".into(), env.token.clone()),
        ]
    }

    /// Install the agent's status + needs-input hooks (fire-and-forget) so it
    /// calls `orrery hook` on those events. Installed GLOBALLY, merged
    /// non-destructively into the user's real config under `home` (e.g.
    /// `~/.claude/settings.json`). Identity-agnostic — only `hook_bin` is used
    /// (baked into the hook command); the env-presence check at hook time
    /// (ORRERY_*) is what gates brokering, so a global install stays harmless for
    /// runs orrery didn't launch. No-op for tools without usable hooks.
    fn install_hooks(&self, home: &Path, hook_bin: &Path) -> std::io::Result<()>;

    /// Does this tool have hooks we drive (vs. PTY-parse fallback)?
    fn supports_hooks(&self) -> bool {
        true
    }

    /// Does this tool need the Rust-side PTY status heuristics (A0.3)?
    /// True only for tools whose working / needs-input state cannot come from
    /// a blocking permission hook (gemini) — the runtime then tees the raw
    /// stream through `runtime::heuristics` and emits `agent://pty-status`,
    /// so status survives even when the agent is in `none` interest mode.
    fn pty_status_fallback(&self) -> bool {
        false
    }

    /// Extra spawn args for the user's per-tool auto-approve policy
    /// (`"off" | "allowlist" | "everything"` — see `Settings::auto_approve`).
    ///
    /// Default: NO extra flags, for every policy. That is deliberate:
    /// * `"off"` — the tool's own permission flow runs untouched.
    /// * `"allowlist"` — orrery has no allowlist editor (removed from the
    ///   dialog), so "allowlist" means "the tool's own config IS the
    ///   allowlist"; the launch is identical to `"off"`.
    /// * `"everything"` — only tools with a real skip-permissions flag
    ///   override (claude / codex / cursor); a tool without one (gemini)
    ///   honestly ignores the policy rather than faking it.
    fn auto_approve_args(&self, _policy: &str) -> Vec<String> {
        Vec::new()
    }

    /// Extra spawn args that pin the agent's MODEL for this launch (the model the
    /// user picked in the spawn modal, stored on the agent record). Default
    /// `--model <model>` — claude / codex / cursor / gemini all accept `--model`
    /// (cursor + gemini also accept `-m`, claude only `--model`). An empty `model`
    /// adds nothing, so a record with no model launches on the tool's own default.
    fn model_args(&self, model: &str) -> Vec<String> {
        if model.is_empty() {
            Vec::new()
        } else {
            vec!["--model".into(), model.into()]
        }
    }

    /// Extra spawn args that set the reasoning EFFORT for this launch. Default:
    /// NONE — most tools expose no effort flag (claude/cursor/gemini all have
    /// `effort: false` in the frontend catalog), so the value is ignored. Tools
    /// with a real effort knob (codex) override this.
    fn effort_args(&self, _effort: &str) -> Vec<String> {
        Vec::new()
    }

    /// Best-effort PTY keystrokes that APPROVE the tool's current permission
    /// prompt. These are typed straight into the agent's PTY stdin — a stop-gap
    /// until real decision-forwarding over hooks lands. The default is a plain
    /// `y\r` (works for tools that ask a literal yes/no); tools whose prompt is a
    /// numbered/arrow SELECT (e.g. Claude Code) MUST override.
    fn allow_keys(&self) -> &str {
        "y\r"
    }

    /// Best-effort PTY keystrokes that DENY the tool's current permission prompt.
    /// Same caveats as [`allow_keys`]: typed into the PTY, best-effort until hook
    /// decision-forwarding exists. Default `n\r`; SELECT-style prompts override.
    fn deny_keys(&self) -> &str {
        "n\r"
    }

    /// Best-effort PTY keystrokes that SELECT option `choice` (1-based) in the
    /// tool's current numbered SELECT prompt — e.g. an AskUserQuestion-style ask
    /// rendered as "1. … 2. … 3. …". The default types the number then Enter
    /// (`"{choice}\r"`), which fits any numbered TUI select.
    ///
    /// Same caveats as [`allow_keys`]: these are typed straight into the agent's
    /// PTY stdin — a fire-and-forget keystroke, NOT a forwarded decision — and
    /// they ASSUME the prompt is a numbered select where the displayed order maps
    /// 1..N to the option index. Claude / Gemini render exactly this, so they use
    /// the default. Codex / Cursor inherit the default too, but their selection UI
    /// is unverified — revisit once their prompt keys are confirmed (real
    /// decision-forwarding will land via hooks anyway).
    fn decide_keys(&self, choice: u32) -> String {
        format!("{choice}\r")
    }

    // ── detection: each adapter owns how its tool reports + how we read it ──

    /// Args that make the binary print its version. Default `["--version"]`;
    /// override for a tool that uses a different flag.
    fn version_args(&self) -> Vec<&'static str> {
        vec!["--version"]
    }

    /// Parse a version out of the version-command output. Default: the first
    /// `MAJOR.MINOR[.PATCH]` token (see [`parse_semver`]); override for a tool
    /// whose `--version` output needs bespoke handling.
    fn parse_version(&self, output: &str) -> Option<String> {
        parse_semver(output)
    }

    /// Run the tool's version command at `path` and report whether it launched:
    /// `Ok(version?)` when it ran cleanly (version parsed if recognizable),
    /// `Err(reason)` when it couldn't launch / timed out / exited non-zero.
    /// Default composes [`run_probe`] + [`AgentAdapter::parse_version`]; override
    /// for a tool that needs entirely custom probing.
    fn probe_version(&self, path: &str) -> Result<Option<String>, String> {
        run_probe(path, &self.version_args()).map(|out| self.parse_version(&out))
    }
}

/// Every known adapter, installed or not.
pub fn registry() -> Vec<Box<dyn AgentAdapter>> {
    vec![
        Box::new(ClaudeAdapter),
        Box::new(CodexAdapter),
        Box::new(CursorAdapter),
        Box::new(GeminiAdapter),
    ]
}

/// The adapter for a tool id, if known.
pub fn adapter_for(tool: &str) -> Option<Box<dyn AgentAdapter>> {
    registry().into_iter().find(|a| a.id() == tool)
}

/// Install orrery's status + needs-input hooks GLOBALLY for every adapter that
/// supports hooks, merging non-destructively into the user's real config under
/// `home`. No `is_installed` gate: the merge is non-destructive, and a tool the
/// user installs later still gets hooks (we ran the global install on startup).
/// Best-effort — a per-adapter failure is logged and the loop keeps going.
pub fn install_global_hooks(home: &Path, hook_bin: &Path) {
    for adapter in registry() {
        if !adapter.supports_hooks() {
            continue;
        }
        if let Err(e) = adapter.install_hooks(home, hook_bin) {
            log::warn!("global hook install failed for {}: {e}", adapter.id());
        }
    }
}

/// Detection: every known tool resolved to a path + probed via the adapter's
/// own version command. `overrides` maps a tool id → a user-set manual path
/// (Settings `tool_paths`); a non-empty override is used INSTEAD of the PATH
/// lookup. Each tool is probed on its own thread so the spawns run concurrently.
pub fn installed(overrides: &BTreeMap<String, String>) -> Vec<ToolStatus> {
    let mut handles = Vec::new();
    for adapter in registry() {
        let id = adapter.id().to_string();
        let id_fallback = id.clone();
        let manual = overrides
            .get(&id)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        // `adapter` is Box<dyn AgentAdapter> (Send) — moved into the probe thread.
        let h = std::thread::spawn(move || detect_one(&*adapter, manual.as_deref()));
        handles.push((id_fallback, h));
    }
    handles
        .into_iter()
        .map(|(id, h)| h.join().unwrap_or_else(|_| ToolStatus::missing(id)))
        .collect()
}

/// Detect one tool via its `adapter`: resolve a path (manual override wins over
/// PATH), then run the adapter's [`probe_version`](AgentAdapter::probe_version).
/// See [`ToolStatus`] for the three outcomes.
pub fn detect_one(adapter: &dyn AgentAdapter, manual: Option<&str>) -> ToolStatus {
    let id = adapter.id().to_string();
    let (path, source) = match manual {
        Some(p) => (Some(p.to_string()), "manual"),
        None => (
            which_path(adapter.binary()).map(|p| p.display().to_string()),
            "path",
        ),
    };
    let Some(path) = path else {
        return ToolStatus::missing(id);
    };
    match adapter.probe_version(&path) {
        Ok(version) => ToolStatus::ok(id, path, source, version),
        Err(reason) => ToolStatus::error(id, path, source, reason),
    }
}

/// Verify a manual `path` for tool `id` (Settings "Use this path"). Routes
/// through the tool's adapter so its own version flag + parser apply; an unknown
/// id falls back to a generic `--version` probe.
pub fn detect_at(id: &str, path: &str) -> ToolStatus {
    match adapter_for(id) {
        Some(adapter) => detect_one(&*adapter, Some(path)),
        None => match run_probe(path, &["--version"]) {
            Ok(out) => ToolStatus::ok(id.into(), path.into(), "manual", parse_semver(&out)),
            Err(reason) => ToolStatus::error(id.into(), path.into(), "manual", reason),
        },
    }
}

/// Shared probe infra: run `<path> <args…>` (best-effort, bounded on a worker
/// thread so a wedged binary can't hang detection). `Ok(combined stdout+stderr)`
/// when it exits cleanly; `Err(reason)` when it can't launch / times out / exits
/// non-zero. Adapters layer their version parsing on top (see `probe_version`).
pub fn run_probe(path: &str, args: &[&str]) -> Result<String, String> {
    // Resolve script-shim / extensionless paths to something CreateProcessW can
    // launch (Windows) — e.g. npm's bare `claude` shim → `cmd.exe /c call
    // claude.cmd`. Without this, probing such a path fails os error 193.
    let (program, prefix) = launch_prefix(path);
    let mut cmd = Command::new(&program);
    cmd.args(&prefix)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW — no console flash
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(cmd.output());
    });
    match rx.recv_timeout(Duration::from_secs(6)) {
        Err(_) => Err("timed out — the binary didn’t respond".into()),
        Ok(Err(e)) => Err(format!("couldn’t launch — {e}")),
        Ok(Ok(out)) => {
            if out.status.success() {
                // Combine streams — tools print --version to stdout OR stderr.
                let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                text.push('\n');
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                Ok(text)
            } else {
                let code = out
                    .status
                    .code()
                    .map(|c| format!("exited with code {c}"))
                    .unwrap_or_else(|| "exited abnormally".into());
                let hint = String::from_utf8_lossy(&out.stderr);
                let hint = hint.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                Err(if hint.is_empty() {
                    format!("found, but {code}")
                } else {
                    format!("found, but {code} — {}", hint.trim())
                })
            }
        }
    }
}

/// Default version parser shared by adapters: the first `MAJOR.MINOR[.PATCH]`
/// token in the output (e.g. "claude 1.4.2" → "1.4.2", "v0.31.0 (abc)" →
/// "0.31.0"). `None` when no version-like token is present.
pub fn parse_semver(s: &str) -> Option<String> {
    for raw in s.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',') {
        let tok = raw.trim_start_matches(['v', 'V']);
        if tok.chars().next().is_some_and(|c| c.is_ascii_digit()) && tok.contains('.') {
            let ver: String = tok
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if ver.contains('.') {
                return Some(ver.trim_end_matches('.').to_string());
            }
        }
    }
    None
}

/// Merge our hook groups into a JSON hooks file without clobbering the user's
/// config. Shared by every adapter whose tool reads a JSON file with a
/// per-event list of hook groups (claude `.claude/settings.json`, cursor
/// `.cursor/hooks.json`). Behaviour:
///
/// * Load the existing file, tolerating missing / malformed / non-object input
///   (any of those → start from an empty object).
/// * Preserve every existing top-level key untouched.
/// * Insert each `(key, value)` from `defaults` only if that top-level key is
///   absent (e.g. cursor's `"version": 1`) — never overwrite a user's value.
/// * For every event in `events`: keep the user's own groups, drop EVERY prior
///   orrery group (idempotency — see below), then append one fresh group built by
///   `make_group`. The per-event lists live under the top-level `"hooks"` object.
/// * Write pretty JSON back.
///
/// A group is "ours" if, scanned recursively, any string value contains both the
/// hook binary's file STEM (e.g. `orrery`) AND `"hook --event"`. Keying on the
/// stem — not the full `hook_bin` path — is deliberate: a prior install from a
/// DIFFERENT location (the dev `target\debug\orrery.exe` vs the bundled
/// `…\orrery.exe`, or a moved app) would otherwise go undetected and pile up a
/// duplicate group on every re-install. The stem catches every past orrery group
/// regardless of where its binary lived, so a re-install REPLACES rather than
/// appends. The test is shape-agnostic (recursive), so it works whether the command
/// sits at `group.hooks[].command` (claude) or `group.command` (cursor).
pub fn merge_json_hooks(
    path: &Path,
    events: &[&str],
    hook_bin: &Path,
    defaults: &[(&str, Value)],
    make_group: impl Fn(&str) -> Value,
) -> std::io::Result<()> {
    // Start from the existing file; tolerate missing/malformed/non-object.
    let mut root: Map<String, Value> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();

    // Insert defaults only where the key is absent — never clobber the user.
    for (k, v) in defaults {
        root.entry((*k).to_string()).or_insert_with(|| v.clone());
    }

    // Existing hooks map; treat a non-object `hooks` field as empty.
    let mut hooks: Map<String, Value> = match root.remove("hooks") {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };

    // Detect OUR groups by a PATH-INDEPENDENT marker — the binary's file stem (e.g.
    // "orrery") — so groups installed from any prior location are all collapsed (no
    // duplicate on a re-install from a different path). Falls back to the full path
    // only if the binary somehow has no file stem.
    let marker = hook_bin
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| hook_bin.display().to_string());
    let is_orrery = |group: &Value| -> bool { group_is_orrery(group, &marker) };

    for event in events {
        // Keep every existing non-orrery group; treat a non-array as empty.
        let mut groups: Vec<Value> = match hooks.remove(*event) {
            Some(Value::Array(a)) => a.into_iter().filter(|g| !is_orrery(g)).collect(),
            _ => Vec::new(),
        };
        groups.push(make_group(event));
        hooks.insert((*event).to_string(), Value::Array(groups));
    }

    root.insert("hooks".to_string(), Value::Object(hooks));
    std::fs::write(path, serde_json::to_vec_pretty(&Value::Object(root))?)
}

/// True if any string value anywhere in `group` contains both the binary `marker`
/// (its file stem, e.g. "orrery") and `"hook --event"` — i.e. this group was
/// installed by orrery, from any path. Recursive so it is agnostic to the group's
/// shape.
fn group_is_orrery(group: &Value, marker: &str) -> bool {
    match group {
        Value::String(s) => s.contains(marker) && s.contains("hook --event"),
        Value::Array(a) => a.iter().any(|v| group_is_orrery(v, marker)),
        Value::Object(m) => m.values().any(|v| group_is_orrery(v, marker)),
        _ => false,
    }
}

/// Build a `CommandBuilder` from an argv vector (`[program, args…]`). Shared by
/// the launch + resume command builders so both stay pure and identically shaped.
fn command_from(argv: Vec<String>) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(&argv[0]);
    for a in &argv[1..] {
        cmd.arg(a);
    }
    cmd
}

/// Is `cmd` an executable on PATH? Pure filesystem check — never spawns.
pub fn which(cmd: &str) -> bool {
    which_path(cmd).is_some()
}

/// Resolve an executable PATH to the (program, leading-args) pair an OS process
/// launcher can actually start. On Windows, a script shim is wrapped in its
/// interpreter (`.cmd`/`.bat` → `cmd.exe /c call <p>`, `.ps1` → PowerShell) and
/// an extensionless shim (npm's bare `claude`, which `CreateProcessW` rejects
/// with os error 193) is redirected to a sibling `.cmd`/`.exe`/`.bat`/`.ps1`.
/// On other platforms it's the path with no prefix. Shared by the version probe
/// and the agent launcher so a manual path works identically in both.
pub fn launch_prefix(path: &str) -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        windows_launch_prefix(path)
    }
    #[cfg(not(windows))]
    {
        (path.to_string(), Vec::new())
    }
}

/// The PowerShell used to run `.ps1` shims: PowerShell 7 (`pwsh.exe`) when it is
/// on PATH — noticeably faster startup and better ANSI/UTF-8 handling than
/// Windows PowerShell — falling back to the always-present `powershell.exe`.
#[cfg(windows)]
pub fn powershell_program() -> String {
    if which_path("pwsh").is_some() {
        "pwsh.exe".into()
    } else {
        "powershell.exe".into()
    }
}

/// Directly-spawnable argv parsed out of an npm-style launcher shim
/// (`claude.cmd` / `claude.ps1` / `claude.bat`): the real interpreter + script,
/// e.g. `["…\node.exe", "…\node_modules\@anthropic-ai\claude-code\cli.js"]`.
/// Spawning that argv deletes the shell wrapper process that would otherwise
/// live for the agent's whole run (A0.1). `None` when the file doesn't match
/// the known npm shim shapes — callers then fall back to wrapping the shim in
/// its interpreter (we never guess).
///
/// Cached per `(path, mtime)`: the parse re-runs only when the tool updates
/// (npm rewrites the shim on upgrade, bumping mtime). Negative results are
/// cached too — a hand-written wrapper script stays wrapped until it changes
/// on disk.
#[cfg(windows)]
pub fn resolve_shim_argv(path: &Path) -> Option<Vec<String>> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::SystemTime;
    type ShimCache = Mutex<HashMap<PathBuf, (SystemTime, Option<Vec<String>>)>>;
    static CACHE: OnceLock<ShimCache> = OnceLock::new();

    let meta = std::fs::metadata(path).ok()?;
    // Why 16 KiB: generated npm/pnpm shims are well under 1 KiB; anything bigger
    // is not a shim, and reading it just to reject it would be wasted IO.
    if meta.len() > 16 * 1024 {
        return None;
    }
    let mtime = meta.modified().ok()?;
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((cached_at, argv)) = cache.lock().unwrap().get(path) {
        if *cached_at == mtime {
            return argv.clone();
        }
    }
    let parsed = parse_shim(path);
    cache
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), (mtime, parsed.clone()));
    parsed
}

/// Uncached parse behind [`resolve_shim_argv`]: dispatch on extension to the
/// `.cmd`/`.bat` or `.ps1` npm-shim parser. Rejects any shim that mentions
/// `NODE_PATH` — some pnpm layouts export it for module resolution, and a
/// direct spawn would drop that env and break `require()` in the target.
#[cfg(windows)]
fn parse_shim(path: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    if text.contains("NODE_PATH") {
        return None;
    }
    let dir = path.parent()?;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "cmd" | "bat" => parse_cmd_shim(&text, dir),
        "ps1" => parse_ps1_shim(&text, dir),
        _ => None,
    }
}

/// npm's `.cmd` shim ends in one exec line shaped like
/// `… "%_prog%"  "%dp0%\node_modules\<pkg>\cli.js" %*` (current npm) or
/// `@"%~dp0\node.exe" "%~dp0\…\cli.js" %*` (older npm). Take the last line
/// carrying `%*`, read its double-quoted tokens, expand `%dp0%`/`%~dp0` to the
/// shim's directory and `%_prog%` per the shim's own logic (sibling `node.exe`,
/// else `node` on PATH). Both the program and the script must exist on disk or
/// the parse is rejected — never guess.
#[cfg(windows)]
fn parse_cmd_shim(text: &str, dir: &Path) -> Option<Vec<String>> {
    let exec_line = text
        .lines()
        .rev()
        .find(|l| l.contains("%*") && l.contains('"'))?;
    let quoted = quoted_tokens(exec_line);
    // Why exactly 2: `program + script` is the entire npm shim contract; extra
    // quoted tokens mean a hand-written wrapper doing something we can't model.
    if quoted.len() != 2 {
        return None;
    }
    let prog = resolve_cmd_shim_prog(&quoted[0], dir)?;
    let script = expand_dp0(&quoted[1], dir);
    if !Path::new(&script).is_file() {
        return None;
    }
    Some(vec![prog, script])
}

/// Resolve the program token of a `.cmd` shim exec line to a real on-disk
/// `.exe`. `%_prog%` mirrors the shim's own `IF EXIST` branch (sibling
/// `node.exe`, else `node` from PATH); an explicit `%dp0%`-relative `.exe` must
/// exist; a bare name goes through PATH. Anything else → `None` (wrap instead).
#[cfg(windows)]
fn resolve_cmd_shim_prog(token: &str, dir: &Path) -> Option<String> {
    if token == "%_prog%" {
        let sibling = dir.join("node.exe");
        if sibling.is_file() {
            return Some(sibling.to_string_lossy().into_owned());
        }
        return which_path("node").map(|p| p.to_string_lossy().into_owned());
    }
    let expanded = expand_dp0(token, dir);
    let p = Path::new(&expanded);
    if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) && p.is_file() {
        return Some(expanded);
    }
    if !expanded.contains('\\') && !expanded.contains('/') {
        // Bare program name (`node`) — a direct spawn needs the real path.
        return which_path(&expanded).map(|p| p.to_string_lossy().into_owned());
    }
    None
}

/// Expand batch `%~dp0` / npm's `SET dp0=%~dp0` variable (both mean "the
/// shim's own directory, trailing backslash") in one token. A doubled `\\` from
/// `"%dp0%\x"` is harmless to the filesystem APIs and CreateProcessW.
#[cfg(windows)]
fn expand_dp0(token: &str, dir: &Path) -> String {
    let d = format!("{}\\", dir.to_string_lossy());
    token.replace("%~dp0", &d).replace("%dp0%", &d)
}

/// npm's `.ps1` shim invokes `& "$basedir/node$exe"  "$basedir/…/cli.js" $args`
/// — a sibling-node branch, then a bare `"node$exe"` PATH fallback. Scan the
/// invocation lines in order and return the first whose program + script both
/// resolve to real files (`$basedir` → shim dir, `$exe` → ".exe" on Windows).
#[cfg(windows)]
fn parse_ps1_shim(text: &str, dir: &Path) -> Option<Vec<String>> {
    for line in text
        .lines()
        .filter(|l| l.contains("$args") && l.contains("& \""))
    {
        let quoted = quoted_tokens(line);
        if quoted.len() != 2 {
            continue;
        }
        let prog_tok = expand_ps1_vars(&quoted[0], dir);
        let script = expand_ps1_vars(&quoted[1], dir);
        let prog = if !prog_tok.contains('\\') && !prog_tok.contains('/') {
            // The else-branch's bare `"node$exe"` → resolve on PATH.
            let stem = prog_tok.trim_end_matches(".exe");
            which_path(stem).map(|p| p.to_string_lossy().into_owned())
        } else if Path::new(&prog_tok).is_file() {
            Some(prog_tok)
        } else {
            None
        };
        let Some(prog) = prog else { continue };
        if !Path::new(&script).is_file() {
            continue;
        }
        return Some(vec![prog, script]);
    }
    None
}

/// Expand the two variables npm's ps1 shim interpolates inside its quoted
/// arguments: `$basedir` (the shim's directory) and `$exe` (".exe" on Windows).
#[cfg(windows)]
fn expand_ps1_vars(token: &str, dir: &Path) -> String {
    token
        .replace("$basedir", &dir.to_string_lossy())
        .replace("$exe", ".exe")
}

/// The double-quoted substrings of `line`, in order. Batch/ps1 shim exec lines
/// quote exactly the program + script paths, so this is the whole tokenizer a
/// known-shape shim needs.
#[cfg(windows)]
fn quoted_tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else { break };
        out.push(after[..end].to_string());
        rest = &after[end + 1..];
    }
    out
}

#[cfg(windows)]
fn windows_launch_prefix(path: &str) -> (String, Vec<String>) {
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("exe") | Some("com") => (path.to_string(), Vec::new()),
        Some("cmd") | Some("bat") | Some("ps1") => {
            // A0.1: an npm-style shim parses to `node <cli.js>` — spawn that
            // directly, no wrapper process at all. Only an unparseable shim
            // falls back to its interpreter below.
            if let Some(argv) = resolve_shim_argv(p) {
                return (argv[0].clone(), argv[1..].to_vec());
            }
            if ext.as_deref() == Some("ps1") {
                // Old pwsh path, kept reachable: only a ps1-ONLY unparseable
                // shim still pays for a PowerShell wrapper.
                (
                    powershell_program(),
                    vec![
                        "-NoProfile".into(),
                        "-ExecutionPolicy".into(),
                        "Bypass".into(),
                        "-File".into(),
                        path.to_string(),
                    ],
                )
            } else {
                (
                    "cmd.exe".into(),
                    vec!["/c".into(), "call".into(), path.to_string()],
                )
            }
        }
        _ => {
            // Extensionless (or unknown) shim — prefer a runnable sibling with
            // the same stem (npm installs `claude` + `claude.cmd` side by side).
            // Why .cmd/.bat before .ps1 even when pwsh is installed: a cmd.exe
            // wrapper is ~3–5 MB RSS vs pwsh's ~60–100 MB, and the old "pwsh
            // runs npm's ps1 shim more smoothly" note predated the `/c call`
            // quoting fix (see runtime::wrap_for_ext) that made the .cmd path
            // survive spaced dirs + spaced prompts.
            let sibs: [&str; 4] = ["exe", "cmd", "bat", "ps1"];
            for sib in sibs {
                let cand = p.with_extension(sib);
                if cand.is_file() {
                    return windows_launch_prefix(&cand.to_string_lossy());
                }
            }
            (path.to_string(), Vec::new()) // nothing better — best effort
        }
    }
}

/// Resolve `cmd` to its full path on PATH (first match, honoring PATHEXT-style
/// extensions on Windows). Pure filesystem check — never spawns. `None` when
/// nothing matches.
pub fn which_path(cmd: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::split_paths(&paths).find_map(|dir| {
        exts.iter().find_map(|ext| {
            let cand = dir.join(format!("{cmd}{ext}"));
            cand.is_file().then_some(cand)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn registry_has_the_four_known_tools() {
        let reg = registry();
        let ids: Vec<&str> = reg.iter().map(|a| a.id()).collect();
        assert_eq!(ids, vec!["claude", "codex", "cursor", "gemini"]);
    }

    #[test]
    fn adapter_for_resolves_by_id_and_misses_unknown() {
        assert_eq!(adapter_for("cursor").unwrap().id(), "cursor");
        assert!(adapter_for("nope").is_none());
    }

    #[test]
    fn installed_reports_every_known_tool() {
        let report = installed(&BTreeMap::new());
        assert_eq!(report.len(), 4);
        for t in &report {
            assert!(
                matches!(t.status.as_str(), "ok" | "error" | "missing"),
                "valid status for {}: {}",
                t.id,
                t.status
            );
            assert_eq!(t.available, t.status == "ok", "available mirrors ok");
        }
        assert!(report.iter().any(|t| t.id == "claude"));
    }

    #[test]
    fn parse_semver_extracts_version_token() {
        assert_eq!(parse_semver("claude 1.4.2"), Some("1.4.2".into()));
        assert_eq!(parse_semver("v0.31.0 (abcdef)"), Some("0.31.0".into()));
        assert_eq!(parse_semver("cursor-agent version 2.10"), Some("2.10".into()));
        assert_eq!(parse_semver("no version here"), None);
    }

    #[test]
    fn every_adapter_defaults_to_version_flag_and_semver_parse() {
        for a in registry() {
            assert_eq!(a.version_args(), vec!["--version"]);
            assert_eq!(a.parse_version("tool 3.2.1"), Some("3.2.1".into()));
        }
    }

    #[test]
    fn which_path_is_none_for_bogus_command() {
        assert!(which_path("definitely-not-a-real-binary-xyzzy").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn launch_prefix_wraps_script_shims() {
        assert_eq!(launch_prefix(r"C:\bin\claude.exe"), (r"C:\bin\claude.exe".into(), vec![]));
        assert_eq!(
            launch_prefix(r"C:\bin\claude.cmd"),
            ("cmd.exe".into(), vec!["/c".into(), "call".into(), r"C:\bin\claude.cmd".into()]),
        );
        let (prog, args) = launch_prefix(r"C:\bin\claude.ps1");
        // pwsh.exe on machines with PowerShell 7, powershell.exe otherwise.
        assert_eq!(prog, powershell_program());
        assert_eq!(args.last().unwrap(), r"C:\bin\claude.ps1");
    }

    /// The exec tail of a real npm-generated `.cmd` shim (npm ≥9 shape) for a
    /// package whose entry is `node_modules\test-pkg\cli.js`.
    #[cfg(windows)]
    const NPM_CMD_SHIM: &str = concat!(
        "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\n",
        "SETLOCAL\r\nCALL :find_dp0\r\n\r\n",
        "IF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n",
        ") ELSE (\r\n  SET \"_prog=node\"\r\n  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\n",
        "endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & ",
        "\"%_prog%\"  \"%dp0%\\node_modules\\test-pkg\\cli.js\" %*\r\n",
    );

    /// A real npm-generated `.ps1` shim body (sibling-node branch + bare-node
    /// fallback) for the same package.
    #[cfg(windows)]
    const NPM_PS1_SHIM: &str = concat!(
        "#!/usr/bin/env pwsh\n",
        "$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent\n\n",
        "$exe=\"\"\n",
        "if ($PSVersionTable.PSVersion -lt \"6.0\" -or $IsWindows) {\n  $exe=\".exe\"\n}\n",
        "$ret=0\n",
        "if (Test-Path \"$basedir/node$exe\") {\n",
        "  if ($MyInvocation.ExpectingInput) {\n",
        "    $input | & \"$basedir/node$exe\"  \"$basedir/node_modules/test-pkg/cli.js\" $args\n",
        "  } else {\n",
        "    & \"$basedir/node$exe\"  \"$basedir/node_modules/test-pkg/cli.js\" $args\n",
        "  }\n  $ret=$LASTEXITCODE\n} else {\n",
        "  & \"node$exe\"  \"$basedir/node_modules/test-pkg/cli.js\" $args\n",
        "  $ret=$LASTEXITCODE\n}\nexit $ret\n",
    );

    /// Lay out a fake npm bin dir: node.exe sibling + the target cli.js.
    #[cfg(windows)]
    fn npm_dir_with(shim_name: &str, shim_body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("node.exe"), b"MZ").unwrap();
        let pkg = dir.path().join("node_modules").join("test-pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("cli.js"), b"// cli\n").unwrap();
        let shim = dir.path().join(shim_name);
        std::fs::write(&shim, shim_body).unwrap();
        (dir, shim)
    }

    // A0.1 primary path: the npm .cmd shim parses to `node.exe <cli.js>` and is
    // spawned directly — no cmd.exe wrapper process.
    #[cfg(windows)]
    #[test]
    fn resolve_shim_argv_parses_npm_cmd_shim_to_direct_node_spawn() {
        let (dir, shim) = npm_dir_with("claude.cmd", NPM_CMD_SHIM);
        let argv = resolve_shim_argv(&shim).expect("npm .cmd shim must resolve");
        assert_eq!(argv.len(), 2, "program + script: {argv:?}");
        assert_eq!(argv[0], dir.path().join("node.exe").to_string_lossy());
        assert!(argv[1].ends_with("cli.js"), "script target: {}", argv[1]);
        assert!(Path::new(&argv[1]).is_file(), "resolved script exists");
        // launch_prefix rides the same resolution: direct spawn, no wrapper.
        let (prog, args) = launch_prefix(&shim.to_string_lossy());
        assert_eq!(prog, argv[0]);
        assert_eq!(args, vec![argv[1].clone()]);
    }

    #[cfg(windows)]
    #[test]
    fn resolve_shim_argv_parses_npm_ps1_shim_to_direct_node_spawn() {
        let (dir, shim) = npm_dir_with("claude.ps1", NPM_PS1_SHIM);
        let argv = resolve_shim_argv(&shim).expect("npm .ps1 shim must resolve");
        assert_eq!(argv.len(), 2, "program + script: {argv:?}");
        assert_eq!(
            Path::new(&argv[0]).file_name().unwrap().to_string_lossy(),
            "node.exe"
        );
        assert!(argv[0].starts_with(&*dir.path().to_string_lossy()), "sibling node wins");
        assert!(Path::new(&argv[1]).is_file(), "resolved script exists");
        // No pwsh wrapper for a resolvable ps1 shim.
        let (prog, _args) = launch_prefix(&shim.to_string_lossy());
        assert_ne!(prog, powershell_program());
    }

    // "Never guess": a hand-written .cmd that doesn't match the npm shape must
    // NOT resolve — launch_prefix falls back to the cmd.exe /c call wrapper.
    #[cfg(windows)]
    #[test]
    fn resolve_shim_argv_rejects_non_npm_cmd_and_falls_back_to_wrapper() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("mytool.cmd");
        std::fs::write(&shim, "@echo off\r\nsomething.exe --flag\r\n").unwrap();
        assert_eq!(resolve_shim_argv(&shim), None);
        let (prog, args) = launch_prefix(&shim.to_string_lossy());
        assert_eq!(prog, "cmd.exe");
        assert_eq!(args[..2], ["/c".to_string(), "call".into()]);
    }

    // A shim that exports NODE_PATH depends on env a direct spawn would drop —
    // it must stay wrapped even though the exec line would otherwise parse.
    #[cfg(windows)]
    #[test]
    fn resolve_shim_argv_rejects_node_path_dependent_shim() {
        let (_dir, shim) = npm_dir_with(
            "claude.cmd",
            &format!("SET NODE_PATH=%dp0%\\node_modules\r\n{NPM_CMD_SHIM}"),
        );
        assert_eq!(resolve_shim_argv(&shim), None);
    }

    // A missing script target (package removed, stale shim) must not resolve.
    #[cfg(windows)]
    #[test]
    fn resolve_shim_argv_rejects_shim_whose_target_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("node.exe"), b"MZ").unwrap();
        let shim = dir.path().join("claude.cmd");
        std::fs::write(&shim, NPM_CMD_SHIM).unwrap(); // no node_modules laid out
        assert_eq!(resolve_shim_argv(&shim), None);
    }

    // The cache is keyed on (path, mtime): same mtime → the cached parse is
    // served even if the bytes changed; a bumped mtime → re-parse.
    #[cfg(windows)]
    #[test]
    fn resolve_shim_argv_caches_by_mtime_and_reparses_on_update() {
        let (_dir, shim) = npm_dir_with("claude.cmd", NPM_CMD_SHIM);
        let first = resolve_shim_argv(&shim).expect("initial parse resolves");
        let original_mtime = std::fs::metadata(&shim).unwrap().modified().unwrap();

        // Rewrite to garbage but RESTORE the mtime — the cache must serve the
        // old parse (we only re-read when the tool updates).
        std::fs::write(&shim, "@echo off\r\ngarbage\r\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&shim)
            .unwrap()
            .set_modified(original_mtime)
            .unwrap();
        assert_eq!(resolve_shim_argv(&shim), Some(first));

        // Bump mtime past the original — the garbage is re-parsed → None.
        std::fs::File::options()
            .write(true)
            .open(&shim)
            .unwrap()
            .set_modified(original_mtime + Duration::from_secs(5))
            .unwrap();
        assert_eq!(resolve_shim_argv(&shim), None);
    }

    // Detection surfaces the install kind: a script-shim path sets `shim`, a
    // native .exe does not (drives the Settings "native installer" hint).
    #[test]
    fn tool_status_flags_shim_installs_on_windows_only() {
        let cmd = ToolStatus::ok("claude".into(), r"C:\npm\claude.cmd".into(), "path", None);
        let exe = ToolStatus::ok("claude".into(), r"C:\winget\claude.exe".into(), "path", None);
        if cfg!(windows) {
            assert!(cmd.shim, ".cmd install is a shim");
            assert!(!exe.shim, ".exe install is native");
        } else {
            assert!(!cmd.shim && !exe.shim, "never flagged off Windows");
        }
        assert!(!ToolStatus::missing("claude".into()).shim);
    }

    #[cfg(windows)]
    #[test]
    fn launch_prefix_redirects_extensionless_shim_to_sibling_cmd() {
        // npm installs `claude` (bare shim) + `claude.cmd` side by side; pointing
        // detection at the bare shim must launch the .cmd (CreateProcessW can't
        // run the extensionless one → os error 193).
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("claude");
        let cmd = dir.path().join("claude.cmd");
        std::fs::write(&bare, b"#!/bin/sh\n").unwrap();
        std::fs::write(&cmd, b"@echo off\n").unwrap();
        let (prog, args) = launch_prefix(&bare.to_string_lossy());
        assert_eq!(prog, "cmd.exe");
        assert_eq!(args[0], "/c");
        assert_eq!(args[1], "call");
        assert_eq!(args[2], cmd.to_string_lossy());
    }

    #[test]
    fn detect_at_with_bad_manual_path_reports_error() {
        let bogus = if cfg!(windows) {
            "C:/nope/not-here.exe"
        } else {
            "/nope/not-here"
        };
        let st = detect_at("claude", bogus);
        assert_eq!(st.status, "error");
        assert_eq!(st.source.as_deref(), Some("manual"));
        assert_eq!(st.path.as_deref(), Some(bogus));
        assert!(st.reason.is_some(), "an error carries a reason");
    }


    #[test]
    fn argv_includes_task_only_on_first_launch() {
        let a = ClaudeAdapter;
        assert_eq!(a.argv(None), vec!["claude"]);
        assert_eq!(a.argv(Some("fix login")), vec!["claude", "fix login"]);
    }

    #[test]
    fn auto_approve_everything_maps_to_each_tools_bypass_flag() {
        let expect: &[(&str, &[&str])] = &[
            ("claude", &["--dangerously-skip-permissions"]),
            ("codex", &["--dangerously-bypass-approvals-and-sandbox"]),
            ("cursor", &["--force"]),
            ("gemini", &[]), // no bypass wired — approval stays in gemini's TUI
        ];
        for (tool, flags) in expect {
            let a = adapter_for(tool).unwrap();
            assert_eq!(
                a.auto_approve_args("everything"),
                flags.to_vec(),
                "{tool} 'everything' flags"
            );
        }
    }

    // "off" and "allowlist" launch every tool with its OWN permission flow — no
    // extra flags (orrery has no allowlist editor; the tool's config is the
    // allowlist). Unknown policies are treated like "off".
    #[test]
    fn auto_approve_off_and_allowlist_add_no_flags() {
        for tool in ["claude", "codex", "cursor", "gemini"] {
            let a = adapter_for(tool).unwrap();
            for policy in ["off", "allowlist", "", "future-policy"] {
                assert!(
                    a.auto_approve_args(policy).is_empty(),
                    "{tool} policy '{policy}' must add nothing"
                );
            }
        }
    }

    // Every adapter forwards the picked model as `--model <model>` (claude only
    // accepts `--model`; codex/cursor/gemini accept it too — and also `-m`, but we
    // emit the long form uniformly). An empty model adds nothing.
    #[test]
    fn model_args_default_is_dash_dash_model_for_every_tool() {
        for tool in ["claude", "codex", "cursor", "gemini"] {
            let a = adapter_for(tool).unwrap();
            assert_eq!(
                a.model_args("the-model"),
                vec!["--model".to_string(), "the-model".into()],
                "{tool} model flag"
            );
            assert!(
                a.model_args("").is_empty(),
                "{tool} empty model adds nothing"
            );
        }
    }

    // Only codex has a reasoning-effort knob: `--config model_reasoning_effort="<v>"`
    // (codex parses the value as TOML, hence the quotes). claude/cursor/gemini have
    // `effort: false` in the catalog and add nothing.
    #[test]
    fn effort_args_only_codex_emits_a_flag() {
        let codex = adapter_for("codex").unwrap();
        assert_eq!(
            codex.effort_args("high"),
            vec![
                "--config".to_string(),
                "model_reasoning_effort=high".into()
            ],
            "codex effort → config override"
        );
        assert!(
            codex.effort_args("").is_empty(),
            "codex empty effort adds nothing"
        );
        for tool in ["claude", "cursor", "gemini"] {
            let a = adapter_for(tool).unwrap();
            assert!(
                a.effort_args("high").is_empty(),
                "{tool} has no effort flag"
            );
        }
    }

    #[test]
    fn permission_keys_per_tool_are_correct_as_best_known() {
        // Claude / Gemini permission prompts are numbered SELECTs: allow = "1"+Enter,
        // deny = Esc. Codex / Cursor keep the best-effort y/n allow with Esc deny.
        let expect: &[(&str, &str, &str)] = &[
            ("claude", "1\r", "\x1b"),
            ("gemini", "1\r", "\x1b"),
            ("codex", "y\r", "\x1b"),
            ("cursor", "y\r", "\x1b"),
        ];
        for (tool, allow, deny) in expect {
            let a = adapter_for(tool).unwrap_or_else(|| panic!("adapter for {tool}"));
            assert_eq!(a.allow_keys(), *allow, "{tool} allow_keys");
            assert_eq!(a.deny_keys(), *deny, "{tool} deny_keys");
        }
    }

    #[test]
    fn agent_allow_deny_resolve_the_right_adapter_keys() {
        // Mirrors the agent_allow/agent_deny command path: resolve tool → adapter,
        // then read its allow/deny keystrokes. Claude is the SELECT-style case.
        let claude = adapter_for("claude").unwrap();
        assert_eq!(
            claude.allow_keys(),
            "1\r",
            "claude allow is option 1 + Enter"
        );
        assert_eq!(claude.deny_keys(), "\x1b", "claude deny is Esc");
        // An unknown tool has no adapter — the command turns this into an error.
        assert!(adapter_for("nope").is_none());
    }

    #[test]
    fn decide_keys_default_is_numbered_select_number_plus_enter() {
        // The default numbered-select keystroke: option N → "N" + Enter.
        let claude = adapter_for("claude").unwrap();
        assert_eq!(claude.decide_keys(1), "1\r", "option 1 → \"1\" + Enter");
        assert_eq!(claude.decide_keys(3), "3\r", "option 3 → \"3\" + Enter");
        // Claude / Gemini render a numbered select, so both use the default.
        let gemini = adapter_for("gemini").unwrap();
        assert_eq!(
            gemini.decide_keys(2),
            "2\r",
            "gemini option 2 → \"2\" + Enter"
        );
    }

    #[test]
    fn agent_decide_resolves_the_right_adapter_keys() {
        // Mirrors the agent_decide command path: resolve tool → adapter, then read
        // its decide keystrokes for a 1-based choice.
        let claude = adapter_for("claude").unwrap();
        assert_eq!(
            claude.decide_keys(2),
            "2\r",
            "claude option 2 → \"2\" + Enter"
        );
        // An unknown tool has no adapter — the command turns this into an error.
        assert!(adapter_for("nope").is_none());
    }

    #[test]
    fn trait_default_permission_keys_are_yn() {
        // A throwaway adapter that overrides nothing must inherit the y/n default.
        struct Bare;
        impl AgentAdapter for Bare {
            fn id(&self) -> &str {
                "bare"
            }
            fn binary(&self) -> &str {
                "bare"
            }
            fn argv(&self, _task: Option<&str>) -> Vec<String> {
                vec!["bare".into()]
            }
            fn install_hooks(&self, _home: &Path, _hook_bin: &Path) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert_eq!(Bare.allow_keys(), "y\r");
        assert_eq!(Bare.deny_keys(), "n\r");
        // decide_keys is the numbered-select default regardless of allow/deny.
        assert_eq!(Bare.decide_keys(1), "1\r");
        assert_eq!(Bare.decide_keys(4), "4\r");
        // resume_argv defaults to None (no resume-by-id flow) → no resume command.
        assert_eq!(Bare.resume_argv("abc"), None);
        assert!(Bare.build_resume_command("abc").is_none());
        // auto_approve_args defaults to no flags — even for "everything".
        assert!(Bare.auto_approve_args("everything").is_empty());
    }

    #[test]
    fn claude_resume_argv_builds_resume_command() {
        // Claude resumes a prior session by id: `claude --resume <id>` (no prompt).
        let claude = adapter_for("claude").unwrap();
        assert_eq!(
            claude.resume_argv("abc"),
            Some(vec![
                "claude".to_string(),
                "--resume".to_string(),
                "abc".to_string()
            ])
        );
        // codex/cursor resume by id too (their own command shapes).
        assert_eq!(
            adapter_for("codex").unwrap().resume_argv("abc"),
            Some(vec!["codex".into(), "resume".into(), "abc".into()])
        );
        assert_eq!(
            adapter_for("cursor").unwrap().resume_argv("abc"),
            Some(vec!["cursor-agent".into(), "--resume".into(), "abc".into()])
        );
        // gemini has no resume-by-id flow → None (fall back to a normal launch).
        assert_eq!(
            adapter_for("gemini").unwrap().resume_argv("abc"),
            None,
            "gemini has no resume_argv"
        );
    }

    #[test]
    fn install_global_hooks_writes_all_four_hooked_tools_incl_gemini() {
        let home = tempfile::tempdir().unwrap();
        let hook_bin = PathBuf::from("/opt/orrery/orrery");
        install_global_hooks(home.path(), &hook_bin);

        // Every adapter now supports hooks, so all four install their global config.
        assert!(
            home.path().join(".claude/settings.json").exists(),
            "claude global settings written"
        );
        assert!(
            home.path().join(".cursor/hooks.json").exists(),
            "cursor global hooks written"
        );
        assert!(
            home.path().join(".codex/config.toml").exists(),
            "codex global config written"
        );
        // gemini is now wired up — it writes into ~/.gemini/settings.json too.
        assert!(
            home.path().join(".gemini/settings.json").exists(),
            "gemini global settings written"
        );
    }
}
