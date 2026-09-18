//! Task 6 · profiles. The phase-5 criterion: two profiles produce measurably
//! different agents from one binary.

mod common;

use common::Fixture;
use orrery_config::{StartupCtx, resolve};
use orrery_policy::{ConsentMode, PendingCall, Verdict};
use orrery_proto::{AgentScope, BranchId, Grant, Subject};

const CONFIG: &str = r#"
[permissions]
allow = ["tool(*)", "read(./**)", "write(./**)"]
ask = ["spawn(*)"]

[profile.review]
model = "claude-sonnet-5"
extensions = ["git", "lsp", "buildgraph"]
interceptors = ["no-write-outside-diff"]
subagents = ["critic"]
skills = ["review-checklist"]
permissions = { write = false }

[profile.ci]
model = "local/qwen-coder"
extensions = ["git", "test-runner"]
consent = "never"
audit = { sink = "otlp://collector.internal" }

[agents.reviewer]
prompt = "Review the diff for correctness and test coverage. Do not edit."
model = "local/qwen-coder"
tools = ["git.*", "lsp.*"]
budget = { maxTurns = 4, maxTokens = 60000, wallClockMs = 120000 }
"#;

fn scope() -> AgentScope {
    AgentScope {
        agent: "test".to_owned(),
        branch: BranchId::new(),
        tools: vec!["*".to_owned()],
        grant: Grant::nothing(),
    }
}

fn fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("home/.orrery/config.toml", CONFIG);
    fx
}

fn assembled(fx: &Fixture, profile: &str) -> orrery_config::Assembled {
    let cfg = resolve(&StartupCtx::new(fx.paths()).with_profile(profile)).expect("resolve");
    cfg.assemble().expect("the profile assembles")
}

#[test]
fn two_profiles_differ_measurably() {
    let fx = fixture();
    let review = assembled(&fx, "review");
    let ci = assembled(&fx, "ci");

    // Different models.
    assert_eq!(review.model.as_deref(), Some("claude-sonnet-5"));
    assert_eq!(ci.model.as_deref(), Some("local/qwen-coder"));
    assert_ne!(review.model, ci.model);

    // Different visible tool sets. Not a suggestion: a tool that is not named
    // here is not offered to the model at all.
    assert_eq!(review.tools, vec!["git.*", "lsp.*", "buildgraph.*"]);
    assert_eq!(ci.tools, vec!["git.*", "test-runner.*"]);
    assert_ne!(review.tools, ci.tools);

    // `review` cannot write.
    let write = PendingCall::write("./src/main.rs");
    assert_eq!(
        review.engine.check(&write, &Subject::Agent, &scope()).verdict(),
        Verdict::Deny,
        "review denies writing"
    );
    assert_eq!(
        ci.engine.check(&write, &Subject::Agent, &scope()).verdict(),
        Verdict::Allow,
        "ci still writes"
    );

    // `ci` never prompts: an ask resolves to its fallback with nobody present.
    let spawn = PendingCall::spawn("cargo test");
    assert_eq!(
        review.engine.check(&spawn, &Subject::Agent, &scope()).verdict(),
        Verdict::Ask,
        "review asks a human"
    );
    assert_eq!(
        ci.engine.check(&spawn, &Subject::Agent, &scope()).verdict(),
        Verdict::Deny,
        "ci has no human, so it refuses instead of hanging"
    );

    // And the rest of the composition came across.
    assert_eq!(review.profile.interceptors, vec!["no-write-outside-diff"]);
    assert_eq!(review.profile.subagents, vec!["critic"]);
    assert_eq!(review.profile.skills, vec!["review-checklist"]);
    assert_eq!(ci.profile.audit_sink.as_deref(), Some("otlp://collector.internal"));
}

#[test]
fn consent_never_denies_instead_of_prompting() {
    let fx = fixture();
    assert_eq!(assembled(&fx, "review").consent, ConsentMode::Ask);
    assert_eq!(assembled(&fx, "ci").consent, ConsentMode::Never);
}

#[test]
fn agent_bindings_and_budgets_parse() {
    let fx = fixture();
    let cfg = resolve(&StartupCtx::new(fx.paths())).expect("resolve");
    let reviewer = cfg.profile.agents.get("reviewer").expect("the agent is bound");
    assert_eq!(reviewer.model.as_deref(), Some("local/qwen-coder"));
    assert_eq!(reviewer.tools, vec!["git.*", "lsp.*"]);
    let budget = reviewer.budget.expect("a budget");
    assert_eq!(budget.max_turns, 4);
    assert_eq!(budget.max_tokens, 60_000);
    assert_eq!(budget.wall_clock_ms, 120_000);
}

#[test]
fn an_unknown_profile_is_an_error_not_a_silent_default() {
    let fx = fixture();
    let err = resolve(&StartupCtx::new(fx.paths()).with_profile("nope"))
        .expect_err("it does not quietly run something else");
    assert!(err.to_string().contains("nope"), "{err}");
}

#[test]
fn an_unknown_permission_shorthand_names_the_file_and_line() {
    let fx = Fixture::new();
    fx.write(
        "home/.orrery/config.toml",
        "[profile.odd]\npermissions = { teleport = false }\n",
    );
    let err = resolve(&StartupCtx::new(fx.paths()).with_profile("odd")).expect_err("refused");
    assert!(err.to_string().contains("teleport"), "{err}");
    assert_eq!(err.line(), Some(2), "{err}");
}
