//! `AguiSession` end to end: the same client code over two transports, feeding
//! the same store.

use std::sync::Arc;
use std::time::Duration;

use orrery_client::{AguiSession, Endpoint, SurfaceStore};
use orrery_proto::{
    Event, Request, Seq, SessionId, Surface, SurfaceId, SurfaceKind, SurfacePatch, TurnId, Usage,
    UserInput,
};
use orrery_transport::listener::{ControlHandler, http};
use orrery_transport::{Hub, TransportError};

fn sid(n: u8) -> SurfaceId {
    SurfaceId::from_uuid(uuid::Uuid::from_u128(u128::from(n)))
}

fn a_session() -> SessionId {
    SessionId::from_uuid(uuid::Uuid::from_u128(0xabc))
}

fn a_turn() -> TurnId {
    TurnId::from_uuid(uuid::Uuid::from_u128(0xff))
}

/// A stand-in for the kernel: it answers control requests by publishing the turn
/// the fixture streams describe.
struct FakeKernel {
    hub: Hub,
}

impl ControlHandler for FakeKernel {
    fn control(&self, request: Request) -> Result<serde_json::Value, TransportError> {
        if matches!(request, Request::TurnSubmit { .. }) {
            for event in [
                Event::TurnStarted {
                    seq: Seq(1),
                    turn: a_turn(),
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
                        text: "three crates".into(),
                    },
                },
                Event::TurnSettled {
                    seq: Seq(4),
                    turn: a_turn(),
                    usage: Usage::default(),
                },
            ] {
                self.hub.publish(&event);
            }
        }
        Ok(serde_json::json!({ "turn": a_turn().to_string() }))
    }
}

#[test]
fn endpoints_parse_all_three_forms() {
    assert_eq!(
        "inproc:".parse::<Endpoint>().expect("inproc"),
        Endpoint::Inproc
    );
    assert_eq!(
        "pipe:orrery-abc".parse::<Endpoint>().expect("pipe"),
        Endpoint::Pipe {
            name: "orrery-abc".into()
        }
    );
    assert_eq!(
        "http://127.0.0.1:7777".parse::<Endpoint>().expect("http"),
        Endpoint::Http {
            url: "http://127.0.0.1:7777".into(),
            token: None
        }
    );
    assert_eq!(
        "http://t0ken@127.0.0.1:7777/"
            .parse::<Endpoint>()
            .expect("http with a token"),
        Endpoint::Http {
            url: "http://127.0.0.1:7777".into(),
            token: Some("t0ken".into())
        }
    );
    assert!("ftp://nope".parse::<Endpoint>().is_err());

    // An endpoint in a log is an endpoint in a bug report, so the token never
    // prints.
    let printed = "http://t0ken@127.0.0.1:7777"
        .parse::<Endpoint>()
        .expect("http")
        .to_string();
    assert!(!printed.contains("t0ken"), "{printed}");
}

/// In-process: the same six calls, no serialisation, straight into the store.
#[tokio::test]
async fn in_process_session_feeds_the_store() {
    let hub = Hub::new(&a_session().to_string());
    let kernel = Arc::new(FakeKernel { hub: hub.clone() });
    let mut session =
        AguiSession::in_process(&hub, kernel as Arc<dyn ControlHandler>, Duration::ZERO);
    session.attach(a_session(), None).await.expect("attach");
    let turn = session
        .submit(UserInput::text("what is in the workspace?"))
        .await
        .expect("submit");
    assert_eq!(turn, a_turn());

    let mut store = SurfaceStore::new();
    for _ in 0..5 {
        let Some(frame) = session.next_frame().await else {
            break;
        };
        store.apply(&frame.expect("no gaps"));
        if store.settled().count() == 1 {
            break;
        }
    }
    let settled = store.turn(&a_turn().to_string()).expect("the turn");
    assert!(settled.settled);
    let SurfaceKind::Markdown { value, complete } = &settled.surfaces[0].kind else {
        panic!("a markdown surface");
    };
    assert_eq!(value, "three crates");
    assert!(complete, "the turn closed the message it left open");
}

/// HTTP: `POST /run` and the SSE stream behind it, through the same store.
#[tokio::test]
async fn http_session_feeds_the_same_store() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });

    let hub = Hub::new(&a_session().to_string());
    let kernel = Arc::new(FakeKernel { hub: hub.clone() });
    let server = http::serve(
        "127.0.0.1:0".parse().expect("loopback"),
        hub,
        kernel as Arc<dyn ControlHandler>,
        http::HttpConfig {
            token: Some("t0ken".into()),
            tick: Duration::ZERO,
        },
    )
    .await
    .expect("bind");

    let endpoint: Endpoint = format!("http://t0ken@{}", server.addr())
        .parse()
        .expect("endpoint");
    let mut session = AguiSession::connect(&endpoint).await.expect("connect");
    session.attach(a_session(), None).await.expect("attach");
    session
        .run(&UserInput::text("what is in the workspace?"))
        .await
        .expect("run");

    let mut store = SurfaceStore::new();
    while store.settled().count() == 0 {
        let Some(frame) = tokio::time::timeout(Duration::from_secs(5), session.next_frame())
            .await
            .expect("no timeout")
        else {
            break;
        };
        store.apply(&frame.expect("no gaps"));
    }

    let settled = store.turn(&a_turn().to_string()).expect("the turn");
    let SurfaceKind::Markdown { value, .. } = &settled.surfaces[0].kind else {
        panic!("a markdown surface");
    };
    assert_eq!(
        value, "three crates",
        "an HTTP client and an in-process one end at the same state"
    );
    server.shutdown();
}

/// HTTP, passively: a client that submits nothing still sees the whole turn.
///
/// `run` is the run-scoped half of AG-UI's transport, and it is the half that
/// kept `orrery attach` on a pipe. `subscribe` is the other half: `GET /events`,
/// the same frames, for a session somebody else is driving.
#[tokio::test]
async fn http_subscribe_renders_somebody_elses_turn() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });

    let hub = Hub::new(&a_session().to_string());
    let kernel = Arc::new(FakeKernel { hub: hub.clone() });
    let server = http::serve(
        "127.0.0.1:0".parse().expect("loopback"),
        hub.clone(),
        Arc::clone(&kernel) as Arc<dyn ControlHandler>,
        http::HttpConfig {
            token: Some("t0ken".into()),
            tick: Duration::ZERO,
        },
    )
    .await
    .expect("bind");

    let endpoint: Endpoint = format!("http://t0ken@{}", server.addr())
        .parse()
        .expect("endpoint");
    let mut watcher = AguiSession::connect(&endpoint).await.expect("connect");
    watcher.subscribe(None).await.expect("subscribe");

    // Somebody else's turn, submitted straight at the kernel.
    kernel
        .control(Request::TurnSubmit {
            id: orrery_proto::ReqId::new(),
            session: a_session(),
            input: UserInput::text("what is in the workspace?"),
        })
        .expect("submit");

    let mut store = SurfaceStore::new();
    while store.settled().count() == 0 {
        let Some(frame) = tokio::time::timeout(Duration::from_secs(5), watcher.next_frame())
            .await
            .expect("no timeout")
        else {
            break;
        };
        store.apply(&frame.expect("no gaps"));
    }
    let settled = store.turn(&a_turn().to_string()).expect("the turn");
    let SurfaceKind::Markdown { value, .. } = &settled.surfaces[0].kind else {
        panic!("a markdown surface");
    };
    assert_eq!(
        value, "three crates",
        "a passive subscriber sees what the submitter sees"
    );
    server.shutdown();
}
