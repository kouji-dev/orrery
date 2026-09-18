//! Task 9 · credentials, which are used and never read.

mod common;

use std::sync::Arc;

use common::Fixture;
use orrery_broker::{Broker, BrokerError, CredStore, CredUse, FileCredStore, RequestSlot};
use orrery_policy::PendingCall;

const SECRET: &str = "sk-live-not-a-real-key-000";

/// The property is about the **API**, not about a filter: there is no method
/// anywhere that hands a secret back.
///
/// The three that would are absent, and each is a compile error rather than a
/// review comment. `CredStore` has `apply`, `store` and `has`, and no `get`.
#[tokio::test]
async fn value_never_returned() {
    let fx = Fixture::new();
    fx.broker
        .creds_store()
        .store("API_KEY", SECRET)
        .expect("stored");

    let slot = RequestSlot::new("https://api.example.com/v1/messages");
    let token = fx.token(&PendingCall::creds("API_KEY"));
    fx.broker
        .creds(
            token,
            "API_KEY",
            CredUse::Header {
                slot: slot.clone(),
                header: "x-api-key".to_owned(),
                scheme: None,
            },
        )
        .await
        .expect("the credential is used");

    // The caller can see that it was authorised...
    assert!(slot.has_header("x-api-key"));
    assert_eq!(slot.header_names(), vec!["x-api-key".to_owned()]);
    // ...and cannot see what with. `Debug` does not leak it either.
    assert!(!format!("{slot:?}").contains(SECRET));

    // And nothing about the value reached the audit: the NAME is what is logged.
    let jsonl = fx.audit.to_jsonl();
    assert!(
        !jsonl.contains(SECRET),
        "a credential value reached the audit"
    );
}

/// Rewrite the stored value; a held reference keeps working.
#[tokio::test]
async fn rotation_is_transparent() {
    let fx = Fixture::new();
    let store = Arc::clone(fx.broker.creds_store());
    store.store("API_KEY", "old-value").unwrap();

    let before = RequestSlot::new("https://api.example.com/");
    let token = fx.token(&PendingCall::creds("API_KEY"));
    fx.broker
        .creds(
            token,
            "API_KEY",
            CredUse::Header {
                slot: before.clone(),
                header: "authorization".to_owned(),
                scheme: Some("Bearer".to_owned()),
            },
        )
        .await
        .unwrap();
    let first = before.header_digest("authorization").expect("set");

    // Rotate. Nothing holding the NAME changes.
    store.store("API_KEY", "new-value").unwrap();

    let after = RequestSlot::new("https://api.example.com/");
    let token = fx.token(&PendingCall::creds("API_KEY"));
    fx.broker
        .creds(
            token,
            "API_KEY",
            CredUse::Header {
                slot: after.clone(),
                header: "authorization".to_owned(),
                scheme: Some("Bearer".to_owned()),
            },
        )
        .await
        .expect("the same name still resolves");
    let second = after.header_digest("authorization").expect("set");

    assert_ne!(first, second, "rotation did not take effect");
    assert_eq!(
        second,
        orrery_audit::Digest::of_bytes(b"Bearer new-value"),
        "the new value is what was applied"
    );
}

#[tokio::test]
async fn an_unknown_name_is_a_value_not_a_panic() {
    let fx = Fixture::new();
    let token = fx.token(&PendingCall::creds("NOT_STORED"));
    let err = fx
        .broker
        .creds(
            token,
            "NOT_STORED",
            CredUse::Header {
                slot: RequestSlot::new("https://x/"),
                header: "authorization".to_owned(),
                scheme: None,
            },
        )
        .await
        .expect_err("nothing is stored under that name");
    assert!(matches!(err, BrokerError::NoCredential(_)), "{err}");
}

/// The file fallback, for a platform with no OS keychain.
#[test]
fn the_file_store_round_trips_a_name() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileCredStore::open(dir.path().join("creds")).expect("opened");
    assert!(!store.has("API_KEY"));
    store.store("API_KEY", SECRET).unwrap();
    assert!(store.has("API_KEY"));

    let slot = RequestSlot::new("https://x/");
    store
        .apply(
            "API_KEY",
            &CredUse::Header {
                slot: slot.clone(),
                header: "x-api-key".to_owned(),
                scheme: None,
            },
        )
        .unwrap();
    assert_eq!(
        slot.header_digest("x-api-key"),
        Some(orrery_audit::Digest::of_bytes(SECRET.as_bytes()))
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(dir.path().join("creds"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the credential file must be 0600");
    }
}
