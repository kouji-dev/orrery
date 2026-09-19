//! Round 5, item 2: the binary has a code path to a real model.
//!
//! `ProviderChoice` had exactly two variants, `Fixture` and `Custom`, so no
//! configuration, key or flag could reach a real model — the product could not
//! talk to one, ever. This asserts the variant exists, that `assemble` selects
//! the provider for it, and that a turn runs end to end through it.
//!
//! **Nothing here reaches the network.** The transport is injectable: the
//! provider's `base_url` points at a `TcpListener` on loopback that speaks
//! Anthropic's SSE shape from a hand-written fixture, and the key is a literal
//! in this file's environment. If this test ever needed an API key or an
//! outbound connection it would be the wrong test.
//!
//! The whole file is behind the `anthropic` feature, which is off by default
//! and off in CI because it links a TLS stack. Run it with:
//!
//! ```text
//! cargo test -p orrery-harness --features anthropic --test anthropic
//! ```

#![cfg(feature = "anthropic")]

use orrery_harness::{Harness, ProviderChoice, ResolvedConfig};
use orrery_kernel::TurnOutcome;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// One complete Messages response, hand-written. No model produced this, and
/// none needs to: the shape is the contract, and the contract is public.
const STREAM: &str = concat!(
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01Stub\",\"usage\":",
    "{\"input_tokens\":11,\"output_tokens\":1,\"cache_read_input_tokens\":0}}}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":0,",
    "\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,",
    "\"delta\":{\"type\":\"text_delta\",\"text\":\"the stub answered\"}}\n\n",
    "event: content_block_stop\n",
    "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    "event: message_delta\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},",
    "\"usage\":{\"output_tokens\":4}}\n\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n\n",
);

/// A loopback server that answers every request with [`STREAM`], and reports
/// the `x-api-key` header it was sent.
async fn stub_server() -> (String, tokio::sync::oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.expect("accept");

        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match sock.read(&mut byte).await {
                Ok(0) | Err(_) => return,
                Ok(_) => head.push(byte[0]),
            }
        }
        let head_text = String::from_utf8_lossy(&head).to_ascii_lowercase();
        let key = head_text
            .lines()
            .find_map(|l| l.strip_prefix("x-api-key:"))
            .map(|v| v.trim().to_owned())
            .unwrap_or_default();

        // Drain the body: content-length is in the head we just read.
        let len: usize = head_text
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        let _ = sock.read_exact(&mut body).await;

        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{STREAM}",
            STREAM.len()
        );
        let _ = sock.write_all(response.as_bytes()).await;
        let _ = sock.flush().await;
        let _ = tx.send(key);
    });

    (format!("http://{addr}"), rx)
}

/// The variant selects the provider, and a turn runs through it.
#[test]
fn the_anthropic_variant_runs_a_turn_through_a_stub() {
    // `EnvCredStore` is the documented development fallback the provider reads
    // through. A literal, in this process, for a server on loopback.
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-stub");
    }

    let dir = tempfile::tempdir().expect("a temporary workspace");
    // **Multi-thread on purpose.** The stub is a `tokio::spawn`ed task, and the
    // request that has to reach it is driven by the *harness's* runtime, not by
    // this one. On a current-thread runtime nothing drives the listener while
    // `harness.block_on` waits, so the two sit waiting for each other and the
    // test hangs rather than fails — which is exactly what it did.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("a runtime for the stub");
    let (base_url, key_seen) = runtime.block_on(stub_server());
    // The listener lives on this runtime, and this runtime must not be dropped
    // before the turn finishes.

    let mut config = ResolvedConfig::fixture(dir.path(), Vec::new());
    config.provider = ProviderChoice::Anthropic {
        model: "claude-sonnet-4-5".to_owned(),
        credential: "anthropic".to_owned(),
        base_url: Some(base_url),
    };
    config.kernel.model = "claude-sonnet-4-5".to_owned();

    let harness = Harness::build(config).expect("the harness builds with a real provider");
    let outcome = harness
        .block_on(harness.submit("hello", CancellationToken::new()))
        .expect("the turn ran");

    match outcome {
        TurnOutcome::Completed { text, .. } => {
            assert!(text.contains("the stub answered"), "{text}");
        }
        other => panic!("the turn did not complete through the provider: {other:?}"),
    }

    let key = runtime
        .block_on(async { tokio::time::timeout(std::time::Duration::from_secs(5), key_seen).await })
        .expect("the stub server saw a request")
        .expect("the stub server reported the key");
    assert_eq!(
        key, "sk-ant-stub",
        "the provider authenticated from the credential store, not from nowhere"
    );
}
