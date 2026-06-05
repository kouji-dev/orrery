//! Per-agent abstraction: one `AgentAdapter` impl per CLI coding tool
//! (claude / codex / cursor / gemini). Everything that differs between tools —
//! how it launches, where its hook config lives, the JSON it expects back for an
//! allow/deny decision, and how we detect it on the machine — lives behind the
//! trait so the runtime + hook bridge stay tool-agnostic.

use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;
use serde::Serialize;

mod claude;
mod codex;
mod cursor;
mod gemini;

pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;
pub use cursor::CursorAdapter;
pub use gemini::GeminiAdapter;

/// What the spawned agent (and its hook child) need to call back into katrix.
/// Stamped onto the agent process env at launch and baked into the hook config.
#[derive(Debug, Clone)]
pub struct HookEnv {
    pub agent_id: String,
    pub tool: String,
    /// Loopback base, e.g. `http://127.0.0.1:54321`.
    pub endpoint: String,
    /// Shared secret the hook echoes so the server only honours our own hooks.
    pub token: String,
    /// The executable the agent's hook invokes — the katrix app itself, re-run as
    /// `katrix hook --event <EVENT>` (see cli::hook::hook_command).
    pub hook_bin: PathBuf,
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

    /// Build the PTY launch command (program + args from `argv`). The runtime
    /// adds cwd + env stamps; this stays pure so it is trivially testable.
    fn build_command(&self, task: Option<&str>) -> CommandBuilder {
        let argv = self.argv(task);
        let mut cmd = CommandBuilder::new(&argv[0]);
        for a in &argv[1..] {
            cmd.arg(a);
        }
        cmd
    }

    /// Extra env to set on the agent process so its hook can call back. Common
    /// `KATRIX_*` stamps plus any tool-specific home (e.g. `CODEX_HOME`).
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
    /// calls `katrix hook` on those events. Scoped to the worktree (or a managed
    /// home) — never the user's real global config. No-op for tools without
    /// usable hooks.
    fn install_hooks(&self, worktree: &Path, env: &HookEnv) -> std::io::Result<()>;

    /// Does this tool have hooks we drive (vs. PTY-parse fallback)?
    fn supports_hooks(&self) -> bool {
        true
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
}
