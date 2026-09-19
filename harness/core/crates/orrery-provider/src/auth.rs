//! Signing in, declared as UI rather than drawn as UI.
//!
//! `login` describes its steps as [`Surface`]s through [`AuthCtx`], so one flow
//! serves ratatui, Ink, the ADE and `--json`, and a profile with
//! `consent = "never"` fails rather than prompting.

use async_trait::async_trait;
use orrery_proto::Surface;

use crate::error::ProviderError;

/// How a provider can be signed in to.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AuthMethod {
    /// A long-lived key held in the credential broker under a named grant.
    ApiKey,
    /// A device-code flow: the person types a short code on a page the
    /// provider names, and the flow polls until they have.
    OAuth,
}

/// Where a provider's credentials stand.
///
/// # It is serialised, because a client has to draw it
///
/// The `auth.state` view binding (`orrery-ext-api`'s §6.7 floor) renders this
/// from **data**: the tag below is the wire contract between this enum and that
/// renderer, which is why the shape is pinned by a test in this file rather
/// than left to whatever serde happened to emit. Before this, the only consumer
/// was the kernel's refusal string, so a device-code login could only reach a
/// person as prose somebody had scraped.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum AuthState {
    /// No credential is configured, and none is needed.
    Anonymous,
    /// Good to go.
    Ready {
        /// Who we are, when the provider says.
        account: Option<String>,
        /// Unix seconds at which this stops being true.
        expires_at: Option<u64>,
    },
    /// A credential is needed and there is not one.
    ///
    /// A turn that starts here is refused at `provider.before` with a typed
    /// error the client renders as a login prompt — never a stall mid-pass
    /// waiting on a browser. That check belongs to the kernel; this crate only
    /// reports the state.
    NeedsLogin {
        /// What to tell the person.
        reason: String,
    },
    /// There was a credential and it has run out.
    Expired,
    /// A device-code login is running and is waiting on the person.
    ///
    /// The variant carries everything a client needs to draw the wait, because
    /// the alternative is each client inventing its own: the code to type, the
    /// page to type it on, when it stops being valid, and how often the flow is
    /// polling. `login` returns this only when it is handed back before the
    /// person finished — a flow that runs to completion returns
    /// [`AuthState::Ready`].
    Pending {
        /// What the person types at [`verification_uri`](Self::Pending::verification_uri).
        user_code: String,
        /// Where they type it.
        verification_uri: String,
        /// The same page with the code already filled in, when the provider
        /// offers one. A client shows this as the link and keeps `user_code`
        /// visible anyway, because the two can be opened on different devices.
        verification_uri_complete: Option<String>,
        /// Unix seconds at which the code stops working.
        expires_at: u64,
        /// How long the flow waits between polls, as the **server** stated it.
        /// A client that draws a countdown draws this one, so what it shows and
        /// what the flow does cannot drift.
        interval_secs: u64,
    },
}

/// The client's side of a login: one question, one answer.
///
/// Implemented by the transport's client bridge (plan 08). A profile that
/// refuses to prompt implements it by returning
/// [`ProviderError::Auth`].
#[async_trait]
pub trait AuthCtx: Send + Sync {
    /// Put a surface in front of whoever is driving, and wait for the answer.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Auth`] when there is nobody to ask, or the person
    /// declined.
    async fn ask(&self, surface: Surface) -> Result<serde_json::Value, ProviderError>;
}

/// A provider's credential lifecycle.
#[async_trait]
pub trait ProviderAuth: Send + Sync {
    /// The ways in.
    fn methods(&self) -> &[AuthMethod];

    /// Where we stand, without side effects.
    ///
    /// # Errors
    ///
    /// Only for a broker that could not be reached — a *missing* credential is
    /// [`AuthState::NeedsLogin`], not an error.
    async fn state(&self) -> Result<AuthState, ProviderError>;

    /// Run the login flow, asking `ctx` for anything it needs.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Auth`] when the flow cannot complete.
    async fn login(&self, ctx: &dyn AuthCtx) -> Result<AuthState, ProviderError>;

    /// Renew. **Idempotent under concurrent passes**: two passes refreshing at
    /// once must not produce two credentials or two browser windows.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Auth`] when renewal is impossible; the caller's answer
    /// is [`AuthState::NeedsLogin`].
    async fn refresh(&self) -> Result<AuthState, ProviderError>;

    /// Forget the credential.
    ///
    /// # Errors
    ///
    /// Only for a broker that could not be reached.
    async fn logout(&self) -> Result<(), ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::AuthState;

    /// The wire shape the `auth.state` view binding reads. A rename here
    /// without a rename there would leave a client drawing "nothing to show"
    /// for a login prompt, which is exactly the failure this pins.
    #[test]
    fn the_tagged_shape_is_the_contract() {
        let pending = AuthState::Pending {
            user_code: "WDJB-MJHT".to_owned(),
            verification_uri: "https://example.test/device".to_owned(),
            verification_uri_complete: None,
            expires_at: 1_800,
            interval_secs: 5,
        };
        let value = serde_json::to_value(&pending).expect("it serialises");
        assert_eq!(value["state"], "pending");
        assert_eq!(value["userCode"], "WDJB-MJHT");
        assert_eq!(value["verificationUri"], "https://example.test/device");
        assert_eq!(value["intervalSecs"], 5);

        assert_eq!(
            serde_json::to_value(AuthState::NeedsLogin {
                reason: "no key".to_owned()
            })
            .expect("it serialises")["state"],
            "needs-login"
        );
        assert_eq!(
            serde_json::to_value(AuthState::Anonymous).expect("it serialises")["state"],
            "anonymous"
        );

        let back: AuthState = serde_json::from_value(value).expect("it round-trips");
        assert_eq!(back, pending);
    }
}
