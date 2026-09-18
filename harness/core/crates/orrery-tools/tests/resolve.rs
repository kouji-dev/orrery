//! Task 1 · registration, the name table and precedence.

mod common;

use common::{ext, register, wide_scope};
use orrery_proto::{Layer, ToolRef};
use orrery_tools::{LedgerEntry, Registry, Resolution};
use std::sync::Arc;

fn r(s: &str) -> ToolRef {
    s.parse().expect("valid tool ref")
}

#[test]
fn two_extensions_claiming_search() {
    let mut reg = Registry::new();
    register(&mut reg, "ripgrep", "search", Layer::Project);
    register(&mut reg, "semantic", "search", Layer::Project);
    let scope = wide_scope();

    // Both resolve by full name.
    assert!(matches!(
        reg.resolve("ripgrep.search", &scope),
        Resolution::Ok { r#ref } if r#ref == r("ripgrep.search")
    ));
    assert!(matches!(
        reg.resolve("semantic.search", &scope),
        Resolution::Ok { r#ref } if r#ref == r("semantic.search")
    ));

    // A bare `search` is ambiguous, resolved, and reported. Nothing errors.
    match reg.resolve("search", &scope) {
        Resolution::Ambiguous { candidates, chose } => {
            assert_eq!(candidates.len(), 2);
            assert!(candidates.contains(&r("ripgrep.search")));
            assert!(candidates.contains(&r("semantic.search")));
            assert!(candidates.contains(&chose));
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

#[test]
fn closest_layer_wins() {
    let mut reg = Registry::new();
    register(&mut reg, "git", "status", Layer::User);
    register(&mut reg, "git", "status", Layer::Project);

    let entry = reg.entry(&r("git.status")).expect("registered");
    assert_eq!(entry.layer, Layer::Project);

    let shadowed = reg
        .ledger()
        .into_iter()
        .find_map(|e| match e {
            LedgerEntry::Shadowed {
                r#ref,
                winner,
                loser,
            } if r#ref == r("git.status") => Some((winner, loser)),
            _ => None,
        })
        .expect("the loser is in the ledger");
    assert_eq!(shadowed, (Layer::Project, Layer::User));
}

#[test]
fn unknown_suggests() {
    let mut reg = Registry::new();
    register(&mut reg, "ripgrep", "search", Layer::Project);

    match reg.resolve("serch", &wide_scope()) {
        Resolution::Unknown {
            name,
            did_you_mean,
        } => {
            assert_eq!(name, "serch");
            assert!(
                did_you_mean.iter().any(|s| s == "search"),
                "expected `search` in {did_you_mean:?}"
            );
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}

#[test]
fn mcp_three_segments() {
    let mut reg = Registry::new();
    register(&mut reg, "mcp.jira", "create_issue", Layer::Workspace);

    match reg.resolve("mcp.jira.create_issue", &wide_scope()) {
        Resolution::Ok { r#ref } => {
            assert_eq!(r#ref.ext, ext("mcp.jira"));
            assert_eq!(r#ref.name, "create_issue");
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn ambiguity_is_recorded() {
    let mut reg = Registry::new();
    register(&mut reg, "ripgrep", "search", Layer::User);
    register(&mut reg, "semantic", "search", Layer::Project);

    let chose = match reg.resolve("search", &wide_scope()) {
        Resolution::Ambiguous { chose, .. } => chose,
        other => panic!("expected Ambiguous, got {other:?}"),
    };
    // Closest layer first, for a name.
    assert_eq!(chose, r("semantic.search"));

    let recorded = reg
        .ledger()
        .into_iter()
        .find_map(|e| match e {
            LedgerEntry::Ambiguous {
                name,
                candidates,
                chose,
            } if name == "search" => Some((candidates, chose)),
            _ => None,
        })
        .expect("the ambiguity is in the ledger");
    assert_eq!(recorded.0.len(), 2);
    assert!(recorded.0.contains(&r("ripgrep.search")));
    assert!(recorded.0.contains(&r("semantic.search")));
    assert_eq!(recorded.1, r("semantic.search"));
}

#[test]
fn user_alias_resolves() {
    let mut reg = Registry::new();
    register(&mut reg, "ripgrep", "search", Layer::Project);
    reg.alias("rg", r("ripgrep.search"));

    assert!(matches!(
        reg.resolve("rg", &wide_scope()),
        Resolution::Ok { r#ref } if r#ref == r("ripgrep.search")
    ));
}

/// Plan 04, task 5, completed once `orrery-audit` existed: the in-crate ledger
/// is the fast path, and the same decision also reaches the audit stream, where
/// "which rule allowed this" is answered from one place.
#[test]
fn ambiguity_reaches_the_audit() {
    let audit = orrery_audit::memory();
    let mut reg = Registry::new().with_audit(Arc::clone(&audit) as orrery_audit::Audit);
    register(&mut reg, "ripgrep", "search", Layer::User);
    register(&mut reg, "semantic", "search", Layer::Project);

    let _ = reg.resolve("search", &wide_scope());

    let recorded = audit
        .records()
        .into_iter()
        .find_map(|rec| match rec.event {
            orrery_audit::AuditEvent::ToolName {
                name,
                candidates,
                chose,
            } if name == "search" => Some((candidates, chose)),
            _ => None,
        })
        .expect("the ambiguity is in the audit stream");

    assert_eq!(recorded.1, "semantic.search");
    assert!(recorded.0.iter().any(|c| c == "ripgrep.search"));
    assert!(recorded.0.iter().any(|c| c == "semantic.search"));
}

#[test]
fn a_registry_without_an_audit_still_records_to_its_ledger() {
    let mut reg = Registry::new();
    register(&mut reg, "ripgrep", "search", Layer::User);
    register(&mut reg, "semantic", "search", Layer::Project);
    let _ = reg.resolve("search", &wide_scope());
    assert!(reg
        .ledger()
        .iter()
        .any(|e| matches!(e, LedgerEntry::Ambiguous { .. })));
}
