//! Plan 06, Task 4: the builtin bundle, against the broker it really runs on.
//!
//! These four are tests **of the broker**: an output ceiling that bounds peak
//! memory, a write that reverts on cancel, a child whose grandchild dies with
//! it. They live here rather than in `orrery-ext-tools-builtin` because
//! `deps-check` rule 3 forbids an extension crate from dev-depending on an
//! unpublished core crate, and `orrery-broker` is exactly that. Which turns out
//! better: the facade under test is [`PolicyBroker`], the one the harness wires
//! in production, rather than a rig written for the tests.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use orrery_broker::LocalBroker;
use orrery_ext_api::{CallCtx, NativeExtension, ToolBudget as ExtBudget};
use orrery_ext_tools_builtin::BuiltinTools;
use orrery_harness::{DEFAULT_RULES, PolicyBroker};
use orrery_policy::{PolicyBuilder, PolicyEngine};
use orrery_proto::{
    AgentScope, Aspect, BranchId, CallId, Capability, Consent, Grant, Layer, Outcome, Subject,
};
use orrery_tools::ToolBudget;
use tokio_util::sync::CancellationToken;

/// 4 KB, the ceiling every bounded test uses.
const CEILING: u64 = 4 * 1024;

/// What `LimitedReader` may pull past the ceiling: one chunk.
const CHUNK: u64 = 8 * 1024;

/// A workspace, a real engine, a real broker, and the facade a tool sees.
struct Rig {
    dir: tempfile::TempDir,
    broker: Arc<PolicyBroker>,
    budget: ToolBudget,
}

impl Rig {
    fn open(budget: ToolBudget) -> Self {
        Self::with_rules(budget, DEFAULT_RULES)
    }

    /// The same, with rules narrower than the default set.
    fn with_rules(budget: ToolBudget, rules: &str) -> Self {
        let dir = tempfile::tempdir().expect("a temporary workspace");
        let rules = PolicyBuilder::new(dir.path())
            .layer_toml(rules, "orrery.toml", Layer::Project, true)
            .expect("the default rules parse")
            .build()
            .expect("the default rules compile");
        let engine = Arc::new(PolicyEngine::new(rules));
        let local = Arc::new(LocalBroker::new(engine.ledger().clone()));
        let scope = AgentScope {
            agent: "main".to_owned(),
            branch: BranchId::new(),
            tools: vec!["*".to_owned()],
            grant: Grant {
                capabilities: vec![
                    Capability::all(Aspect::Tool),
                    Capability::all(Aspect::Read),
                    Capability::all(Aspect::Write),
                    Capability::all(Aspect::Spawn),
                ],
                consent: Consent::Always,
            },
        };
        let broker = PolicyBroker::new(engine, local, dir.path(), Subject::Agent, scope, budget);
        Self {
            dir,
            broker,
            budget,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// A call context over the session-wide facade.
    fn ctx(&self, tool: &str) -> CallCtx {
        self.ctx_with(tool, CancellationToken::new())
    }

    /// The same, tied to a call that can be cancelled.
    fn ctx_with(&self, tool: &str, cancel: CancellationToken) -> CallCtx {
        let call = CallId::new();
        CallCtx::new(
            call,
            "builtin".parse().expect("`builtin` is an ext id"),
            tool,
            ExtBudget {
                wall_clock_ms: self.budget.wall_clock_ms,
                output_bytes: self.budget.output_bytes,
                memory_bytes: self.budget.memory_bytes,
            },
            cancel.clone(),
            self.broker.for_call(call, cancel),
        )
    }
}

fn text_of(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Ok { value, .. } => value
            .as_ref()
            .and_then(|v| v.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_owned(),
        other => panic!("expected content, got {other:?}"),
    }
}

/// The `contain_probe` example, built beside the test binary.
fn probe_exe() -> PathBuf {
    let mut dir = std::env::current_exe().expect("a test binary knows where it is");
    dir.pop();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir.join("examples")
        .join(format!("contain_probe{}", std::env::consts::EXE_SUFFIX))
}

/// A 10 MB file under a 4 KB ceiling comes back `Truncated` — and, the half
/// that matters, **nothing ever pulled more than the ceiling plus one buffer**.
/// A limit applied after the read has already put 10 MB in the heap.
#[tokio::test]
async fn read_respects_the_output_ceiling() {
    let rig = Rig::open(ToolBudget::new(30_000, CEILING));
    let big = rig.path("big.txt");
    std::fs::write(&big, "x".repeat(10 * 1024 * 1024)).unwrap();

    let outcome = BuiltinTools::new()
        .call(
            "read",
            serde_json::json!({ "path": "big.txt" }),
            &rig.ctx("read"),
        )
        .await
        .expect("the harness carried the call");

    match &outcome {
        Outcome::Truncated {
            bytes_emitted,
            limit,
            ..
        } => {
            assert_eq!(*limit, CEILING);
            assert!(*bytes_emitted <= CEILING, "emitted {bytes_emitted} bytes");
        }
        other => panic!("a 10 MB file under a 4 KB ceiling must truncate, got {other:?}"),
    }

    let peak = rig.broker.peak_pulled();
    assert!(
        peak <= CEILING + CHUNK,
        "peak memory is not bounded: {peak} bytes were pulled for a {CEILING}-byte ceiling"
    );
}

/// Cancel while the bytes are going out: the original file is exactly as it
/// was, and no temporary file is left beside it.
#[tokio::test]
async fn write_is_atomic() {
    let rig = Rig::open(ToolBudget::new(30_000, 64 << 20));
    let target = rig.path("kept.txt");
    std::fs::write(&target, "the original").unwrap();

    let cancel = CancellationToken::new();
    let ctx = rig.ctx_with("write", cancel.clone());
    let content = "y".repeat(8 * 1024 * 1024);
    let call = tokio::spawn(async move {
        BuiltinTools::new()
            .call(
                "write",
                serde_json::json!({ "path": "kept.txt", "content": content }),
                &ctx,
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(15)).await;
    cancel.cancel();

    let outcome = call.await.unwrap().expect("the harness carried the call");
    assert!(
        matches!(outcome, Outcome::Cancelled { .. }),
        "a cancelled write settles cancelled, got {outcome:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "the original",
        "the original must survive a cancelled write untouched"
    );
    let strays: Vec<String> = std::fs::read_dir(rig.dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".orrery-") || n.ends_with(".tmp"))
        .collect();
    assert!(
        strays.is_empty(),
        "a temporary file was left behind: {strays:?}"
    );
}

/// A child that starts a grandchild and then ignores everything: when the
/// call's wall clock runs out, **both** are gone. A `Child::kill` would reach
/// only the first of them.
#[tokio::test]
async fn bash_is_contained() {
    let rig = Rig::open(ToolBudget::new(700, 1 << 20));
    let beacon = rig.path("alive.log");
    let probe = probe_exe();
    assert!(
        probe.exists(),
        "the contain_probe example must be built beside the test: {}",
        probe.display()
    );

    // Unquoted on purpose: `cmd /C` eats the outer pair of quotes in a command
    // line that begins with one, and neither path here contains a space.
    let command = format!(
        "{probe} spawn {beacon}",
        probe = probe.display(),
        beacon = beacon.display()
    );
    let outcome = BuiltinTools::new()
        .call(
            "bash",
            serde_json::json!({ "command": command }),
            &rig.ctx("bash"),
        )
        .await
        .expect("the harness carried the call");
    assert!(
        !matches!(outcome, Outcome::Denied { .. }),
        "spawn is granted by the default rules, got {outcome:?}"
    );

    tokio::time::sleep(Duration::from_millis(700)).await;
    let first = std::fs::metadata(&beacon).map(|m| m.len()).unwrap_or(0);
    assert!(
        first > 0,
        "the grandchild never ran, so nothing was contained; the call said {outcome:?}"
    );
    tokio::time::sleep(Duration::from_millis(700)).await;
    let second = std::fs::metadata(&beacon).map(|m| m.len()).unwrap_or(0);
    assert_eq!(
        first, second,
        "the grandchild outlived the call: the beacon grew from {first} to {second}"
    );
}

/// Searching a tree pulls no more than the ceiling out of any one file.
#[tokio::test]
async fn grep_streams() {
    let rig = Rig::open(ToolBudget::new(30_000, CEILING));
    std::fs::create_dir_all(rig.path("src")).unwrap();
    std::fs::write(
        rig.path("src/small.txt"),
        "a needle in here\nand nothing else\n",
    )
    .unwrap();
    std::fs::write(
        rig.path("src/huge.txt"),
        format!("{}\nneedle at the end\n", "z".repeat(10 * 1024 * 1024)),
    )
    .unwrap();

    let outcome = BuiltinTools::new()
        .call(
            "grep",
            serde_json::json!({
                "pattern": "needle",
                "path": rig.dir.path().display().to_string(),
            }),
            &rig.ctx("grep"),
        )
        .await
        .expect("the harness carried the call");

    let text = text_of(&outcome);
    assert!(
        text.contains("small.txt"),
        "the match in the small file must be reported: {text}"
    );
    let peak = rig.broker.peak_pulled();
    assert!(
        peak <= CEILING + CHUNK,
        "grep buffered a whole file: {peak} bytes for a {CEILING}-byte ceiling"
    );
}

/// `glob` lists what matches and nothing else.
#[tokio::test]
async fn glob_lists_matches_only() {
    let rig = Rig::open(ToolBudget::new(30_000, CEILING));
    std::fs::create_dir_all(rig.path("src")).unwrap();
    std::fs::write(rig.path("src/a.rs"), "").unwrap();
    std::fs::write(rig.path("src/b.txt"), "").unwrap();

    let outcome = BuiltinTools::new()
        .call(
            "glob",
            serde_json::json!({
                "pattern": "**/*.rs",
                "path": rig.dir.path().display().to_string(),
            }),
            &rig.ctx("glob"),
        )
        .await
        .expect("the harness carried the call");
    let text = text_of(&outcome);
    assert!(text.contains("a.rs"), "`a.rs` matches `**/*.rs`: {text}");
    assert!(!text.contains("b.txt"), "`b.txt` does not: {text}");
}

/// An edit rewrites the file it was pointed at, through the broker, atomically.
#[tokio::test]
async fn edit_replaces_through_the_broker() {
    let rig = Rig::open(ToolBudget::new(30_000, 1 << 20));
    let file = rig.path("edit.txt");
    std::fs::write(&file, "hello world\n").unwrap();

    let outcome = BuiltinTools::new()
        .call(
            "edit",
            serde_json::json!({
                "path": "edit.txt",
                "old_text": "world",
                "new_text": "harness",
            }),
            &rig.ctx("edit"),
        )
        .await
        .expect("the harness carried the call");
    assert!(outcome.is_ok(), "the edit should apply: {outcome:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello harness\n");
}

/// A path outside the workspace is refused by the engine, before any file is
/// opened — and the tool reports it as a denial, not as a failure.
#[tokio::test]
async fn outside_the_workspace_is_denied() {
    let rig = Rig::open(ToolBudget::new(30_000, CEILING));
    let outside = if cfg!(windows) {
        "C:/Windows/System32/drivers/etc/hosts"
    } else {
        "/etc/hosts"
    };
    let outcome = BuiltinTools::new()
        .call(
            "read",
            serde_json::json!({ "path": outside }),
            &rig.ctx("read"),
        )
        .await
        .expect("the harness carried the call");
    assert!(
        matches!(outcome, Outcome::Denied { .. }),
        "the default rules cover the workspace and nothing else: {outcome:?}"
    );
}

/// The gap wave 3 wrote down: discovery used to be `std::fs::read_dir`, around
/// the broker, so a path the policy refuses to read could still be **named** in
/// a glob result. Discovery now goes through `BrokerFacade::list`, which omits
/// what this call may not read — a name is information too.
#[tokio::test]
async fn glob_never_names_what_the_policy_hides() {
    let rig = Rig::with_rules(
        ToolBudget::new(30_000, CEILING),
        "[permissions]
allow = [\"tool(*)\", \"read(./src/**)\"]
",
    );
    std::fs::create_dir_all(rig.path("src")).unwrap();
    std::fs::write(rig.path("src/a.rs"), "").unwrap();
    std::fs::write(rig.path("secret.txt"), "shh").unwrap();

    let outcome = BuiltinTools::new()
        .call(
            "glob",
            serde_json::json!({
                "pattern": "**/*",
                "path": rig.dir.path().display().to_string(),
            }),
            &rig.ctx("glob"),
        )
        .await
        .expect("the harness carried the call");
    let text = text_of(&outcome);
    assert!(text.contains("a.rs"), "`src/a.rs` is readable: {text}");
    assert!(
        !text.contains("secret.txt"),
        "a file this call may not read must not be named either: {text}"
    );
}

/// The same for `grep`, which reads bytes as well as names.
#[tokio::test]
async fn grep_never_names_what_the_policy_hides() {
    let rig = Rig::with_rules(
        ToolBudget::new(30_000, CEILING),
        "[permissions]
allow = [\"tool(*)\", \"read(./src/**)\"]
",
    );
    std::fs::create_dir_all(rig.path("src")).unwrap();
    std::fs::write(rig.path("src/a.rs"), "needle here
").unwrap();
    std::fs::write(rig.path("secret.txt"), "needle here too
").unwrap();

    let outcome = BuiltinTools::new()
        .call(
            "grep",
            serde_json::json!({
                "pattern": "needle",
                "path": rig.dir.path().display().to_string(),
            }),
            &rig.ctx("grep"),
        )
        .await
        .expect("the harness carried the call");
    let text = text_of(&outcome);
    assert!(text.contains("a.rs"), "the readable match is reported: {text}");
    assert!(
        !text.contains("secret.txt"),
        "the unreadable one is not, by name or by line: {text}"
    );
}
