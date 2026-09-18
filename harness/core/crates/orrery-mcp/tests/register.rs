//! Registration, namespacing, collisions and `list_changed`.
//!
//! Every assertion here is about the **ordinary** registry: nothing in
//! `orrery-mcp` holds a tool table of its own, so if an MCP tool behaves
//! differently from a local one it is a bug in this crate rather than a feature
//! of MCP.

use std::sync::Arc;

use orrery_audit::AuditEvent;
use orrery_mcp::client::McpTool;
use orrery_mcp::register::{self, Admission};
use orrery_proto::{AgentScope, BranchId, Grant, Layer, Outcome, Subject, ToolRef};
use orrery_tools::{
    CallCtx, PolicyCheck, PolicyDecision, Registry, Resolution, ToolBudget, ToolSpec,
};

fn tool(name: &str) -> McpTool {
    McpTool {
        name: name.to_owned(),
        description: format!("the server's {name}"),
        input_schema: serde_json::json!({ "type": "object" }),
    }
}

fn scope() -> AgentScope {
    AgentScope {
        agent: "main".to_owned(),
        branch: BranchId::new(),
        tools: vec!["*".to_owned()],
        grant: Grant::nothing(),
    }
}

fn subject() -> Subject {
    Subject::Agent
}

/// Refuses exactly the tools it was told to, whatever the input.
#[derive(Debug, Default)]
struct DenyList(Vec<String>);

impl PolicyCheck for DenyList {
    fn check(&self, r#ref: &ToolRef, _input: &serde_json::Value, _ctx: &CallCtx) -> PolicyDecision {
        if self.0.contains(&r#ref.to_string()) {
            return PolicyDecision::Deny {
                rule: orrery_policy::no_rule(),
                reason: format!("`{ref}` is not in the approved manifest", r#ref = r#ref),
            };
        }
        PolicyDecision::Allow
    }

    fn categorically_denies(&self, _subject: &Subject, r#ref: &ToolRef) -> bool {
        self.0.contains(&r#ref.to_string())
    }
}

#[test]
fn no_second_scheme() {
    let mut registry = Registry::new();
    let audit = orrery_audit::memory();
    register::register(
        &mut registry,
        "jira",
        Layer::User,
        &[tool("create_issue")],
        &subject(),
        &orrery_tools::AllowAll,
        &(audit.clone() as orrery_audit::Audit),
    )
    .expect("`jira` is a legal namespace");

    // It is an ordinary reference, in the ordinary table.
    let expected = ToolRef {
        ext: "mcp.jira".parse().unwrap(),
        name: "create_issue".to_owned(),
    };
    assert!(registry.entry(&expected).is_some());
    assert_eq!(expected.to_string(), "mcp.jira.create_issue");

    // And the fully-qualified name the model emits parses back to it — the
    // last-dot split in `ToolRef` is what makes `mcp.jira` the extension rather
    // than `mcp`.
    assert_eq!(
        "mcp.jira.create_issue".parse::<ToolRef>().unwrap(),
        expected
    );
    assert_eq!(
        registry.resolve("mcp.jira.create_issue", &scope()),
        Resolution::Ok {
            r#ref: expected.clone()
        }
    );

    // The schema and the description came from the server, unchanged.
    let entry = registry.entry(&expected).unwrap();
    assert_eq!(entry.spec.description, "the server's create_issue");
    assert_eq!(entry.spec.input_schema["type"], "object");
}

#[test]
fn collision_is_a_resolution_event() {
    let mut registry = Registry::new();
    let audit = orrery_audit::memory();

    // An extension's `search`, and an MCP server's `search`.
    registry.register(
        &"ripgrep".parse().unwrap(),
        Layer::User,
        ToolSpec::new("search"),
    );
    register::register(
        &mut registry,
        "jira",
        Layer::Project,
        &[tool("search")],
        &subject(),
        &orrery_tools::AllowAll,
        &(audit.clone() as orrery_audit::Audit),
    )
    .unwrap();

    // Both survive. Nothing exits 1 over a duplicate name.
    assert!(registry.entry(&"ripgrep.search".parse().unwrap()).is_some());
    assert!(
        registry
            .entry(&"mcp.jira.search".parse().unwrap())
            .is_some()
    );

    // The short name resolves, to the closer layer, and the ledger says so.
    let resolution = registry.resolve("search", &scope());
    let Resolution::Ambiguous { candidates, chose } = resolution else {
        panic!("two claimants is ambiguous: {resolution:?}");
    };
    assert_eq!(candidates.len(), 2);
    assert_eq!(chose.to_string(), "mcp.jira.search", "Project beats User");
    assert!(
        registry.ledger().iter().any(
            |e| matches!(e, orrery_tools::LedgerEntry::Ambiguous { name, .. } if name == "search")
        ),
        "the choice is in the ledger: {:?}",
        registry.ledger()
    );

    // And each one is still reachable by its full name.
    assert_eq!(
        registry
            .resolve("ripgrep.search", &scope())
            .r#ref()
            .unwrap()
            .to_string(),
        "ripgrep.search"
    );
}

#[tokio::test]
async fn calls_are_policy_checked() {
    let audit = orrery_audit::memory();
    let policy = Arc::new(DenyList(vec!["mcp.jira.create_issue".to_owned()]));
    let mut registry = Registry::new().with_policy(policy.clone());
    register::register(
        &mut registry,
        "jira",
        Layer::User,
        &[tool("search")],
        &subject(),
        &orrery_tools::AllowAll,
        &(audit.clone() as orrery_audit::Audit),
    )
    .unwrap();
    // Registered directly, so the call — not the registration — is what the
    // policy refuses.
    registry.register(
        &"mcp.jira".parse().unwrap(),
        Layer::User,
        ToolSpec::new("create_issue"),
    );

    let ctx = CallCtx::new(
        orrery_proto::CallId::new(),
        subject(),
        scope(),
        ToolBudget::new(5_000, 1 << 20),
    );
    let outcome = registry
        .dispatch(
            &"mcp.jira.create_issue".parse().unwrap(),
            serde_json::json!({}),
            ctx,
        )
        .await
        .expect("a denial is not an error");

    // Exactly like a local tool: `Ok(Outcome::Denied)`, with a reason.
    assert!(
        matches!(&outcome, Outcome::Denied { reason, .. } if reason.contains("approved manifest")),
        "{outcome:?}"
    );
}

#[test]
fn list_changed_is_re_resolved() {
    let audit = orrery_audit::memory();
    let policy = DenyList(vec!["mcp.jira.delete_project".to_owned()]);
    let mut registry = Registry::new();

    // Session start: one tool, approved.
    let first = register::register(
        &mut registry,
        "jira",
        Layer::User,
        &[tool("search")],
        &subject(),
        &policy,
        &(audit.clone() as orrery_audit::Audit),
    )
    .unwrap();
    assert_eq!(first.admitted().len(), 1);
    assert_eq!(registry.len(), 1);

    // Mid-session the server says its tool set moved: the old one, one new one
    // policy is happy with, one it is not.
    let again = register::reconcile(
        &mut registry,
        "jira",
        Layer::User,
        &[tool("search"), tool("create_issue"), tool("delete_project")],
        &subject(),
        &policy,
        &(audit.clone() as orrery_audit::Audit),
    )
    .unwrap();

    assert_eq!(
        again.decisions[0],
        Admission::Unchanged {
            r#ref: "mcp.jira.search".parse().unwrap()
        }
    );
    assert_eq!(
        again
            .admitted()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["mcp.jira.create_issue"]
    );
    assert_eq!(
        again
            .refused()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["mcp.jira.delete_project"]
    );

    // The refused one is NOT in the registry. Not listed-and-then-denied:
    // absent, so no call can reach it and the model is never offered it.
    assert!(
        registry
            .entry(&"mcp.jira.delete_project".parse().unwrap())
            .is_none()
    );
    assert!(
        registry
            .entry(&"mcp.jira.create_issue".parse().unwrap())
            .is_some()
    );
    assert!(matches!(
        registry.resolve("mcp.jira.delete_project", &scope()),
        Resolution::Unknown { .. }
    ));

    // And both decisions are in the stream, so "the tool set grew mid-session"
    // is answerable from the audit alone.
    let events: Vec<(String, Vec<String>)> = audit
        .records()
        .into_iter()
        .filter_map(|r| match r.event {
            AuditEvent::ExtensionLoad {
                status,
                contributions,
                problems,
                ..
            } => Some((
                status,
                if contributions.is_empty() {
                    problems
                } else {
                    contributions
                },
            )),
            _ => None,
        })
        .collect();
    assert!(
        events
            .iter()
            .any(|(s, what)| s == "tool-admitted"
                && what.iter().any(|c| c == "mcp.jira.create_issue")),
        "{events:?}"
    );
    assert!(
        events.iter().any(|(s, what)| s == "tool-refused"
            && what.iter().any(|p| p.contains("mcp.jira.delete_project"))),
        "{events:?}"
    );
}

#[test]
fn a_server_name_that_is_not_a_namespace_is_refused() {
    let mut registry = Registry::new();
    let audit = orrery_audit::memory();
    let err = register::register(
        &mut registry,
        "Jira Cloud",
        Layer::User,
        &[tool("search")],
        &subject(),
        &orrery_tools::AllowAll,
        &(audit as orrery_audit::Audit),
    )
    .expect_err("a namespace with a space in it is not a namespace");
    assert!(err.to_string().contains("Jira Cloud"));
    assert!(registry.is_empty());
}
