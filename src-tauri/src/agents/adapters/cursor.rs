use std::path::{Path, PathBuf};

use super::AgentAdapter;

/// Cursor Agent. Reads the user's GLOBAL `~/.cursor/hooks.json` (per the official
/// docs, home-directory hooks apply across all projects), so we merge
/// fire-and-forget status pings there (non-destructively):
/// `beforeShellExecution` / `beforeMCPExecution` (working — tool REQUEST),
/// `afterShellExecution` / `afterFileEdit` (working — tool RESULT, e.g.
/// "cargo build ✓", "edit src/lib.rs ✓"), `postToolUseFailure` (tool failed/timed
/// out/denied — "<tool> ✗"), `afterAgentResponse` / `afterAgentThought` (streamed
/// assistant prose / thinking for the feed), `beforeSubmitPrompt` (turn start),
/// `sessionStart` / `sessionEnd` (lifecycle) + `stop` (idle). All are
/// activity/status only and never raise a notification. Hook names + payload shapes
/// per Cursor's hooks docs (cursor.com/docs/hooks). Cursor has NO dedicated
/// permission EVENT — its allow/deny decision is returned inline from the before*
/// hooks, so orrery surfaces no permission card for cursor (PTY fallback). Harmless
/// for non-orrery runs — `orrery hook` only brokers when ORRERY_* env is present.
pub struct CursorAdapter;

impl AgentAdapter for CursorAdapter {
    fn id(&self) -> &str {
        "cursor"
    }
    fn binary(&self) -> &str {
        "cursor-agent"
    }

    /// cursor-agent's install script does not use a package manager: it unpacks
    /// into `%LOCALAPPDATA%\cursor-agent` and then edits PATH — the edit our
    /// inherited environment block can predate. VERIFIED: this directory holds a
    /// real install on the developer's machine. Off Windows the same script
    /// leaves its launcher in `~/.local/bin`, which the shared sweep covers, so
    /// the `~/.local/share/cursor-agent` version tree (whose binaries sit one
    /// level further down, under `versions/<v>/`) and `~/.cursor/bin` (never
    /// observed) are both gone rather than guessed at.
    fn extra_dirs(&self) -> Vec<PathBuf> {
        std::env::var_os("LOCALAPPDATA")
            .map(|d| PathBuf::from(d).join("cursor-agent"))
            .into_iter()
            .collect()
    }

    fn base_argv(&self) -> Vec<String> {
        vec!["cursor-agent".to_string()]
    }

    // Resume a prior cursor-agent chat by id: `cursor-agent --resume <id>`
    // continues that conversation. Per cursor's CLI docs (cursor.com/docs/cli);
    // NOT verified against a local `--help` here (cursor-agent absent on this
    // box) — switch to the `--resume=<id>` equals form if the parser requires it.
    fn resume_argv(&self, session_id: &str) -> Option<Vec<String>> {
        Some(vec![
            "cursor-agent".to_string(),
            "--resume".to_string(),
            session_id.to_string(),
        ])
    }

    // autoApprove "everything" → `--force` (cursor-agent's documented
    // run-without-confirmations flag). "off"/"allowlist" add nothing: off keeps
    // cursor's own confirm flow, and allowlist defers to the user's own cursor
    // permission config (orrery ships no allowlist editor).
    fn auto_approve_args(&self, policy: &str) -> Vec<String> {
        match policy {
            "everything" => vec!["--force".into()],
            _ => Vec::new(),
        }
    }

    // cursor-agent's interactive permission prompt keys are not stably
    // documented, so we keep the best-effort y/n default for ALLOW and use Esc
    // for DENY (Esc reliably cancels a select/confirm prompt). Revisit once the
    // prompt keys are verified — real decision-forwarding will land via hooks.
    fn allow_keys(&self) -> &str {
        "y\r"
    }
    fn deny_keys(&self) -> &str {
        "\x1b"
    }

    /// cursor-agent can enumerate the models the ACCOUNT may run:
    /// `cursor-agent models` (the global `--list-models` flag is an alias).
    /// Needs auth + network. There is no `--json` form.
    fn list_models_args(&self) -> Option<Vec<&'static str>> {
        Some(vec!["models"])
    }

    /// The output is one bare model name per line, wrapped in spinner/ANSI
    /// redraws: a repainted `Loading models…` line first, then the names. An
    /// unauthenticated / empty account prints `No models available for this
    /// account.` and still EXITS 0, so empty must be read out of the TEXT and
    /// never from the status.
    ///
    /// So: strip ANSI, then keep only lines that are a single bare token — a
    /// model slug never contains whitespace. That alone drops the spinner line,
    /// the "No models available for this account." notice and any other prose.
    fn parse_models(&self, output: &str) -> Vec<String> {
        use super::strip_ansi;
        let mut out: Vec<String> = Vec::new();
        // A spinner redraws in place with `\r`, so split on both terminators and
        // judge each repaint on its own instead of one concatenated line.
        for line in strip_ansi(output).split(['\n', '\r']) {
            let line = line.trim();
            // one bare token only: prose, the spinner line and the empty notice
            // all carry whitespace; blank lines carry nothing.
            if line.is_empty() || line.contains(char::is_whitespace) {
                continue;
            }
            // a one-word spinner frame ("Loading…") is not a model
            if line.ends_with('…') || line.ends_with("...") {
                continue;
            }
            let id = line.to_string();
            if !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }

    fn install_hooks(&self, home: &Path, hook_bin: &Path) -> std::io::Result<()> {
        use super::merge_json_hooks;
        use crate::cli::hook::hook_command;

        let dir = home.join(".cursor");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("hooks.json");

        // Each managed event gets a `{"command":…}` group. The shared helper
        // seeds `"version": 1` only if absent, preserves the user's other keys +
        // own hooks, and drops only our prior groups (idempotency).
        merge_json_hooks(
            &path,
            &[
                "beforeShellExecution",
                "beforeMCPExecution",
                "afterShellExecution",
                "afterFileEdit",
                "postToolUseFailure",
                "afterAgentResponse",
                "afterAgentThought",
                "beforeSubmitPrompt",
                "sessionStart",
                "sessionEnd",
                "stop",
            ],
            hook_bin,
            &[("version", serde_json::json!(1))],
            |event| serde_json::json!({ "command": hook_command(hook_bin, event) }),
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
    fn installs_global_shell_hook() {
        let home = tempfile::tempdir().unwrap();
        CursorAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();
        let body = std::fs::read_to_string(home.path().join(".cursor/hooks.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["version"], 1);
        assert!(v["hooks"]["beforeShellExecution"][0]["command"]
            .as_str()
            .unwrap()
            .contains("hook --event beforeShellExecution"));
    }

    // The after* result hooks (afterShellExecution / afterFileEdit) are installed
    // as activity/status hooks — they feed the result lines in the activity feed.
    #[test]
    fn installs_after_result_hooks() {
        let home = tempfile::tempdir().unwrap();
        CursorAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();
        let v = read_hooks(home.path());
        for event in ["afterShellExecution", "afterFileEdit"] {
            let cmd = v["hooks"][event][0]["command"]
                .as_str()
                .unwrap_or_else(|| panic!("{event} hook installed"));
            assert!(
                cmd.contains(&format!("hook --event {event}")),
                "{event} hook: {cmd}"
            );
        }
    }

    // Phase-2 additions: the failure result hook, the assistant prose/thinking
    // hooks, the turn-start prompt hook, and the session lifecycle hooks.
    #[test]
    fn installs_failure_message_prompt_and_session_hooks() {
        let home = tempfile::tempdir().unwrap();
        CursorAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();
        let v = read_hooks(home.path());
        for event in [
            "postToolUseFailure",
            "afterAgentResponse",
            "afterAgentThought",
            "beforeSubmitPrompt",
            "sessionStart",
            "sessionEnd",
        ] {
            let cmd = v["hooks"][event][0]["command"]
                .as_str()
                .unwrap_or_else(|| panic!("{event} hook installed"));
            assert!(
                cmd.contains(&format!("hook --event {event}")),
                "{event} hook: {cmd}"
            );
        }
    }

    fn read_hooks(home: &Path) -> serde_json::Value {
        let body = std::fs::read_to_string(home.join(".cursor/hooks.json")).unwrap();
        serde_json::from_str(&body).unwrap()
    }

    /// A faithful `cursor-agent models` capture: the spinner repainting its
    /// "Loading models…" line over `\r` with colour codes, then one bare name
    /// per line, then a trailing blank.
    const MODELS_OUT: &str = concat!(
        "\x1b[?25l\x1b[36m⠋ Loading models…\x1b[0m\r",
        "\x1b[36m⠙ Loading models…\x1b[0m\r\x1b[2K",
        "composer-2.5\n",
        "composer-2.5-fast\n",
        "auto\n",
        "claude-sonnet-5\n",
        "\n\x1b[?25h",
    );

    #[test]
    fn cursor_lists_models_via_its_own_models_subcommand() {
        assert_eq!(CursorAdapter.list_models_args(), Some(vec!["models"]));
    }

    #[test]
    fn parse_models_keeps_bare_names_and_drops_spinner_noise() {
        assert_eq!(
            CursorAdapter.parse_models(MODELS_OUT),
            vec!["composer-2.5", "composer-2.5-fast", "auto", "claude-sonnet-5"],
            "ANSI stripped, spinner repaints dropped, one name per line"
        );
    }

    // The empty case EXITS 0, so it must be read out of the text: the notice is
    // prose (it carries whitespace) and yields nothing — the frontend then falls
    // back to the curated catalog rather than showing an empty picker.
    #[test]
    fn parse_models_is_empty_for_the_no_models_notice() {
        assert!(CursorAdapter
            .parse_models("\x1b[2KNo models available for this account.\n")
            .is_empty());
        assert!(CursorAdapter.parse_models("").is_empty());
        assert!(CursorAdapter.parse_models("\n\n  \n").is_empty());
        assert!(CursorAdapter
            .parse_models("You are not logged in. Run cursor-agent login.\n")
            .is_empty());
    }

    #[test]
    fn resume_argv_is_cursor_resume_session_id() {
        assert_eq!(
            CursorAdapter.resume_argv("abc"),
            Some(vec!["cursor-agent".into(), "--resume".into(), "abc".into()])
        );
    }

    fn is_orrery(cmd: &str) -> bool {
        cmd.contains("/opt/orrery/orrery") && cmd.contains("hook --event")
    }

    #[test]
    fn merge_preserves_existing_unrelated_top_level_key() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".cursor");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("hooks.json"),
            r#"{"version":1,"customSetting":{"foo":"bar"}}"#,
        )
        .unwrap();

        CursorAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();

        let v = read_hooks(home.path());
        assert_eq!(
            v["customSetting"]["foo"].as_str(),
            Some("bar"),
            "existing top-level key preserved"
        );
        let cmd = v["hooks"]["beforeShellExecution"][0]["command"]
            .as_str()
            .unwrap();
        assert!(
            cmd.contains("hook --event beforeShellExecution"),
            "our hook present: {cmd}"
        );
    }

    #[test]
    fn merge_preserves_users_own_hook_on_managed_event() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".cursor");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("hooks.json"),
            r#"{"version":1,"hooks":{"beforeShellExecution":[{"command":"my-own-guard"}]}}"#,
        )
        .unwrap();

        CursorAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();

        let v = read_hooks(home.path());
        let groups = v["hooks"]["beforeShellExecution"].as_array().unwrap();
        let commands: Vec<&str> = groups
            .iter()
            .filter_map(|g| g["command"].as_str())
            .collect();
        assert!(
            commands.iter().any(|c| *c == "my-own-guard"),
            "user's own beforeShellExecution hook preserved: {commands:?}"
        );
        assert!(
            commands.iter().any(|c| is_orrery(c)),
            "orrery beforeShellExecution hook present: {commands:?}"
        );
    }

    #[test]
    fn install_is_idempotent_no_duplicate_orrery_groups() {
        let home = tempfile::tempdir().unwrap();
        CursorAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();
        CursorAdapter
            .install_hooks(home.path(), &hook_bin())
            .unwrap();

        let v = read_hooks(home.path());
        let groups = v["hooks"]["beforeShellExecution"].as_array().unwrap();
        let orrery_count = groups
            .iter()
            .filter(|g| g["command"].as_str().map(is_orrery).unwrap_or(false))
            .count();
        assert_eq!(
            orrery_count, 1,
            "exactly one orrery beforeShellExecution group"
        );
    }
}
