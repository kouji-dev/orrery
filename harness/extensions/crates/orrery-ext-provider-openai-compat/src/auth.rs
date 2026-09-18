//! A key that is usually not there.
//!
//! This is the one provider in the tree where **no credential is the normal
//! case**: ollama on a laptop wants nothing, vllm behind a reverse proxy wants
//! a bearer token, and a hosted compatible gateway wants a real key. So an
//! absent grant is [`AuthState::Anonymous`] rather than
//! [`AuthState::NeedsLogin`] — the kernel refuses a turn that starts at
//! `NeedsLogin`, and refusing "I am talking to my own laptop" would make the
//! crate useless for the thing it exists for.
//!
//! The store is `orrery_ext_api::creds`'s, shared with every other provider:
//! the key lives behind the `creds` grant, not in this crate's idea of a config
//! file.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_proto::{Field, FieldKind, Surface, SurfaceKind};
use orrery_provider::{AuthCtx, AuthMethod, AuthState, ProviderAuth, ProviderError};
use parking_lot::RwLock;

pub use orrery_ext_api::creds::{
    BrokerCredStore, CredStore, EnvCredStore, LayeredCredStore, MemoryCredStore,
};

/// An API key, if there happens to be one.
pub struct OptionalKeyAuth {
    grant: String,
    store: Arc<dyn CredStore>,
    /// The last value read, so [`key_if_present`](Self::key_if_present) can
    /// answer a *synchronous* caller.
    ///
    /// Reading a credential is a broker call and therefore async, and
    /// `Provider::stream` is a plain fn — that is what lets the kernel hold
    /// `Arc<dyn Provider>`. The cache is filled by `state`, `login` and
    /// `refresh`, all of which the kernel calls before a turn starts. An empty
    /// cache signs nothing, which for a local server is exactly right.
    cached: RwLock<Option<String>>,
}

impl std::fmt::Debug for OptionalKeyAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key, and never whether one is cached — that leaks whether
        // the endpoint is authenticated, which is not this type's to tell.
        f.debug_struct("OptionalKeyAuth")
            .field("grant", &self.grant)
            .finish_non_exhaustive()
    }
}

impl OptionalKeyAuth {
    /// Auth for one grant name.
    #[must_use]
    pub fn new(grant: impl Into<String>, store: Arc<dyn CredStore>) -> Self {
        Self {
            grant: grant.into(),
            store,
            cached: RwLock::new(None),
        }
    }

    /// The key, if one has been read. Never blocks and never errors: a server
    /// that wants no key must not be held up by a credential lookup.
    #[must_use]
    pub fn key_if_present(&self) -> Option<String> {
        self.cached.read().clone()
    }

    /// Read the grant and remember what it said.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Auth`] when the store refused — a denial is not "there
    /// is no key", and treating it as one would send an unsigned request to a
    /// server that will answer 401.
    pub async fn resolve(&self) -> Result<Option<String>, ProviderError> {
        let held = self
            .store
            .get(&self.grant)
            .await
            .map_err(|e| ProviderError::Auth(e.to_string()))?;
        *self.cached.write() = held.clone();
        Ok(held)
    }

    /// The form a client draws to collect a key. Declared, not drawn.
    fn form(&self) -> Surface {
        Surface::new(SurfaceKind::Form {
            fields: vec![Field {
                name: "api_key".to_owned(),
                label: format!(
                    "API key for the `{}` grant (leave blank for a local server)",
                    self.grant
                ),
                // Secret, so no client echoes it into a transcript.
                kind: FieldKind::Secret {},
                // Not required: blank is a legitimate answer here.
                required: false,
                default: None,
            }],
            submit: "Save".to_owned(),
        })
    }
}

/// One way in. There is no OAuth flow for "whatever you are running", because
/// there is no authorization server to name.
const METHODS: &[AuthMethod] = &[AuthMethod::ApiKey];

#[async_trait]
impl ProviderAuth for OptionalKeyAuth {
    fn methods(&self) -> &[AuthMethod] {
        METHODS
    }

    async fn state(&self) -> Result<AuthState, ProviderError> {
        Ok(match self.resolve().await? {
            Some(_) => AuthState::Ready {
                // A key names no account and never expires. Saying so beats a
                // round trip to `/v1/models` that a cold-started server will
                // not answer quickly.
                account: None,
                expires_at: None,
            },
            // The normal case. **Not** `NeedsLogin`: the kernel refuses a turn
            // that starts there, and a local model needs no sign-in.
            None => AuthState::Anonymous,
        })
    }

    async fn login(&self, ctx: &dyn AuthCtx) -> Result<AuthState, ProviderError> {
        let answer = ctx.ask(self.form()).await?;
        let key = answer
            .get("api_key")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        if key.is_empty() {
            // Blank means "this server wants no key", which is a decision, not
            // an error — and it clears any stale one rather than leaving a key
            // behind that the person believes they removed.
            let _ = self.store.clear(&self.grant).await;
            *self.cached.write() = None;
            return Ok(AuthState::Anonymous);
        }
        self.store
            .put(&self.grant, &key)
            .await
            .map_err(|e| ProviderError::Auth(e.to_string()))?;
        self.state().await
    }

    async fn refresh(&self) -> Result<AuthState, ProviderError> {
        // A key has nothing to refresh, so this is `state` — which makes it
        // trivially idempotent under concurrent passes, as the trait requires.
        self.state().await
    }

    async fn logout(&self) -> Result<(), ProviderError> {
        *self.cached.write() = None;
        self.store
            .clear(&self.grant)
            .await
            .map_err(|e| ProviderError::Auth(e.to_string()))
    }
}
