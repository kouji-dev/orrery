use std::path::Path;

use super::{claude_style_decision, AgentAdapter, Decision, HookEnv};

/// Gemini CLI. No blocking pre-tool hook we drive yet, so it runs as a plain
/// interactive session and falls back to PTY title/output parsing for status.
/// Kept in the registry so detection still reports whether it is installed.
pub struct GeminiAdapter;

impl AgentAdapter for GeminiAdapter {
    fn id(&self) -> &str {
        "gemini"
    }
    fn binary(&self) -> &str {
        "gemini"
    }

    fn argv(&self, task: Option<&str>) -> Vec<String> {
        let mut v = vec!["gemini".to_string()];
        if let Some(t) = task {
            if !t.is_empty() {
                v.push(t.to_string());
            }
        }
        v
    }

    fn supports_hooks(&self) -> bool {
        false
    }

    fn install_hooks(&self, _worktree: &Path, _env: &HookEnv) -> std::io::Result<()> {
        Ok(()) // no usable hook surface yet — PTY-parse fallback only
    }

    fn format_decision(&self, decision: Decision, reason: &str) -> String {
        claude_style_decision(decision, reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_declares_no_hooks_and_install_is_noop() {
        assert!(!GeminiAdapter.supports_hooks());
        let wt = tempfile::tempdir().unwrap();
        let env = HookEnv {
            agent_id: "a".into(),
            tool: "gemini".into(),
            endpoint: "e".into(),
            token: "t".into(),
            hook_bin: std::path::PathBuf::from("kat-hook"),
        };
        // no-op must not create any config dir
        GeminiAdapter.install_hooks(wt.path(), &env).unwrap();
        assert!(!wt.path().join(".gemini").exists());
    }
}
