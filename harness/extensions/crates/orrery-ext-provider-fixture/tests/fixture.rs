//! Task 4: the provider every other plan's tests run against.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use orrery_ext_provider_fixture::FixtureProvider;
use orrery_proto::{Message, MessageRole};
use orrery_provider::{ModelEvent, ModelRequest, Provider, StopReason};
use tokio_util::sync::CancellationToken;

fn write(dir: &Path, name: &str, lines: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, lines).expect("write fixture");
    p
}

fn request() -> ModelRequest {
    ModelRequest::new(
        "fixture",
        Arc::from([Message::text(MessageRole::User, "hi")]),
        256,
    )
}

async fn drain(p: &FixtureProvider) -> Vec<Result<ModelEvent, orrery_provider::ProviderError>> {
    p.stream(request(), CancellationToken::new())
        .collect::<Vec<_>>()
        .await
}

/// Where the six shared fixtures live, relative to this crate.
fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../clients/conformance/streams")
        .join(name)
}

#[tokio::test]
async fn replays_in_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write(
        dir.path(),
        "three.jsonl",
        concat!(
            "{\"t\":\"started\",\"id\":\"msg_1\"}\n",
            "{\"t\":\"text-delta\",\"text\":\"hello\"}\n",
            "{\"t\":\"done\",\"stop\":\"end-turn\"}\n",
        ),
    );
    let p = FixtureProvider::load(&path).expect("load");
    let events: Vec<_> = drain(&p)
        .await
        .into_iter()
        .map(|e| e.expect("no error lines"))
        .collect();
    assert_eq!(
        events,
        vec![
            ModelEvent::Started {
                id: "msg_1".to_owned()
            },
            ModelEvent::TextDelta {
                text: "hello".to_owned()
            },
            ModelEvent::Done {
                stop: StopReason::EndTurn
            },
        ]
    );
}

#[tokio::test]
async fn cancellation_ends_the_stream() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write(
        dir.path(),
        "slow.jsonl",
        concat!(
            "{\"t\":\"started\",\"id\":\"msg_1\"}\n",
            "{\"delay_ms\":3600000,\"t\":\"text-delta\",\"text\":\"never\"}\n",
        ),
    );
    let p = FixtureProvider::load(&path).expect("load");
    let cancel = CancellationToken::new();
    let mut s = p.stream(request(), cancel.clone());

    assert!(matches!(
        s.next().await,
        Some(Ok(ModelEvent::Started { .. }))
    ));

    let started = Instant::now();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        c.cancel();
    });
    // The stream ends; it does not yield the delayed event and it does not
    // wait an hour.
    assert!(s.next().await.is_none(), "cancelled stream must end");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn an_error_line_ends_the_stream_with_that_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write(
        dir.path(),
        "boom.jsonl",
        concat!(
            "{\"t\":\"text-delta\",\"text\":\"partial\"}\n",
            "{\"error\":{\"code\":\"rate_limited\",\"retry_after_ms\":1200}}\n",
        ),
    );
    let p = FixtureProvider::load(&path).expect("load");
    let out = drain(&p).await;
    assert_eq!(out.len(), 2);
    let err = out[1].as_ref().expect_err("second line is an error");
    assert!(err.is_retryable());
    assert_eq!(err.code(), "rate_limited");
}

#[test]
fn spec_parsing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write(dir.path(), "s.jsonl", "{\"t\":\"done\",\"stop\":\"end-turn\"}\n");
    let spec = format!("fixture:{}", path.display());
    let p = FixtureProvider::from_spec(&spec).expect("spec");
    assert_eq!(p.id(), "fixture");

    assert!(FixtureProvider::from_spec(&path.display().to_string()).is_err());
    assert!(FixtureProvider::from_spec("fixture:").is_err());
}

#[test]
fn a_malformed_line_fails_at_load_not_mid_stream() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write(dir.path(), "bad.jsonl", "{\"t\":\"no-such-event\"}\n");
    let e = FixtureProvider::load(&path).expect_err("rejected");
    assert!(e.to_string().contains("line 1"), "{e}");
}

#[test]
fn capabilities_are_declared() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write(dir.path(), "s.jsonl", "{\"t\":\"done\",\"stop\":\"end-turn\"}\n");
    let p = FixtureProvider::load(&path).expect("load");
    assert!(p.capabilities().tools);
    assert!(!p.counter().is_exact());
}

/// The six shared fixtures are the corpus plans 05, 08, 09b and 09c consume.
/// They must exist, parse, and replay as their names claim.
#[tokio::test]
async fn the_six_conformance_streams_replay() {
    for name in [
        "text-turn.jsonl",
        "tool-call.jsonl",
        "tool-call-consent.jsonl",
        "max-tokens.jsonl",
        "retryable-error.jsonl",
        "never-ends.jsonl",
    ] {
        let p = FixtureProvider::load(&corpus(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        // `never-ends` is the cancellation fixture: draining it would hang, so
        // loading it is all this case asserts.
        if name == "never-ends.jsonl" {
            continue;
        }
        let out = drain(&p).await;
        assert!(!out.is_empty(), "{name} is empty");
        match name {
            "max-tokens.jsonl" => assert_eq!(
                out.last().expect("last").as_ref().expect("event"),
                &ModelEvent::Done {
                    stop: StopReason::MaxTokens
                }
            ),
            "retryable-error.jsonl" => {
                let e = out.last().expect("last").as_ref().expect_err("errors");
                assert!(e.is_retryable(), "{name}: {e}");
            }
            "tool-call.jsonl" | "tool-call-consent.jsonl" => {
                assert!(
                    out.iter()
                        .any(|e| matches!(e, Ok(ModelEvent::ToolUseStart { .. }))),
                    "{name} has no tool call"
                );
                assert_eq!(
                    out.last().expect("last").as_ref().expect("event"),
                    &ModelEvent::Done {
                        stop: StopReason::ToolUse
                    }
                );
            }
            _ => assert_eq!(
                out.last().expect("last").as_ref().expect("event"),
                &ModelEvent::Done {
                    stop: StopReason::EndTurn
                }
            ),
        }
    }
}
