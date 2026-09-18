//! Lifecycle: discovered is not connected, dead degrades, and a server that
//! comes back is usable again.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use orrery_mcp::health::{Health, McpHost, ServerSpec, Servers};
use orrery_mcp::transport::StdioSpec;
use orrery_proto::{AgentScope, BranchId, CallId, Grant, Layer, Outcome, Subject, ToolRef};
use orrery_tools::{CallCtx, Registry, ToolBudget, ToolHost, ToolSpec};

const SERVER: &str = env!("CARGO_BIN_EXE_orrery-mcp-fixture-server");

fn spec() -> ServerSpec {
    ServerSpec::stdio(
        "notes",
        StdioSpec {
            program: SERVER.to_owned(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
        },
        Layer::User,
    )
}

fn scope() -> AgentScope {
    AgentScope {
        agent: "main".to_owned(),
        branch: BranchId::new(),
        tools: vec!["*".to_owned()],
        grant: Grant::nothing(),
    }
}

fn ctx() -> CallCtx {
    CallCtx::new(
        CallId::new(),
        Subject::Agent,
        scope(),
        // A generous ceiling on purpose: if a dead server stalled, the test
        // would wait this long, and it does not.
        ToolBudget::new(30_000, 1 << 20),
    )
}

#[tokio::test]
async fn discovered_but_not_connected() {
    let servers = Servers::discover(vec![spec()]);

    // In the manifest at session start.
    assert_eq!(servers.discovered().len(), 1);
    assert_eq!(servers.health("notes"), Some(Health::Discovered));

    // And **no process was started**. A count, not a flag.
    assert_eq!(servers.started(), 0);

    // Registering its tools does not start it either: a tool list can come
    // from the manifest, and connecting is what a *call* causes.
    let mut registry = Registry::new();
    registry.register(
        &"mcp.notes".parse().unwrap(),
        Layer::User,
        ToolSpec::new("echo"),
    );
    assert_eq!(servers.started(), 0);

    // One call, and now it is connected.
    let host = McpHost::new(Arc::new(servers));
    let r#ref: ToolRef = "mcp.notes.echo".parse().unwrap();
    let outcome = host
        .call(&r#ref, serde_json::json!({ "text": "hi" }), &ctx())
        .await
        .unwrap();
    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(host.servers().started(), 1);
    assert_eq!(host.servers().health("notes"), Some(Health::Connected));
}

#[tokio::test]
async fn dead_server_degrades() {
    let host = McpHost::new(Arc::new(Servers::discover(vec![spec()])));
    let echo: ToolRef = "mcp.notes.echo".parse().unwrap();
    let die: ToolRef = "mcp.notes.die".parse().unwrap();

    // Alive.
    assert!(
        host.call(&echo, serde_json::json!({ "text": "hi" }), &ctx())
            .await
            .unwrap()
            .is_ok()
    );

    // Kill it mid-session. The server exits without answering, so this call is
    // the one whose connection goes away underneath it.
    let _ = host.call(&die, serde_json::json!({}), &ctx()).await;

    // The next call does not stall: the turn carries on with a degraded tool.
    let started = Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_secs(5),
        host.call(&echo, serde_json::json!({ "text": "still?" }), &ctx()),
    )
    .await
    .expect("a dead server must not stall the turn")
    .unwrap();
    let elapsed = started.elapsed();

    // It either came back `Unloaded`, or the host reconnected and answered —
    // both are the turn carrying on. What it must never be is a wait.
    assert!(
        elapsed < Duration::from_secs(5),
        "a dead server waited {elapsed:?}; the wall-clock budget is 30s and nothing may sit on it"
    );
    assert!(
        matches!(outcome, Outcome::Unloaded { .. } | Outcome::Ok { .. }),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_server_that_will_not_start_is_unloaded_not_an_error() {
    let servers = Servers::discover(vec![ServerSpec::stdio(
        "notes",
        StdioSpec::new("this-program-does-not-exist-anywhere"),
        Layer::User,
    )]);
    let host = McpHost::new(Arc::new(servers));

    let outcome = host
        .call(
            &"mcp.notes.echo".parse().unwrap(),
            serde_json::json!({}),
            &ctx(),
        )
        .await
        .expect("a missing server is a degraded tool, not a failed dispatch");
    assert!(matches!(outcome, Outcome::Unloaded { .. }), "{outcome:?}");
    assert!(matches!(
        host.servers().health("notes"),
        Some(Health::Failed { .. })
    ));
}

#[tokio::test]
async fn reconnects() {
    let servers = Arc::new(Servers::discover(vec![spec()]));
    let host = McpHost::new(servers.clone());
    let echo: ToolRef = "mcp.notes.echo".parse().unwrap();

    assert!(
        host.call(&echo, serde_json::json!({ "text": "one" }), &ctx())
            .await
            .unwrap()
            .is_ok()
    );
    assert_eq!(servers.started(), 1);

    // It goes away.
    let _ = host
        .call(
            &"mcp.notes.die".parse().unwrap(),
            serde_json::json!({}),
            &ctx(),
        )
        .await;
    servers.mark_failed("notes", "it exited");
    assert!(matches!(
        servers.health("notes"),
        Some(Health::Failed { .. })
    ));

    // And it is usable again: the next call starts a fresh process.
    let outcome = host
        .call(&echo, serde_json::json!({ "text": "two" }), &ctx())
        .await
        .unwrap();
    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(servers.health("notes"), Some(Health::Connected));
    assert!(servers.started() >= 2, "a fresh process was started");
}

#[tokio::test]
async fn a_tool_whose_server_was_never_discovered_is_unloaded() {
    let host = McpHost::new(Arc::new(Servers::discover(Vec::new())));
    let outcome = host
        .call(
            &"mcp.nobody.echo".parse().unwrap(),
            serde_json::json!({}),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::Unloaded { .. }), "{outcome:?}");
}

/// The manifest-integrity rule, end to end against the real server rather than
/// against a list this test made up.
///
/// The fixture's `grow` tool adds `reverse` to its own tool set and sends
/// `notifications/tools/list_changed`, exactly as the spec says a server
/// should. The point is what happens next: the new tool is re-resolved against
/// policy, and a policy that refuses it leaves it out of the registry
/// altogether.
#[tokio::test]
async fn list_changed_from_a_real_server_is_re_resolved() {
    use orrery_mcp::client::McpTool;
    use orrery_mcp::register::{self, ListChangedWatch};
    use orrery_tools::{PolicyCheck, PolicyDecision};

    /// Refuses `reverse` categorically. Everything else is fine.
    #[derive(Debug)]
    struct NoReverse;
    impl PolicyCheck for NoReverse {
        fn check(
            &self,
            _ref: &ToolRef,
            _input: &serde_json::Value,
            _ctx: &CallCtx,
        ) -> PolicyDecision {
            PolicyDecision::Allow
        }
        fn categorically_denies(&self, _subject: &Subject, r#ref: &ToolRef) -> bool {
            r#ref.name == "reverse"
        }
    }

    let watch = Arc::new(ListChangedWatch::new());
    let servers = Arc::new(
        Servers::discover(vec![spec()])
            .with_handler(watch.clone() as Arc<dyn orrery_jsonrpc::Handler>),
    );
    let client = servers.connect("notes").await.expect("it starts");

    let audit = orrery_audit::memory();
    let mut registry = Registry::new();
    let before: Vec<McpTool> = client.list_tools().await.unwrap();
    register::register(
        &mut registry,
        "notes",
        Layer::User,
        &before,
        &Subject::Agent,
        &NoReverse,
        &(audit.clone() as orrery_audit::Audit),
    )
    .unwrap();
    let approved = registry.len();
    assert!(
        !before.iter().any(|t| t.name == "reverse"),
        "the manifest that was approved does not contain it"
    );

    // The server grows a tool and says so.
    let changed = watch.changed();
    client
        .call_tool("grow", serde_json::json!({}))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), changed)
        .await
        .expect("the server announced its new tool set");
    assert_eq!(watch.seen(), 1);

    // The notification on its own added nothing.
    assert_eq!(
        registry.len(),
        approved,
        "a notification is not a registration"
    );

    let after = client.list_tools().await.unwrap();
    assert!(after.iter().any(|t| t.name == "reverse"));

    // Re-resolved, and refused.
    let again = register::reconcile(
        &mut registry,
        "notes",
        Layer::User,
        &after,
        &Subject::Agent,
        &NoReverse,
        &(audit.clone() as orrery_audit::Audit),
    )
    .unwrap();
    assert_eq!(
        again
            .refused()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["mcp.notes.reverse"]
    );
    assert_eq!(registry.len(), approved, "the tool set did not grow");
    assert!(
        registry
            .entry(&"mcp.notes.reverse".parse().unwrap())
            .is_none()
    );

    // With a policy that admits it, the same call lets it in — and records it.
    let mut permissive = Registry::new();
    register::register(
        &mut permissive,
        "notes",
        Layer::User,
        &after,
        &Subject::Agent,
        &orrery_tools::AllowAll,
        &(audit as orrery_audit::Audit),
    )
    .unwrap();
    assert!(
        permissive
            .entry(&"mcp.notes.reverse".parse().unwrap())
            .is_some()
    );
}
