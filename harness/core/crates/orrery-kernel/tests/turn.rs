//! Plan 05, Tasks 3, 4 and 9: context assembly, the loop, and the auth gate.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::{Passes, Rig, SignedOut, TestHost, fixture, registry, write_stream};
use orrery_kernel::{
    ContextBuild, ContextDraft, InterceptCtx, Interceptor, Kernel, KernelConfig, MemoryRecall,
    NoCompactor, TurnInput, TurnOutcome,
};
use orrery_proto::{BudgetKind, Outcome, TokenBudget, UserInput, Verdict};
use orrery_provider::Capabilities;
use orrery_session::RecalledEntry;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

/// Watches `context.build` and keeps every draft it saw.
///
/// The observation goes through the phase it is observing: a rewrite-only
/// interceptor is exactly what a memory provider or a prompt tweak would be, so
/// the test rig is the mechanism under test.
#[derive(Default)]
struct Watcher(Arc<Mutex<Vec<ContextDraft>>>);

impl Watcher {
    fn new() -> (Self, Arc<Mutex<Vec<ContextDraft>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        (Self(seen.clone()), seen)
    }
}

impl Interceptor<ContextBuild> for Watcher {
    fn can_deny(&self) -> bool {
        false
    }

    fn run(&self, _ctx: &InterceptCtx<'_>, payload: &ContextDraft) -> Verdict<ContextDraft> {
        self.0.lock().push(payload.clone());
        Verdict::Continue
    }
}

/// A memory provider with one thing to say.
struct OneMemory;

#[async_trait]
impl MemoryRecall for OneMemory {
    fn name(&self) -> &str {
        "notes"
    }

    async fn recall(&self, _input: &UserInput, _budget: TokenBudget) -> Vec<RecalledEntry> {
        vec![RecalledEntry {
            key: "workspace".to_owned(),
            text: "the user prefers tabs".to_owned(),
            score: Some(0.9),
        }]
    }
}

/// A compactor that answers, and counts.
struct Shrinking(Arc<Mutex<u32>>);

#[async_trait]
impl orrery_kernel::Compactor for Shrinking {
    async fn compact(
        &self,
        plan: &orrery_kernel::CompactPlan,
        _cancel: &CancellationToken,
    ) -> Result<orrery_kernel::Compacted, orrery_kernel::CompactError> {
        *self.0.lock() += 1;
        Ok(orrery_kernel::Compacted {
            summary: format!("(summary of {} messages)", plan.messages.len()),
            usage: orrery_proto::Usage {
                input_tokens: 10,
                output_tokens: 2,
                ..orrery_proto::Usage::default()
            },
        })
    }
}

/// A compactor that answers and does not help: the summary is as long as what
/// it replaced, so the rebuild still does not fit.
struct Useless(Arc<Mutex<u32>>);

#[async_trait]
impl orrery_kernel::Compactor for Useless {
    async fn compact(
        &self,
        plan: &orrery_kernel::CompactPlan,
        _cancel: &CancellationToken,
    ) -> Result<orrery_kernel::Compacted, orrery_kernel::CompactError> {
        *self.0.lock() += 1;
        let _ = plan;
        // Fixed, and far longer than the window: a summary whose length
        // depended on what it summarised would get shorter each round and
        // eventually fit, which is the opposite of what this test is for.
        Ok(orrery_kernel::Compacted {
            summary: "still far too long to fit in this window. ".repeat(200),
            usage: orrery_proto::Usage::default(),
        })
    }
}

fn kernel(rig: &Rig, provider: Arc<dyn orrery_provider::Provider>, host: Arc<TestHost>) -> Kernel {
    Kernel::new(
        rig.store.clone(),
        provider,
        Arc::new(registry(host)),
        KernelConfig::default(),
    )
}

async fn run(rig: &Rig, kernel: &Kernel) -> TurnOutcome {
    kernel
        .run_turn(
            rig.lease().await,
            TurnInput::new(rig.session, UserInput::text("go"), rig.scope()),
            CancellationToken::new(),
        )
        .await
        .expect("the harness carried the turn")
}

// ---------------------------------------------------------------- Task 4 ---

/// A text-only stream completes, with usage, and appends one assistant turn.
#[tokio::test]
async fn text_only_completes() {
    let rig = Rig::open().await;
    let kernel = kernel(
        &rig,
        Passes::repeating(fixture("text-turn.jsonl")),
        TestHost::echoing(),
    );

    let outcome = run(&rig, &kernel).await;
    match outcome {
        TurnOutcome::Completed { usage, text, .. } => {
            assert!(text.contains("three crates"), "the model's answer: {text}");
            assert_eq!(usage.input_tokens, 812);
            assert_eq!(usage.output_tokens, 11);
        }
        other => panic!("a text-only stream completes: {other:?}"),
    }
    assert_eq!(rig.transcript().await, vec!["user", "assistant"]);
}

/// One tool call: it is dispatched, a `ToolResult` turn is appended, a second
/// pass runs, and the turn completes.
#[tokio::test]
async fn tool_call_round_trips() {
    let rig = Rig::open().await;
    let host = TestHost::echoing();
    let provider = Passes::of(vec![fixture("tool-call.jsonl"), fixture("text-turn.jsonl")]);
    let kernel = kernel(&rig, provider.clone(), host.clone());

    let outcome = run(&rig, &kernel).await;
    assert!(
        matches!(outcome, TurnOutcome::Completed { .. }),
        "{outcome:?}"
    );
    assert_eq!(provider.served(), 2, "two passes");
    assert_eq!(
        host.recorder.calls()[0].0,
        "builtin.read",
        "the tool the model named was the tool that ran"
    );
    assert_eq!(
        rig.shaped().await,
        vec![
            "user",
            "assistant(text+tool_use)",
            "tool_result",
            "assistant(text)"
        ],
        "the transcript is the acceptance criterion's shape"
    );
}

/// A denied call is appended as a `ToolResult` holding `Outcome::Denied`, and
/// the loop carries on rather than raising.
#[tokio::test]
async fn denial_is_appended_not_raised() {
    struct NoTools;
    impl orrery_tools::PolicyCheck for NoTools {
        fn check(
            &self,
            _ref: &orrery_proto::ToolRef,
            _input: &serde_json::Value,
            _ctx: &orrery_tools::CallCtx,
        ) -> orrery_tools::PolicyDecision {
            orrery_tools::PolicyDecision::Deny {
                rule: "00000000-0000-0000-0000-000000000000".parse().unwrap(),
                reason: "reading is not allowed in this profile".to_owned(),
            }
        }
    }

    let rig = Rig::open().await;
    let host = TestHost::echoing();
    let registry = registry(host.clone()).with_policy(Arc::new(NoTools));
    let kernel = Kernel::new(
        rig.store.clone(),
        Passes::of(vec![fixture("tool-call.jsonl"), fixture("text-turn.jsonl")]),
        Arc::new(registry),
        KernelConfig::default(),
    );

    let outcome = run(&rig, &kernel).await;
    assert!(
        matches!(outcome, TurnOutcome::Completed { .. }),
        "a denial is not the end of the turn: {outcome:?}"
    );
    let rows = rig.rows().await;
    assert!(
        rows.iter().any(|r| common::is_denied(&r.kind)),
        "the denial is in the tree as a value"
    );
    assert!(host.recorder.is_empty());
}

/// The pass is in the tree before the next one begins, so a harness killed
/// between passes resumes instead of restarting.
#[tokio::test]
async fn appends_before_next_pass() {
    let rig = Rig::open().await;
    let seen = Arc::new(Mutex::new(Vec::new()));

    // A tool host that reads the tree back *while* the first pass's rows are
    // the only ones written. If the append happened after the loop, this would
    // see nothing.
    struct Peeking {
        rig_rows: Arc<Mutex<Vec<String>>>,
        store: Arc<dyn orrery_session::SessionStore>,
        branch: orrery_proto::BranchId,
    }

    #[async_trait]
    impl orrery_tools::ToolHost for Peeking {
        async fn call(
            &self,
            _ref: &orrery_proto::ToolRef,
            input: serde_json::Value,
            _ctx: &orrery_tools::CallCtx,
        ) -> Result<Outcome, orrery_tools::ToolError> {
            let view = self
                .store
                .materialise(
                    self.branch,
                    TokenBudget {
                        max: u64::MAX,
                        reserve: 0,
                    },
                    &orrery_session::CharsOverFour,
                )
                .await
                .expect("the branch materialises");
            *self.rig_rows.lock() = view
                .messages
                .iter()
                .map(|m| format!("{:?}", m.role))
                .collect();
            Ok(Outcome::Ok {
                surface: None,
                value: Some(input),
            })
        }
    }

    let host = Arc::new(Peeking {
        rig_rows: seen.clone(),
        store: rig.store.clone(),
        branch: rig.branch,
    });
    let mut reg = orrery_tools::Registry::with_host(host);
    reg.register(
        &"builtin".parse().unwrap(),
        orrery_proto::Layer::Project,
        orrery_tools::ToolSpec::new("read"),
    );
    let kernel = Kernel::new(
        rig.store.clone(),
        Passes::of(vec![fixture("tool-call.jsonl"), fixture("text-turn.jsonl")]),
        Arc::new(reg),
        KernelConfig::default(),
    );
    run(&rig, &kernel).await;

    assert_eq!(
        *seen.lock(),
        vec!["User".to_owned(), "Assistant".to_owned()],
        "the assistant turn was already in the tree when the tool ran"
    );
}

// ---------------------------------------------------------------- Task 3 ---

/// The stable prefix is first: the recalled block comes after the tool
/// descriptors, and the breakpoint points past them.
#[tokio::test]
async fn stable_prefix_is_first() {
    let rig = Rig::open().await;
    let (watcher, seen) = Watcher::new();
    let mut set = orrery_kernel::InterceptorSet::new();
    set.register::<ContextBuild>(watcher).expect("registers");

    let kernel = Kernel::new(
        rig.store.clone(),
        Passes::repeating(fixture("text-turn.jsonl")),
        Arc::new(registry(TestHost::echoing())),
        KernelConfig::default(),
    )
    .with_interceptors(Arc::new(set))
    .with_memory(Arc::new(OneMemory));

    run(&rig, &kernel).await;

    let drafts = seen.lock();
    let draft = drafts.first().expect("one context was built");
    let tools_at = draft
        .section_index("tools")
        .expect("the descriptors are a section of the system prompt");
    assert!(
        draft.cache_breakpoint > tools_at,
        "the breakpoint ({}) must point past the descriptors ({tools_at})",
        draft.cache_breakpoint
    );
    assert_eq!(draft.recalled.len(), 1, "memory contributed");
    let messages = draft.messages();
    assert!(
        format!("{:?}", messages[0]).contains("prefers tabs"),
        "recalled memory opens the volatile suffix, after the whole prefix"
    );
    assert_eq!(
        draft.message_breakpoint(),
        messages.len() - 1,
        "everything but this pass's newest message is cacheable"
    );
}

/// Building twice with the same scope produces byte-identical prefixes. This is
/// the test that catches a `HashMap` sneaking into the registry.
#[tokio::test]
async fn tool_order_is_stable() {
    let mut prefixes = Vec::new();
    for _ in 0..2 {
        let rig = Rig::open().await;
        let (watcher, seen) = Watcher::new();
        let mut set = orrery_kernel::InterceptorSet::new();
        set.register::<ContextBuild>(watcher).expect("registers");

        let mut reg = orrery_tools::Registry::with_host(TestHost::echoing());
        for name in ["read", "write", "edit", "bash", "grep", "glob"] {
            reg.register(
                &"builtin".parse().unwrap(),
                orrery_proto::Layer::Project,
                orrery_tools::ToolSpec::new(name).described("a tool"),
            );
        }
        for name in ["status", "diff"] {
            reg.register(
                &"git".parse().unwrap(),
                orrery_proto::Layer::User,
                orrery_tools::ToolSpec::new(name).described("a git tool"),
            );
        }

        let kernel = Kernel::new(
            rig.store.clone(),
            Passes::repeating(fixture("text-turn.jsonl")),
            Arc::new(reg),
            KernelConfig::default(),
        )
        .with_interceptors(Arc::new(set));
        run(&rig, &kernel).await;

        let drafts = seen.lock();
        prefixes.push(drafts[0].system_text());
    }
    assert_eq!(
        prefixes[0], prefixes[1],
        "the same registrations must produce the same bytes, or provider caching dies"
    );
    assert!(prefixes[0].contains("## read"));
}

/// A provider without tools is never sent any.
#[tokio::test]
async fn no_tools_when_provider_lacks_them() {
    let rig = Rig::open().await;
    let (watcher, seen) = Watcher::new();
    let mut set = orrery_kernel::InterceptorSet::new();
    set.register::<ContextBuild>(watcher).expect("registers");

    let provider = Passes::repeating(fixture("text-turn.jsonl")).with_capabilities(Capabilities {
        tools: false,
        images: false,
        cache: false,
        max_context: 200_000,
        max_output: 4_096,
    });
    let kernel = Kernel::new(
        rig.store.clone(),
        provider,
        Arc::new(registry(TestHost::echoing())),
        KernelConfig::default(),
    )
    .with_interceptors(Arc::new(set));
    run(&rig, &kernel).await;

    let drafts = seen.lock();
    assert!(
        drafts[0].tools.is_empty(),
        "a provider that cannot take tools is not shown any"
    );
    assert_eq!(
        drafts[0]
            .section_index("tools")
            .map(|i| drafts[0].system[i].text.clone()),
        Some(String::new()),
        "and the section it would have filled is empty rather than absent"
    );
}

/// A context that does not fit is compacted once, and the rebuild fits.
#[tokio::test]
async fn overflow_triggers_compact_then_rebuild() {
    let rig = Rig::open().await;
    let compactions = Arc::new(Mutex::new(0u32));

    // Something long enough that a tiny window cannot hold it.
    let lease = rig.lease().await;
    for i in 0..6 {
        rig.store
            .append(
                &lease,
                orrery_session::NewTurn::new(orrery_session::TurnKind::User {
                    input: UserInput::text(format!("{i}: {}", "a fairly long message ".repeat(40))),
                }),
            )
            .await
            .expect("the history is written");
    }
    drop(lease);

    let provider = Passes::repeating(fixture("text-turn.jsonl")).with_capabilities(Capabilities {
        tools: true,
        images: false,
        cache: false,
        max_context: 400,
        max_output: 64,
    });
    let kernel = Kernel::new(
        rig.store.clone(),
        provider,
        Arc::new(registry(TestHost::echoing())),
        KernelConfig {
            context_reserve_tokens: 0,
            system_prompt: "s".to_owned(),
            ..KernelConfig::default()
        },
    )
    .with_compactor(Arc::new(Shrinking(compactions.clone())));

    let outcome = run(&rig, &kernel).await;
    assert_eq!(*compactions.lock(), 1, "compaction ran once");
    assert!(
        matches!(outcome, TurnOutcome::Completed { .. }),
        "the rebuild fits and the turn runs: {outcome:?}"
    );
    assert!(
        rig.transcript().await.contains(&"summary".to_owned()),
        "the summary is a row like any other, and the originals are still there"
    );
}

/// After the second attempt, the turn stops with `Tokens` rather than looping.
#[tokio::test]
async fn compaction_gives_up() {
    let rig = Rig::open().await;
    let attempts = Arc::new(Mutex::new(0u32));

    let lease = rig.lease().await;
    for i in 0..6 {
        rig.store
            .append(
                &lease,
                orrery_session::NewTurn::new(orrery_session::TurnKind::User {
                    input: UserInput::text(format!("{i}: {}", "a fairly long message ".repeat(40))),
                }),
            )
            .await
            .expect("the history is written");
    }
    drop(lease);

    let provider = Passes::repeating(fixture("text-turn.jsonl")).with_capabilities(Capabilities {
        tools: true,
        images: false,
        cache: false,
        max_context: 200,
        max_output: 64,
    });
    let kernel = Kernel::new(
        rig.store.clone(),
        provider.clone(),
        Arc::new(registry(TestHost::echoing())),
        KernelConfig {
            context_reserve_tokens: 0,
            system_prompt: "s".to_owned(),
            ..KernelConfig::default()
        },
    )
    .with_compactor(Arc::new(Useless(attempts.clone())));

    let outcome = run(&rig, &kernel).await;
    assert_eq!(
        *attempts.lock(),
        2,
        "two attempts, and then it stops trying"
    );
    match outcome {
        TurnOutcome::StoppedByBudget { kind, .. } => assert_eq!(kind, BudgetKind::Tokens),
        other => panic!("giving up is a budget stop, not a loop or an error: {other:?}"),
    }
    assert_eq!(provider.served(), 0, "nothing was ever sent");
}

/// With no compactor at all, the same thing happens without a model call.
#[tokio::test]
async fn no_compactor_stops_on_tokens() {
    let rig = Rig::open().await;
    let lease = rig.lease().await;
    rig.store
        .append(
            &lease,
            orrery_session::NewTurn::new(orrery_session::TurnKind::User {
                input: UserInput::text("x".repeat(4_000)),
            }),
        )
        .await
        .expect("the history is written");
    drop(lease);

    let provider = Passes::repeating(fixture("text-turn.jsonl")).with_capabilities(Capabilities {
        tools: true,
        images: false,
        cache: false,
        max_context: 100,
        max_output: 16,
    });
    let kernel = Kernel::new(
        rig.store.clone(),
        provider,
        Arc::new(registry(TestHost::echoing())),
        KernelConfig {
            context_reserve_tokens: 0,
            ..KernelConfig::default()
        },
    )
    .with_compactor(Arc::new(NoCompactor));

    match run(&rig, &kernel).await {
        TurnOutcome::StoppedByBudget { kind, .. } => assert_eq!(kind, BudgetKind::Tokens),
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------- Task 9 ---

/// A provider reporting `NeedsLogin` refuses the turn before any context is
/// built, and nothing is appended.
#[tokio::test]
async fn needs_login_is_refused_early() {
    let rig = Rig::open().await;
    let (watcher, seen) = Watcher::new();
    let mut set = orrery_kernel::InterceptorSet::new();
    set.register::<ContextBuild>(watcher).expect("registers");

    let provider = SignedOut::over(Passes::repeating(fixture("text-turn.jsonl")));
    let kernel = Kernel::new(
        rig.store.clone(),
        provider.clone(),
        Arc::new(registry(TestHost::echoing())),
        KernelConfig::default(),
    )
    .with_interceptors(Arc::new(set));

    match run(&rig, &kernel).await {
        TurnOutcome::NeedsLogin { reason } => assert!(reason.contains("api key"), "{reason}"),
        other => panic!("a signed-out provider refuses the turn: {other:?}"),
    }
    assert!(seen.lock().is_empty(), "no context was built");
    assert_eq!(provider.streamed(), 0, "nothing was sent");
    assert!(
        rig.transcript().await.is_empty(),
        "and nothing was appended"
    );
}

/// A model that emits an unknown tool name is told so, as a value, and the loop
/// carries on.
#[tokio::test]
async fn an_unknown_tool_settles_rather_than_raising() {
    let rig = Rig::open().await;
    let dir = tempfile::tempdir().unwrap();
    let asking = write_stream(
        dir.path(),
        "unknown-tool.jsonl",
        &[
            r#"{"t":"started","id":"msg_unknown"}"#,
            r#"{"t":"tool-use-start","call":"0192f3a0-0000-7000-8000-0000000000aa","name":"builtin.raed"}"#,
            r#"{"t":"tool-use-end","call":"0192f3a0-0000-7000-8000-0000000000aa"}"#,
            r#"{"t":"done","stop":"tool-use"}"#,
        ],
    );
    let host = TestHost::echoing();
    let kernel = kernel(
        &rig,
        Passes::of(vec![asking, fixture("text-turn.jsonl")]),
        host.clone(),
    );

    let outcome = run(&rig, &kernel).await;
    assert!(
        matches!(outcome, TurnOutcome::Completed { .. }),
        "{outcome:?}"
    );
    assert!(host.recorder.is_empty());
    let rows = rig.rows().await;
    match common::first_outcome(&rows).expect("the unanswered tool_use got an answer") {
        Outcome::Failed { code, message } => {
            assert_eq!(code, "no-such-tool");
            assert!(message.contains("read"), "and a suggestion: {message}");
        }
        other => panic!("{other:?}"),
    }
}
