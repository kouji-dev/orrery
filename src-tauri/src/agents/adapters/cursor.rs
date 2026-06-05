use std::path::Path;

use super::{AgentAdapter, Decision, HookEnv};

/// Cursor Agent. Reads `.cursor/hooks.json` from the project root, so a
/// worktree-scoped hooks file drives `beforeShellExecution` / `beforeMCPExecution`
/// (the blocking gates) plus a `stop` status ping. Decisions use Cursor's own
/// `{"permission": ...}` schema.
pub struct CursorAdapter;

impl AgentAdapter for CursorAdapter {
    fn id(&self) -> &str {
        "cursor"
    }
    fn binary(&self) -> &str {
        "cursor-agent"
    }

    fn argv(&self, task: Option<&str>) -> Vec<String> {
        let mut v = vec!["cursor-agent".to_string()];
        if let Some(t) = task {
            if !t.is_empty() {
                v.push(t.to_string());
            }
        }
        v
    }

    fn install_hooks(&self, worktree: &Path, env: &HookEnv) -> std::io::Result<()> {
        let dir = worktree.join(".cursor");
        std::fs::create_dir_all(&dir)?;
        let hook = env.hook_bin.to_string_lossy().to_string();
        let hooks = serde_json::json!({
            "version": 1,
            "hooks": {
                "beforeShellExecution": [{ "command": hook }],
                "beforeMCPExecution": [{ "command": hook }],
                "stop": [{ "command": hook }]
            }
        });
        std::fs::write(dir.join("hooks.json"), serde_json::to_vec_pretty(&hooks)?)
    }

    fn format_decision(&self, decision: Decision, reason: &str) -> String {
        let permission = match decision {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::Ask => "ask",
        };
        serde_json::json!({ "permission": permission, "userMessage": reason }).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn env() -> HookEnv {
        HookEnv {
            agent_id: "a1".into(),
            tool: "cursor".into(),
            endpoint: "http://127.0.0.1:5000".into(),
            token: "tok".into(),
            hook_bin: PathBuf::from("/opt/katrix/kat-hook"),
        }
    }

    #[test]
    fn installs_worktree_scoped_shell_hook() {
        let wt = tempfile::tempdir().unwrap();
        CursorAdapter.install_hooks(wt.path(), &env()).unwrap();
        let body = std::fs::read_to_string(wt.path().join(".cursor/hooks.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["version"], 1);
        assert!(v["hooks"]["beforeShellExecution"][0]["command"]
            .as_str()
            .unwrap()
            .contains("kat-hook"));
    }

    #[test]
    fn decision_uses_cursor_permission_schema() {
        let json = CursorAdapter.format_decision(Decision::Deny, "no");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["permission"], "deny");
    }
}
