//! Streamable HTTP, through the broker's `net`.
//!
//! The server is a `TcpListener` this test starts on loopback and stops when it
//! is done — nothing is dialled that this process did not just create, and
//! `orrery-broker` cannot dial at all until a transport is installed, which is
//! why the one below lives here in `tests/` rather than in the crate.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use orrery_broker::{Broker, BrokerError, LocalBroker, NetResponse, NetTransport};
use orrery_mcp::transport::{HttpSpec, HttpTransport};
use orrery_policy::{Decision, PendingCall, PolicyBuilder, PolicyEngine};
use orrery_proto::{AgentScope, BranchId, Consent, Grant, Layer, Subject};
use orrery_tools::ToolBudget;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

/// The smallest HTTP/1.1 client that can carry a JSON-RPC frame: a deployment
/// installs its own, with its own TLS and its own proxy rules. This one refuses
/// anything but `http://`, so it cannot reach anywhere a test did not start.
#[derive(Debug)]
struct LoopbackHttp;

#[async_trait]
impl NetTransport for LoopbackHttp {
    async fn send(
        &self,
        method: &str,
        url: &str,
        headers: &BTreeMap<String, String>,
        body: &[u8],
        ceiling: u64,
    ) -> Result<NetResponse, BrokerError> {
        let rest = url.strip_prefix("http://").ok_or_else(|| {
            BrokerError::Unsupported("this transport is loopback plaintext only".to_owned())
        })?;
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));

        let mut request = format!("{method} /{path} HTTP/1.1\r\nhost: {authority}\r\n");
        for (key, value) in headers {
            request.push_str(&format!("{key}: {value}\r\n"));
        }
        request.push_str(&format!(
            "content-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        ));

        let mut stream = tokio::net::TcpStream::connect(authority)
            .await
            .map_err(|e| BrokerError::Unsupported(format!("{url}: {e}")))?;
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| BrokerError::Unsupported(format!("{url}: {e}")))?;
        stream
            .write_all(body)
            .await
            .map_err(|e| BrokerError::Unsupported(format!("{url}: {e}")))?;
        stream
            .flush()
            .await
            .map_err(|e| BrokerError::Unsupported(format!("{url}: {e}")))?;

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .await
            .map_err(|e| BrokerError::Unsupported(format!("{url}: {e}")))?;

        let text = String::from_utf8_lossy(&raw).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
        let status: u16 = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let headers: BTreeMap<String, String> = head
            .lines()
            .skip(1)
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_owned()))
            .collect();

        let truncated = body.len() as u64 > ceiling;
        let kept = body.as_bytes()[..body.len().min(ceiling as usize)].to_vec();
        Ok(NetResponse {
            status,
            headers,
            body: kept,
            truncated,
        })
    }
}

/// Answer one request with a fixed body, then stop. Returns the port.
async fn one_shot(content_type: &'static str, body: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let Ok((mut socket, _)) = listener.accept().await else {
            return;
        };
        // Read until the headers end; the body follows and we do not need it.
        let mut seen = Vec::new();
        let mut byte = [0u8; 1];
        while socket.read_exact(&mut byte).await.is_ok() {
            seen.push(byte[0]);
            if seen.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = socket.flush().await;
        let _ = socket.shutdown().await;
    });
    port
}

struct Harness {
    engine: Arc<PolicyEngine>,
    broker: Arc<dyn Broker>,
}

fn harness() -> Harness {
    let resolved = PolicyBuilder::new(".")
        .layer_toml(
            "[permissions]\nallow = [\"net(127.0.0.1)\"]\n",
            "test.toml",
            Layer::Project,
            false,
        )
        .expect("the rules parse")
        .build()
        .expect("the rules compile");
    let engine = Arc::new(PolicyEngine::new(resolved));
    let broker =
        Arc::new(LocalBroker::new(engine.ledger().clone()).with_transport(Arc::new(LoopbackHttp)));
    Harness { engine, broker }
}

fn scope() -> AgentScope {
    AgentScope {
        agent: "main".to_owned(),
        branch: BranchId::new(),
        tools: vec!["*".to_owned()],
        grant: Grant {
            capabilities: Vec::new(),
            consent: Consent::Always,
        },
    }
}

fn token(h: &Harness) -> orrery_policy::CapabilityToken {
    match h
        .engine
        .check(&PendingCall::net("127.0.0.1"), &Subject::Agent, &scope())
    {
        Decision::Allow { token, .. } => token,
        other => panic!("the rules allow it: {other:?}"),
    }
}

#[tokio::test]
async fn http_transport() {
    let port = one_shot(
        "application/json",
        r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}"#,
    )
    .await;
    let h = harness();
    let transport = HttpTransport::new(
        h.broker.clone(),
        "notes",
        HttpSpec::new(format!("http://127.0.0.1:{port}/mcp")),
    );

    assert_eq!(transport.host(), "127.0.0.1");

    let frame = transport
        .send(
            token(&h),
            &serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            &ToolBudget::new(5_000, 1 << 20),
        )
        .await
        .expect("the round trip");

    assert_eq!(frame["result"]["tools"][0]["name"], "echo");
}

#[tokio::test]
async fn an_event_stream_answer_is_read_too() {
    // Streamable HTTP lets the server answer with SSE instead of a JSON body.
    let port = one_shot(
        "text/event-stream",
        "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n",
    )
    .await;
    let h = harness();
    let transport = HttpTransport::new(
        h.broker.clone(),
        "notes",
        HttpSpec::new(format!("http://127.0.0.1:{port}/mcp")),
    );

    let frame = transport
        .send(
            token(&h),
            &serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }),
            &ToolBudget::new(5_000, 1 << 20),
        )
        .await
        .expect("the round trip");
    assert_eq!(frame["result"]["ok"], true);
}

#[tokio::test]
async fn without_a_token_for_that_host_nothing_is_dialled() {
    let port = one_shot(
        "application/json",
        r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
    )
    .await;
    let h = harness();
    // A token minted for somewhere else. The broker checks the token's resolved
    // target, not the rule that produced it.
    let elsewhere = match h
        .engine
        .check(&PendingCall::net("127.0.0.1"), &Subject::Agent, &scope())
    {
        Decision::Allow { token, .. } => token,
        other => panic!("{other:?}"),
    };
    let transport = HttpTransport::new(
        h.broker.clone(),
        "notes",
        HttpSpec::new(format!("http://localhost:{port}/mcp")),
    );

    let err = transport
        .send(
            elsewhere,
            &serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }),
            &ToolBudget::new(5_000, 1 << 20),
        )
        .await
        .expect_err("`localhost` is not `127.0.0.1` as far as the token is concerned");
    assert!(err.to_string().contains("could not be reached"), "{err}");
}
