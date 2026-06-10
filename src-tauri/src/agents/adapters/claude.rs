use std::path::Path;

use super::AgentAdapter;

/// Claude Code. Reads the user's GLOBAL `~/.claude/settings.json`, so we merge
/// fire-and-forget status + needs-input hooks there (non-destructively):
/// `Notification` (Claude needs the user — permission prompt or idle),
/// `PermissionRequest` (the dedicated permission ask — FULL request detail +
/// suggestions), `UserPromptSubmit` (working), `PreToolUse` (working + carries the
/// tool/command REQUEST detail for the activity feed), `PostToolUse` (working +
/// carries the tool RESULT, e.g. "Edit ✓"), `PostToolUseFailure` (tool failed —
/// "Bash ✗"), `MessageDisplay` (streamed assistant prose for the feed),
/// `SessionStart`/`SessionEnd` (lifecycle pings), `Stop` (idle). The post/lifecycle
/// hooks are activity/status-only and NEVER raise a notification — only
/// `Notification`/`PermissionRequest` do. The hooks are harmless for runs orrery
/// didn't launch — `orrery hook` only brokers when the ORRERY_* env is present.
/// Claude's own permission flow is untouched — user approves in its TUI. Hook
/// names + payload shapes per Claude Code hooks docs (code.claude.com).
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

    // Resume a prior Claude Code session by its id: `claude --resume <id>` opens
    // that session restored (no prompt — the conversation continues).
    fn resume_argv(&self, session_id: &str) -> Option<Vec<String>> {
        Some(vec![
            "claude".to_string(),
            "--resume".to_string(),
            session_id.to_string(),
        ])
    }

    // autoApprove "everything" → `--dangerously-skip-permissions` (Claude Code's
    // documented skip-all-permission-prompts flag). "off"/"allowlist" add nothing:
    // off keeps Claude's own prompt flow, and allowlist defers to the user's own
    // `~/.claude/settings.json` permissions (orrery ships no allowlist editor).
    fn auto_approve_args(&self, policy: &str) -> Vec<String> {
        match policy {
            "everything" => vec!["--dangerously-skip-permissions".into()],
            _ => Vec::new(),
        }
    }

    // Claude Code's permission prompt is a numbered/arrow SELECT list, not a
    // literal y/n: "1. Yes  2. Yes, don't ask again  3. No" (verified via the
    // docs — option count varies, e.g. some prompts show "2. Yes, and allow …").
    // ALLOW: type "1" + Enter — option 1 is always the plain one-time "Yes".
    // DENY:  send Esc (\x1b) — Esc reliably cancels/denies the prompt regardless
    //        of how many options are shown, so it is safer than hard-coding "3"
    //        (which is "No" only when the list happens to have three entries).
    fn allow_keys(&self) -> &str {
        "1\r"
    }
    fn deny_keys(&self) -> &str {
        "\x1b"
    }

    fn install_hooks(&self, home: &Path, hook_bin: &Path) -> std::io::Result<()> {
        use super::merge_json_hooks;
        use crate::cli::hook::hook_command;

        let dir = home.join(".claude");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("settings.json");

        // Each managed event gets a `{"hooks":[{"type":"command","command":…}]}`
        // group. The shared helper preserves the user's other settings + own
        // hooks and drops only our prior groups (idempotency).
        merge_json_hooks(
            &path,
            &[
                "Notification",
                "PermissionRequest",
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PostToolUseFailure",
                "MessageDisplay",
                "SessionStart",
                "SessionEnd",
                "Stop",
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

    #[test]
    fn resume_argv_is_claude_resume_session_id() {
        assert_eq!(
            ClaudeAdapter.resume_argv("abc"),
            Some(vec!["claude".into(), "--resume".into(), "abc".into()])
        );
    }

    #[test]
    fn installs_global_notification_and_status_hooks() {
        let home = tempfile::tempdir().unwrap();
        ClaudeAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();
        let body = std::fs::read_to_string(home.path().join(".claude/settings.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let cmd = v["hooks"]["Notification"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(
            cmd.contains("hook --event Notification"),
            "needs-input hook: {cmd}"
        );
        assert!(
            v["hooks"]["Stop"][0]["hooks"][0]["command"].is_string(),
            "status hook present"
        );
    }

    // PreToolUse is installed as an activity/status hook (carries the tool/command
    // detail surfaced in the overview feed) — never a notification gate.
    #[test]
    fn installs_pre_tool_use_activity_hook() {
        let home = tempfile::tempdir().unwrap();
        ClaudeAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();
        let v = read_settings(home.path());
        let cmd = v["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .expect("PreToolUse hook installed");
        assert!(
            cmd.contains("hook --event PreToolUse"),
            "activity hook: {cmd}"
        );
    }

    // PostToolUse (tool result) + SessionStart/SessionEnd (lifecycle) are
    // installed as activity/status hooks — never notification gates.
    #[test]
    fn installs_post_tool_and_lifecycle_hooks() {
        let home = tempfile::tempdir().unwrap();
        ClaudeAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();
        let v = read_settings(home.path());
        for event in ["PostToolUse", "SessionStart", "SessionEnd"] {
            let cmd = v["hooks"][event][0]["hooks"][0]["command"]
                .as_str()
                .unwrap_or_else(|| panic!("{event} hook installed"));
            assert!(
                cmd.contains(&format!("hook --event {event}")),
                "{event} hook: {cmd}"
            );
        }
    }

    // PermissionRequest (the dedicated permission ask), PostToolUseFailure (tool
    // failure), and MessageDisplay (streamed assistant prose) are the Phase-2
    // additions to the installed set.
    #[test]
    fn installs_permission_failure_and_message_hooks() {
        let home = tempfile::tempdir().unwrap();
        ClaudeAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();
        let v = read_settings(home.path());
        for event in ["PermissionRequest", "PostToolUseFailure", "MessageDisplay"] {
            let cmd = v["hooks"][event][0]["hooks"][0]["command"]
                .as_str()
                .unwrap_or_else(|| panic!("{event} hook installed"));
            assert!(
                cmd.contains(&format!("hook --event {event}")),
                "{event} hook: {cmd}"
            );
        }
    }

    fn read_settings(home: &Path) -> serde_json::Value {
        let body = std::fs::read_to_string(home.join(".claude/settings.json")).unwrap();
        serde_json::from_str(&body).unwrap()
    }

    fn is_orrery(cmd: &str) -> bool {
        cmd.contains("/opt/orrery/orrery") && cmd.contains("hook --event")
    }

    #[test]
    fn merge_preserves_existing_unrelated_top_level_key() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"permissions":{"allow":["Bash(ls:*)"]}}"#,
        )
        .unwrap();

        ClaudeAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();

        let v = read_settings(home.path());
        assert_eq!(
            v["permissions"]["allow"][0].as_str(),
            Some("Bash(ls:*)"),
            "existing top-level permissions preserved"
        );
        let cmd = v["hooks"]["Notification"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(
            cmd.contains("hook --event Notification"),
            "our hook present: {cmd}"
        );
    }

    #[test]
    fn merge_preserves_users_own_hook_on_managed_event() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"hooks":{"Notification":[{"hooks":[{"type":"command","command":"my-own-notifier"}]}]}}"#,
        )
        .unwrap();

        ClaudeAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();

        let v = read_settings(home.path());
        let groups = v["hooks"]["Notification"].as_array().unwrap();
        let commands: Vec<&str> = groups
            .iter()
            .filter_map(|g| g["hooks"][0]["command"].as_str())
            .collect();
        assert!(
            commands.iter().any(|c| *c == "my-own-notifier"),
            "user's own Notification hook preserved: {commands:?}"
        );
        assert!(
            commands.iter().any(|c| is_orrery(c)),
            "orrery Notification hook present: {commands:?}"
        );
    }

    #[test]
    fn install_is_idempotent_no_duplicate_orrery_groups() {
        let home = tempfile::tempdir().unwrap();
        ClaudeAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();
        ClaudeAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();

        let v = read_settings(home.path());
        let groups = v["hooks"]["Notification"].as_array().unwrap();
        let orrery_count = groups
            .iter()
            .filter(|g| {
                g["hooks"][0]["command"]
                    .as_str()
                    .map(is_orrery)
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(orrery_count, 1, "exactly one orrery Notification group");
    }

    // Re-installing from a DIFFERENT binary path (dev build → bundled app, or a
    // moved exe) must REPLACE the prior orrery group, not pile up a second one:
    // detection keys on the binary STEM ("orrery") + "hook --event", not the full
    // path. This is the duplicate-on-reinstall bug.
    #[test]
    fn reinstall_from_different_path_replaces_not_duplicates() {
        let home = tempfile::tempdir().unwrap();
        ClaudeAdapter
            .install_hooks(home.path(), &PathBuf::from("/old/place/orrery"))
            .unwrap();
        ClaudeAdapter
            .install_hooks(home.path(), &PathBuf::from("/new/place/orrery"))
            .unwrap();

        let v = read_settings(home.path());
        let groups = v["hooks"]["MessageDisplay"].as_array().unwrap();
        let orrery: Vec<&str> = groups
            .iter()
            .filter_map(|g| g["hooks"][0]["command"].as_str())
            .filter(|c| c.contains("orrery") && c.contains("hook --event"))
            .collect();
        assert_eq!(
            orrery.len(),
            1,
            "one orrery group after cross-path reinstall: {orrery:?}"
        );
        assert!(
            orrery[0].contains("/new/place/orrery"),
            "kept current path: {orrery:?}"
        );
        assert!(
            !orrery[0].contains("/old/place/orrery"),
            "dropped stale path: {orrery:?}"
        );
    }
}
