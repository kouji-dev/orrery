use std::path::Path;

use super::AgentAdapter;

/// OpenAI Codex CLI. Codex has no per-directory hook file; it reads config from
/// its real `~/.codex` (`$CODEX_HOME`). We MERGE fire-and-forget status hooks
/// into the user's real `~/.codex/config.toml` non-destructively (preserving all
/// other keys + comments via `toml_edit`). Harmless for non-orrery runs —
/// `orrery hook` only brokers when the ORRERY_* env is present.
///
/// SCHEMA CAUTION: codex's hook config format is the least stable of the three.
/// The current docs (developers.openai.com/codex/hooks) document an array-table
/// schema — `[[hooks.PostToolUse]]` with `[[hooks.PostToolUse.hooks]]` command
/// groups, matching Claude's event names (PreToolUse / PostToolUse /
/// PermissionRequest / SessionStart / Stop / …) + a `tool_response` stdin payload —
/// but the established orrery install (and many in-the-wild configs) use the
/// simpler flat `[hooks] pre_tool_use = "…"` keys. We keep the flat keys to avoid
/// churn / a breaking migration and add the Phase-2 events under the SAME flat
/// convention: `permission_request` (the dedicated permission ask, a documented
/// codex event) and `session_start` (lifecycle). `permission_request`'s parsed
/// payload carries the FULL request detail (command/description + permission_mode);
/// codex offers no allow/deny suggestions, so the suggestions list is empty.
/// Revisit if/when we migrate codex to the array-table schema.
pub struct CodexAdapter;

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

    // Codex's interactive approval prompt is a SELECT list, but its exact key
    // mapping is not stably documented across versions, so we keep the
    // best-effort y/n default for ALLOW and use Esc for DENY (Esc reliably
    // cancels a select prompt). Revisit once codex's prompt keys are verified —
    // real decision-forwarding will land via hooks anyway.
    fn allow_keys(&self) -> &str {
        "y\r"
    }
    fn deny_keys(&self) -> &str {
        "\x1b"
    }

    fn install_hooks(&self, home: &Path, hook_bin: &Path) -> std::io::Result<()> {
        use crate::cli::hook::hook_command;
        use toml_edit::{value, DocumentMut};

        let dir = home.join(".codex");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");

        // Load the user's existing config (tolerate missing/malformed → empty),
        // then set only our two hook keys under [hooks], preserving every other
        // key and comment. toml_edit edits in place, so this is idempotent: a
        // re-run overwrites the same two keys rather than duplicating them.
        let mut doc = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.parse::<DocumentMut>().ok())
            .unwrap_or_default();

        let pre = hook_command(hook_bin, "PreToolUse");
        let post = hook_command(hook_bin, "PostToolUse");
        let permission = hook_command(hook_bin, "PermissionRequest");
        let session = hook_command(hook_bin, "SessionStart");
        let stop = hook_command(hook_bin, "Stop");
        doc["hooks"]["pre_tool_use"] = value(pre);
        // PostToolUse runs after Bash / apply_patch / MCP tools produce output
        // (documented); summarized as a "<Tool> ✓/✗" result line in the feed.
        doc["hooks"]["post_tool_use"] = value(post);
        // PermissionRequest: codex's dedicated approval ask (a documented event) —
        // parsed to a full PermissionRequest with command/description + mode.
        doc["hooks"]["permission_request"] = value(permission);
        // SessionStart: lifecycle ping (working) when a codex session comes up.
        doc["hooks"]["session_start"] = value(session);
        doc["hooks"]["stop"] = value(stop);

        std::fs::write(path, doc.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn hook_bin() -> PathBuf {
        PathBuf::from("/opt/orrery/orrery")
    }

    fn read_config(home: &Path) -> toml_edit::DocumentMut {
        let body = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        body.parse().unwrap()
    }

    #[test]
    fn install_sets_pre_post_and_stop_hook_keys_in_global_config() {
        let home = tempfile::tempdir().unwrap();
        CodexAdapter.install_hooks(home.path(), &hook_bin()).unwrap();

        let doc = read_config(home.path());
        let pre = doc["hooks"]["pre_tool_use"].as_str().unwrap();
        let post = doc["hooks"]["post_tool_use"].as_str().unwrap();
        let stop = doc["hooks"]["stop"].as_str().unwrap();
        assert!(pre.contains("hook --event PreToolUse"), "pre_tool_use: {pre}");
        assert!(post.contains("hook --event PostToolUse"), "post_tool_use: {post}");
        assert!(stop.contains("hook --event Stop"), "stop: {stop}");
    }

    // Phase-2 additions: the dedicated permission ask + the SessionStart lifecycle
    // hook, under the same flat-key convention.
    #[test]
    fn install_sets_permission_and_session_start_hook_keys() {
        let home = tempfile::tempdir().unwrap();
        CodexAdapter.install_hooks(home.path(), &hook_bin()).unwrap();

        let doc = read_config(home.path());
        let perm = doc["hooks"]["permission_request"].as_str().unwrap();
        let session = doc["hooks"]["session_start"].as_str().unwrap();
        assert!(
            perm.contains("hook --event PermissionRequest"),
            "permission_request: {perm}"
        );
        assert!(
            session.contains("hook --event SessionStart"),
            "session_start: {session}"
        );
    }

    #[test]
    fn merge_preserves_existing_unrelated_key() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".codex");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "# my own config\nmodel = \"o3\"\n[mcp_servers]\nfoo = \"bar\"\n",
        )
        .unwrap();

        CodexAdapter.install_hooks(home.path(), &hook_bin()).unwrap();

        let doc = read_config(home.path());
        assert_eq!(doc["model"].as_str(), Some("o3"), "user's model preserved");
        assert_eq!(
            doc["mcp_servers"]["foo"].as_str(),
            Some("bar"),
            "user's mcp_servers table preserved"
        );
        assert!(doc["hooks"]["pre_tool_use"].as_str().unwrap().contains("hook --event"));
    }

    #[test]
    fn install_is_idempotent_no_duplicate_keys() {
        let home = tempfile::tempdir().unwrap();
        CodexAdapter.install_hooks(home.path(), &hook_bin()).unwrap();
        CodexAdapter.install_hooks(home.path(), &hook_bin()).unwrap();

        let body = std::fs::read_to_string(home.path().join(".codex/config.toml")).unwrap();
        // a re-run must overwrite, not append — exactly one of each key.
        // ("pre_tool_use" is a substring of nothing else; "post_tool_use" is its
        // own distinct key, so each count is exactly one.)
        assert_eq!(body.matches("pre_tool_use").count(), 1, "one pre_tool_use:\n{body}");
        assert_eq!(body.matches("post_tool_use").count(), 1, "one post_tool_use:\n{body}");
        assert_eq!(body.matches("permission_request").count(), 1, "one permission_request:\n{body}");
        assert_eq!(body.matches("session_start").count(), 1, "one session_start:\n{body}");
        assert_eq!(body.matches("stop =").count(), 1, "one stop:\n{body}");
        // and it must still parse cleanly.
        let _: toml_edit::DocumentMut = body.parse().unwrap();
    }
}
