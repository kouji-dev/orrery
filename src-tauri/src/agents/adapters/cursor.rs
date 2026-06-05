use std::path::Path;

use super::{AgentAdapter, HookEnv};

/// Cursor Agent. Reads `.cursor/hooks.json` from the project root, so a
/// worktree-scoped hooks file installs fire-and-forget status pings:
/// `beforeShellExecution` / `beforeMCPExecution` (working) + `stop` (idle).
/// Cursor has no dedicated needs-input hook — that falls to the PTY fallback.
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
        use crate::cli::hook::hook_command;
        let dir = worktree.join(".cursor");
        std::fs::create_dir_all(&dir)?;
        let hooks = serde_json::json!({
            "version": 1,
            "hooks": {
                "beforeShellExecution": [{ "command": hook_command(&env.hook_bin, "beforeShellExecution") }],
                "beforeMCPExecution": [{ "command": hook_command(&env.hook_bin, "beforeMCPExecution") }],
                "stop": [{ "command": hook_command(&env.hook_bin, "stop") }]
            }
        });
        std::fs::write(dir.join("hooks.json"), serde_json::to_vec_pretty(&hooks)?)
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
            hook_bin: PathBuf::from("/opt/katrix/katrix"),
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
            .contains("hook --event beforeShellExecution"));
    }
}
