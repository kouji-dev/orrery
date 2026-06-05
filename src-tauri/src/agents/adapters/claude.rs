use std::path::Path;

use super::{claude_style_decision, AgentAdapter, Decision, HookEnv};

/// Claude Code. Reads `.claude/settings.json` from the cwd, so a worktree-scoped
/// settings file with a `PreToolUse` command hook drives both detection and the
/// blocking allow/deny round-trip. Decisions use the `hookSpecificOutput` schema.
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
        let dir = worktree.join(".claude");
        std::fs::create_dir_all(&dir)?;
        let hook = env.hook_bin.to_string_lossy().to_string();
        // PreToolUse blocks (timeout 600s) so the user has time to decide; Stop +
        // UserPromptSubmit are non-blocking status pings (working / done).
        let settings = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "*",
                    "hooks": [{ "type": "command", "command": hook, "timeout": 600 }]
                }],
                "UserPromptSubmit": [{
                    "hooks": [{ "type": "command", "command": hook }]
                }],
                "Stop": [{
                    "hooks": [{ "type": "command", "command": hook }]
                }]
            }
        });
        std::fs::write(dir.join("settings.json"), serde_json::to_vec_pretty(&settings)?)
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
            tool: "claude".into(),
            endpoint: "http://127.0.0.1:5000".into(),
            token: "tok".into(),
            hook_bin: PathBuf::from("/opt/katrix/kat-hook"),
        }
    }

    #[test]
    fn installs_worktree_scoped_pretooluse_hook() {
        let wt = tempfile::tempdir().unwrap();
        ClaudeAdapter.install_hooks(wt.path(), &env()).unwrap();
        let body = std::fs::read_to_string(wt.path().join(".claude/settings.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "*");
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"], 600);
        assert!(body.contains("kat-hook"), "hook command points at kat-hook");
    }

    #[test]
    fn allow_decision_is_claude_schema() {
        let json = ClaudeAdapter.format_decision(Decision::Allow, "ok");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "allow");
    }
}
