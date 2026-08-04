use std::path::Path;

use super::AgentAdapter;

/// Gemini CLI. Reads the user's GLOBAL `~/.gemini/settings.json` (project settings
/// in `.gemini/settings.json` merge over it), so we merge fire-and-forget status
/// pings there (non-destructively):
/// `BeforeTool` (working — tool REQUEST), `AfterTool` (working — tool RESULT, e.g.
/// "run_shell_command ✓"), `BeforeAgent` (session/turn start), `AfterAgent` (idle —
/// turn finished), `AfterModel` (streamed assistant prose for the feed),
/// `Notification` (best-effort notice), `SessionStart` / `SessionEnd` (lifecycle).
/// Gemini's JSON hook shape is identical to Claude's —
/// `hooks.<Event> = [{ hooks: [{ name, type:"command", command }] }]` (an optional
/// `matcher` defaults to "all") — so we reuse the shared `merge_json_hooks` helper.
/// Gemini has NO permission "ask" event, so orrery raises no permission card for it
/// (a tool denial only surfaces as a result-derived "✗" via AfterTool); approving
/// still happens in gemini's own TUI. Harmless for runs orrery didn't launch —
/// `orrery hook` only brokers when the ORRERY_* env is present. Hook names + config
/// shape per the Gemini CLI hooks docs (geminicli.com/docs/hooks).
pub struct GeminiAdapter;

impl AgentAdapter for GeminiAdapter {
    fn id(&self) -> &str {
        "gemini"
    }
    fn binary(&self) -> &str {
        "gemini"
    }

    /// Gemini has no BLOCKING permission hook, so its working / needs-input
    /// state is derived from the PTY stream — now in Rust (A0.3, see
    /// `runtime::heuristics`) so it keeps working when the agent is in
    /// `none` interest mode and ships no bytes to the renderer.
    fn pty_status_fallback(&self) -> bool {
        true
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

    // autoApprove: gemini inherits the trait default (NO flags for any policy)
    // per the settings plan — its bypass mode is not wired here; approval stays
    // in gemini's own TUI even when the user picks "everything".

    // Gemini CLI's tool-confirmation is a numbered/arrow SELECT, not a literal
    // y/n: "1. Yes, allow once  2. Yes, allow always  …  No, suggest changes
    // (esc)". ALLOW: "1" + Enter (option 1 is the one-time "Yes, allow once").
    // DENY: Esc (\x1b) — the prompt's own "No … (esc)" affordance, robust to the
    // option count. Best-effort PTY keystrokes (gemini has no permission hook).
    fn allow_keys(&self) -> &str {
        "1\r"
    }
    fn deny_keys(&self) -> &str {
        "\x1b"
    }

    fn install_hooks(&self, home: &Path, hook_bin: &Path) -> std::io::Result<()> {
        use super::merge_json_hooks;
        use crate::cli::hook::hook_command;

        let dir = home.join(".gemini");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("settings.json");

        // Gemini's group shape matches Claude's `{"hooks":[{"type":"command",…}]}`,
        // so the shared helper applies as-is: it preserves the user's other
        // settings + own hooks and drops only our prior groups (idempotency).
        merge_json_hooks(
            &path,
            &[
                "BeforeTool",
                "AfterTool",
                "BeforeAgent",
                "AfterAgent",
                "AfterModel",
                "Notification",
                "SessionStart",
                "SessionEnd",
            ],
            hook_bin,
            &[],
            |event| {
                serde_json::json!({
                    "hooks": [{ "type": "command", "command": hook_command(hook_bin, event) }]
                })
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn hook_bin() -> PathBuf {
        PathBuf::from("/opt/orrery/orrery")
    }

    fn read_settings(home: &Path) -> serde_json::Value {
        let body = std::fs::read_to_string(home.join(".gemini/settings.json")).unwrap();
        serde_json::from_str(&body).unwrap()
    }

    fn is_orrery(cmd: &str) -> bool {
        cmd.contains("/opt/orrery/orrery") && cmd.contains("hook --event")
    }

    #[test]
    fn gemini_supports_hooks() {
        assert!(GeminiAdapter.supports_hooks());
    }

    // Gemini's fire-and-forget hooks carry activity, but NOT a blocking
    // permission flow — the Rust PTY heuristics tee (A0.3) must stay on.
    #[test]
    fn gemini_needs_the_pty_status_fallback() {
        assert!(GeminiAdapter.pty_status_fallback());
    }

    // Gemini's full installed event set, written into ~/.gemini/settings.json in
    // the Claude-shaped nested-group layout.
    #[test]
    fn installs_all_managed_events() {
        let home = tempfile::tempdir().unwrap();
        GeminiAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();
        let v = read_settings(home.path());
        for event in [
            "BeforeTool",
            "AfterTool",
            "BeforeAgent",
            "AfterAgent",
            "AfterModel",
            "Notification",
            "SessionStart",
            "SessionEnd",
        ] {
            let cmd = v["hooks"][event][0]["hooks"][0]["command"]
                .as_str()
                .unwrap_or_else(|| panic!("{event} hook installed"));
            assert!(
                cmd.contains(&format!("hook --event {event}")),
                "{event} hook: {cmd}"
            );
        }
    }

    // The merge must preserve the user's existing settings.json keys (do NOT
    // clobber a user's model / theme / their own hooks).
    #[test]
    fn merge_preserves_existing_unrelated_top_level_key() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".gemini");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"theme":"GitHub","selectedAuthType":"oauth-personal"}"#,
        )
        .unwrap();

        GeminiAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();

        let v = read_settings(home.path());
        assert_eq!(
            v["theme"].as_str(),
            Some("GitHub"),
            "user's theme preserved"
        );
        assert_eq!(
            v["selectedAuthType"].as_str(),
            Some("oauth-personal"),
            "user's auth type preserved"
        );
        let cmd = v["hooks"]["BeforeTool"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(
            cmd.contains("hook --event BeforeTool"),
            "our hook present: {cmd}"
        );
    }

    // A user's own hook on a managed event survives — we append, not replace.
    #[test]
    fn merge_preserves_users_own_hook_on_managed_event() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".gemini");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"hooks":{"BeforeTool":[{"hooks":[{"type":"command","command":"my-own-guard"}]}]}}"#,
        )
        .unwrap();

        GeminiAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();

        let v = read_settings(home.path());
        let groups = v["hooks"]["BeforeTool"].as_array().unwrap();
        let commands: Vec<&str> = groups
            .iter()
            .filter_map(|g| g["hooks"][0]["command"].as_str())
            .collect();
        assert!(
            commands.iter().any(|c| *c == "my-own-guard"),
            "user's own BeforeTool hook preserved: {commands:?}"
        );
        assert!(
            commands.iter().any(|c| is_orrery(c)),
            "orrery BeforeTool hook present: {commands:?}"
        );
    }

    // A re-run overwrites our prior groups rather than duplicating them.
    #[test]
    fn install_is_idempotent_no_duplicate_orrery_groups() {
        let home = tempfile::tempdir().unwrap();
        GeminiAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();
        GeminiAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();

        let v = read_settings(home.path());
        let groups = v["hooks"]["BeforeTool"].as_array().unwrap();
        let orrery_count = groups
            .iter()
            .filter(|g| {
                g["hooks"][0]["command"]
                    .as_str()
                    .map(is_orrery)
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(orrery_count, 1, "exactly one orrery BeforeTool group");
    }
}
