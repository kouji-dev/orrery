//! Task 6: Anthropic frames onto `ModelEvent`.

use std::path::Path;

use orrery_ext_provider_anthropic::map::{EventMapper, stop_reason};
use orrery_ext_provider_anthropic::sse::SseParser;
use orrery_provider::{ModelEvent, ProviderError, StopReason, ToolCallAccumulator};

fn replay(name: &str) -> Vec<Result<ModelEvent, ProviderError>> {
    let raw = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut parser = SseParser::new();
    let mut mapper = EventMapper::new();
    let mut out = Vec::new();
    // Seven-byte chunks on purpose: mapping must not depend on framing.
    for chunk in raw.chunks(7) {
        for frame in parser.push(chunk).expect("well-formed") {
            match mapper.frame(&frame) {
                Ok(events) => out.extend(events.into_iter().map(Ok)),
                Err(e) => out.push(Err(e)),
            }
        }
    }
    out
}

fn oks(name: &str) -> Vec<ModelEvent> {
    replay(name)
        .into_iter()
        .map(|e| e.unwrap_or_else(|e| panic!("{name}: {e}")))
        .collect()
}

#[test]
fn a_text_turn_maps() {
    let events = oks("text-turn.sse");
    assert_eq!(
        events.first(),
        Some(&ModelEvent::Started {
            id: "msg_01TextTurn".to_owned()
        })
    );
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            ModelEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "The workspace has three crates.");
    assert_eq!(
        events.last(),
        Some(&ModelEvent::Done {
            stop: StopReason::EndTurn
        })
    );
}

#[test]
fn usage_includes_cache_hits() {
    let usage: Vec<orrery_proto::Usage> = oks("tool-call.sse")
        .into_iter()
        .filter_map(|e| match e {
            ModelEvent::Usage { usage } => Some(usage),
            _ => None,
        })
        .collect();
    assert!(!usage.is_empty(), "no usage reported");
    let total: orrery_proto::Usage = usage.into_iter().sum();
    // `Usage::input_tokens` counts cache hits inside it (see `orrery-proto`),
    // while Anthropic reports them beside `input_tokens`. Folding them in here
    // is what makes a budget comparable across providers.
    assert_eq!(total.input_tokens, 1_876 + 1_536);
    assert_eq!(total.cache_hits, 1_536);
    // `message_delta` carries the final output count; `message_start` carries a
    // provisional 2. Summing must not double-count the input side.
    assert_eq!(total.output_tokens, 60);
}

#[test]
fn a_tool_call_accumulates_into_one_parsed_call() {
    let events = oks("tool-call.sse");
    let thinking: String = events
        .iter()
        .filter_map(|e| match e {
            ModelEvent::ThinkingDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(thinking, "The manifest is the place to look.");

    let mut acc = ToolCallAccumulator::new();
    let calls: Vec<_> = events
        .iter()
        .filter_map(|e| acc.feed(e))
        .map(|r| r.expect("well-formed"))
        .collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "builtin.read");
    assert_eq!(calls[0].input, serde_json::json!({ "path": "Cargo.toml" }));
    assert_eq!(
        events.last(),
        Some(&ModelEvent::Done {
            stop: StopReason::ToolUse
        })
    );
}

#[test]
fn a_truncated_turn_says_so() {
    assert_eq!(
        oks("max-tokens.sse").last(),
        Some(&ModelEvent::Done {
            stop: StopReason::MaxTokens
        })
    );
}

#[test]
fn an_error_frame_becomes_a_classified_error() {
    let out = replay("overloaded.sse");
    let e = out.last().expect("something").as_ref().expect_err("errors");
    assert!(e.is_retryable(), "overloaded_error is retryable: {e}");
    assert_eq!(e.code(), "server");
}

#[test]
fn stop_reason_maps() {
    for (raw, expected) in [
        ("end_turn", StopReason::EndTurn),
        ("tool_use", StopReason::ToolUse),
        ("max_tokens", StopReason::MaxTokens),
        ("stop_sequence", StopReason::StopSequence),
        ("refusal", StopReason::Refusal),
    ] {
        assert_eq!(stop_reason(raw).expect(raw), expected);
    }
    // An unknown stop reason is an error, not a silent `EndTurn` — that is
    // exactly the lie that hides a truncation.
    let e = stop_reason("pause_turn").expect_err("unknown");
    assert_eq!(e.code(), "bad_request");
    assert!(e.to_string().contains("pause_turn"), "{e}");
}
