//! Per-agent abstraction: one `AgentAdapter` impl per CLI coding tool
//! (claude / codex / cursor / gemini). Everything that differs between tools —
//! how it launches, where its hook config lives, the JSON it expects back for an
//! allow/deny decision, and how we detect it on the machine — lives behind the
//! trait so the runtime + hook bridge stay tool-agnostic.

use std::path::Path;

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

/// Identity + connection a katrix-launched agent's hook needs to broker with the
/// bridge. Stamped onto the agent process env at launch (the `KATRIX_*` vars);
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

/// Installed-tool report surfaced to the frontend (`detect_tools`).
#[derive(Debug, Clone, Serialize)]
pub struct ToolStatus {
    pub id: String,
    pub available: bool,
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
    /// recognises this run as katrix-launched and brokers with the bridge: the
    /// `KATRIX_*` identity + connection stamps. Default suits every tool now that
    /// hooks live in the user's real config (no per-tool managed home).
    fn env(&self, worktree: &Path, env: &HookEnv) -> Vec<(String, String)> {
        let _ = worktree;
        vec![
            ("KATRIX_AGENT_ID".into(), env.agent_id.clone()),
            ("KATRIX_TOOL".into(), env.tool.clone()),
            ("KATRIX_ENDPOINT".into(), env.endpoint.clone()),
            ("KATRIX_TOKEN".into(), env.token.clone()),
        ]
    }

    /// Install the agent's status + needs-input hooks (fire-and-forget) so it
    /// calls `katrix hook` on those events. Installed GLOBALLY, merged
    /// non-destructively into the user's real config under `home` (e.g.
    /// `~/.claude/settings.json`). Identity-agnostic — only `hook_bin` is used
    /// (baked into the hook command); the env-presence check at hook time
    /// (KATRIX_*) is what gates brokering, so a global install stays harmless for
    /// runs katrix didn't launch. No-op for tools without usable hooks.
    fn install_hooks(&self, home: &Path, hook_bin: &Path) -> std::io::Result<()>;

    /// Does this tool have hooks we drive (vs. PTY-parse fallback)?
    fn supports_hooks(&self) -> bool {
        true
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

/// Install katrix's status + needs-input hooks GLOBALLY for every adapter that
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

/// Detection: every known tool tagged with whether it is installed.
pub fn installed() -> Vec<ToolStatus> {
    registry()
        .iter()
        .map(|a| ToolStatus {
            id: a.id().to_string(),
            available: a.is_installed(),
        })
        .collect()
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
/// * For every event in `events`: keep the user's own groups, drop only our
///   prior katrix groups (idempotency), then append one fresh group built by
///   `make_group`. The per-event lists live under the top-level `"hooks"` object.
/// * Write pretty JSON back.
///
/// A group is "ours" if, scanned recursively, any string value contains both the
/// `hook_bin` path AND `"hook --event"`. That shape-agnostic test works whether
/// the command sits at `group.hooks[].command` (claude) or `group.command`
/// (cursor), so the helper never assumes a particular group layout.
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

    let hook_bin = hook_bin.display().to_string();
    let is_katrix = |group: &Value| -> bool { group_is_katrix(group, &hook_bin) };

    for event in events {
        // Keep every existing non-katrix group; treat a non-array as empty.
        let mut groups: Vec<Value> = match hooks.remove(*event) {
            Some(Value::Array(a)) => a.into_iter().filter(|g| !is_katrix(g)).collect(),
            _ => Vec::new(),
        };
        groups.push(make_group(event));
        hooks.insert((*event).to_string(), Value::Array(groups));
    }

    root.insert("hooks".to_string(), Value::Object(hooks));
    std::fs::write(path, serde_json::to_vec_pretty(&Value::Object(root))?)
}

/// True if any string value anywhere in `group` contains both the hook_bin path
/// and `"hook --event"` — i.e. this group was installed by katrix. Recursive so
/// it is agnostic to the group's shape.
fn group_is_katrix(group: &Value, hook_bin: &str) -> bool {
    match group {
        Value::String(s) => s.contains(hook_bin) && s.contains("hook --event"),
        Value::Array(a) => a.iter().any(|v| group_is_katrix(v, hook_bin)),
        Value::Object(m) => m.values().any(|v| group_is_katrix(v, hook_bin)),
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
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::split_paths(&paths)
        .any(|dir| exts.iter().any(|ext| dir.join(format!("{cmd}{ext}")).is_file()))
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
        let report = installed();
        assert_eq!(report.len(), 4);
        // claude is the first adapter; its availability mirrors a PATH check.
        let claude = report.iter().find(|t| t.id == "claude").unwrap();
        assert_eq!(claude.available, which("claude"));
    }

    #[test]
    fn argv_includes_task_only_on_first_launch() {
        let a = ClaudeAdapter;
        assert_eq!(a.argv(None), vec!["claude"]);
        assert_eq!(a.argv(Some("fix login")), vec!["claude", "fix login"]);
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
        assert_eq!(claude.allow_keys(), "1\r", "claude allow is option 1 + Enter");
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
        assert_eq!(gemini.decide_keys(2), "2\r", "gemini option 2 → \"2\" + Enter");
    }

    #[test]
    fn agent_decide_resolves_the_right_adapter_keys() {
        // Mirrors the agent_decide command path: resolve tool → adapter, then read
        // its decide keystrokes for a 1-based choice.
        let claude = adapter_for("claude").unwrap();
        assert_eq!(claude.decide_keys(2), "2\r", "claude option 2 → \"2\" + Enter");
        // An unknown tool has no adapter — the command turns this into an error.
        assert!(adapter_for("nope").is_none());
    }

    #[test]
    fn trait_default_permission_keys_are_yn() {
        // A throwaway adapter that overrides nothing must inherit the y/n default.
        struct Bare;
        impl AgentAdapter for Bare {
            fn id(&self) -> &str { "bare" }
            fn binary(&self) -> &str { "bare" }
            fn argv(&self, _task: Option<&str>) -> Vec<String> { vec!["bare".into()] }
            fn install_hooks(&self, _home: &Path, _hook_bin: &Path) -> std::io::Result<()> { Ok(()) }
        }
        assert_eq!(Bare.allow_keys(), "y\r");
        assert_eq!(Bare.deny_keys(), "n\r");
        // decide_keys is the numbered-select default regardless of allow/deny.
        assert_eq!(Bare.decide_keys(1), "1\r");
        assert_eq!(Bare.decide_keys(4), "4\r");
        // resume_argv defaults to None (no resume-by-id flow) → no resume command.
        assert_eq!(Bare.resume_argv("abc"), None);
        assert!(Bare.build_resume_command("abc").is_none());
    }

    #[test]
    fn claude_resume_argv_builds_resume_command() {
        // Claude resumes a prior session by id: `claude --resume <id>` (no prompt).
        let claude = adapter_for("claude").unwrap();
        assert_eq!(
            claude.resume_argv("abc"),
            Some(vec!["claude".to_string(), "--resume".to_string(), "abc".to_string()])
        );
        // Other tools have no resume-by-id flow → None (fall back to a normal launch).
        for tool in ["codex", "cursor", "gemini"] {
            assert_eq!(
                adapter_for(tool).unwrap().resume_argv("abc"),
                None,
                "{tool} has no resume_argv"
            );
        }
    }

    #[test]
    fn install_global_hooks_writes_all_four_hooked_tools_incl_gemini() {
        let home = tempfile::tempdir().unwrap();
        let hook_bin = PathBuf::from("/opt/katrix/katrix");
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
