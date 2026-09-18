//! Building the `POST /v1/messages` body by hand.
//!
//! No vendored SDK. The body is eleven keys and a content-block match; a
//! dependency would buy us a second opinion about serde versions and a
//! translation layer to debug when the API adds a field.

use orrery_proto::{ContentBlock, Message, MessageRole, Outcome};
use orrery_provider::{Capabilities, ModelRequest, ProviderError};
use serde_json::{Map, Value, json};

/// The whole request body, ready to `POST`.
///
/// # Errors
///
/// [`ProviderError::BadRequest`] when the request asks for something the
/// declared [`Capabilities`] rule out — an image to a provider without
/// `images`, say. Rejecting is the point: silently dropping a block would send
/// the model a conversation that never happened.
pub fn build_body(req: &ModelRequest, caps: &Capabilities) -> Result<Value, ProviderError> {
    let mut system = String::new();
    if let Some(s) = &req.system {
        system.push_str(s);
    }

    let mut messages = Vec::with_capacity(req.messages.len());
    for (i, m) in req.messages.iter().enumerate() {
        // Anthropic has no system role inside `messages`; an in-band system
        // message is hoisted rather than rejected, because the conversation
        // shape is the kernel's business and this crate's job is to say it in
        // Anthropic.
        if m.role == MessageRole::System {
            for block in &m.content {
                if let ContentBlock::Text { text } = block {
                    if !system.is_empty() {
                        system.push_str("\n\n");
                    }
                    system.push_str(text);
                }
            }
            continue;
        }
        let mark = caps.cache && req.cache_breakpoint == Some(i);
        if let Some(value) = message(m, caps, mark)? {
            messages.push(value);
        }
    }

    let mut body = Map::new();
    body.insert("model".to_owned(), json!(req.model));
    body.insert("max_tokens".to_owned(), json!(req.max_output_tokens));
    body.insert("stream".to_owned(), json!(true));
    body.insert("messages".to_owned(), Value::Array(messages));
    if !system.is_empty() {
        body.insert("system".to_owned(), json!(system));
    }
    if let Some(t) = req.temperature {
        body.insert("temperature".to_owned(), json!(t));
    }
    if !req.stop.is_empty() {
        body.insert("stop_sequences".to_owned(), json!(req.stop));
    }
    // A provider without `tools` is never sent tool descriptors — not an empty
    // array, no key at all, because some endpoints treat `"tools": []` as
    // "tools are on and there are none".
    if caps.tools && !req.tools.is_empty() {
        body.insert(
            "tools".to_owned(),
            Value::Array(
                req.tools
                    .iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description,
                            "input_schema": t.input_schema,
                        })
                    })
                    .collect(),
            ),
        );
    }
    Ok(Value::Object(body))
}

/// `None` when every block of the message was dropped, which would otherwise
/// put an empty `content` array on the wire and be rejected.
fn message(m: &Message, caps: &Capabilities, mark: bool) -> Result<Option<Value>, ProviderError> {
    let mut blocks = Vec::with_capacity(m.content.len());
    for b in &m.content {
        if let Some(v) = block(b, caps)? {
            blocks.push(v);
        }
    }
    if blocks.is_empty() {
        return Ok(None);
    }
    if mark {
        // The breakpoint marks the *end* of the stable prefix, so the marker
        // goes on the last block of that message: everything up to and
        // including it is cacheable.
        if let Some(Value::Object(last)) = blocks.last_mut() {
            last.insert("cache_control".to_owned(), json!({ "type": "ephemeral" }));
        }
    }
    Ok(Some(json!({
        "role": match m.role {
            MessageRole::Assistant => "assistant",
            // `System` was hoisted before we got here.
            _ => "user",
        },
        "content": blocks,
    })))
}

fn block(b: &ContentBlock, caps: &Capabilities) -> Result<Option<Value>, ProviderError> {
    Ok(match b {
        ContentBlock::Text { text } => Some(json!({ "type": "text", "text": text })),
        ContentBlock::Image { media_type, data } => {
            if !caps.images {
                return Err(ProviderError::BadRequest(format!(
                    "this model does not accept images, and a message carries a {media_type} block"
                )));
            }
            Some(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": media_type, "data": data },
            }))
        }
        // The `CallId` is used verbatim as the Anthropic id. Both halves of the
        // pairing are written by us in the same request, so they match without
        // a mapping table that could go stale between passes.
        ContentBlock::ToolUse { call, name, input } => Some(json!({
            "type": "tool_use",
            "id": call.to_string(),
            "name": name,
            "input": input,
        })),
        ContentBlock::ToolResult { call, outcome } => {
            let (is_error, text) = outcome_text(outcome);
            Some(json!({
                "type": "tool_result",
                "tool_use_id": call.to_string(),
                "is_error": is_error,
                "content": [{ "type": "text", "text": text }],
            }))
        }
        // Thinking blocks come back to us without the signature Anthropic
        // requires on input — `signature_delta` has nowhere to live in a
        // provider-neutral event. Sending an unsigned one is a 400, so the
        // block stays in the transcript and off the wire.
        ContentBlock::Thinking { .. } => None,
        // `ContentBlock` is `#[non_exhaustive]`: a block this build does not
        // know is not silently dropped, because the model would then be
        // answering a conversation that never happened.
        other => {
            return Err(ProviderError::BadRequest(format!(
                "this build cannot express {other:?} to Anthropic"
            )));
        }
    })
}

/// What the model is told a tool did, and whether it failed.
///
/// The same [`Outcome`] the transcript carries — a denial reads identically to
/// the person and to the model, instead of being a structured value in one and
/// a rendered apology in the other.
fn outcome_text(outcome: &Outcome) -> (bool, String) {
    match outcome {
        Outcome::Ok { value, surface } => (
            false,
            value
                .as_ref()
                .map(ToString::to_string)
                .or_else(|| surface.as_ref().map(|s| json!(s).to_string()))
                .unwrap_or_else(|| "ok".to_owned()),
        ),
        Outcome::Denied { reason, .. } => (true, format!("denied by policy: {reason}")),
        Outcome::Truncated {
            bytes_emitted,
            limit,
            surface,
        } => (
            false,
            format!(
                "{}\n\n[truncated: {bytes_emitted} bytes produced, {limit} allowed]",
                surface
                    .as_ref()
                    .map_or_else(String::new, |s| json!(s).to_string())
            ),
        ),
        Outcome::Cancelled { reason } => (true, format!("cancelled: {reason:?}")),
        Outcome::Unloaded { ext } => (true, format!("extension `{ext}` is no longer loaded")),
        Outcome::Failed { code, message } => (true, format!("{code}: {message}")),
        other => (true, format!("{other:?}")),
    }
}
