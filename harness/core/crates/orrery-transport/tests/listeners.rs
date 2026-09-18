//! Three listeners, one frame set: what each of them owes the session.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use orrery_agui::{AguiEvent, Frame};
use orrery_proto::{
    Event, Request, Seq, SessionId, Surface, SurfaceId, SurfaceKind, SurfacePatch, TurnId,
};
use orrery_transport::frame::Hello;
use orrery_transport::listener::http::{HttpConfig, RunAgentInput, RunMessage};
use orrery_transport::listener::{ControlHandler, http, inproc, pipe};
use orrery_transport::{Hub, TransportError};

fn sid(n: u8) -> SurfaceId {
    SurfaceId::from_uuid(uuid::Uuid::from_u128(u128::from(n)))
}

fn turn() -> TurnId {
    TurnId::from_uuid(uuid::Uuid::from_u128(0xff))
}

/// A whole small turn: open a message, say something, settle.
fn a_turn() -> Vec<Event> {
    vec![
        Event::TurnStarted {
            seq: Seq(1),
            turn: turn(),
        },
        Event::Delta {
            seq: Seq(2),
            surface: sid(1),
            patch: SurfacePatch::Replace {
                id: sid(1),
                value: Surface {
                    id: Some(sid(1)),
                    status: None,
                    kind: SurfaceKind::Markdown {
                        value: String::new(),
                        complete: false,
                    },
                },
            },
        },
        Event::Delta {
            seq: Seq(3),
            surface: sid(1),
            patch: SurfacePatch::Append {
                id: sid(1),
                text: "hello".into(),
            },
        },
        Event::TurnSettled {
            seq: Seq(4),
            turn: turn(),
            usage: orrery_proto::Usage::default(),
        },
    ]
}

fn unique_name(tag: &str) -> String {
    static N: AtomicUsize = AtomicUsize::new(0);
    format!(
        "orrery-test-{tag}-{}-{}.sock",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    )
}

// ---------------------------------------------------------------- in-process

/// An in-process round trip never touches serde.
///
/// Two proofs, because either alone is weak. The **type-level** one: `Probe`
/// implements neither `Serialize` nor `Deserialize`, so a channel that carried
/// it could not have a serde bound anywhere in its signature. The **runtime**
/// one: `Counted` does implement `Serialize`, counting every call, and the
/// counter has to still be zero on the other side.
#[tokio::test]
async fn inproc_same_types_no_serialisation() {
    struct Probe(#[allow(dead_code)] TurnId);

    static SERIALISED: AtomicUsize = AtomicUsize::new(0);
    struct Counted(u8);
    impl serde::Serialize for Counted {
        fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            SERIALISED.fetch_add(1, Ordering::SeqCst);
            s.serialize_u8(self.0)
        }
    }

    let (a, mut b) = inproc::pair::<Probe>();
    a.send(Probe(turn())).expect("in-process send");
    assert!(
        b.recv().await.is_some(),
        "a non-serialisable payload arrives"
    );

    let (a, mut b) = inproc::pair::<Counted>();
    a.send(Counted(7)).expect("in-process send");
    let got = b.recv().await.expect("arrives");
    assert_eq!(got.0, 7);
    assert_eq!(
        SERIALISED.load(Ordering::SeqCst),
        0,
        "the in-process transport must not serialise"
    );

    // And the thing the kernel actually sends, as itself.
    let (a, mut b) = inproc::pair::<Event>();
    for event in a_turn() {
        a.send(event).expect("send");
    }
    let mut seen = 0;
    while b.try_recv().is_some() {
        seen += 1;
    }
    assert_eq!(seen, 4);
}

// ---------------------------------------------------------------------- pipe

async fn wait_for_clients(hub: &Hub, n: usize) {
    for _ in 0..200 {
        if hub.client_count() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("only {} of {n} clients attached", hub.client_count());
}

async fn collect(
    client: &mut pipe::PipeClient<interprocess::local_socket::tokio::Stream>,
    n: usize,
) -> Vec<Frame> {
    let mut out = Vec::new();
    for _ in 0..n {
        match tokio::time::timeout(Duration::from_secs(5), client.next_frame()).await {
            Ok(Some(frame)) => out.push(frame),
            _ => break,
        }
    }
    out
}

/// Two connections attach to one session; both receive the same turn, each with
/// its own coalescer.
#[tokio::test]
async fn pipe_two_clients_one_session() {
    let name = unique_name("two");
    let hub = Hub::new("sess-1");
    let server = pipe::serve(&name, hub.clone(), Duration::ZERO, |_: Request| {
        Ok(serde_json::Value::Null)
    })
    .expect("listen");

    let mut one = pipe::connect(&name, Hello::default())
        .await
        .expect("connect");
    let mut two = pipe::connect(&name, Hello::default())
        .await
        .expect("connect");
    wait_for_clients(&hub, 2).await;

    for event in a_turn() {
        hub.publish(&event);
    }

    let a = collect(&mut one, 5).await;
    let b = collect(&mut two, 5).await;
    let seqs = |fs: &[Frame]| fs.iter().map(|f| f.seq).collect::<Vec<_>>();
    assert_eq!(
        seqs(&a),
        vec![1, 2, 3, 4, 5],
        "first client sees the whole turn"
    );
    assert_eq!(seqs(&b), seqs(&a), "and the second sees the same numbers");
    assert_eq!(hub.client_count(), 2, "one coalescer each, not one shared");
    server.shutdown();
}

/// Drop a client mid-turn: the turn completes, and re-attaching replays it.
#[tokio::test]
async fn pipe_disconnect_does_not_end_the_session() {
    let name = unique_name("drop");
    let hub = Hub::new("sess-1");
    let server = pipe::serve(&name, hub.clone(), Duration::ZERO, |_: Request| {
        Ok(serde_json::Value::Null)
    })
    .expect("listen");

    let events = a_turn();
    let mut client = pipe::connect(&name, Hello::default())
        .await
        .expect("connect");
    wait_for_clients(&hub, 1).await;
    hub.publish(&events[0]);
    let seen = collect(&mut client, 1).await;
    assert_eq!(seen.len(), 1);
    let last = seen[0].seq;

    drop(client);

    // The rest of the turn happens with nobody watching.
    for event in &events[1..] {
        hub.publish(event);
    }
    // Five frames, not four: `turn.settled` closes the message that was still
    // open before it finishes the run.
    assert_eq!(hub.last_seq(), Some(5), "the turn completed regardless");

    let mut again = pipe::connect(
        &name,
        Hello {
            since: Some(last),
            ..Hello::default()
        },
    )
    .await
    .expect("re-attach");
    let replayed = collect(&mut again, 4).await;
    assert_eq!(
        replayed.iter().map(|f| f.seq).collect::<Vec<_>>(),
        vec![2, 3, 4, 5],
        "re-attaching with `since` hands over exactly the gap"
    );
    server.shutdown();
}

// ---------------------------------------------------------------------- http

/// A control handler that records what it was asked and drives the hub.
struct Recorder {
    hub: Hub,
    seen: Mutex<Vec<String>>,
    publish_on_submit: bool,
}

impl ControlHandler for Recorder {
    fn control(&self, request: Request) -> Result<serde_json::Value, TransportError> {
        let tag = match &request {
            Request::TurnSubmit { input, .. } => format!("turn.submit:{}", input.text),
            Request::SessionAttach { since, .. } => format!("session.attach:{since:?}"),
            Request::TurnCancel { .. } => "turn.cancel".into(),
            Request::ConsentAnswer { answer, .. } => format!("consent.answer:{answer:?}"),
            Request::Intent { .. } => "intent".into(),
            Request::Query { .. } => "query".into(),
            other => format!("{other:?}"),
        };
        self.seen.lock().expect("seen").push(tag);
        if self.publish_on_submit && matches!(request, Request::TurnSubmit { .. }) {
            for event in a_turn() {
                self.hub.publish(&event);
            }
        }
        Ok(serde_json::json!({"ok": true}))
    }
}

/// reqwest is built against `rustls-no-provider`, which mirrors the updater's
/// TLS setup exactly so no second crypto stack lands in the tree. A test client
/// therefore has to install the provider itself, once.
fn http_client() -> reqwest::Client {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    reqwest::Client::new()
}

async fn start_http(hub: Hub, publish_on_submit: bool) -> (http::HttpServer, Arc<Recorder>) {
    let recorder = Arc::new(Recorder {
        hub: hub.clone(),
        seen: Mutex::new(Vec::new()),
        publish_on_submit,
    });
    let addr: SocketAddr = "127.0.0.1:0".parse().expect("loopback");
    let server = http::serve(
        addr,
        hub,
        Arc::clone(&recorder) as Arc<dyn ControlHandler>,
        HttpConfig {
            token: Some("t0ken".into()),
            tick: Duration::ZERO,
        },
    )
    .await
    .expect("bind");
    (server, recorder)
}

fn a_session() -> SessionId {
    SessionId::from_uuid(uuid::Uuid::from_u128(0xabc))
}

/// `POST /run` returns `text/event-stream` and the expected event sequence.
#[tokio::test]
async fn http_run_returns_sse() {
    let hub = Hub::new("sess-1");
    let (server, _rec) = start_http(hub.clone(), true).await;
    let client = http_client();

    let response = client
        .post(format!("{}/run", server.base_url()))
        .bearer_auth("t0ken")
        .json(&RunAgentInput {
            thread_id: Some(a_session().to_string()),
            messages: vec![RunMessage {
                role: "user".into(),
                content: Some("what is in the workspace?".into()),
                id: None,
            }],
            ..RunAgentInput::default()
        })
        .send()
        .await
        .expect("post /run");

    assert_eq!(response.status(), 200);
    assert!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream")),
        "AG-UI's own transport, or it is not AG-UI"
    );

    let frames = read_sse(response, 5).await;
    let types: Vec<&str> = frames.iter().map(|f| f.event.type_name()).collect();
    assert_eq!(
        types,
        vec![
            "RUN_STARTED",
            "TEXT_MESSAGE_START",
            "TEXT_MESSAGE_CONTENT",
            "TEXT_MESSAGE_END",
            "RUN_FINISHED"
        ],
        "the turn, as AG-UI spells it"
    );
    assert_eq!(
        frames.iter().map(|f| f.seq).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    server.shutdown();
}

/// Read `n` SSE `data:` payloads and stop.
async fn read_sse(response: reqwest::Response, n: usize) -> Vec<Frame> {
    use futures_util::StreamExt;
    let mut out = Vec::new();
    let mut buffer = String::new();
    let mut body = response.bytes_stream();
    while out.len() < n {
        let Ok(Some(Ok(chunk))) = tokio::time::timeout(Duration::from_secs(5), body.next()).await
        else {
            break;
        };
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(at) = buffer.find("\n\n") {
            let block: String = buffer.drain(..at + 2).collect();
            for line in block.lines() {
                if let Some(data) = line.strip_prefix("data: ")
                    && let Ok(frame) = serde_json::from_str::<Frame>(data)
                {
                    out.push(frame);
                }
            }
        }
    }
    out
}

/// Attach, cancel, consent answer, intent and query all ride `POST /control`.
#[tokio::test]
async fn http_control_endpoints() {
    let hub = Hub::new("sess-1");
    let (server, recorder) = start_http(hub, false).await;
    let client = http_client();

    let requests = vec![
        Request::SessionAttach {
            id: orrery_proto::ReqId::new(),
            session: a_session(),
            since: Some(orrery_proto::Seq(5)),
        },
        Request::TurnCancel {
            id: orrery_proto::ReqId::new(),
            session: a_session(),
            turn: turn(),
        },
        Request::ConsentAnswer {
            id: orrery_proto::ReqId::new(),
            prompt: orrery_proto::PromptId::from_uuid(uuid::Uuid::from_u128(9)),
            answer: orrery_proto::ConsentAnswerKind::AllowOnce,
        },
        Request::Intent {
            id: orrery_proto::ReqId::new(),
            session: a_session(),
            surface: sid(1),
            value: serde_json::json!({"choice": "proto"}),
        },
        Request::Query {
            id: orrery_proto::ReqId::new(),
            of: orrery_proto::QueryOf::Sessions {},
        },
    ];
    for request in &requests {
        let response = client
            .post(format!("{}/control", server.base_url()))
            .bearer_auth("t0ken")
            .json(request)
            .send()
            .await
            .expect("post /control");
        assert_eq!(response.status(), 200, "control request {request:?}");
    }

    let seen = recorder.seen.lock().expect("seen").clone();
    assert_eq!(
        seen,
        vec![
            "session.attach:Some(Seq(5))",
            "turn.cancel",
            "consent.answer:AllowOnce",
            "intent",
            "query",
        ]
    );

    // And the token is not decoration.
    let unauthorised = client
        .post(format!("{}/control", server.base_url()))
        .json(&requests[0])
        .send()
        .await
        .expect("post /control");
    assert_eq!(unauthorised.status(), 401);
    server.shutdown();
}

/// A client that never reads does not stall the kernel.
#[tokio::test]
async fn http_slow_consumer_does_not_stall_the_kernel() {
    let hub = Hub::new("sess-1");
    let (server, _rec) = start_http(hub.clone(), false).await;
    let client = http_client();

    // Open the stream and then never touch it again.
    let _dead_weight = client
        .post(format!("{}/run", server.base_url()))
        .bearer_auth("t0ken")
        .json(&RunAgentInput {
            thread_id: Some(a_session().to_string()),
            ..RunAgentInput::default()
        })
        .send()
        .await
        .expect("post /run");
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut attentive = hub.subscribe(Duration::ZERO);
    let started = std::time::Instant::now();
    for i in 0..2_000u64 {
        hub.publish(&Event::Delta {
            seq: Seq(i),
            surface: sid(1),
            patch: SurfacePatch::Append {
                id: sid(1),
                text: "x".into(),
            },
        });
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "publishing must not wait on a reader: took {elapsed:?}"
    );

    let mut seen = 0usize;
    while seen < 2_000 {
        let Ok(Some(batch)) =
            tokio::time::timeout(Duration::from_secs(5), attentive.next_batch()).await
        else {
            break;
        };
        seen += batch.len();
        if attentive.is_idle() {
            break;
        }
    }
    assert_eq!(seen, 2_000, "the attentive client got the whole turn");
    assert!(matches!(
        hub.attach(None).map(|f| f.len()),
        Ok(n) if n >= 2_000
    ));
    server.shutdown();
}

/// The `RunAgentInput` adapter takes the last user message and nothing else.
#[test]
fn run_agent_input_maps_to_user_input() {
    let input = RunAgentInput {
        thread_id: Some(a_session().to_string()),
        messages: vec![
            RunMessage {
                role: "user".into(),
                content: Some("first".into()),
                id: None,
            },
            RunMessage {
                role: "assistant".into(),
                content: Some("an answer".into()),
                id: None,
            },
            RunMessage {
                role: "user".into(),
                content: Some("second".into()),
                id: None,
            },
        ],
        state: Some(serde_json::json!({"client": "would like to write this"})),
        ..RunAgentInput::default()
    };
    assert_eq!(input.to_user_input().text, "second");
    assert_eq!(input.session().expect("a session id"), a_session());
    let _ = AguiEvent::RunError {
        message: String::new(),
        code: None,
    };
}

/// `GET /events` is a passive subscribe: a turn somebody else submitted, seen
/// whole, replay first and then live.
///
/// This is what an SSE client needs to render a session it did not start —
/// `POST /run` only ever carries the frames of the run it opened, which is why
/// `orrery attach` could not take an `http://` endpoint.
#[tokio::test]
async fn http_events_is_a_passive_subscribe() {
    let hub = Hub::new("sess-1");
    let (server, _rec) = start_http(hub.clone(), false).await;
    let client = http_client();

    // A turn nobody on HTTP submitted, already over before anyone subscribes.
    for event in a_turn() {
        hub.publish(&event);
    }

    // No token: the route is guarded like every other.
    let unauthorised = client
        .get(format!("{}/events", server.base_url()))
        .send()
        .await
        .expect("get /events");
    assert_eq!(unauthorised.status(), 401);

    let response = client
        .get(format!("{}/events", server.base_url()))
        .bearer_auth("t0ken")
        .send()
        .await
        .expect("get /events");
    assert_eq!(response.status(), 200);
    assert!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream")),
        "a subscribe is AG-UI's own transport too"
    );

    // Five replayed frames, then one that had not happened when we connected.
    let hub2 = hub.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        hub2.publish(&Event::Error {
            seq: Seq(5),
            scope: orrery_proto::ErrorScope::Turn,
            detail: orrery_proto::ErrorDetail {
                code: "late".into(),
                message: "after the subscribe".into(),
                retryable: false,
                data: None,
            },
        });
    });
    let frames = read_sse(response, 6).await;
    let types: Vec<&str> = frames.iter().map(|f| f.event.type_name()).collect();
    assert_eq!(
        types,
        vec![
            "RUN_STARTED",
            "TEXT_MESSAGE_START",
            "TEXT_MESSAGE_CONTENT",
            "TEXT_MESSAGE_END",
            "RUN_FINISHED",
            "RUN_ERROR",
        ],
        "everything the session has, then everything it gets"
    );
    assert_eq!(
        frames.iter().map(|f| f.seq).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6],
        "one numbering space, replay and live alike"
    );
    server.shutdown();
}

/// `GET /events?since=` replays from where a client left off, with no gap and
/// no repeat.
#[tokio::test]
async fn http_events_since_is_contiguous() {
    let hub = Hub::new("sess-1");
    let (server, _rec) = start_http(hub.clone(), false).await;
    let client = http_client();
    for event in a_turn() {
        hub.publish(&event);
    }

    let response = client
        .get(format!("{}/events?since=2", server.base_url()))
        .bearer_auth("t0ken")
        .send()
        .await
        .expect("get /events");
    assert_eq!(response.status(), 200);
    let frames = read_sse(response, 3).await;
    assert_eq!(
        frames.iter().map(|f| f.seq).collect::<Vec<_>>(),
        vec![3, 4, 5],
        "since=2 means 3 onwards"
    );
    server.shutdown();
}
