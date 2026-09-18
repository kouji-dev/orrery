//! Plan 03, phase 5 · the whole provider, driven end to end over committed
//! bytes.
//!
//! **No socket is opened anywhere in this file.** Every case replays a `.sse`
//! file under `tests/fixtures/` through [`RecordedTransport`], in seven-byte
//! slices, so a frame split across a chunk boundary is the default rather than
//! a case somebody remembered to write.

use std::sync::Arc;

use futures_util::StreamExt;
use orrery_ext_api::{CredStore, MemoryCredStore};
use orrery_ext_provider_openai_compat::transport::{ChatTransport, RecordedTransport};
use orrery_ext_provider_openai_compat::{OpenAiCompatProvider, default_capabilities};
use orrery_proto::{Message, MessageRole};
use orrery_provider::{
    ModelEvent, ModelRequest, Provider, ProviderError, StopReason, ToolCallAccumulator,
};
use tokio_util::sync::CancellationToken;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn provider(transport: Arc<dyn ChatTransport>) -> OpenAiCompatProvider {
    OpenAiCompatProvider::new(
        "http://localhost:11434/v1",
        transport,
        Arc::new(MemoryCredStore::default()) as Arc<dyn CredStore>,
    )
}

fn request() -> ModelRequest {
    ModelRequest::new(
        "qwen2.5-coder",
        Arc::from([Message::text(MessageRole::User, "hello")]),
        1024,
    )
}

async fn drain(
    transport: Arc<dyn ChatTransport>,
) -> Vec<Result<ModelEvent, ProviderError>> {
    provider(transport)
        .stream(request(), CancellationToken::new())
        .collect()
        .await
}

async fn events(name: &str) -> Vec<ModelEvent> {
    drain(Arc::new(RecordedTransport::ok(fixture(name))))
        .await
        .into_iter()
        .map(|e| e.unwrap_or_else(|e| panic!("{name}: {e}")))
        .collect()
}

#[tokio::test]
async fn a_text_turn_maps() {
    let events = events("text-turn.sse").await;
    assert!(
        matches!(&events[0], ModelEvent::Started { id } if id == "chatcmpl-1"),
        "{:?}",
        events[0]
    );
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            ModelEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello, world");
    // An empty `content` on the role-announcing chunk is not a delta. Emitting
    // it would put a zero-length event in every transcript.
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, ModelEvent::TextDelta { .. }))
            .count(),
        2
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ModelEvent::Done { stop: StopReason::EndTurn })),
        "{events:?}"
    );
}

#[tokio::test]
async fn usage_carries_the_cached_prompt_tokens() {
    let usage = events("text-turn.sse")
        .await
        .into_iter()
        .find_map(|e| match e {
            ModelEvent::Usage { usage } => Some(usage),
            _ => None,
        })
        .expect("a usage event");
    assert_eq!(usage.input_tokens, 24);
    assert_eq!(usage.output_tokens, 5);
    // Cached tokens are *inside* `prompt_tokens` on this wire, unlike
    // Anthropic's. Summing them would bill the prefix twice.
    assert_eq!(usage.cache_hits, 8);
    assert_eq!(usage.total_tokens(), 29);
    assert_eq!(usage.micro_usd, None, "the kernel holds the rate card");
}

#[tokio::test]
async fn a_tool_call_accumulates_into_one_parsed_call() {
    let events = events("tool-call.sse").await;
    let mut acc = ToolCallAccumulator::new();
    let mut done = Vec::new();
    for event in &events {
        if let Some(call) = acc.feed(event) {
            done.push(call.expect("the fragments reassemble"));
        }
    }
    assert_eq!(done.len(), 1, "{events:?}");
    assert_eq!(done[0].name, "read");
    assert_eq!(done[0].input["path"], "Cargo.toml");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ModelEvent::Done { stop: StopReason::ToolUse })),
        "{events:?}"
    );
}

#[tokio::test]
async fn a_truncated_turn_says_so() {
    let events = events("max-tokens.sse").await;
    // Never `EndTurn`: "the turn finished normally" is the lie that hides a
    // truncation.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ModelEvent::Done { stop: StopReason::MaxTokens })),
        "{events:?}"
    );
}

#[tokio::test]
async fn exposed_reasoning_is_kept_and_kept_separate() {
    let events = events("thinking.sse").await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ModelEvent::ThinkingDelta { text } if text == "let me think")),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ModelEvent::TextDelta { text } if text == "42")),
        "{events:?}"
    );
}

#[tokio::test]
async fn an_error_object_in_a_chunk_is_classified() {
    let out = drain(Arc::new(RecordedTransport::ok(fixture("overloaded.sse")))).await;
    let e = out
        .into_iter()
        .find_map(Result::err)
        .expect("the chunk was an error");
    assert!(matches!(e, ProviderError::ServerError { .. }), "{e:?}");
    assert!(e.is_retryable());
}

#[tokio::test]
async fn frames_survive_being_split_anywhere() {
    // One byte at a time: the pathological case for a parser that assumed a
    // chunk was a frame.
    let transport = Arc::new(RecordedTransport::ok(fixture("text-turn.sse")).with_chunk_size(1));
    let text: String = drain(transport)
        .await
        .into_iter()
        .filter_map(|e| match e.expect("no error") {
            ModelEvent::TextDelta { text } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello, world");
}

#[tokio::test]
async fn a_stream_that_ends_without_done_still_closes_its_tool_call() {
    // Everything up to, but not including, the `[DONE]` sentinel — which is
    // what a server that dies mid-turn leaves behind.
    let whole = String::from_utf8(fixture("tool-call.sse")).expect("utf-8");
    let truncated = whole.split("data: [DONE]").next().expect("a prefix").to_owned();
    let events: Vec<ModelEvent> = drain(Arc::new(RecordedTransport::ok(truncated)))
        .await
        .into_iter()
        .map(|e| e.expect("no error"))
        .collect();

    let starts = events
        .iter()
        .filter(|e| matches!(e, ModelEvent::ToolUseStart { .. }))
        .count();
    let ends = events
        .iter()
        .filter(|e| matches!(e, ModelEvent::ToolUseEnd { .. }))
        .count();
    assert_eq!((starts, ends), (1, 1), "an open call would hang a turn");
}

#[tokio::test]
async fn a_failing_status_is_classified_not_parsed() {
    let transport = Arc::new(RecordedTransport::failing(
        429,
        vec![("retry-after".to_owned(), "30".to_owned())],
        br#"{"error":{"message":"slow down","type":"rate_limit_error"}}"#.to_vec(),
    ));
    let e = drain(transport)
        .await
        .into_iter()
        .find_map(Result::err)
        .expect("a 429");
    // Seconds on the wire, milliseconds in the type.
    assert_eq!(
        e,
        ProviderError::RateLimited {
            retry_after_ms: Some(30_000)
        }
    );
}

#[tokio::test]
async fn a_context_overflow_is_not_a_bad_request() {
    let transport = Arc::new(RecordedTransport::failing(
        400,
        Vec::new(),
        br#"{"error":{"message":"This model's maximum context length is 8192 tokens"}}"#.to_vec(),
    ));
    let e = drain(transport)
        .await
        .into_iter()
        .find_map(Result::err)
        .expect("a 400");
    // The kernel's answer to this is "compact and try again", and to a
    // `BadRequest` is "fail the turn". Getting it wrong costs the turn.
    assert!(matches!(e, ProviderError::ContextTooLong { .. }), "{e:?}");
}

#[tokio::test]
async fn a_model_this_endpoint_does_not_serve_is_refused_without_a_request() {
    let transport = Arc::new(RecordedTransport::ok(fixture("text-turn.sse")));
    let provider = provider(transport).serving("llama");
    let out: Vec<_> = provider
        .stream(request(), CancellationToken::new())
        .collect()
        .await;
    assert_eq!(out.len(), 1);
    assert!(
        matches!(out[0], Err(ProviderError::NoSuchModel(_))),
        "{:?}",
        out[0]
    );
}

#[tokio::test]
async fn cancelling_stops_the_stream() {
    let transport = Arc::new(RecordedTransport::ok(fixture("text-turn.sse")).with_chunk_size(1));
    let cancel = CancellationToken::new();
    cancel.cancel();
    let out: Vec<_> = provider(transport)
        .stream(request(), cancel)
        .collect()
        .await;
    assert!(out.is_empty(), "a cancelled stream produces nothing: {out:?}");
}

#[tokio::test]
async fn a_local_server_needs_no_key_and_is_not_asked_to_log_in() {
    let provider = provider(Arc::new(RecordedTransport::ok(Vec::new())));
    let state = provider.auth().state().await.expect("state");
    // `NeedsLogin` would have the kernel refuse the turn before it starts,
    // which for ollama on localhost is the wrong answer.
    assert_eq!(state, orrery_provider::AuthState::Anonymous);
}

#[test]
fn the_endpoint_is_built_from_the_base_url() {
    let p = provider(Arc::new(RecordedTransport::ok(Vec::new())));
    assert_eq!(p.endpoint(), "http://localhost:11434/v1/chat/completions");
    assert_eq!(p.id(), "openai-compat");
    assert_eq!(p.capabilities(), &default_capabilities());
    // Conservative on purpose: believing 128k on a laptop builds a request the
    // server rejects.
    assert!(!p.capabilities().cache);
    assert_eq!(p.capabilities().max_context, 8_192);
}

#[test]
fn the_manifest_is_the_same_shape_a_third_party_ships() {
    let manifest = orrery_ext_api::ExtensionManifest::from_toml_str(
        orrery_ext_provider_openai_compat::MANIFEST,
        "orrery.toml",
    )
    .expect("the shipped manifest parses");
    assert_eq!(manifest.name.as_str(), "openai-compat");
    assert!(
        manifest
            .contributions()
            .iter()
            .any(|c| c.name == "openai-compat")
    );
}
