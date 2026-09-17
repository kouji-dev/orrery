use std::path::Path;

use super::AgentAdapter;

/// Pi coding agent (`@earendil-works/pi-coding-agent`, binary `pi` — install
/// `npm install -g --ignore-scripts @earendil-works/pi-coding-agent`). A minimal
/// terminal harness: read / write / edit / bash, BYOK across ~20 providers.
///
/// Two deliberate consequences for orrery, both taken from pi's own README
/// (github.com/badlogic/pi-mono, packages/coding-agent):
///
/// * NO SHELL HOOKS. Pi's extension points are TypeScript modules loaded from
///   `~/.pi/agent/extensions` (`pi.on("tool_call", …)`) — there is no JSON/TOML
///   hook table we can merge a `orrery hook` command into, the way claude /
///   codex / cursor / gemini all offer. So `supports_hooks()` is FALSE:
///   `install_hooks` is a no-op, the global installer skips pi, and the runtime
///   stamps no `ORRERY_*` env (nothing would read it). Status therefore comes
///   from the Rust PTY heuristics (`pty_status_fallback()`), the same path
///   gemini takes.
/// * NO PERMISSION PROMPT. Pi lists "No permission popups" among the features it
///   deliberately omits (they are left to extensions), so there is nothing to
///   auto-approve and nothing to answer: `auto_approve_args` stays the trait
///   default (no flags for ANY policy — honest rather than faking a bypass), and
///   the allow/deny keystrokes are never exercised.
///
/// Config lives in `~/.pi/agent` (overridable via `PI_CODING_AGENT_DIR`) — we
/// never write there.
pub struct PiAdapter;

impl AgentAdapter for PiAdapter {
    fn id(&self) -> &str {
        "pi"
    }
    fn binary(&self) -> &str {
        "pi"
    }

    fn base_argv(&self) -> Vec<String> {
        vec!["pi".to_string()]
    }

    /// No hooks at all (TypeScript extensions only) — see the type docs.
    fn supports_hooks(&self) -> bool {
        false
    }

    /// …so working / needs-input state must be derived from the PTY stream by
    /// `runtime::heuristics`, exactly as for gemini.
    fn pty_status_fallback(&self) -> bool {
        true
    }

    /// Resume a prior pi session by id: `pi --session <id>`. Documented as
    /// "`--session <path|id>` — use a specific session file or partial UUID"
    /// (pi README, Session Management). `-c/--continue` (most recent) and
    /// `-r/--resume` (interactive picker) exist too, but neither targets the id
    /// orrery captured, so `--session` is the one that fits resume-by-id.
    fn resume_argv(&self, session_id: &str) -> Option<Vec<String>> {
        Some(vec![
            "pi".to_string(),
            "--session".to_string(),
            session_id.to_string(),
        ])
    }

    /// Pi's reasoning knob is `--thinking <level>`, levels
    /// `off | minimal | low | medium | high | xhigh | max` (pi README, Model
    /// Selection). An empty effort adds nothing, so a record with no effort
    /// launches on pi's own default. (`--model` for the model is the trait
    /// default and is what pi documents; a pattern may be a bare id or a
    /// `provider/model` pair.)
    fn effort_args(&self, effort: &str) -> Vec<String> {
        if effort.is_empty() {
            Vec::new()
        } else {
            vec!["--thinking".into(), effort.into()]
        }
    }

    /// Pi is the one supported CLI that can enumerate its own `--model`
    /// vocabulary: `pi --list-models [search]` (pi `src/cli/args.ts`:
    /// "List available models (with optional fuzzy search)"). We pass no search
    /// so we get the whole table.
    fn list_models_args(&self) -> Option<Vec<&'static str>> {
        Some(vec!["--list-models"])
    }

    /// `pi --list-models` prints a chalk-coloured, two-space-padded table
    /// (pi `src/cli/list-models.ts`): a header row
    /// `provider  model  context  max-out  thinking  images`, then one row per
    /// model sorted by provider then id. There is NO `--json` form, so we parse
    /// the columns: strip ANSI, drop the header, take the first two fields and
    /// rejoin them as `provider/model` — the "supports `provider/id`" spelling
    /// pi's own `--model <pattern>` documents, which is unambiguous when two
    /// providers serve the same id.
    fn parse_models(&self, output: &str) -> Vec<String> {
        use super::strip_ansi;
        let mut out: Vec<String> = Vec::new();
        for line in strip_ansi(output).lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Columns are separated by 2+ spaces; a provider or model id never
            // contains one, so this splits the padded table exactly.
            let mut cols = line
                .split("  ")
                .map(str::trim)
                .filter(|c| !c.is_empty());
            let (Some(provider), Some(model)) = (cols.next(), cols.next()) else {
                continue;
            };
            // the header row, and any stray prose/warning line
            if provider.eq_ignore_ascii_case("provider") || model.eq_ignore_ascii_case("model") {
                continue;
            }
            if provider.contains(' ') || model.contains(' ') {
                continue;
            }
            let id = format!("{provider}/{model}");
            if !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }

    /// No-op: pi has no hook config file to merge into (see the type docs).
    /// Never reached in practice — `install_global_hooks` skips adapters whose
    /// `supports_hooks()` is false — but the trait requires an impl.
    fn install_hooks(&self, _home: &Path, _hook_bin: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_launches_the_pi_binary_with_a_positional_prompt() {
        assert_eq!(PiAdapter.id(), "pi");
        assert_eq!(PiAdapter.binary(), "pi");
        assert_eq!(
            PiAdapter.argv(Some("fix the bug"), &[]),
            vec!["pi".to_string(), "fix the bug".to_string()],
        );
    }

    #[test]
    fn resume_argv_is_pi_session_id() {
        assert_eq!(
            PiAdapter.resume_argv("abc"),
            Some(vec!["pi".into(), "--session".into(), "abc".into()])
        );
    }

    #[test]
    fn effort_maps_to_the_thinking_flag_and_empty_adds_nothing() {
        assert_eq!(
            PiAdapter.effort_args("high"),
            vec!["--thinking".to_string(), "high".to_string()]
        );
        assert_eq!(PiAdapter.effort_args("xhigh"), vec!["--thinking", "xhigh"]);
        assert!(PiAdapter.effort_args("").is_empty());
    }

    #[test]
    fn model_uses_the_shared_model_flag() {
        assert_eq!(
            PiAdapter.model_args("anthropic/claude-opus-5"),
            vec!["--model".to_string(), "anthropic/claude-opus-5".to_string()]
        );
        assert!(PiAdapter.model_args("").is_empty());
    }

    // Pi ships "No permission popups" by design, so no policy may invent a
    // bypass flag — every policy must launch identically.
    #[test]
    fn auto_approve_is_a_no_op_for_every_policy() {
        for policy in ["off", "allowlist", "everything"] {
            assert!(
                PiAdapter.auto_approve_args(policy).is_empty(),
                "pi has no permission flow to bypass: {policy}"
            );
        }
    }

    // No shell-hook config exists for pi → it opts out of hooks and onto the
    // Rust PTY heuristics, and installing writes nothing.
    #[test]
    fn pi_has_no_hooks_and_uses_the_pty_status_fallback() {
        assert!(!PiAdapter.supports_hooks());
        assert!(PiAdapter.pty_status_fallback());

        let home = tempfile::tempdir().unwrap();
        PiAdapter
            .install_hooks(home.path(), Path::new("/opt/orrery/orrery"))
            .unwrap();
        assert_eq!(
            std::fs::read_dir(home.path()).unwrap().count(),
            0,
            "install_hooks must not create any file for pi"
        );
    }

    /// A faithful `pi --list-models` sample: the documented header, two
    /// providers, and chalk colour on one row (pi colours its output).
    const LIST_MODELS_OUT: &str = concat!(
        "provider    model                context  max-out  thinking  images
",
        "anthropic   claude-opus-4-5      200K     64K      yes       yes
",
        "anthropic   claude-sonnet-4-5    200K     64K      yes       yes
",
        "[32mopenai      gpt-5.1              400K     128K     yes       yes[0m
",
        "google      gemini-3-pro         1M       64K      yes       yes
",
    );

    #[test]
    fn pi_lists_models_via_its_own_list_models_command() {
        assert_eq!(PiAdapter.list_models_args(), Some(vec!["--list-models"]));
    }

    #[test]
    fn parse_models_reads_the_table_as_provider_slash_model() {
        assert_eq!(
            PiAdapter.parse_models(LIST_MODELS_OUT),
            vec![
                "anthropic/claude-opus-4-5",
                "anthropic/claude-sonnet-4-5",
                "openai/gpt-5.1",
                "google/gemini-3-pro",
            ],
            "header dropped, ANSI stripped, first two columns joined"
        );
    }

    #[test]
    fn parse_models_ignores_prose_and_empty_output() {
        assert!(PiAdapter.parse_models("").is_empty());
        assert!(PiAdapter
            .parse_models("No models matching \"zzz\"
")
            .is_empty());
        // a chalk warning line has spaces inside its columns → skipped
        assert!(PiAdapter
            .parse_models("warning: could not load models from disk
")
            .is_empty());
    }

    // Only pi and cursor-agent can enumerate their models. claude (no models
    // subcommand), codex (its list is a private ~/.codex/models_cache.json) and
    // gemini (a hardcoded VALID_GEMINI_MODELS) must stay opted out, so the
    // frontend keeps their curated catalogs.
    #[test]
    fn only_pi_and_cursor_claim_a_model_listing_command() {
        for a in super::super::registry() {
            let has = a.list_models_args().is_some();
            let expected = matches!(a.id(), "pi" | "cursor");
            assert_eq!(has, expected, "{} list_models_args", a.id());
        }
    }

    #[test]
    fn version_probe_uses_the_default_version_flag() {
        assert_eq!(PiAdapter.version_args(), vec!["--version"]);
        assert_eq!(PiAdapter.parse_version("pi 0.9.3"), Some("0.9.3".into()));
    }
}
