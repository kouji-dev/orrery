//! Orrery as an MCP server, driven by Orrery's own MCP client over a pipe.
//!
//! Both ends of these tests are real: [`serve`] answers on one half of a duplex
//! stream and [`McpClient`] asks on the other, over the same line-delimited
//! framing a child process would use. What is asserted is not "the handler
//! returns the right list" but "a client on the far end of a socket cannot see
//! or call what the scope could not".

use std::sync::Arc;

use async_trait::async_trait;
use orrery_mcp::client::McpClient;
use orrery_mcp::expose::{McpServerHandle, serve};
use orrery_proto::{AgentScope, BranchId, Grant, Layer, Outcome, ToolRef};
use orrery_tools::{
    CallCtx, PolicyCheck, PolicyDecision, Registry, ToolBudget, ToolError, ToolHost, ToolSpec,
};

/// Answers every call with the tool's own name, so a result proves which tool
/// actually ran.
#[derive(Debug, Default)]
struct Echo;

#[async_trait]
impl ToolHost for Echo {
    async fn call(
        &self,
        r#ref: &ToolRef,
        _input: serde_json::Value,
        _ctx: &CallCtx,
    ) -> Result<Outcome, ToolError> {
        Ok(Outcome::Ok {
            surface: None,
            value: Some(serde_json::Value::String(r#ref.to_string())),
        })
    }
}

/// Refuses one tool for everybody, per call — not categorically, so `visible`
/// still offers it and the refusal has to come from dispatch.
#[derive(Debug)]
struct DenyOnCall(&'static str);

impl PolicyCheck for DenyOnCall {
    fn check(&self, r#ref: &ToolRef, _input: &serde_json::Value, _ctx: &CallCtx) -> PolicyDecision {
        if r#ref.to_string() == self.0 {
            return PolicyDecision::Deny {
                rule: orrery_policy::no_rule(),
                reason: "not from here".to_owned(),
            };
        }
        PolicyDecision::Allow
    }
}

fn registry(policy: Arc<dyn PolicyCheck>) -> Registry {
    let mut registry = Registry::with_host(Arc::new(Echo)).with_policy(policy);
    registry.register(
        &"ripgrep".parse().unwrap(),
        Layer::User,
        ToolSpec::new("search").described("find things"),
    );
    registry.register(
        &"shell".parse().unwrap(),
        Layer::User,
        ToolSpec::new("exec").described("run things"),
    );
    registry.register(
        &"mcp.jira".parse().unwrap(),
        Layer::User,
        ToolSpec::new("create_issue"),
    );
    registry
}

/// A scope that can see the searcher and nothing else.
fn narrow() -> AgentScope {
    AgentScope {
        agent: "reader".to_owned(),
        branch: BranchId::new(),
        tools: vec!["ripgrep.*".to_owned()],
        grant: Grant::nothing(),
    }
}

/// Serve a handle over a pipe and hand back a client on the other end.
async fn connected(handle: McpServerHandle) -> McpClient {
    let (theirs, ours) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(ours);
    let _peer = serve(Arc::new(handle), server_read, server_write);
    // Keep the server peer alive for as long as the client is.
    Box::leak(Box::new(_peer));

    let (client_read, client_write) = tokio::io::split(theirs);
    let client = McpClient::over(
        orrery_jsonrpc::Peer::spawn(
            client_read,
            client_write,
            orrery_jsonrpc::Framing::LineDelimited,
            Arc::new(orrery_jsonrpc::NoHandler),
        ),
        "orrery",
    );
    client.initialize().await.expect("the handshake");
    client
}

#[tokio::test]
async fn only_the_visible_set() {
    let registry = Arc::new(registry(Arc::new(orrery_tools::AllowAll)));
    let handle = McpServerHandle::new(registry.clone(), narrow(), ToolBudget::new(5_000, 1 << 20));
    let client = connected(handle).await;

    let tools = client.list_tools().await.expect("tools/list");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["search"], "only what `visible(scope)` offers");

    // Not listed — and, which is the part that matters, not callable either.
    // The listing is not the boundary.
    let refused = client
        .call_tool("shell.exec", serde_json::json!({}))
        .await
        .expect("a refusal comes back as a result, not a protocol error");
    assert!(refused.is_error, "{refused:?}");
    assert!(
        refused.text().contains("no such tool"),
        "{}",
        refused.text()
    );

    // Nor by its short name.
    let refused = client
        .call_tool("exec", serde_json::json!({}))
        .await
        .unwrap();
    assert!(refused.is_error);

    // And the one it can see works, through the ordinary dispatch path.
    let ok = client
        .call_tool("search", serde_json::json!({}))
        .await
        .unwrap();
    assert!(!ok.is_error, "{ok:?}");
    assert_eq!(ok.text(), "ripgrep.search");
}

#[tokio::test]
async fn inbound_calls_are_policy_checked() {
    let registry = Arc::new(registry(Arc::new(DenyOnCall("ripgrep.search"))));
    let scope = AgentScope {
        tools: vec!["*".to_owned()],
        ..narrow()
    };
    let handle = McpServerHandle::new(registry, scope, ToolBudget::new(5_000, 1 << 20));
    let client = connected(handle).await;

    // It is offered — the policy refuses per call, not categorically — so this
    // is dispatch's check doing the work and not `visible`'s.
    let names: Vec<String> = client
        .list_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert!(names.iter().any(|n| n == "search"), "{names:?}");

    let refused = client
        .call_tool("search", serde_json::json!({}))
        .await
        .unwrap();
    assert!(refused.is_error, "{refused:?}");
    assert_eq!(refused.text(), "not from here");
    // The rule that refused is named, so the caller can be told why.
    assert!(
        refused.raw["_meta"]["orrery/rule"].is_string(),
        "{refused:?}"
    );

    // An MCP tool exposed onward is checked the same way: no special case in
    // either direction.
    let ok = client
        .call_tool("create_issue", serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(ok.text(), "mcp.jira.create_issue");
}

#[tokio::test]
async fn a_method_orrery_does_not_serve_is_a_method_not_found() {
    let registry = Arc::new(registry(Arc::new(orrery_tools::AllowAll)));
    let handle = McpServerHandle::new(registry, narrow(), ToolBudget::new(5_000, 1 << 20));
    let client = connected(handle).await;

    let err = client
        .peer()
        .call(
            "resources/list",
            serde_json::json!({}),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect_err("we do not serve resources yet, and say so");
    assert!(err.to_string().contains("resources/list"), "{err}");
}
