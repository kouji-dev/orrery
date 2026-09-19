//! Task 11 · the load-time gate. **This is the phase-3 criterion's first
//! half.**
//!
//! The second half — the tool actually disappearing from what a turn offers,
//! with the session still alive — is `orrery-cli/tests/degrade.rs`, driven
//! through the binary. That split is deliberate and was learned the hard way:
//! this file used to test an `install` function whose only caller was this
//! file, while the run path handed every extension `Capability::all` and could
//! never be missing anything. A green test here proves nothing about the
//! product, so the criterion is claimed from the binary and only the *rule* is
//! claimed here.

mod common;

use common::Fixture;
use orrery_broker::grant_for;
use orrery_proto::{Aspect, Capability, Consent, ExtId};

/// `buildgraph` wants `spawn` for one tool and `read` for the others. The
/// operator denies `spawn` to that extension and to nobody else.
const RULES: &str = r#"
[permissions]
allow = ["tool(*)", "read(./**)", "write(./**)", "spawn(*)"]

[permissions."ext:buildgraph"]
allow = ["tool(*)", "read(./**)", "write(./**)"]
deny  = ["spawn(*)"]
"#;

fn wants() -> Vec<Capability> {
    vec![
        Capability::scoped(Aspect::Spawn, vec!["*".to_owned()]),
        Capability::scoped(Aspect::Read, vec!["./**".to_owned()]),
    ]
}

fn ext() -> ExtId {
    ExtId::new("buildgraph").expect("a valid id")
}

fn carries(grant: &orrery_proto::Grant, aspect: Aspect) -> bool {
    grant.capabilities.iter().any(|c| c.aspect == aspect)
}

/// The refused aspect is the one that goes, and only it.
#[test]
fn a_denied_aspect_is_absent_from_the_grant() {
    let fx = Fixture::with_rules(RULES);
    let grant = grant_for(
        &fx.engine,
        &ext(),
        &wants(),
        Consent::Always,
        &Fixture::scope(),
    );

    assert!(
        !carries(&grant, Aspect::Spawn),
        "a refused capability must not be granted: {:?}",
        grant.capabilities
    );
    for kept in [Aspect::Tool, Aspect::Read, Aspect::Write] {
        assert!(
            carries(&grant, kept),
            "`{kept:?}` was not refused, so it stays: {:?}",
            grant.capabilities
        );
    }
}

/// Nothing denied: the same manifest gets everything it asked for.
#[test]
fn an_extension_nobody_refused_keeps_its_capabilities() {
    let fx = Fixture::new();
    let grant = grant_for(
        &fx.engine,
        &ext(),
        &wants(),
        Consent::Always,
        &Fixture::scope(),
    );
    for aspect in [Aspect::Tool, Aspect::Read, Aspect::Write, Aspect::Spawn] {
        assert!(
            carries(&grant, aspect),
            "`{aspect:?}` should be granted: {:?}",
            grant.capabilities
        );
    }
}

/// The rule is read for **this** extension, not for the agent: a deny written
/// under one `ext:` heading does not cut another one down.
#[test]
fn the_refusal_is_read_for_the_named_extension() {
    let fx = Fixture::with_rules(RULES);
    let other = ExtId::new("cartographer").expect("a valid id");
    let grant = grant_for(
        &fx.engine,
        &other,
        &wants(),
        Consent::Always,
        &Fixture::scope(),
    );
    assert!(
        carries(&grant, Aspect::Spawn),
        "the deny named `buildgraph`: {:?}",
        grant.capabilities
    );
}

/// An aspect the manifest never mentioned is still asked about, because the
/// baseline is what a tool's `requires` may name whatever the manifest said.
#[test]
fn an_undeclared_aspect_is_still_refusable() {
    let fx = Fixture::with_rules(RULES);
    let grant = grant_for(
        &fx.engine,
        &ext(),
        // Declares nothing at all.
        &[],
        Consent::Always,
        &Fixture::scope(),
    );
    assert!(
        !carries(&grant, Aspect::Spawn),
        "the deny applies whether or not the manifest asked: {:?}",
        grant.capabilities
    );
}

/// The decision is in the audit, with the rule that produced it.
#[test]
fn the_decision_is_evidence() {
    let fx = Fixture::with_rules(RULES);
    let _ = grant_for(
        &fx.engine,
        &ext(),
        &wants(),
        Consent::Always,
        &Fixture::scope(),
    );
    let records = fx.audit.records();
    let decision = records
        .iter()
        .find_map(|r| match &r.event {
            orrery_audit::AuditEvent::CapabilityDecision {
                subject,
                request,
                verdict,
                rule_text,
                ..
            } if request.starts_with("spawn(") => {
                Some((subject.clone(), *verdict, rule_text.clone()))
            }
            _ => None,
        })
        .expect("the spawn decision is in the audit");
    assert_eq!(decision.0, orrery_proto::Subject::Ext(ext()));
    assert_eq!(decision.1, orrery_audit::Verdict::Deny);
    assert_eq!(
        decision.2.as_deref(),
        Some("spawn(*)"),
        "the audit must name the rule that produced the decision"
    );
}
