//! Task 8: cancellation has to stop costing money, not just stop rendering.
//!
//! The server is a plain `TcpListener` on loopback — no network request leaves
//! the machine, and nothing here talks to Anthropic.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use orrery_ext_provider_anthropic::AnthropicProvider;
use orrery_ext_provider_anthropic::auth::{CredStore, MemoryCredStore};
use orrery_proto::{Message, MessageRole};
use orrery_provider::{ModelEvent, ModelRequest, Provider};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

const STARTED: &str = concat!(
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01Hang\",\"usage\":",
    "{\"input_tokens\":10,\"output_tokens\":1,\"cache_read_input_tokens\":0}}}\n\n",
);

/// Serves one response that opens and then never ends, and reports when the
/// client's half of the connection goes away.
async fn never_ending_server() -> (String, oneshot::Receiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (closed_tx, closed_rx) = oneshot::channel();

    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.expect("accept");

        // Drain the request head so the client's write completes.
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match sock.read(&mut byte).await {
                Ok(0) | Err(_) => return,
                Ok(_) => head.push(byte[0]),
            }
        }

        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n{}\r\n",
            STARTED.len(),
            STARTED
        );
        if sock.write_all(head.as_bytes()).await.is_err() {
            return;
        }
        let _ = sock.flush().await;

        // Then nothing, for as long as the client is willing to wait. The read
        // returns 0 the moment the client drops its end — which is the whole
        // assertion of this file.
        let mut sink = [0u8; 1024];
        loop {
            match sock.read(&mut sink).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
        let _ = closed_tx.send(());
    });

    (format!("http://{addr}"), closed_rx)
}

fn request() -> ModelRequest {
    ModelRequest::new(
        "claude-sonnet-4-5",
        Arc::from([Message::text(MessageRole::User, "hello")]),
        1024,
    )
}

async fn provider(base_url: String) -> AnthropicProvider {
    let store: Arc<dyn CredStore> = Arc::new(MemoryCredStore::default());
    store.put("anthropic", "sk-ant-test").await.expect("stored");
    AnthropicProvider::new(store).with_base_url(base_url)
}

#[tokio::test]
async fn drops_the_body() {
    let (base_url, closed) = never_ending_server().await;
    let cancel = CancellationToken::new();
    let mut stream = provider(base_url).await.stream(request(), cancel.clone());

    // The response opened: we are past the headers and into a body that will
    // never finish on its own.
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("the server answered")
        .expect("one event")
        .expect("not an error");
    assert!(matches!(first, ModelEvent::Started { .. }), "{first:?}");

    cancel.cancel();

    // The stream ends. Events already buffered when the token flipped still
    // come out — cancelling stops the request, it does not un-emit what the
    // provider already saw — so drain to the end rather than demanding the very
    // next poll be `None`.
    loop {
        let next = tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .expect("cancelled stream ends promptly");
        match next {
            None => break,
            Some(buffered) => assert!(buffered.is_ok(), "{buffered:?}"),
        }
    }
    drop(stream);

    // ...and the connection actually closed, which is the part that stops the
    // meter. A stream that merely stopped yielding would leave the request
    // running and the tokens billing.
    tokio::time::timeout(Duration::from_secs(10), closed)
        .await
        .expect("the server saw the connection close")
        .expect("server task alive");
}

#[tokio::test]
async fn dropping_the_stream_also_closes_it() {
    let (base_url, closed) = never_ending_server().await;
    let mut stream = provider(base_url)
        .await
        .stream(request(), CancellationToken::new());
    let _ = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("the server answered");
    drop(stream);
    tokio::time::timeout(Duration::from_secs(10), closed)
        .await
        .expect("the server saw the connection close")
        .expect("server task alive");
}
