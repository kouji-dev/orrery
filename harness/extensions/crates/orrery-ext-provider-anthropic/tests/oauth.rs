//! Phase 5 · the device-code flow, against a fake authorization server that
//! lives in this process.
//!
//! **No socket is opened anywhere in this file.** The HTTP side of the flow is
//! [`OAuthTransport`], and `FakeAuthServer` implements it by matching on the
//! form body and answering from a script. That is the only way the five
//! interesting cases — `authorization_pending`, `slow_down`, `expired_token`,
//! `access_denied`, and a refresh — are reachable at all: a real authorization
//! server produces them minutes apart and needs a person.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use orrery_ext_api::{CredStore, MemoryCredStore};
use orrery_ext_provider_anthropic::oauth::{
    Clock, DeviceCodeAuth, OAuthConfig, OAuthTransport, TokenForm,
};
use orrery_proto::{Surface, SurfaceKind};
use orrery_provider::{AuthCtx, AuthMethod, AuthState, ProviderAuth, ProviderError};
use parking_lot::Mutex;
use serde_json::{Value, json};

/// A clock the test moves by hand. The flow must never sleep against a real
/// one, or every case below costs its own `interval` in wall time.
#[derive(Debug, Default)]
struct TestClock {
    now: AtomicU64,
    slept: Mutex<Vec<u64>>,
}

#[async_trait]
impl Clock for TestClock {
    fn now_unix(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }

    async fn sleep_secs(&self, secs: u64) {
        self.slept.lock().push(secs);
        self.now.fetch_add(secs, Ordering::SeqCst);
    }
}

impl TestClock {
    fn waits(&self) -> Vec<u64> {
        self.slept.lock().clone()
    }
}

/// An authorization server with a script: each `/token` poll takes the next
/// answer, and the last one repeats.
struct FakeAuthServer {
    device: Value,
    token_script: Mutex<std::collections::VecDeque<(u16, Value)>>,
    refresh: Mutex<Option<(u16, Value)>>,
    seen: Mutex<Vec<TokenForm>>,
}

impl FakeAuthServer {
    fn new(device: Value, script: Vec<(u16, Value)>) -> Arc<Self> {
        Arc::new(Self {
            device,
            token_script: Mutex::new(script.into()),
            refresh: Mutex::new(None),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn with_refresh(self: Arc<Self>, status: u16, body: Value) -> Arc<Self> {
        *self.refresh.lock() = Some((status, body));
        self
    }

    fn polls(&self) -> usize {
        self.seen
            .lock()
            .iter()
            .filter(|f| f.grant_type.contains("device_code"))
            .count()
    }

    fn forms(&self) -> Vec<TokenForm> {
        self.seen.lock().clone()
    }
}

#[async_trait]
impl OAuthTransport for FakeAuthServer {
    async fn device_code(&self, _client_id: &str, _scope: &str) -> Result<Value, ProviderError> {
        Ok(self.device.clone())
    }

    async fn token(&self, form: TokenForm) -> Result<(u16, Value), ProviderError> {
        // A real round trip suspends. Answering without ever yielding would
        // make `tokio::join!` run the two passes one after the other, and then
        // `concurrent_refreshes_make_one_request_not_two` would be testing
        // nothing at all.
        tokio::task::yield_now().await;
        let is_refresh = form.grant_type == "refresh_token";
        self.seen.lock().push(form);
        if is_refresh {
            return Ok(self
                .refresh
                .lock()
                .clone()
                .unwrap_or_else(|| (400, json!({ "error": "invalid_grant" }))));
        }
        let mut script = self.token_script.lock();
        let next = if script.len() > 1 {
            script.pop_front()
        } else {
            script.front().cloned()
        };
        Ok(next.unwrap_or((400, json!({ "error": "invalid_grant" }))))
    }
}

/// A client that draws the pending surface and says nothing back.
#[derive(Default)]
struct Watcher(Mutex<Vec<Surface>>);

#[async_trait]
impl AuthCtx for Watcher {
    async fn ask(&self, surface: Surface) -> Result<Value, ProviderError> {
        self.0.lock().push(surface);
        Ok(Value::Null)
    }
}

fn device_body() -> Value {
    json!({
        "device_code": "dev-abc",
        "user_code": "WDJB-MJHT",
        "verification_uri": "https://claude.ai/device",
        "verification_uri_complete": "https://claude.ai/device?code=WDJB-MJHT",
        "expires_in": 900,
        "interval": 5,
    })
}

fn granted() -> Value {
    json!({
        "access_token": "oat-live-1",
        "refresh_token": "ort-live-1",
        "token_type": "Bearer",
        "expires_in": 3600,
        "account": { "email_address": "someone@example.test" },
    })
}

fn build(
    server: Arc<FakeAuthServer>,
    clock: Arc<TestClock>,
    store: Arc<dyn CredStore>,
) -> DeviceCodeAuth {
    DeviceCodeAuth::new(OAuthConfig::anthropic(), store, server).with_clock(clock)
}

#[tokio::test]
async fn the_happy_path_stores_a_token_and_reports_the_account() {
    let clock = Arc::new(TestClock::default());
    let store: Arc<dyn CredStore> = Arc::new(MemoryCredStore::default());
    let server = FakeAuthServer::new(device_body(), vec![(200, granted())]);
    let auth = build(Arc::clone(&server), Arc::clone(&clock), Arc::clone(&store));
    let watcher = Watcher::default();

    let state = auth.login(&watcher).await.expect("login");
    match state {
        AuthState::Ready {
            account,
            expires_at,
        } => {
            assert_eq!(account.as_deref(), Some("someone@example.test"));
            assert_eq!(expires_at, Some(3_600));
        }
        other => panic!("expected Ready, got {other:?}"),
    }

    // The token is in the store, under the grant, and the refresh token with
    // it — without one, an expiry an hour from now is a login an hour from now.
    assert_eq!(
        store.get("anthropic").await.expect("read").as_deref(),
        Some("oat-live-1")
    );
    assert_eq!(
        store
            .get("anthropic.refresh")
            .await
            .expect("read")
            .as_deref(),
        Some("ort-live-1")
    );

    assert!(auth.methods().contains(&AuthMethod::OAuth));
}

#[tokio::test]
async fn the_person_is_shown_the_code_the_uri_and_the_expiry() {
    let clock = Arc::new(TestClock::default());
    let server = FakeAuthServer::new(device_body(), vec![(200, granted())]);
    let auth = build(
        server,
        clock,
        Arc::new(MemoryCredStore::default()) as Arc<dyn CredStore>,
    );
    let watcher = Watcher::default();
    auth.login(&watcher).await.expect("login");

    let drawn = watcher.0.lock().clone();
    assert_eq!(drawn.len(), 1, "one surface, describing the wait");
    // Declared as a surface, never drawn: one description serves ratatui, Ink,
    // the ADE and `--json`.
    let SurfaceKind::Markdown { ref value, .. } = drawn[0].kind else {
        panic!("expected markdown, got {:?}", drawn[0].kind);
    };
    assert!(value.contains("WDJB-MJHT"), "{value}");
    assert!(value.contains("https://claude.ai/device"), "{value}");
}

#[tokio::test]
async fn authorization_pending_is_polled_at_the_stated_interval() {
    let clock = Arc::new(TestClock::default());
    let pending = (400, json!({ "error": "authorization_pending" }));
    let server = FakeAuthServer::new(
        device_body(),
        vec![pending.clone(), pending, (200, granted())],
    );
    let auth = build(
        Arc::clone(&server),
        Arc::clone(&clock),
        Arc::new(MemoryCredStore::default()) as Arc<dyn CredStore>,
    );

    let state = auth.login(&Watcher::default()).await.expect("login");
    assert!(matches!(state, AuthState::Ready { .. }), "{state:?}");
    assert_eq!(server.polls(), 3);
    // The server said 5. Not 1, not a number this crate picked.
    assert_eq!(clock.waits(), vec![5, 5]);
}

#[tokio::test]
async fn slow_down_widens_the_interval_and_keeps_it_widened() {
    let clock = Arc::new(TestClock::default());
    let server = FakeAuthServer::new(
        device_body(),
        vec![
            (400, json!({ "error": "slow_down" })),
            (400, json!({ "error": "authorization_pending" })),
            (200, granted()),
        ],
    );
    let auth = build(
        Arc::clone(&server),
        Arc::clone(&clock),
        Arc::new(MemoryCredStore::default()) as Arc<dyn CredStore>,
    );
    auth.login(&Watcher::default()).await.expect("login");

    // RFC 8628 §3.5: `slow_down` widens the interval by five seconds, and the
    // wider interval is the one used from then on. Narrowing back would earn
    // the next `slow_down` immediately.
    assert_eq!(clock.waits(), vec![10, 10]);
}

#[tokio::test]
async fn slow_down_honours_a_server_stated_interval_when_it_sends_one() {
    let clock = Arc::new(TestClock::default());
    let server = FakeAuthServer::new(
        device_body(),
        vec![
            (400, json!({ "error": "slow_down", "interval": 30 })),
            (200, granted()),
        ],
    );
    let auth = build(
        server,
        Arc::clone(&clock),
        Arc::new(MemoryCredStore::default()) as Arc<dyn CredStore>,
    );
    auth.login(&Watcher::default()).await.expect("login");
    assert_eq!(clock.waits(), vec![30]);
}

#[tokio::test]
async fn expired_token_ends_the_flow_and_stores_nothing() {
    let clock = Arc::new(TestClock::default());
    let store: Arc<dyn CredStore> = Arc::new(MemoryCredStore::default());
    let server = FakeAuthServer::new(
        device_body(),
        vec![
            (400, json!({ "error": "authorization_pending" })),
            (400, json!({ "error": "expired_token" })),
        ],
    );
    let auth = build(server, Arc::clone(&clock), Arc::clone(&store));

    let e = auth.login(&Watcher::default()).await.expect_err("expired");
    assert_eq!(e.code(), "auth");
    assert!(e.to_string().contains("expired"), "{e}");
    assert!(
        !e.is_retryable(),
        "a person has to start over, not the kernel"
    );
    assert_eq!(store.get("anthropic").await.expect("read"), None);
}

#[tokio::test]
async fn a_denial_is_reported_as_a_denial_not_a_timeout() {
    let clock = Arc::new(TestClock::default());
    let server = FakeAuthServer::new(
        device_body(),
        vec![(400, json!({ "error": "access_denied" }))],
    );
    let auth = build(
        server,
        clock,
        Arc::new(MemoryCredStore::default()) as Arc<dyn CredStore>,
    );

    let e = auth.login(&Watcher::default()).await.expect_err("denied");
    assert_eq!(e.code(), "auth");
    assert!(e.to_string().contains("declined"), "{e}");
}

#[tokio::test]
async fn the_flow_gives_up_when_the_code_itself_expires() {
    let clock = Arc::new(TestClock::default());
    // A server that never says `expired_token` and just keeps saying pending.
    let server = FakeAuthServer::new(
        json!({
            "device_code": "dev-abc",
            "user_code": "WDJB-MJHT",
            "verification_uri": "https://claude.ai/device",
            "expires_in": 12,
            "interval": 5,
        }),
        vec![(400, json!({ "error": "authorization_pending" }))],
    );
    let auth = build(
        Arc::clone(&server),
        Arc::clone(&clock),
        Arc::new(MemoryCredStore::default()) as Arc<dyn CredStore>,
    );

    let e = auth.login(&Watcher::default()).await.expect_err("gave up");
    assert!(e.to_string().contains("expired"), "{e}");
    // Three polls at t=0, 5, 10; the fourth would be at 15, past the code's own
    // `expires_in`. The flow stops on its own rather than polling forever
    // against a server that will never say so.
    assert_eq!(server.polls(), 3);
}

#[tokio::test]
async fn refresh_swaps_the_token_and_keeps_the_new_refresh_token() {
    let clock = Arc::new(TestClock::default());
    let store: Arc<dyn CredStore> = Arc::new(MemoryCredStore::default());
    store.put("anthropic", "oat-old").await.expect("stored");
    store
        .put("anthropic.refresh", "ort-old")
        .await
        .expect("stored");

    let server = FakeAuthServer::new(device_body(), vec![(200, granted())]).with_refresh(
        200,
        json!({
            "access_token": "oat-live-2",
            "refresh_token": "ort-live-2",
            "expires_in": 7200,
        }),
    );
    let auth = build(Arc::clone(&server), Arc::clone(&clock), Arc::clone(&store));

    let state = auth.refresh().await.expect("refresh");
    assert!(matches!(state, AuthState::Ready { .. }), "{state:?}");
    assert_eq!(
        store.get("anthropic").await.expect("read").as_deref(),
        Some("oat-live-2")
    );
    // A rotating refresh token that is not stored is a login next hour.
    assert_eq!(
        store
            .get("anthropic.refresh")
            .await
            .expect("read")
            .as_deref(),
        Some("ort-live-2")
    );
    let forms = server.forms();
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].grant_type, "refresh_token");
    assert_eq!(forms[0].refresh_token.as_deref(), Some("ort-old"));
}

#[tokio::test]
async fn concurrent_refreshes_make_one_request_not_two() {
    let clock = Arc::new(TestClock::default());
    let store: Arc<dyn CredStore> = Arc::new(MemoryCredStore::default());
    store.put("anthropic", "oat-old").await.expect("stored");
    store
        .put("anthropic.refresh", "ort-old")
        .await
        .expect("stored");
    let server = FakeAuthServer::new(device_body(), vec![(200, granted())]).with_refresh(
        200,
        json!({ "access_token": "oat-live-2", "refresh_token": "ort-live-2", "expires_in": 7200 }),
    );
    let auth = build(Arc::clone(&server), clock, Arc::clone(&store));

    // The trait requires this: two passes refreshing at once must not produce
    // two credentials or two browser windows.
    let (a, b) = tokio::join!(auth.refresh(), auth.refresh());
    assert_eq!(a.expect("a"), b.expect("b"));
    assert_eq!(server.forms().len(), 1, "one round trip, not two");
}

#[tokio::test]
async fn a_refresh_with_nothing_to_refresh_is_needs_login() {
    let clock = Arc::new(TestClock::default());
    let server = FakeAuthServer::new(device_body(), vec![(200, granted())]);
    let auth = build(
        server,
        clock,
        Arc::new(MemoryCredStore::default()) as Arc<dyn CredStore>,
    );
    match auth.refresh().await.expect("no error") {
        AuthState::NeedsLogin { reason } => assert!(reason.contains("anthropic"), "{reason}"),
        other => panic!("expected NeedsLogin, got {other:?}"),
    }
}

#[tokio::test]
async fn an_expired_token_with_no_refresh_token_reports_expired() {
    let clock = Arc::new(TestClock::default());
    clock.now.store(10_000, Ordering::SeqCst);
    let store: Arc<dyn CredStore> = Arc::new(MemoryCredStore::default());
    store.put("anthropic", "oat-old").await.expect("stored");
    store
        .put("anthropic.expires", "9000")
        .await
        .expect("stored");

    let server = FakeAuthServer::new(device_body(), vec![(200, granted())]);
    let auth = build(server, Arc::clone(&clock), Arc::clone(&store));
    assert_eq!(auth.state().await.expect("state"), AuthState::Expired);
}

#[tokio::test]
async fn logout_forgets_the_token_and_the_refresh_token() {
    let clock = Arc::new(TestClock::default());
    let store: Arc<dyn CredStore> = Arc::new(MemoryCredStore::default());
    let server = FakeAuthServer::new(device_body(), vec![(200, granted())]);
    let auth = build(server, clock, Arc::clone(&store));
    auth.login(&Watcher::default()).await.expect("login");

    auth.logout().await.expect("logout");
    assert_eq!(store.get("anthropic").await.expect("read"), None);
    assert_eq!(store.get("anthropic.refresh").await.expect("read"), None);
    assert!(matches!(
        auth.state().await.expect("state"),
        AuthState::NeedsLogin { .. }
    ));
}

#[tokio::test]
async fn a_pending_state_carries_what_a_client_has_to_draw() {
    // `pending_state` is what `login` hands a caller that will not block —
    // `orrery auth login --no-wait`, and the ADE, which draws the wait itself.
    let clock = Arc::new(TestClock::default());
    let server = FakeAuthServer::new(
        device_body(),
        vec![(400, json!({"error":"authorization_pending"}))],
    );
    let auth = build(
        server,
        Arc::clone(&clock),
        Arc::new(MemoryCredStore::default()) as Arc<dyn CredStore>,
    );

    let (state, _handle) = auth.begin(&Watcher::default()).await.expect("begun");
    match state {
        AuthState::Pending {
            user_code,
            verification_uri,
            verification_uri_complete,
            expires_at,
            interval_secs,
        } => {
            assert_eq!(user_code, "WDJB-MJHT");
            assert_eq!(verification_uri, "https://claude.ai/device");
            assert_eq!(
                verification_uri_complete.as_deref(),
                Some("https://claude.ai/device?code=WDJB-MJHT")
            );
            assert_eq!(expires_at, 900, "an absolute unix second, not a duration");
            assert_eq!(interval_secs, 5);
        }
        other => panic!("expected Pending, got {other:?}"),
    }
}
