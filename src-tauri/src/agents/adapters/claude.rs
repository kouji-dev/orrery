use std::path::Path;

use super::{AgentAdapter, HookEnv};

/// Claude Code. Reads `.claude/settings.json` from the cwd, so a worktree-scoped
/// settings file installs fire-and-forget status + needs-input hooks:
/// `Notification` (Claude needs the user — permission prompt or idle),
/// `UserPromptSubmit` (working), `Stop` (idle). Claude's own permission flow is
/// untouched — the user still approves in its TUI.
pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn id(&self) -> &str {
        "claude"
    }
    fn binary(&self) -> &str {
        "claude"
    }

    fn argv(&self, task: Option<&str>) -> Vec<String> {
        let mut v = vec!["claude".to_string()];
        if let Some(t) = task {
            if !t.is_empty() {
                v.push(t.to_string());
            }
        }
        v
    }

    fn install_hooks(&self, worktree: &Path, env: &HookEnv) -> std::io::Result<()> {
        use crate::cli::hook::hook_command;
        let dir = worktree.join(".claude");
        std::fs::create_dir_all(&dir)?;
        let settings = serde_json::json!({
            "hooks": {
                "Notification": [{
                    "hooks": [{ "type": "command", "command": hook_command(&env.hook_bin, "Notification") }]
                }],
                "UserPromptSubmit": [{
                    "hooks": [{ "type": "command", "command": hook_command(&env.hook_bin, "UserPromptSubmit") }]
                }],
                "Stop": [{
                    "hooks": [{ "type": "command", "command": hook_command(&env.hook_bin, "Stop") }]
                }]
            }
        });
        std::fs::write(dir.join("settings.json"), serde_json::to_vec_pretty(&settings)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn env() -> HookEnv {
        HookEnv {
            agent_id: "a1".into(),
            tool: "claude".into(),
            endpoint: "http://127.0.0.1:5000".into(),
            token: "tok".into(),
            hook_bin: PathBuf::from("/opt/katrix/katrix"),
        }
    }

    #[test]
    fn installs_worktree_scoped_notification_and_status_hooks() {
        let wt = tempfile::tempdir().unwrap();
        ClaudeAdapter.install_hooks(wt.path(), &env()).unwrap();
        let body = std::fs::read_to_string(wt.path().join(".claude/settings.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let cmd = v["hooks"]["Notification"][0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("hook --event Notification"), "needs-input hook: {cmd}");
        assert!(v["hooks"]["Stop"][0]["hooks"][0]["command"].is_string(), "status hook present");
    }
}
