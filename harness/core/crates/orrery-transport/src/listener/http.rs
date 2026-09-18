//! The HTTP + SSE listener: AG-UI's own transport, so an off-the-shelf client works.
//!
//! `POST /run` takes a `RunAgentInput` and returns `text/event-stream`.
//! `GET /events` is the same stream without a run: a passive subscribe to a
//! session somebody else is driving, which is what `orrery attach` needs and
//! what AG-UI's own vocabulary has no route for.
//! `POST /control` takes a [`Request`] and returns JSON — that is where
//! `session.attach(since)`, `turn.cancel`, `intent`, `consent.answer` and
//! `query` live, because AG-UI's input path is a run invocation and has no
//! vocabulary for any of them.
//!
//! `tower` sits here and nowhere else: at the HTTP edge, never inside the loop.
//!
//! # Auth, in phase 1
//!
//! **Loopback plus a bearer token, and nothing more.** That is enough for a
//! developer machine and it is not enough for §5.1's remote execution host,
//! which is phase 4's problem together with TLS. Recorded as a limitation
//! rather than dressed up: there is no per-session authorisation, no audience
//! check and no replay protection on the token.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event as SseEvent, Sse};
use axum::response::{IntoResponse, Response};
use axum::extract::Query;
use axum::routing::{get, post};
use futures_util::StreamExt;
use orrery_proto::{Request, SessionId, UserInput};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::error::TransportError;
use crate::listener::ControlHandler;
use crate::{ClientStream, Hub};

/// One message on an AG-UI `RunAgentInput`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMessage {
    /// `user`, `assistant`, `system`, `tool`.
    #[serde(default)]
    pub role: String,
    /// What was said.
    #[serde(default)]
    pub content: Option<String>,
    /// AG-UI's own message id, if the client assigned one.
    #[serde(default)]
    pub id: Option<String>,
}

/// AG-UI's run invocation.
///
/// # The adapter, and where it is lossy
///
/// AG-UI's run carries the whole message list and a state blob; ours carries a
/// [`UserInput`] against a session the kernel already has. The adapter takes the
/// **last `user` message** as the input and ignores the rest, because the
/// kernel's own transcript is the authority on what was said before — a client
/// that replayed its idea of the history into a run would be writing state, and
/// clients do not write state.
///
/// A stock AG-UI client therefore works without special-casing, and the one
/// thing it loses is the ability to rewrite history on the way in, which it was
/// never allowed to do.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentInput {
    /// The session. Must be a session id the kernel knows.
    #[serde(default)]
    pub thread_id: Option<String>,
    /// The run. Ours is assigned by the kernel, so this is advisory.
    #[serde(default)]
    pub run_id: Option<String>,
    /// The conversation as the client has it.
    #[serde(default)]
    pub messages: Vec<RunMessage>,
    /// AG-UI's shared state. Read and discarded: state flows outward only.
    #[serde(default)]
    pub state: Option<serde_json::Value>,
    /// Anything else the client attached.
    #[serde(default)]
    pub forwarded_props: Option<serde_json::Value>,
}

impl RunAgentInput {
    /// The [`UserInput`] this run means.
    #[must_use]
    pub fn to_user_input(&self) -> UserInput {
        let text = self
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .and_then(|m| m.content.clone())
            .unwrap_or_default();
        UserInput::text(text)
    }

    /// The session this run is against.
    ///
    /// # Errors
    ///
    /// The string, when it is not a session id.
    pub fn session(&self) -> Result<SessionId, String> {
        self.thread_id
            .as_deref()
            .ok_or_else(|| "threadId is required".to_owned())?
            .parse()
            .map_err(|_| "threadId is not a session id".to_owned())
    }
}

/// How the HTTP listener is set up.
#[derive(Clone, Debug)]
pub struct HttpConfig {
    /// The bearer token every request must carry. `None` disables the check,
    /// which is only ever right in a test.
    pub token: Option<String>,
    /// The coalescing tick each SSE client gets.
    pub tick: Duration,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            token: None,
            tick: Duration::from_millis(33),
        }
    }
}

#[derive(Clone)]
struct AppState {
    hub: Hub,
    control: Arc<dyn ControlHandler>,
    config: HttpConfig,
}

/// A running HTTP listener.
#[derive(Debug)]
pub struct HttpServer {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl HttpServer {
    /// Where it is listening. Loopback unless the caller said otherwise.
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The base URL clients connect to.
    #[must_use]
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Stop listening.
    pub fn shutdown(self) {
        self.task.abort();
    }
}

/// Start the HTTP listener.
///
/// Binds to `addr`, which should be loopback: see the module note on auth.
///
/// # Errors
///
/// [`TransportError::Io`] when the address cannot be bound.
pub async fn serve(
    addr: SocketAddr,
    hub: Hub,
    control: Arc<dyn ControlHandler>,
    config: HttpConfig,
) -> Result<HttpServer, TransportError> {
    let state = AppState {
        hub,
        control,
        config,
    };
    let app = Router::new()
        .route("/run", post(run))
        .route("/events", get(events))
        .route("/control", post(control_rpc))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(HttpServer { addr, task })
}

fn authorised(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(expected) = state.config.token.as_deref() else {
        return true;
    };
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|got| got == expected)
}

/// `POST /run` — AG-UI's run invocation in, `text/event-stream` out.
async fn run(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::Json<RunAgentInput>,
) -> Response {
    if !authorised(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let input = body.0;
    let session = match input.session() {
        Ok(s) => s,
        Err(why) => return (StatusCode::BAD_REQUEST, why).into_response(),
    };

    // Subscribe *before* submitting, so nothing the turn emits between the two
    // is lost. This client's coalescer is its own.
    let client = state.hub.subscribe(state.config.tick);

    let submitted = state.control.control(Request::TurnSubmit {
        id: orrery_proto::ReqId::new(),
        session,
        input: input.to_user_input(),
    });
    if let Err(err) = submitted {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    let stream =
        live_frames(client).map(|frame| Ok::<_, std::convert::Infallible>(sse_frame(&frame)));
    Sse::new(stream).into_response()
}

/// Every frame this client is owed, batches flattened.
fn live_frames(client: ClientStream) -> impl futures_util::Stream<Item = crate::Frame> {
    futures_util::stream::unfold(client, |mut client| async move {
        let batch = client.next_batch().await?;
        Some((batch, client))
    })
    .flat_map(futures_util::stream::iter)
}

/// One frame as one SSE event, named by its AG-UI type.
fn sse_frame(frame: &crate::Frame) -> SseEvent {
    SseEvent::default()
        .event(frame.event.type_name())
        .data(serde_json::to_string(frame).unwrap_or_default())
}

/// What `GET /events` accepts.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct EventsQuery {
    /// Replay everything after this `seq` before going live. Absent is "from
    /// wherever the session is now", which is what a fresh client wants.
    #[serde(default)]
    pub since: Option<u64>,
}

/// `GET /events` — subscribe to a session without starting a run.
///
/// The passive half of the transport, and the reason an SSE client can render a
/// turn it did not submit. It serves exactly what the pipe handshake serves:
/// [`Hub::attach`] for the replay, then this client's own coalescer for
/// everything after it — one numbering space across both, because `seq` is
/// assigned at the encoder's output and not per connection.
///
/// Subscribing *before* replaying is deliberate: the other order has a window
/// between the two in which a frame belongs to neither.
async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Response {
    if !authorised(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let client = state.hub.subscribe(state.config.tick);
    let replay = match state.hub.attach(query.since) {
        Ok(frames) => frames,
        // The ring no longer reaches back that far. A cold replay is the
        // caller's to ask for — say so rather than serve a hole.
        Err(err) => return (StatusCode::GONE, err.to_string()).into_response(),
    };
    let last = replay.last().map(|f| f.seq);
    // The replay and the subscription overlap by however many frames landed
    // between the two calls. Numbers a client already has are dropped here
    // rather than deduplicated there.
    let live = live_frames(client).filter(move |frame| {
        std::future::ready(last.is_none_or(|last| frame.seq > last))
    });
    let stream = futures_util::stream::iter(replay)
        .chain(live)
        .map(|frame| Ok::<_, std::convert::Infallible>(sse_frame(&frame)));
    Sse::new(stream).into_response()
}

/// `POST /control` — everything AG-UI has no vocabulary for.
async fn control_rpc(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::Json<Request>,
) -> Response {
    if !authorised(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.control.control(body.0) {
        Ok(value) => axum::Json(value).into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}
