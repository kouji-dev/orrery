//! The four things `lsp/client.rs` could not do, asserted.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use orrery_jsonrpc::{Framing, Handler, Peer, RpcError, StderrRing, cancel};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// A far side that answers after however long the caller asked it to wait, and
/// remembers every notification it was sent.
#[derive(Default)]
struct Slow {
    notifications: Mutex<Vec<(String, Value)>>,
}

#[async_trait]
impl Handler for Slow {
    async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let delay = params.get("delay_ms").and_then(Value::as_u64).unwrap_or(0);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        Ok(json!({ "echo": method, "params": params }))
    }

    async fn notify(&self, method: &str, params: Value) {
        self.notifications.lock().push((method.to_owned(), params));
    }
}

/// Two peers over one in-memory pipe. No process, no node, no network.
fn pair(near: Arc<dyn Handler>, far: Arc<dyn Handler>) -> (Peer, Peer) {
    let (a, b) = tokio::io::duplex(64 * 1024);
    let (ar, aw) = tokio::io::split(a);
    let (br, bw) = tokio::io::split(b);
    (
        Peer::spawn(ar, aw, Framing::ContentLength, near),
        Peer::spawn(br, bw, Framing::ContentLength, far),
    )
}

#[tokio::test]
async fn concurrent_calls_demux() {
    let far = Arc::new(Slow::default());
    let (near, _far_peer) = pair(Arc::new(orrery_jsonrpc::NoHandler), far);

    // Three in flight, answered in the reverse of the order they were sent.
    let never = CancellationToken::new();
    let a = near.call("a", json!({ "delay_ms": 90 }), &never);
    let b = near.call("b", json!({ "delay_ms": 40 }), &never);
    let c = near.call("c", json!({ "delay_ms": 5 }), &never);
    let (a, b, c) = tokio::join!(a, b, c);

    assert_eq!(a.unwrap()["echo"], "a", "each caller gets its own reply");
    assert_eq!(b.unwrap()["echo"], "b");
    assert_eq!(c.unwrap()["echo"], "c");
}

#[tokio::test]
async fn cancel_reaches_one_call() {
    let far = Arc::new(Slow::default());
    let (near, _far_peer) = pair(Arc::new(orrery_jsonrpc::NoHandler), far.clone());

    let never = CancellationToken::new();
    let cancel_two = CancellationToken::new();
    let one = near.call("one", json!({ "delay_ms": 30 }), &never);
    let two = near.call("two", json!({ "delay_ms": 5_000 }), &cancel_two);
    let three = near.call("three", json!({ "delay_ms": 30 }), &never);

    let canceller = async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancel_two.cancel();
    };

    let (one, two, three, ()) = tokio::join!(one, two, three, canceller);
    // The notification is on the wire the moment the call returns; give the far
    // side a turn of the loop to take delivery of it.
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(one.is_ok(), "call 1 completes: {one:?}");
    assert!(three.is_ok(), "call 3 completes: {three:?}");
    assert!(
        matches!(two, Err(RpcError::Cancelled)),
        "only call 2 is cancelled: {two:?}"
    );

    // `turn.cancel` reaches one in-flight call, not the connection.
    let sent = far.notifications.lock().clone();
    let cancels: Vec<&(String, Value)> = sent.iter().filter(|(m, _)| m == cancel::METHOD).collect();
    assert_eq!(cancels.len(), 1, "one cancel, for one call: {sent:?}");
    assert_eq!(cancels[0].1["id"], 2, "and it names call 2");
}

/// The guest calling back into the broker: a request that travels the other way
/// down the same connection.
#[derive(Default)]
struct Broker {
    asked: Mutex<Vec<String>>,
}

#[async_trait]
impl Handler for Broker {
    async fn request(&self, method: &str, _params: Value) -> Result<Value, RpcError> {
        self.asked.lock().push(method.to_owned());
        match method {
            "broker/read" => Ok(json!({ "bytes": "hello", "eof": true })),
            other => Err(RpcError::method_not_found(other)),
        }
    }
}

#[tokio::test]
async fn server_initiated_request() {
    let broker = Arc::new(Broker::default());
    let (_near, far) = pair(broker.clone(), Arc::new(orrery_jsonrpc::NoHandler));

    // The far side (the guest) calls back into the near side (the host).
    let answer = far
        .call(
            "broker/read",
            json!({ "path": "a.txt" }),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(answer["bytes"], "hello");
    assert_eq!(&*broker.asked.lock(), &["broker/read"]);

    // And a method the host does not have is an error, not a hang.
    let missing = far
        .call(
            "broker/launch-missiles",
            json!({}),
            &CancellationToken::new(),
        )
        .await;
    assert!(
        matches!(missing, Err(RpcError::Rpc { code, .. }) if code == RpcError::METHOD_NOT_FOUND),
        "{missing:?}"
    );
}

#[tokio::test]
async fn stderr_is_ringed() {
    // A chatty child must not grow the host without bound.
    let ring = StderrRing::new(512);
    for i in 0..100_000 {
        ring.push(format!("line {i} with a good deal of padding on it\n").as_bytes());
    }
    assert!(
        ring.len() <= 512,
        "the ring is bounded, not the child: {}",
        ring.len()
    );
    let tail = ring.tail();
    assert!(
        tail.ends_with("padding on it\n"),
        "and it keeps the *end*, which is where the reason lives: {tail:?}"
    );
}

#[tokio::test]
async fn a_dead_connection_fails_every_pending_call() {
    // The far end is a bare pipe with nobody on it. Dropping it is a child
    // process dying mid-call, which is the case that matters.
    let (near_end, far_end) = tokio::io::duplex(64 * 1024);
    let (reader, writer) = tokio::io::split(near_end);
    let near = Peer::spawn(
        reader,
        writer,
        Framing::ContentLength,
        Arc::new(orrery_jsonrpc::NoHandler),
    );

    let never = CancellationToken::new();
    let pending = near.call("slow", json!({ "delay_ms": 5_000 }), &never);
    let kill = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(far_end);
    };

    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(pending, kill)
    })
    .await
    .expect("a dead far side must not hang the caller");

    assert!(
        matches!(result, Err(RpcError::Closed)),
        "a call whose connection died is Closed, not forgotten: {result:?}"
    );
    assert_eq!(near.in_flight(), 0, "and the pending map is emptied");
}
