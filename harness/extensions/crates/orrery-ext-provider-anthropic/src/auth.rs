//! API-key auth over a named credential grant.
//!
//! The provider never reads a key off disk or out of config. It asks a
//! [`CredStore`] for a grant by name, and what is behind that name is the
//! broker's business — which is what keeps a credential out of every log line,
//! every crash dump and every extension that was not granted it.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_proto::{Field, FieldKind, Surface, SurfaceKind};
use orrery_provider::{AuthCtx, AuthMethod, AuthState, ProviderAuth, ProviderError};
use parking_lot::RwLock;

/// The broker, as this crate needs to see it.
///
/// TODO(plan-07): the real implementation is the `creds` grant in
/// `orrery-broker`. Until it lands, [`EnvCredStore`] is the dev-only stand-in
/// and [`MemoryCredStore`] is what the tests use. Both go when the broker
/// arrives; this trait does not.
pub trait CredStore: Send + Sync {
    /// Read a grant.
    ///
    /// # Errors
    ///
    /// Only when the store itself is unreachable. A grant that is simply not
    /// set is `Ok(None)` — a missing credential is a login prompt, not a
    /// failure.
    fn get(&self, grant: &str) -> Result<Option<String>, ProviderError>;

    /// Write a grant.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Auth`] when the store cannot hold secrets — the
    /// environment-variable stand-in cannot, and says so instead of pretending.
    fn put(&self, grant: &str, secret: &str) -> Result<(), ProviderError>;

    /// Forget a grant.
    ///
    /// # Errors
    ///
    /// As [`CredStore::put`].
    fn clear(&self, grant: &str) -> Result<(), ProviderError>;
}

/// A store backed by `ANTHROPIC_API_KEY` and friends.
///
/// TODO(plan-07): dev-only. Read-only on purpose: a login that wrote to the
/// process environment would be lost at exit and invisible to every other
/// process, which is worse than refusing.
#[derive(Debug, Default, Clone, Copy)]
pub struct EnvCredStore;

impl EnvCredStore {
    fn var(grant: &str) -> String {
        format!("{}_API_KEY", grant.to_uppercase().replace('-', "_"))
    }
}

impl CredStore for EnvCredStore {
    fn get(&self, grant: &str) -> Result<Option<String>, ProviderError> {
        Ok(std::env::var(Self::var(grant))
            .ok()
            .filter(|v| !v.trim().is_empty()))
    }

    fn put(&self, grant: &str, _secret: &str) -> Result<(), ProviderError> {
        Err(ProviderError::Auth(format!(
            "cannot store a credential in the environment: set {} instead",
            Self::var(grant)
        )))
    }

    fn clear(&self, grant: &str) -> Result<(), ProviderError> {
        Err(ProviderError::Auth(format!(
            "cannot clear a credential in the environment: unset {}",
            Self::var(grant)
        )))
    }
}

/// An in-process store, for tests and for the cancellation harness.
#[derive(Debug, Default)]
pub struct MemoryCredStore {
    grants: RwLock<std::collections::HashMap<String, String>>,
}

impl CredStore for MemoryCredStore {
    fn get(&self, grant: &str) -> Result<Option<String>, ProviderError> {
        Ok(self.grants.read().get(grant).cloned())
    }

    fn put(&self, grant: &str, secret: &str) -> Result<(), ProviderError> {
        self.grants
            .write()
            .insert(grant.to_owned(), secret.to_owned());
        Ok(())
    }

    fn clear(&self, grant: &str) -> Result<(), ProviderError> {
        self.grants.write().remove(grant);
        Ok(())
    }
}

/// `x-api-key` auth over a named grant.
#[derive(Clone)]
pub struct ApiKeyAuth {
    grant: String,
    store: Arc<dyn CredStore>,
}

impl std::fmt::Debug for ApiKeyAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the store's contents.
        f.debug_struct("ApiKeyAuth")
            .field("grant", &self.grant)
            .finish_non_exhaustive()
    }
}

/// Both variants are declared. `OAuth` has no flow yet — see `login`.
const METHODS: &[AuthMethod] = &[AuthMethod::ApiKey, AuthMethod::OAuth];

impl ApiKeyAuth {
    /// Auth for one grant name.
    #[must_use]
    pub fn new(grant: impl Into<String>, store: Arc<dyn CredStore>) -> Self {
        Self {
            grant: grant.into(),
            store,
        }
    }

    /// The key, when there is one.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Auth`] when the grant is not set — the caller is about
    /// to make a request and has nothing to sign it with.
    pub fn key(&self) -> Result<String, ProviderError> {
        self.store
            .get(&self.grant)?
            .ok_or_else(|| ProviderError::Auth(self.missing()))
    }

    fn missing(&self) -> String {
        format!(
            "no credential for the `{}` grant: run `orrery auth login {}`",
            self.grant, self.grant
        )
    }

    /// The form a client draws to collect a key. Declared, not drawn: one
    /// description serves ratatui, Ink, the ADE and `--json`.
    fn form(&self) -> Surface {
        Surface::new(SurfaceKind::Form {
            fields: vec![Field {
                name: "api_key".to_owned(),
                label: format!("Anthropic API key for the `{}` grant", self.grant),
                // Secret, so no client echoes it into a transcript.
                kind: FieldKind::Secret {},
                required: true,
                default: None,
            }],
            submit: "Save".to_owned(),
        })
    }
}

#[async_trait]
impl ProviderAuth for ApiKeyAuth {
    fn methods(&self) -> &[AuthMethod] {
        METHODS
    }

    async fn state(&self) -> Result<AuthState, ProviderError> {
        Ok(match self.store.get(&self.grant)? {
            Some(_) => AuthState::Ready {
                // An API key names no account and never expires. Saying so
                // beats inventing a value, and beats a round trip to
                // `/v1/models` that would cost money to answer a UI question.
                account: None,
                expires_at: None,
            },
            None => AuthState::NeedsLogin {
                reason: self.missing(),
            },
        })
    }

    async fn login(&self, ctx: &dyn AuthCtx) -> Result<AuthState, ProviderError> {
        // TODO(phase-5): `AuthMethod::OAuth`. The device-code flow declares its
        // steps through the same `AuthCtx`; until it exists, asking for one is
        // `NeedsLogin` rather than a half-finished browser dance.
        let answer = ctx.ask(self.form()).await?;
        let key = answer
            .get("api_key")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if key.is_empty() {
            return Err(ProviderError::Auth(
                "no API key was provided; nothing was stored".to_owned(),
            ));
        }
        self.store.put(&self.grant, key)?;
        self.state().await
    }

    async fn refresh(&self) -> Result<AuthState, ProviderError> {
        // An API key has nothing to refresh, so this is `state` — which makes
        // it trivially idempotent under concurrent passes, as the trait
        // requires.
        self.state().await
    }

    async fn logout(&self) -> Result<(), ProviderError> {
        self.store.clear(&self.grant)
    }
}

/// The OAuth flow, once there is one.
///
/// TODO(phase-5). Declared here rather than left out so that a client can show
/// "sign in with Anthropic" as unavailable rather than as absent.
#[must_use]
pub fn oauth_not_implemented() -> AuthState {
    AuthState::NeedsLogin {
        reason: "oauth not implemented".to_owned(),
    }
}
