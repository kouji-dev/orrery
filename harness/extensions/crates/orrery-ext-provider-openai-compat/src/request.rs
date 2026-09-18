//! Building the `POST /v1/chat/completions` body by hand.
//!
//! No vendored SDK, for the reason `orrery-ext-provider-anthropic` gives: the
//! body is nine keys and a content-block match, and a dependency would buy a
//! second opinion about serde versions plus a translation layer to debug when
//! the API adds a field.
//!
//! # Where this differs from Anthropic
//!
//! - `system` is a message with `role: "system"`, not a top-level key.
//! - a tool result is its own `role: "tool"` message keyed by `tool_call_id`,
//!   not a block inside a user message.
//! - `max_completion_tokens` is the current spelling; `max_tokens` is the one
//!   ollama and vllm still accept, so both go on the wire.
//! - there is no prompt-cache breakpoint to mark: `Capabilities::cache` is
//!   false for this family and `cache_breakpoint` is ignored, which the trait
//!   documents as a no-op rather than an error.

use orrery_proto::{ContentBlock, Message, MessageRole, Outcome};
use orrery_provider::{Capabilities, ModelRequest, ProviderError};
use serde_json::{Map, Value, json};

/// The whole request body, ready to `POST`.
///
/// # Errors
///
/// [`ProviderError::BadRequest`] when the request asks for something the
/// declared [`Capabilities`] rule out. Rejecting is the point: silently
/// dropping a block would send the model a conversation that never happened.
pub fn build_body(req: &ModelRequest, caps: &Capabilities) -> Result<Value, ProviderError> {
    let mut messages: Vec<Value> = Vec::with_capacity(req.messages.len() + 1);
    if let Some(system) = &req.system {
        messages.push(json!({ "role": "system", "content": system.to_string() }));
    }
    for m in req.messages.iter() {
        push_message(&mut messages, m, caps)?;
    }

    let mut body = Map::new();
    body.insert("model".to_owned(), json!(req.model));
    body.insert("stream".to_owned(), json!(true));
    // Ask for usage on the terminal chunk. Servers that do not know the option
    // ignore it; the ones that do stop making us guess what a turn cost.
    body.insert(
        "stream_options".to_owned(),
        json!({ "include_usage": true }),
    );
    body.insert("messages".to_owned(), Value::Array(messages));
    body.insert(
        "max_completion_tokens".to_owned(),
        json!(req.max_output_tokens),
    );
    // The superseded spelling, kept because ollama and vllm still read it and
    // ignore the new one. Sending both is how one body serves both.
    body.insert("max_tokens".to_owned(), json!(req.max_output_tokens));
    if let Some(t) = req.temperature {
        body.insert("temperature".to_owned(), json!(t));
    }
    if !req.stop.is_empty() {
        body.insert("stop".to_owned(), json!(req.stop));
    }
    // Never `"tools": []`: some compatible servers read an empty array as
    // "tools are on and there are none" and then refuse every turn.
    if caps.tools && !req.tools.is_empty() {
        body.insert(
            "tools".to_owned(),
            Value::Array(
                req.tools
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.input_schema,
                            },
                        })
                    })
                    .collect(),
            ),
        );
    }
    Ok(Value::Object(body))
}

/// One [`Message`] becomes one *or more* wire messages: a tool result cannot
/// ride inside a user message here, so each becomes its own `role: "tool"`.
fn push_message(
    out: &mut Vec<Value>,
    m: &Message,
    caps: &Capabilities,
) -> Result<(), ProviderError> {
    let role = match m.role {
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
        _ => "user",
    };

    let mut content: Vec<Value> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut results: Vec<Value> = Vec::new();

    for b in &m.content {
        match b {
            ContentBlock::Text { text } => {
                content.push(json!({ "type": "text", "text": text }));
            }
            ContentBlock::Image { media_type, data } => {
                if !caps.images {
                    return Err(ProviderError::BadRequest(format!(
                        "this model does not accept images, and a message carries a \
                         {media_type} block"
                    )));
                }
                content.push(json!({
                    "type": "image_url",
                    "image_url": { "url": format!("data:{media_type};base64,{data}") },
                }));
            }
            ContentBlock::ToolUse { call, name, input } => {
                tool_calls.push(json!({
                    "id": call.to_string(),
                    "type": "function",
                    "function": {
                        "name": name,
                        // Arguments are a *string* of JSON on this wire, not an
                        // object. Sending the object is the single most common
                        // way a compatible server 400s.
                        "arguments": serde_json::to_string(input)
                            .unwrap_or_else(|_| "{}".to_owned()),
                    },
                }));
            }
            ContentBlock::ToolResult { call, outcome } => {
                results.push(json!({
                    "role": "tool",
                    "tool_call_id": call.to_string(),
                    "content": outcome_text(outcome),
                }));
            }
            // Reasoning comes back without whatever the server needs to accept
            // it again, and no compatible endpoint has a field for it. It stays
            // in the transcript and off the wire.
            ContentBlock::Thinking { .. } => {}
            other => {
                return Err(ProviderError::BadRequest(format!(
                    "this build cannot express {other:?} to an OpenAI-compatible endpoint"
                )));
            }
        }
    }

    if !content.is_empty() || !tool_calls.is_empty() {
        let mut msg = Map::new();
        msg.insert("role".to_owned(), json!(role));
        // A single text block goes as a bare string: the array form is newer
        // than several of the servers this crate exists for.
        match content.as_slice() {
            [] => {
                msg.insert("content".to_owned(), Value::Null);
            }
            [one] if one["type"] == "text" => {
                msg.insert("content".to_owned(), one["text"].clone());
            }
            _ => {
                msg.insert("content".to_owned(), Value::Array(content));
            }
        }
        if !tool_calls.is_empty() {
            msg.insert("tool_calls".to_owned(), Value::Array(tool_calls));
        }
        out.push(Value::Object(msg));
    }
    out.extend(results);
    Ok(())
}

/// What the model is told a tool did.
///
/// This wire has no `is_error` flag on a tool message, so the failure has to be
/// in the text or the model never learns the call went wrong.
fn outcome_text(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Ok { value, surface } => value
            .as_ref()
            .map(ToString::to_string)
            .or_else(|| surface.as_ref().map(|s| json!(s).to_string()))
            .unwrap_or_else(|| "ok".to_owned()),
        Outcome::Denied { reason, .. } => format!("error: denied by policy: {reason}"),
        Outcome::Truncated {
            bytes_emitted,
            limit,
            surface,
        } => format!(
            "{}\n\n[truncated: {bytes_emitted} bytes produced, {limit} allowed]",
            surface
                .as_ref()
                .map_or_else(String::new, |s| json!(s).to_string())
        ),
        Outcome::Cancelled { reason } => format!("error: cancelled: {reason:?}"),
        Outcome::Unloaded { ext } => format!("error: extension `{ext}` is no longer loaded"),
        Outcome::Failed { code, message } => format!("error: {code}: {message}"),
        other => format!("error: {other:?}"),
    }
}
