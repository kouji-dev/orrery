//! `--provider` reaches every provider the config route reaches.
//!
//! Round 5 left `--provider` parsing exactly one prefix, `fixture:`, while
//! `orrery_harness::config::provider_choice` could name Anthropic and any
//! OpenAI-compatible endpoint from a `[provider]` table. Two routes to the same
//! enum, one of them three variants short, is how a flag comes to lie about
//! what the build can do.
//!
//! **Nothing here reaches the network.** Both real providers are behind cargo
//! features that are off by default, so what these tests assert is that the
//! flag *parses* and *selects*: the failure a default build gives is "this
//! build has no `anthropic` provider", which is the provider being chosen and
//! then found absent — not the flag refusing to understand the word.

mod common;

use common::{FINAL_TAIL, args, base, home_with, orrery, orrery_in, quiet, stream, workspace};

/// Every kind `provider_choice` can name is a kind the flag can name.
///
/// The assertion is deliberately about *which* refusal comes back. A default
/// build has neither TLS-linking provider compiled in, so the honest answer is
/// the one that names the cargo feature. The answer that must **not** come back
/// is the old one, which said the word itself was not a provider.
#[test]
fn the_flag_reaches_every_kind_the_config_route_reaches() {
    let dir = workspace();
    for (spec, feature) in [
        ("anthropic:claude-sonnet-4-5", "anthropic"),
        ("anthropic:claude-sonnet-4-5@http://127.0.0.1:1", "anthropic"),
        ("openai-compat:llama3@http://127.0.0.1:1/v1", "openai-compat"),
        ("ollama:llama3", "openai-compat"),
        ("vllm:llama3", "openai-compat"),
    ] {
        let out = orrery(&args(
            &["--workspace".to_owned(), dir.path().display().to_string()],
            &["--provider", spec, "run", "-p", "hello"],
        ));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("is not a provider"),
            "`{spec}` was refused by the parser rather than selected: {stderr}"
        );
        assert!(
            stderr.contains(feature),
            "`{spec}` should fail naming the `{feature}` feature, not: {stderr}"
        );
    }
}

/// A word nothing answers to is a usage error that says what is understood.
#[test]
fn an_unknown_kind_lists_the_forms() {
    let dir = workspace();
    let out = orrery(&args(
        &["--workspace".to_owned(), dir.path().display().to_string()],
        &["--provider", "wishful:gpt-9", "run", "-p", "hello"],
    ));
    assert_eq!(out.status.code(), Some(2), "the person typed it");
    let stderr = String::from_utf8_lossy(&out.stderr);
    for form in ["fixture:", "anthropic:", "openai-compat:", "ollama:", "vllm:"] {
        assert!(stderr.contains(form), "`{form}` missing from: {stderr}");
    }
}

/// `fixture:` still takes one stream per pass, in order.
#[test]
fn fixture_passes_still_accumulate() {
    let dir = workspace();
    let out = orrery(&args(
        &base(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]),
        &["run", "-p", "what is in Cargo.toml?"],
    ));
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains(FINAL_TAIL));
}

/// Two kinds in one invocation is a usage error rather than a silent winner.
#[test]
fn two_kinds_at_once_is_refused() {
    let dir = workspace();
    let out = orrery(&args(
        &["--workspace".to_owned(), dir.path().display().to_string()],
        &[
            "--provider",
            &format!("fixture:{}", stream("text-turn.jsonl").display()),
            "--provider",
            "anthropic:claude-sonnet-4-5",
            "run",
            "-p",
            "hello",
        ],
    ));
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("one kind"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// With no flag at all, a `[provider]` table starts the session.
///
/// This is the half of the gap that was not about parsing: `Session::build`
/// refused outright unless `--provider fixture:` had been passed, so the
/// config route could name a provider the CLI would never build. The table
/// lives in the **user** layer, which needs no trust decision.
#[test]
fn a_configured_provider_starts_without_the_flag() {
    let dir = workspace();
    let home = home_with(&format!(
        "[provider]\nkind = \"fixture\"\npasses = [{path:?}]\n",
        path = stream("text-turn.jsonl").display().to_string()
    ));
    let out = orrery_in(
        home.path(),
        &args(&quiet(dir.path()), &["run", "-p", "what is here?"]),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(FINAL_TAIL),
        "the configured fixture ran the turn: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// With neither a flag nor a table, the message says both ways in.
#[test]
fn with_nothing_named_the_refusal_names_both_routes() {
    let dir = workspace();
    let home = home_with("");
    let out = orrery_in(
        home.path(),
        &args(&quiet(dir.path()), &["run", "-p", "hello"]),
    );
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--provider"), "{stderr}");
    assert!(stderr.contains("[provider]"), "{stderr}");
}
