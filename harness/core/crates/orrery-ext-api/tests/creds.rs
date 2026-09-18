//! Plan 07, task 9: a credential lives behind the `creds` grant, not in an
//! environment variable, and a store that cannot hold one says so.

use std::sync::Arc;

use orrery_ext_api::creds::{BrokerCredStore, CredStore, EnvCredStore, MemoryCredStore};
use orrery_ext_api::testing::{BrokerCall, load_for_test};
use orrery_ext_api::{BrokerError, DeniesEverything};

const MANIFEST: &str = r#"
api = "orrery-ext/1"
runtime = "native"

[extension]
id = "signer"
version = "0.1.0"

[provides]
providers = ["signer"]

[requires]
creds = ["signer"]
"#;

#[tokio::test]
async fn a_granted_credential_comes_through_the_broker_and_is_recorded() {
    let harness = load_for_test(MANIFEST, &["creds:signer"]).expect("the manifest loads");
    harness
        .broker
        .add_credential("signer", "sk-from-the-broker");
    let store = BrokerCredStore::new("ext", harness.ctx("sign").broker);

    assert_eq!(
        store.get("signer").await.expect("read"),
        Some("sk-from-the-broker".to_owned())
    );
    // The point of the grant: the read is a broker call, so it is in the
    // ledger and a person can see that the extension asked.
    assert_eq!(
        harness.recorded(),
        vec![BrokerCall::Credential {
            name: "signer".to_owned(),
            allowed: true,
        }]
    );
}

#[tokio::test]
async fn an_ungranted_credential_is_denied_rather_than_empty() {
    // No `creds` grant at all.
    let harness = load_for_test(MANIFEST, &[]).expect("the manifest loads");
    harness.broker.add_credential("signer", "sk-never-seen");
    let store = BrokerCredStore::new("ext", harness.ctx("sign").broker);

    let e = store.get("signer").await.expect_err("denied");
    assert!(matches!(e, BrokerError::Denied { .. }), "{e:?}");
    // Denied, not `Ok(None)`: "you may not have this" and "there is none" are
    // different answers, and collapsing them sends a person to a login flow
    // that will not help.
}

#[tokio::test]
async fn a_missing_credential_is_none_not_an_error() {
    let harness = load_for_test(MANIFEST, &["creds:signer"]).expect("the manifest loads");
    let store = BrokerCredStore::new("ext", harness.ctx("sign").broker);
    assert_eq!(store.get("signer").await.expect("read"), None);
}

#[tokio::test]
async fn storing_writes_through_the_broker() {
    let harness = load_for_test(MANIFEST, &["creds:signer"]).expect("the manifest loads");
    let store = BrokerCredStore::new("ext", harness.ctx("sign").broker);

    store.put("signer", "sk-typed-at-login").await.expect("put");
    assert_eq!(
        store.get("signer").await.expect("read"),
        Some("sk-typed-at-login".to_owned())
    );
    store.clear("signer").await.expect("clear");
    assert_eq!(store.get("signer").await.expect("read"), None);
}

#[tokio::test]
async fn a_broker_that_denies_everything_denies_this_too() {
    let store = BrokerCredStore::new("ext", Arc::new(DeniesEverything));
    assert!(store.get("signer").await.is_err());
    assert!(store.put("signer", "x").await.is_err());
}

#[tokio::test]
async fn the_env_fallback_reads_but_refuses_to_write() {
    // Named so no real key can collide with it.
    let var = EnvCredStore::var("orrery-test-grant");
    assert_eq!(var, "ORRERY_TEST_GRANT_API_KEY");

    let store = EnvCredStore;
    assert_eq!(store.get("orrery-test-grant").await.expect("read"), None);

    let e = store
        .put("orrery-test-grant", "x")
        .await
        .expect_err("the environment cannot hold a secret");
    assert!(e.to_string().contains(&var), "{e}");
}

#[tokio::test]
async fn the_memory_store_round_trips() {
    let store = MemoryCredStore::default();
    assert_eq!(store.get("signer").await.expect("read"), None);
    store.put("signer", "sk-1").await.expect("put");
    assert_eq!(
        store.get("signer").await.expect("read"),
        Some("sk-1".to_owned())
    );
    store.clear("signer").await.expect("clear");
    assert_eq!(store.get("signer").await.expect("read"), None);
}
