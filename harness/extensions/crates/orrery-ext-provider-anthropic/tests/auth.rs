//! Task 7: a missing key is a login prompt, not a 401 halfway through a turn.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_ext_provider_anthropic::auth::{ApiKeyAuth, CredStore, MemoryCredStore};
use orrery_proto::{Surface, SurfaceKind};
use orrery_provider::{AuthCtx, AuthMethod, AuthState, ProviderAuth, ProviderError};

/// Answers whatever it is asked with a fixed value.
struct Canned(serde_json::Value);

#[async_trait]
impl AuthCtx for Canned {
    async fn ask(&self, surface: Surface) -> Result<serde_json::Value, ProviderError> {
        // The flow must declare itself as a surface, not draw a prompt.
        match surface.kind {
            SurfaceKind::Form { ref fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "api_key");
                // A key is a secret, and the field has to say so or a client
                // will echo it into a transcript.
                assert!(
                    matches!(fields[0].kind, orrery_proto::FieldKind::Secret {}),
                    "{:?}",
                    fields[0].kind
                );
            }
            ref other => panic!("expected a form, got {other:?}"),
        }
        Ok(self.0.clone())
    }
}

/// Refuses to ask anyone anything, as `consent = "never"` does.
struct NeverAsks;

#[async_trait]
impl AuthCtx for NeverAsks {
    async fn ask(&self, _surface: Surface) -> Result<serde_json::Value, ProviderError> {
        Err(ProviderError::Auth("this profile never prompts".to_owned()))
    }
}

#[tokio::test]
async fn missing_key_is_needs_login() {
    let auth = ApiKeyAuth::new("anthropic", Arc::new(MemoryCredStore::default()));
    match auth.state().await.expect("no error for a missing key") {
        AuthState::NeedsLogin { reason } => assert!(reason.contains("anthropic"), "{reason}"),
        other => panic!("expected NeedsLogin, got {other:?}"),
    }
}

#[tokio::test]
async fn a_present_key_is_ready() {
    let store: Arc<dyn CredStore> = Arc::new(MemoryCredStore::default());
    store.put("anthropic", "sk-ant-test").await.expect("stored");
    let auth = ApiKeyAuth::new("anthropic", store);
    assert!(matches!(
        auth.state().await.expect("state"),
        AuthState::Ready { .. }
    ));
    assert!(auth.methods().contains(&AuthMethod::ApiKey));
    // OAuth is `oauth::DeviceCodeAuth`, a `ProviderAuth` of its own, and
    // `tests/oauth.rs` is where it is exercised. This half offers one method
    // and says so, rather than advertising a flow it does not run.
    assert!(!auth.methods().contains(&AuthMethod::OAuth));
}

#[tokio::test]
async fn login_asks_for_the_key_and_stores_it() {
    let store: Arc<dyn CredStore> = Arc::new(MemoryCredStore::default());
    let auth = ApiKeyAuth::new("anthropic", Arc::clone(&store));
    let state = auth
        .login(&Canned(serde_json::json!({ "api_key": "sk-ant-typed" })))
        .await
        .expect("login");
    assert!(matches!(state, AuthState::Ready { .. }));
    assert_eq!(
        store.get("anthropic").await.expect("read").as_deref(),
        Some("sk-ant-typed")
    );
}

#[tokio::test]
async fn a_profile_that_never_prompts_fails_rather_than_hanging() {
    let auth = ApiKeyAuth::new("anthropic", Arc::new(MemoryCredStore::default()));
    let e = auth.login(&NeverAsks).await.expect_err("refused");
    assert_eq!(e.code(), "auth");
    assert!(!e.is_retryable());
}

#[tokio::test]
async fn an_empty_answer_is_rejected() {
    let auth = ApiKeyAuth::new("anthropic", Arc::new(MemoryCredStore::default()));
    let e = auth
        .login(&Canned(serde_json::json!({ "api_key": "   " })))
        .await
        .expect_err("rejected");
    assert_eq!(e.code(), "auth");
}

#[tokio::test]
async fn refresh_is_idempotent_and_logout_forgets() {
    let store: Arc<dyn CredStore> = Arc::new(MemoryCredStore::default());
    store.put("anthropic", "sk-ant-test").await.expect("stored");
    let auth = ApiKeyAuth::new("anthropic", Arc::clone(&store));

    let (a, b) = tokio::join!(auth.refresh(), auth.refresh());
    assert_eq!(a.expect("a"), b.expect("b"));

    auth.logout().await.expect("logout");
    assert!(matches!(
        auth.state().await.expect("state"),
        AuthState::NeedsLogin { .. }
    ));
}
