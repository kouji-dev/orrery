//! Task 5 · tokens.
//!
//! The two compile-fail properties — `token::cannot_be_constructed_externally`
//! and `token::is_not_serializable` — are `compile_fail` **doctests** on
//! [`orrery_policy::CapabilityToken`], not `trybuild` cases. A doctest is
//! compiled as a separate crate against the real rlib, which is exactly the
//! out-of-crate vantage point the property is about, and it needs no dependency
//! that is not already here. `cargo test -p orrery-policy --doc` runs them.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Workspace, wide_scope};
use orrery_policy::{
    Decision, PendingCall, PolicyBuilder, PolicyEngine, TokenError, TokenLedger, TokenMinter,
};
use orrery_proto::{CallId, Layer, Subject};

fn allowing(ws: &Workspace, ttl: Duration) -> PolicyEngine {
    let rules = PolicyBuilder::new(ws.root())
        .layer_toml(
            "[permissions]\nallow = [\"tool(*)\"]\n",
            "p.toml",
            Layer::Project,
            false,
        )
        .unwrap()
        .build()
        .unwrap();
    PolicyEngine::new(rules)
        .with_minter(TokenMinter::new(Arc::new(TokenLedger::new())).with_ttl(ttl))
}

fn token_for(engine: &PolicyEngine, call: &PendingCall) -> orrery_policy::CapabilityToken {
    match engine.check(call, &Subject::Agent, &wide_scope()) {
        Decision::Allow { token, .. } => token,
        other => panic!("expected Allow, got {other:?}"),
    }
}

#[test]
fn single_use() {
    let ws = Workspace::new();
    let engine = allowing(&ws, Duration::from_secs(60));
    let token = token_for(&engine, &PendingCall::tool("git.status"));
    let nonce = token.nonce();

    assert_eq!(engine.ledger().redeem(nonce), Ok(()));
    assert_eq!(engine.ledger().redeem(nonce), Err(TokenError::Spent));
}

#[test]
fn revoked_on_cancel() {
    let ws = Workspace::new();
    let engine = allowing(&ws, Duration::from_secs(60));
    let call = CallId::new();
    let token = token_for(&engine, &PendingCall::tool("git.status").in_call(call));
    let nonce = token.nonce();

    engine.ledger().revoke_call(call);
    assert_eq!(engine.ledger().redeem(nonce), Err(TokenError::Revoked));
}

#[test]
fn revoking_one_call_leaves_another_alone() {
    let ws = Workspace::new();
    let engine = allowing(&ws, Duration::from_secs(60));
    let doomed = CallId::new();
    let spared = CallId::new();
    let a = token_for(&engine, &PendingCall::tool("git.status").in_call(doomed));
    let b = token_for(&engine, &PendingCall::tool("git.status").in_call(spared));

    engine.ledger().revoke_call(doomed);
    assert_eq!(engine.ledger().redeem(a.nonce()), Err(TokenError::Revoked));
    assert_eq!(engine.ledger().redeem(b.nonce()), Ok(()));
}

#[test]
fn expires() {
    let ws = Workspace::new();
    let engine = allowing(&ws, Duration::from_millis(1));
    let token = token_for(&engine, &PendingCall::tool("git.status"));
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(
        engine.ledger().redeem(token.nonce()),
        Err(TokenError::Expired),
        "a live nonce past its deadline is not good enough"
    );
}

#[test]
fn a_nonce_from_nowhere_is_unknown() {
    let ledger = TokenLedger::new();
    assert_eq!(ledger.redeem(9_999_999), Err(TokenError::Unknown));
}

/// The token names the rule that produced it, so the broker's own audit record
/// can say which rule allowed the thing it is about to do.
#[test]
fn a_token_names_its_rule_and_its_call() {
    let ws = Workspace::new();
    let engine = allowing(&ws, Duration::from_secs(60));
    let call = CallId::new();
    let pending = PendingCall::tool("git.status").in_call(call);
    let token = token_for(&engine, &pending);

    assert_eq!(token.call(), call);
    assert_eq!(token.aspect(), orrery_proto::Aspect::Tool);
    assert_eq!(token.scope().target, "git.status");
    assert_eq!(
        engine.rules().rule(token.rule()).map(|r| r.text.clone()),
        Some("tool(*)".to_owned())
    );
}

/// Revocation outlives the moment it happens.
///
/// A token is minted per broker call and redeemed immediately, so a tool whose
/// call has been cancelled would simply mint another one — and "cancelling a
/// call takes its capabilities back" would hold for a few microseconds and then
/// stop. A call that has been revoked mints nothing redeemable.
#[test]
fn a_revoked_call_cannot_mint_its_way_back() {
    let ws = Workspace::new();
    let engine = allowing(&ws, Duration::from_secs(60));
    let call = CallId::new();
    engine.ledger().revoke_call(call);

    let after = token_for(&engine, &PendingCall::tool("git.status").in_call(call));
    assert_eq!(
        engine.ledger().redeem(after.nonce()),
        Err(TokenError::Revoked),
        "a token minted for a call that was already revoked is born revoked"
    );
}
