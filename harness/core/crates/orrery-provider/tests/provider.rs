//! Task 1: the trait stays object-safe.

use std::sync::Arc;

use orrery_provider::Provider;

/// Compiles only while `Provider` is object-safe. The day someone adds a
/// generic method, or an `async fn`, this file stops building.
fn takes(_: Arc<dyn Provider>) {}

#[test]
fn is_object_safe() {
    // The assertion is the signature above; naming it keeps the fn live.
    let _ = takes as fn(Arc<dyn Provider>);
}
