//! API-key auth over a named credential grant.
//!
//! The provider never reads a key off disk or out of config. It asks a
//! [`CredStore`] for a grant by name, and what is behind that name is the
//! broker's business — which is what keeps a credential out of every log line,
//! every crash dump and every extension that was not granted it.
//!
//! # Where the store comes from
//!
//! [`CredStore`] is `orrery_ext_api::creds`'s, not this crate's: plan 07 landed,
//! and there is now one definition every provider extension shares rather than
//! a copy per vendor. A session wires
//! [`LayeredCredStore::over_broker`](orrery_ext_api::creds::LayeredCredStore::over_broker),
//! so a login writes through the `creds` grant and the environment variable is
//! only a documented development fallback — see [`EnvCredStore`] for exactly
//! what it is for and what it refuses to do.
//!
//! [`OAuth`](orrery_provider::AuthMethod::OAuth) is in [`crate::oauth`].

use std::sync::Arc;

use async_trait::async_trait;
use orrery_ext_api::BrokerError;
use orrery_proto::{Field, FieldKind, Surface, SurfaceKind};
use orrery_provider::{AuthCtx, AuthMethod, AuthState, ProviderAuth, ProviderError};

pub use orrery_ext_api::creds::{
    BrokerCredStore, CredStore, EnvCredStore, LayeredCredStore, MemoryCredStore,
};

/// A broker refusal, in the vocabulary a provider reports.
///
/// A denial and an unreachable store are both `Auth` here: from the kernel's
/// side they mean the same thing — this turn cannot be signed — and the
/// distinction that matters is already in the message.
#[must_use]
pub fn store_error(e: BrokerError) -> ProviderError {
    ProviderError::Auth(e.to_string())
}

/// What a client is told when a grant is not set.
#[must_use]
pub fn missing_reason(grant: &str) -> String {
    format!("no credential for the `{grant}` grant: run `orrery auth login {grant}`")
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

/// What this half offers. The OAuth half is [`DeviceCodeAuth`](crate::oauth::DeviceCodeAuth),
/// which is a separate [`ProviderAuth`] rather than a branch inside this one:
/// they store different things under different names and share nothing but the
/// grant.
const METHODS: &[AuthMethod] = &[AuthMethod::ApiKey];

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
    pub async fn key(&self) -> Result<String, ProviderError> {
        self.store
            .get(&self.grant)
            .await
            .map_err(store_error)?
            .ok_or_else(|| ProviderError::Auth(self.missing()))
    }

    fn missing(&self) -> String {
        missing_reason(&self.grant)
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
        Ok(
            match self.store.has(&self.grant).await.map_err(store_error)? {
                true => AuthState::Ready {
                    // An API key names no account and never expires. Saying so
                    // beats inventing a value, and beats a round trip to
                    // `/v1/models` that would cost money to answer a UI question.
                    account: None,
                    expires_at: None,
                },
                false => AuthState::NeedsLogin {
                    reason: self.missing(),
                },
            },
        )
    }

    async fn login(&self, ctx: &dyn AuthCtx) -> Result<AuthState, ProviderError> {
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
        self.store
            .put(&self.grant, key)
            .await
            .map_err(store_error)?;
        self.state().await
    }

    async fn refresh(&self) -> Result<AuthState, ProviderError> {
        // An API key has nothing to refresh, so this is `state` — which makes
        // it trivially idempotent under concurrent passes, as the trait
        // requires.
        self.state().await
    }

    async fn logout(&self) -> Result<(), ProviderError> {
        self.store.clear(&self.grant).await.map_err(store_error)
    }
}
