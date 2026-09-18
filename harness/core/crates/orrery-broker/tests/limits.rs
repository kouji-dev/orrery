//! Task 7 · the filesystem and the ceilings that apply while it is being read.

mod common;

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use common::Fixture;
use orrery_broker::{Broker, BrokerError, LimitedReader};
use orrery_policy::PendingCall;
use orrery_tools::ToolBudget;
use tokio::io::{AsyncRead, ReadBuf};

/// A 100 MB source that counts every byte anybody pulls out of it.
///
/// The assertion is about the **peak**, not the result, so the source has to be
/// the witness: a truncated `Vec` proves nothing about what was in memory on the
/// way there.
#[derive(Debug)]
struct Instrumented {
    remaining: u64,
    pulled: Arc<AtomicU64>,
}

impl Instrumented {
    fn new(total: u64) -> (Self, Arc<AtomicU64>) {
        let pulled = Arc::new(AtomicU64::new(0));
        (
            Self {
                remaining: total,
                pulled: Arc::clone(&pulled),
            },
            pulled,
        )
    }
}

impl AsyncRead for Instrumented {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.remaining == 0 {
            return Poll::Ready(Ok(()));
        }
        let n = buf
            .remaining()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        buf.initialize_unfilled_to(n);
        buf.advance(n);
        self.remaining -= n as u64;
        self.pulled.fetch_add(n as u64, Ordering::SeqCst);
        Poll::Ready(Ok(()))
    }
}

const MB: u64 = 1024 * 1024;

#[tokio::test]
async fn read_is_bounded_while_reading() {
    let ceiling = 4 * 1024;
    let (source, pulled) = Instrumented::new(100 * MB);
    let mut reader = LimitedReader::new(source, ceiling);

    let err = reader.read_to_end().await.expect_err("100 MB is over 4 KB");
    assert!(matches!(err, BrokerError::LimitExceeded { .. }), "{err}");

    let peak = pulled.load(Ordering::SeqCst);
    assert!(
        peak <= ceiling + orrery_broker::limit::CHUNK as u64,
        "pulled {peak} bytes for a {ceiling}-byte ceiling: the limit was applied after the read, not during it"
    );
    assert!(peak >= ceiling, "nothing was read at all: {peak}");
}

#[tokio::test]
async fn a_file_under_the_ceiling_reads_whole() {
    let fx = Fixture::new();
    let path = fx.path("small.txt");
    tokio::fs::write(&path, b"hello").await.unwrap();

    let token = fx.read_token(&path);
    let mut reader = fx
        .broker
        .read(token, &path, &ToolBudget::new(1_000, 1024))
        .await
        .expect("the read is allowed");
    assert_eq!(reader.read_to_end().await.unwrap(), b"hello");
}

#[tokio::test]
async fn a_file_over_the_ceiling_is_refused_not_buffered() {
    let fx = Fixture::new();
    let path = fx.path("big.txt");
    tokio::fs::write(&path, vec![b'x'; 64 * 1024])
        .await
        .unwrap();

    let token = fx.read_token(&path);
    let mut reader = fx
        .broker
        .read(token, &path, &ToolBudget::new(1_000, 4 * 1024))
        .await
        .unwrap();
    let err = reader.read_to_end().await.expect_err("over the ceiling");
    assert!(matches!(err, BrokerError::LimitExceeded { .. }), "{err}");
    assert!(reader.pulled() <= 4 * 1024 + orrery_broker::limit::CHUNK as u64);
}

/// Every broker method rejects without a valid token. The type system stops a
/// token being *made*; the ledger stops one being *reused*.
#[tokio::test]
async fn no_token_no_call() {
    let fx = Fixture::new();
    let path = fx.path("f.txt");
    tokio::fs::write(&path, b"x").await.unwrap();
    let budget = ToolBudget::new(1_000, 1024);

    // Spent: the same nonce twice.
    let token = fx.read_token(&path);
    let nonce = token.nonce();
    let _ok = fx.broker.read(token, &path, &budget).await.unwrap();
    assert!(matches!(
        fx.engine.ledger().redeem(nonce),
        Err(orrery_policy::TokenError::Spent)
    ));

    // Revoked: the call was cancelled while the token was in flight.
    let call = orrery_proto::CallId::new();
    let token = fx.token(&PendingCall::read(path.display().to_string()).in_call(call));
    fx.engine.ledger().revoke_call(call);
    let err = fx
        .broker
        .read(token, &path, &budget)
        .await
        .expect_err("revoked");
    assert!(
        matches!(err, BrokerError::Token(orrery_policy::TokenError::Revoked)),
        "{err}"
    );

    // Wrong aspect: a read token cannot open a write.
    let token = fx.read_token(&path);
    let err = fx
        .broker
        .write(token, &path, true)
        .await
        .expect_err("a read token is not a write token");
    assert!(matches!(err, BrokerError::WrongAspect { .. }), "{err}");

    // Wrong target: a token for one path does not open another.
    let other = fx.path("other.txt");
    let token = fx.read_token(&path);
    let err = fx
        .broker
        .read(token, &other, &budget)
        .await
        .expect_err("a token is scoped to what it was minted for");
    assert!(matches!(err, BrokerError::OutsideScope { .. }), "{err}");

    // And the network refuses outright, because nothing installed a transport.
    let token = fx.token(&PendingCall::net("example.com"));
    let err = fx
        .broker
        .net(
            token,
            orrery_broker::NetRequest::get("https://example.com/"),
            &budget,
        )
        .await
        .expect_err("the broker does not dial by default");
    assert!(matches!(err, BrokerError::NoTransport), "{err}");
}
