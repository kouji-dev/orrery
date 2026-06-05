use std::path::Path;

use super::{claude_style_decision, AgentAdapter, Decision, HookEnv};

/// OpenAI Codex CLI. Codex has no per-directory hook file; it reads config from
/// `$CODEX_HOME`. We point it at a *managed* home inside the worktree
/// (`.katrix/codex`) via the `CODEX_HOME` env stamp, so we never touch the
/// user's real `~/.codex`. Its pre-tool hook uses the same `hookSpecificOutput`
/// decision schema as Claude.
pub struct CodexAdapter;

impl CodexAdapter {
    /// The managed `CODEX_HOME` for an agent's worktree.
    pub fn home(worktree: &Path) -> std::path::PathBuf {
        worktree.join(".katrix").join("codex")
    }
}

impl AgentAdapter for CodexAdapter {
    fn id(&self) -> &str {
        "codex"
    }
    fn binary(&self) -> &str {
        "codex"
    }

    fn argv(&self, task: Option<&str>) -> Vec<String> {
        let mut v = vec!["codex".to_string()];
        if let Some(t) = task {
            if !t.is_empty() {
                v.push(t.to_string());
            }
        }
        v
    }

    fn env(&self, worktree: &Path, env: &HookEnv) -> Vec<(String, String)> {
        // common KATRIX_* stamps + the managed home so config.toml is picked up.
        let mut e = vec![
            ("KATRIX_AGENT_ID".into(), env.agent_id.clone()),
            ("KATRIX_TOOL".into(), env.tool.clone()),
            ("KATRIX_ENDPOINT".into(), env.endpoint.clone()),
            ("KATRIX_TOKEN".into(), env.token.clone()),
        ];
        e.push(("CODEX_HOME".into(), Self::home(worktree).to_string_lossy().to_string()));
        e
    }

    fn install_hooks(&self, worktree: &Path, env: &HookEnv) -> std::io::Result<()> {
        let home = Self::home(worktree);
        std::fs::create_dir_all(&home)?;
        let hook = env.hook_bin.to_string_lossy().to_string();
        // Best-effort: codex reads hooks from config.toml in CODEX_HOME. Exact
        // schema is still firming up upstream; the command + blocking pre-tool
        // gate are the load-bearing parts.
        let config = format!(
            "# managed by katrix — do not edit\n\
             [hooks]\n\
             pre_tool_use = {hook:?}\n\
             stop = {hook:?}\n"
        );
        std::fs::write(home.join("config.toml"), config)
    }

    fn format_decision(&self, decision: Decision, reason: &str) -> String {
        claude_style_decision(decision, reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn env() -> HookEnv {
        HookEnv {
            agent_id: "a1".into(),
            tool: "codex".into(),
            endpoint: "http://127.0.0.1:5000".into(),
            token: "tok".into(),
            hook_bin: PathBuf::from("/opt/katrix/kat-hook"),
        }
    }

    #[test]
    fn install_writes_config_into_managed_home() {
        let wt = tempfile::tempdir().unwrap();
        CodexAdapter.install_hooks(wt.path(), &env()).unwrap();
        let cfg = CodexAdapter::home(wt.path()).join("config.toml");
        let body = std::fs::read_to_string(cfg).unwrap();
        assert!(body.contains("kat-hook"));
        assert!(body.contains("[hooks]"));
    }

    #[test]
    fn env_points_codex_home_at_managed_dir() {
        let wt = tempfile::tempdir().unwrap();
        let e = CodexAdapter.env(wt.path(), &env());
        let home = e.iter().find(|(k, _)| k == "CODEX_HOME").unwrap();
        assert!(home.1.contains(".katrix"));
    }
}
