//! Task 2 · the tool list the model is actually given.

mod common;

use common::{ext, register, scope, wide_scope};
use orrery_proto::{CallId, Layer, Subject, ToolRef};
use orrery_tools::{CallCtx, ExtState, Registry, ToolBudget};

fn populated() -> Registry {
    let mut reg = Registry::new();
    register(&mut reg, "git", "status", Layer::Project);
    register(&mut reg, "git", "commit", Layer::User);
    register(&mut reg, "ripgrep", "search", Layer::Workspace);
    register(&mut reg, "semantic", "search", Layer::Org);
    register(&mut reg, "builtin", "read", Layer::Managed);
    reg
}

#[test]
fn order_is_stable() {
    // Twenty runs, because a single pass cannot catch hash ordering.
    for _ in 0..20 {
        let reg = populated();
        let scope = wide_scope();
        let a = serde_json::to_vec(&reg.visible(&scope)).expect("serialisable");
        let b = serde_json::to_vec(&reg.visible(&scope)).expect("serialisable");
        assert_eq!(a, b, "two passes over one registry must be byte-identical");

        let other = populated();
        assert_eq!(
            serde_json::to_vec(&other.visible(&scope)).expect("serialisable"),
            a,
            "the same registrations must produce the same list"
        );
    }
}

#[test]
fn order_is_layers_then_registration() {
    let reg = populated();
    let names: Vec<String> = reg
        .visible(&wide_scope())
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(
        names,
        vec![
            "status".to_owned(),          // Project
            "ripgrep.search".to_owned(),  // Workspace
            "commit".to_owned(),          // User
            "semantic.search".to_owned(), // Org
            "read".to_owned(),            // Managed
        ]
    );
}

#[tokio::test]
async fn scope_is_a_real_subset() {
    let reg = populated();
    let scope = scope(&["git.*"]);

    let names: Vec<String> = reg.visible(&scope).into_iter().map(|d| d.name).collect();
    assert_eq!(names, vec!["status".to_owned(), "commit".to_owned()]);

    // And a tool outside the set cannot be dispatched either.
    let r#ref: ToolRef = "ripgrep.search".parse().expect("valid");
    let ctx = CallCtx::new(
        CallId::new(),
        Subject::Agent,
        scope,
        ToolBudget::new(1_000, 1_024),
    );
    let outcome = reg
        .dispatch(&r#ref, serde_json::json!({}), ctx)
        .await
        .expect("dispatch does not error");
    assert!(
        matches!(outcome, orrery_proto::Outcome::Denied { .. }),
        "expected a denial, got {outcome:?}"
    );
}

#[test]
fn draining_extension_is_hidden() {
    let mut reg = populated();
    reg.set_state(&ext("git"), ExtState::Draining);

    let names: Vec<String> = reg
        .visible(&wide_scope())
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert!(
        !names.iter().any(|n| n.contains("status") || n.contains("commit")),
        "a draining extension is not offered: {names:?}"
    );
}

#[test]
fn short_name_only_where_unambiguous() {
    let reg = populated();
    let names: Vec<String> = reg
        .visible(&wide_scope())
        .into_iter()
        .map(|d| d.name)
        .collect();
    // Two extensions claim `search`, so neither gets the short form.
    assert!(names.contains(&"ripgrep.search".to_owned()));
    assert!(names.contains(&"semantic.search".to_owned()));
    assert!(!names.contains(&"search".to_owned()));
    // Nobody else claims `read`.
    assert!(names.contains(&"read".to_owned()));
}
