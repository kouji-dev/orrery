//! The device-code flow, for real.
//!
//! RFC 8628. A person opens a page, types a short code, and this polls until
//! they have — which is the only login shape that works when the thing signing
//! in is a terminal on a machine with no browser, over ssh, or in a container.
//!
//! # Three seams, and why each is one
//!
//! - **[`OAuthTransport`]** is the HTTP side. It is a trait because the five
//!   answers that matter — `authorization_pending`, `slow_down`,
//!   `expired_token`, `access_denied` and a refresh — arrive minutes apart from
//!   a real server and need a person to produce any of them. `tests/oauth.rs`
//!   drives a fake server in this process and opens no socket.
//! - **[`Clock`]** is time. The flow waits the interval the *server* stated, so
//!   a test that could not move time would cost that interval per case in wall
//!   clock.
//! - **[`CredStore`]** is where the token lands: the `creds` grant, not a file
//!   this crate picked. A refresh token written anywhere else is a credential
//!   nobody can revoke.
//!
//! # What is stored, and under what names
//!
//! Three names under one grant, because a token has three parts and a client
//! asking "am I signed in" must not have to read any of them:
//!
//! | name | what |
//! |---|---|
//! | `anthropic` | the access token, what signs a request |
//! | `anthropic.refresh` | the refresh token, what avoids the next login |
//! | `anthropic.expires` | unix seconds, so `state()` can say `Expired` without a round trip |
//!
//! Implementation plan: `harness/docs/plans/03-provider-layer.md`, phase 5.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_ext_api::CredStore;
use orrery_proto::{Surface, SurfaceKind};
use orrery_provider::{AuthCtx, AuthMethod, AuthState, ProviderAuth, ProviderError};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::auth::store_error;

/// The suffix of the grant that holds the refresh token.
const REFRESH_SUFFIX: &str = ".refresh";
/// The suffix of the grant that holds the expiry.
const EXPIRES_SUFFIX: &str = ".expires";
/// What RFC 8628 §3.5 widens the interval by when the server says `slow_down`.
const SLOW_DOWN_STEP: u64 = 5;
/// What to poll at when the server named no interval. The RFC's own default.
const DEFAULT_INTERVAL: u64 = 5;

/// Where the flow talks to, and as whom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthConfig {
    /// The grant name the token is stored under.
    pub grant: String,
    /// The public client id. Not a secret: a device-code client has none, which
    /// is the reason the flow exists.
    pub client_id: String,
    /// What is being asked for.
    pub scope: String,
}

impl OAuthConfig {
    /// The first-party client.
    ///
    /// The id is a placeholder until Anthropic publishes one; it is public
    /// either way, so it is a constant here rather than a credential.
    #[must_use]
    pub fn anthropic() -> Self {
        Self {
            grant: crate::GRANT.to_owned(),
            client_id: "orrery-cli".to_owned(),
            scope: "user:inference".to_owned(),
        }
    }
}

/// One `POST` to the token endpoint, as fields rather than as a body.
///
/// A struct rather than a `HashMap` so that a fake server can match on
/// `grant_type` without parsing a form, and so that adding a field is a
/// compile error in every transport rather than a silently ignored key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenForm {
    /// `urn:ietf:params:oauth:grant-type:device_code`, or `refresh_token`.
    pub grant_type: String,
    /// The public client id.
    pub client_id: String,
    /// The device code, while polling.
    pub device_code: Option<String>,
    /// The refresh token, while refreshing.
    pub refresh_token: Option<String>,
}

/// The HTTP side of the flow.
///
/// Both methods return the *parsed* body rather than bytes: the shape is four
/// keys and this crate would otherwise have two JSON decoders, one of them only
/// exercised by tests.
#[async_trait]
pub trait OAuthTransport: Send + Sync {
    /// `POST /oauth/device/code`. Returns the device-code response.
    ///
    /// # Errors
    ///
    /// [`ProviderError`] as the transport classifies it. A failure here is
    /// before the person has done anything, so it is safe to retry.
    async fn device_code(&self, client_id: &str, scope: &str) -> Result<Value, ProviderError>;

    /// `POST /oauth/token`. Returns the status **and** the body, because the
    /// whole protocol is carried in `400` bodies: an `authorization_pending` is
    /// a `400`, and a transport that turned it into an error would make the
    /// normal case unreachable.
    ///
    /// # Errors
    ///
    /// [`ProviderError`] only when the request itself did not happen.
    async fn token(&self, form: TokenForm) -> Result<(u16, Value), ProviderError>;
}

/// Time, so a test can move it.
#[async_trait]
pub trait Clock: Send + Sync {
    /// Now, in unix seconds.
    fn now_unix(&self) -> u64;
    /// Wait.
    async fn sleep_secs(&self, secs: u64);
}

/// The real one.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

#[async_trait]
impl Clock for SystemClock {
    fn now_unix(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    async fn sleep_secs(&self, secs: u64) {
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
    }
}

/// What a started flow needs to finish, once a client has drawn the wait.
///
/// Handed back by [`DeviceCodeAuth::begin`] so a client that draws its own
/// spinner can poll on its own schedule. `login` is `begin` followed by
/// [`DeviceCodeAuth::wait`], which is the shape a terminal wants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingLogin {
    /// What is polled with.
    pub device_code: String,
    /// The interval the server stated, in seconds. Widens on `slow_down`.
    pub interval_secs: u64,
    /// Unix seconds at which the device code stops working.
    pub expires_at: u64,
}

/// `AuthMethod::OAuth`, implemented.
pub struct DeviceCodeAuth {
    config: OAuthConfig,
    store: Arc<dyn CredStore>,
    transport: Arc<dyn OAuthTransport>,
    clock: Arc<dyn Clock>,
    /// Held across a refresh so two concurrent passes make one request. The
    /// trait requires it: "two passes refreshing at once must not produce two
    /// credentials or two browser windows."
    refreshing: Mutex<()>,
}

impl std::fmt::Debug for DeviceCodeAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the store's contents.
        f.debug_struct("DeviceCodeAuth")
            .field("grant", &self.config.grant)
            .finish_non_exhaustive()
    }
}

/// Both variants, both implemented.
const METHODS: &[AuthMethod] = &[AuthMethod::ApiKey, AuthMethod::OAuth];

impl DeviceCodeAuth {
    /// A flow over a transport and a store of the caller's choosing.
    #[must_use]
    pub fn new(
        config: OAuthConfig,
        store: Arc<dyn CredStore>,
        transport: Arc<dyn OAuthTransport>,
    ) -> Self {
        Self {
            config,
            store,
            transport,
            clock: Arc::new(SystemClock),
            refreshing: Mutex::new(()),
        }
    }

    /// Use a clock other than the system's.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    fn refresh_grant(&self) -> String {
        format!("{}{REFRESH_SUFFIX}", self.config.grant)
    }

    fn expires_grant(&self) -> String {
        format!("{}{EXPIRES_SUFFIX}", self.config.grant)
    }

    /// Ask for a device code, describe the wait, and hand back what finishes
    /// it.
    ///
    /// The [`AuthState::Pending`] it returns is the whole reason the variant
    /// exists: a client that draws its own progress — the ADE, `--json` — gets
    /// the code, the URI and the expiry as data instead of scraping them out of
    /// a rendered string.
    ///
    /// # Errors
    ///
    /// [`ProviderError`] when the device-code request failed or answered
    /// something that is not a device-code response.
    pub async fn begin(
        &self,
        ctx: &dyn AuthCtx,
    ) -> Result<(AuthState, PendingLogin), ProviderError> {
        let body = self
            .transport
            .device_code(&self.config.client_id, &self.config.scope)
            .await?;

        let device_code = string(&body, "device_code").ok_or_else(|| {
            ProviderError::Auth("the device-code response carried no `device_code`".to_owned())
        })?;
        let user_code = string(&body, "user_code").ok_or_else(|| {
            ProviderError::Auth("the device-code response carried no `user_code`".to_owned())
        })?;
        let verification_uri = string(&body, "verification_uri")
            .or_else(|| string(&body, "verification_url"))
            .ok_or_else(|| {
                ProviderError::Auth(
                    "the device-code response named no page for the person to open".to_owned(),
                )
            })?;
        let verification_uri_complete = string(&body, "verification_uri_complete");
        let interval_secs = body
            .get("interval")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_INTERVAL)
            .max(1);
        // `expires_in` is a duration; everything downstream wants an instant,
        // because a duration is only meaningful next to the moment it was read.
        let expires_at = self.clock.now_unix()
            + body
                .get("expires_in")
                .and_then(Value::as_u64)
                .unwrap_or(900);

        // Described, not drawn. One description serves ratatui, Ink, the ADE
        // and `--json`, which is the same rule the API-key form follows.
        let _ = ctx
            .ask(Self::wait_surface(
                &user_code,
                &verification_uri,
                verification_uri_complete.as_deref(),
            ))
            .await;

        Ok((
            AuthState::Pending {
                user_code,
                verification_uri,
                verification_uri_complete,
                expires_at,
                interval_secs,
            },
            PendingLogin {
                device_code,
                interval_secs,
                expires_at,
            },
        ))
    }

    /// Poll until the person has finished, the code expires, or they decline.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Auth`] for `access_denied`, `expired_token`, and for a
    /// code this flow outlived. None of them is retryable: a person has to
    /// start over, and a kernel backing off would only burn budget.
    pub async fn wait(&self, mut pending: PendingLogin) -> Result<AuthState, ProviderError> {
        loop {
            if self.clock.now_unix() > pending.expires_at {
                return Err(ProviderError::Auth(format!(
                    "the login code expired before it was used: run \
                     `orrery auth login {}` again",
                    self.config.grant
                )));
            }
            let (status, body) = self
                .transport
                .token(TokenForm {
                    grant_type: "urn:ietf:params:oauth:grant-type:device_code".to_owned(),
                    client_id: self.config.client_id.clone(),
                    device_code: Some(pending.device_code.clone()),
                    refresh_token: None,
                })
                .await?;

            if (200..300).contains(&status) {
                return self.persist(&body).await;
            }

            match string(&body, "error").unwrap_or_default().as_str() {
                "authorization_pending" => {}
                "slow_down" => {
                    // §3.5: widen, and stay widened. Narrowing back would earn
                    // the next `slow_down` immediately. A server that names the
                    // new interval outright wins over the +5 rule.
                    pending.interval_secs = body
                        .get("interval")
                        .and_then(Value::as_u64)
                        .unwrap_or(pending.interval_secs + SLOW_DOWN_STEP)
                        .max(1);
                }
                "expired_token" => {
                    return Err(ProviderError::Auth(format!(
                        "the login code expired before it was used: run \
                         `orrery auth login {}` again",
                        self.config.grant
                    )));
                }
                "access_denied" => {
                    return Err(ProviderError::Auth(
                        "the sign-in was declined; nothing was stored".to_owned(),
                    ));
                }
                other => {
                    return Err(ProviderError::Auth(format!(
                        "the authorization server refused the login: {}",
                        if other.is_empty() {
                            "no reason given"
                        } else {
                            other
                        }
                    )));
                }
            }

            // Wait *after* deciding to keep going, so the first poll is
            // immediate: a person who was already looking at the page should
            // not wait an interval for nothing.
            self.clock.sleep_secs(pending.interval_secs).await;
        }
    }

    /// Store a granted token, its refresh token and its expiry, and report what
    /// a client should show.
    async fn persist(&self, body: &Value) -> Result<AuthState, ProviderError> {
        let access = string(body, "access_token").ok_or_else(|| {
            ProviderError::Auth("the token response carried no `access_token`".to_owned())
        })?;
        let expires_at = body
            .get("expires_in")
            .and_then(Value::as_u64)
            .map(|d| self.clock.now_unix() + d);

        self.store
            .put(&self.config.grant, &access)
            .await
            .map_err(store_error)?;
        // A rotating refresh token that is not stored is a login next hour. A
        // response that omits one keeps the one already held, which is what a
        // non-rotating server expects.
        if let Some(refresh) = string(body, "refresh_token") {
            self.store
                .put(&self.refresh_grant(), &refresh)
                .await
                .map_err(store_error)?;
        }
        match expires_at {
            Some(at) => self
                .store
                .put(&self.expires_grant(), &at.to_string())
                .await
                .map_err(store_error)?,
            // A token with no stated expiry is not an expired one, and leaving
            // a stale instant behind would make `state()` say it was.
            None => {
                let _ = self.store.clear(&self.expires_grant()).await;
            }
        }

        Ok(AuthState::Ready {
            account: account_of(body),
            expires_at,
        })
    }

    /// What a client draws while it waits.
    fn wait_surface(user_code: &str, uri: &str, complete: Option<&str>) -> Surface {
        let mut text = format!(
            "**Sign in to Anthropic**\n\nOpen <{uri}> and enter the code:\n\n\
             ## `{user_code}`\n"
        );
        if let Some(complete) = complete {
            // Shown *as well as* the code, never instead: the page and the
            // terminal are often on different devices.
            text.push_str(&format!("\nOr open <{complete}>, which fills it in.\n"));
        }
        text.push_str("\nWaiting…\n");
        Surface::new(SurfaceKind::Markdown {
            value: text,
            complete: false,
        })
    }
}

#[async_trait]
impl ProviderAuth for DeviceCodeAuth {
    fn methods(&self) -> &[AuthMethod] {
        METHODS
    }

    async fn state(&self) -> Result<AuthState, ProviderError> {
        let held = self
            .store
            .has(&self.config.grant)
            .await
            .map_err(store_error)?;
        if !held {
            return Ok(AuthState::NeedsLogin {
                reason: crate::auth::missing_reason(&self.config.grant),
            });
        }
        let expires_at = self
            .store
            .get(&self.expires_grant())
            .await
            .map_err(store_error)?
            .and_then(|s| s.trim().parse::<u64>().ok());
        if expires_at.is_some_and(|at| at <= self.clock.now_unix()) {
            // `Expired`, not `NeedsLogin`: the kernel's answer to the first is
            // "refresh and carry on", and to the second is "stop and ask a
            // person". Collapsing them costs a turn.
            return Ok(AuthState::Expired);
        }
        Ok(AuthState::Ready {
            account: None,
            expires_at,
        })
    }

    async fn login(&self, ctx: &dyn AuthCtx) -> Result<AuthState, ProviderError> {
        let (_pending_state, pending) = self.begin(ctx).await?;
        self.wait(pending).await
    }

    async fn refresh(&self) -> Result<AuthState, ProviderError> {
        // What this pass saw before it queued. Read *outside* the lock on
        // purpose: it is the only way the pass that waited can tell that the
        // pass that ran already did the work.
        let on_entry = self
            .store
            .get(&self.refresh_grant())
            .await
            .map_err(store_error)?;

        // One at a time. A second pass arriving here waits, and then sees the
        // token the first one stored rather than spending the refresh token a
        // second time — which a rotating server would reject, leaving the
        // person signed out by the act of checking twice.
        let _one_at_a_time = self.refreshing.lock().await;

        let held = self
            .store
            .get(&self.refresh_grant())
            .await
            .map_err(store_error)?;
        if held != on_entry {
            // Somebody refreshed while this pass was queued. Nothing to do,
            // and doing it anyway is the bug this guard exists for.
            return self.state().await;
        }

        let Some(refresh_token) = held else {
            // Nothing to refresh is not a failure: it is either an API key,
            // which has nothing to refresh, or a signed-out provider.
            return self.state().await;
        };

        let (status, body) = self
            .transport
            .token(TokenForm {
                grant_type: "refresh_token".to_owned(),
                client_id: self.config.client_id.clone(),
                device_code: None,
                refresh_token: Some(refresh_token),
            })
            .await?;

        if (200..300).contains(&status) {
            return self.persist(&body).await;
        }
        // A refused refresh means the person has to sign in again. Reporting it
        // as an error would fail the turn; reporting it as `NeedsLogin` puts a
        // login prompt in front of them, which is the only thing that helps.
        Ok(AuthState::NeedsLogin {
            reason: format!(
                "the saved sign-in for `{}` is no longer accepted ({}): run \
                 `orrery auth login {}`",
                self.config.grant,
                string(&body, "error").unwrap_or_else(|| format!("http {status}")),
                self.config.grant
            ),
        })
    }

    async fn logout(&self) -> Result<(), ProviderError> {
        // Every part, and the expiry with them. A refresh token left behind is
        // a credential the person believes they revoked.
        for name in [
            self.config.grant.clone(),
            self.refresh_grant(),
            self.expires_grant(),
        ] {
            self.store.clear(&name).await.map_err(store_error)?;
        }
        Ok(())
    }
}

/// `body[key]`, when it is a non-empty string.
fn string(body: &Value, key: &str) -> Option<String> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

/// Who the token belongs to, in whichever of the three shapes a server uses.
fn account_of(body: &Value) -> Option<String> {
    string(body, "account")
        .or_else(|| string(&body["account"], "email_address"))
        .or_else(|| string(&body["account"], "email"))
        .or_else(|| string(body, "email"))
}

/// The real HTTP side of the flow.
///
/// [`OAuthTransport`] existed with one implementation — a fake in the tests —
/// which is exactly as far as a device-code flow gets you: fourteen green tests
/// and no way for a person to sign in. This is the other implementation, and
/// `orrery auth login` is what calls it.
///
/// # Why `base_url` is a field
///
/// It is the seam a test drives, the same way [`crate::AnthropicProvider`]'s
/// is: a loopback authorization server in a test process, and nothing leaves
/// the machine. It is also what lets an organisation point the flow at a
/// gateway without a rebuild.
pub struct HttpTransport {
    client: reqwest::Client,
    base_url: String,
}

impl std::fmt::Debug for HttpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpTransport")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

/// Where the first-party device-code endpoints live.
pub const AUTH_BASE_URL: &str = "https://console.anthropic.com";

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new(AUTH_BASE_URL)
    }
}

impl HttpTransport {
    /// Talk to this origin.
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        crate::install_crypto_provider();
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        }
    }

    /// Post a form and read the body back, whatever the status.
    async fn post(&self, path: &str, form: &[(&str, String)]) -> Result<(u16, Value), ProviderError> {
        let response = self
            .client
            .post(format!("{}{path}", self.base_url))
            .header("content-type", "application/x-www-form-urlencoded")
            .header("accept", "application/json")
            .body(urlencode(form))
            .send()
            .await
            .map_err(|e| ProviderError::Auth(format!("the authorization server is unreachable: {e}")))?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        // A body that is not JSON is still an answer: report the status and the
        // first of what it said, rather than "the login failed".
        let body = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| {
            Value::String(text.chars().take(200).collect::<String>())
        });
        Ok((status, body))
    }
}

#[async_trait]
impl OAuthTransport for HttpTransport {
    async fn device_code(&self, client_id: &str, scope: &str) -> Result<Value, ProviderError> {
        let (status, body) = self
            .post(
                "/oauth/device/code",
                &[
                    ("client_id", client_id.to_owned()),
                    ("scope", scope.to_owned()),
                ],
            )
            .await?;
        if !(200..300).contains(&status) {
            return Err(ProviderError::Auth(format!(
                "the authorization server would not start a login (http {status}): {}",
                string(&body, "error_description")
                    .or_else(|| string(&body, "error"))
                    .unwrap_or_else(|| "no reason given".to_owned())
            )));
        }
        Ok(body)
    }

    async fn token(&self, form: TokenForm) -> Result<(u16, Value), ProviderError> {
        let mut fields = vec![
            ("grant_type", form.grant_type),
            ("client_id", form.client_id),
        ];
        if let Some(device_code) = form.device_code {
            fields.push(("device_code", device_code));
        }
        if let Some(refresh_token) = form.refresh_token {
            fields.push(("refresh_token", refresh_token));
        }
        self.post("/oauth/token", &fields).await
    }
}

/// `a=1&b=2`, percent-encoded.
///
/// Written here rather than pulled in: the whole need is four fields of opaque
/// ASCII, and `reqwest`'s `form` support is a cargo feature this crate does not
/// otherwise want.
fn urlencode(fields: &[(&str, String)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{}={}", percent(k), percent(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// One value, with everything but the unreserved set escaped.
fn percent(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}
