//! Anthropic's frames onto [`ModelEvent`].
//!
//! The mapper is stateful because the wire format is: a
//! `content_block_delta` says only `index`, and what an index *means* was
//! decided by the `content_block_start` that opened it.

use std::collections::HashMap;

use orrery_proto::{CallId, Usage};
use orrery_provider::{ModelEvent, ProviderError, StopReason};
use serde::Deserialize;

use crate::sse::SseEvent;

/// One Anthropic `stop_reason` string onto [`StopReason`].
///
/// # Errors
///
/// [`ProviderError::BadRequest`] for a reason this build has never heard of.
/// Deliberately not a silent `EndTurn`: "the turn finished normally" is exactly
/// the lie that hides a truncation, and a new stop reason is a thing a human
/// should look at.
pub fn stop_reason(raw: &str) -> Result<StopReason, ProviderError> {
    Ok(match raw {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        "refusal" => StopReason::Refusal,
        other => {
            return Err(ProviderError::BadRequest(format!(
                "unknown Anthropic stop_reason `{other}`: this build cannot say how the turn ended"
            )));
        }
    })
}

/// What an open content block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Text,
    Thinking,
    /// The call minted for this block.
    ToolUse(CallId),
}

#[derive(Debug, Deserialize)]
struct WireUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
}

impl WireUsage {
    fn into_usage(self) -> Usage {
        Usage {
            // Cache creation is billed as input, and Anthropic reports it
            // separately from `input_tokens`. Summing them is what makes the
            // budget match the invoice.
            input_tokens: self.input_tokens.unwrap_or(0)
                + self.cache_read_input_tokens.unwrap_or(0)
                + self.cache_creation_input_tokens.unwrap_or(0),
            output_tokens: self.output_tokens.unwrap_or(0),
            cache_hits: self.cache_read_input_tokens.unwrap_or(0),
            // Pricing is not the provider's to know: the kernel holds the rate
            // card, because it is the thing that has to compare a spend to a
            // budget across providers.
            micro_usd: None,
        }
    }
}

/// Turns Anthropic frames into provider-neutral events.
#[derive(Debug, Default)]
pub struct EventMapper {
    blocks: HashMap<u64, Block>,
}

impl EventMapper {
    /// A mapper with no blocks open.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Map one frame. Most produce one event; `message_start` produces two and
    /// `ping` produces none.
    ///
    /// # Errors
    ///
    /// [`ProviderError`] when the frame is an `error` event — classified by its
    /// Anthropic error type — or when it is not the JSON its name promises.
    pub fn frame(&mut self, ev: &SseEvent) -> Result<Vec<ModelEvent>, ProviderError> {
        let json = |ev: &SseEvent| -> Result<serde_json::Value, ProviderError> {
            serde_json::from_str(&ev.data).map_err(|e| {
                ProviderError::BadRequest(format!("`{}` frame is not JSON: {e}", ev.name))
            })
        };
        match ev.name.as_str() {
            // Keep-alive. Not nothing — it proves the stream is alive — but it
            // carries no model output, so it maps to no event.
            "ping" => Ok(Vec::new()),
            "message_start" => {
                let v = json(ev)?;
                let id = v["message"]["id"].as_str().unwrap_or_default().to_owned();
                let mut out = vec![ModelEvent::Started { id }];
                if let Some(u) = Self::usage(&v["message"]["usage"])? {
                    out.push(ModelEvent::Usage { usage: u });
                }
                Ok(out)
            }
            "content_block_start" => {
                let v = json(ev)?;
                let index = Self::index(&v)?;
                let block = &v["content_block"];
                match block["type"].as_str() {
                    Some("text") => {
                        self.blocks.insert(index, Block::Text);
                        Ok(Vec::new())
                    }
                    Some("thinking" | "redacted_thinking") => {
                        self.blocks.insert(index, Block::Thinking);
                        Ok(Vec::new())
                    }
                    Some("tool_use" | "server_tool_use") => {
                        // Anthropic's `toolu_…` id is not a uuid, and `CallId`
                        // is. We mint our own and use it on the way back out
                        // too, so the two sides of a request match without a
                        // mapping table that could go stale.
                        let call = CallId::new();
                        self.blocks.insert(index, Block::ToolUse(call));
                        Ok(vec![ModelEvent::ToolUseStart {
                            call,
                            name: block["name"].as_str().unwrap_or_default().to_owned(),
                        }])
                    }
                    other => Err(ProviderError::BadRequest(format!(
                        "unknown Anthropic content block type `{}`",
                        other.unwrap_or("<missing>")
                    ))),
                }
            }
            "content_block_delta" => {
                let v = json(ev)?;
                let index = Self::index(&v)?;
                let delta = &v["delta"];
                let text = |k: &str| delta[k].as_str().unwrap_or_default().to_owned();
                Ok(match delta["type"].as_str() {
                    Some("text_delta") => vec![ModelEvent::TextDelta { text: text("text") }],
                    Some("thinking_delta") => vec![ModelEvent::ThinkingDelta {
                        text: text("thinking"),
                    }],
                    // The cryptographic signature of a thinking block. It is not
                    // model output and has nowhere to go in a provider-neutral
                    // event; dropping it is why this crate does not send
                    // thinking blocks back (see `request.rs`).
                    Some("signature_delta") => Vec::new(),
                    Some("input_json_delta") => {
                        let Some(Block::ToolUse(call)) = self.blocks.get(&index).copied() else {
                            return Err(ProviderError::BadRequest(format!(
                                "input_json_delta for block {index}, which is not a tool use"
                            )));
                        };
                        vec![ModelEvent::ToolUseDelta {
                            call,
                            json_fragment: text("partial_json"),
                        }]
                    }
                    other => {
                        return Err(ProviderError::BadRequest(format!(
                            "unknown Anthropic delta type `{}`",
                            other.unwrap_or("<missing>")
                        )));
                    }
                })
            }
            "content_block_stop" => {
                let v = json(ev)?;
                let index = Self::index(&v)?;
                Ok(match self.blocks.remove(&index) {
                    Some(Block::ToolUse(call)) => vec![ModelEvent::ToolUseEnd { call }],
                    _ => Vec::new(),
                })
            }
            "message_delta" => {
                let v = json(ev)?;
                let mut out = Vec::new();
                if let Some(u) = Self::usage(&v["usage"])? {
                    out.push(ModelEvent::Usage { usage: u });
                }
                if let Some(raw) = v["delta"]["stop_reason"].as_str() {
                    out.push(ModelEvent::Done {
                        stop: stop_reason(raw)?,
                    });
                }
                Ok(out)
            }
            // `Done` already went out with `message_delta`'s stop reason; the
            // stop frame is the envelope closing, not a second ending.
            "message_stop" => Ok(Vec::new()),
            "error" => {
                let v = json(ev)?;
                Err(crate::classify_wire_error(
                    v["error"]["type"].as_str().unwrap_or_default(),
                    v["error"]["message"].as_str().unwrap_or_default(),
                ))
            }
            other => Err(ProviderError::BadRequest(format!(
                "unknown Anthropic event `{other}`"
            ))),
        }
    }

    fn index(v: &serde_json::Value) -> Result<u64, ProviderError> {
        v["index"].as_u64().ok_or_else(|| {
            ProviderError::BadRequest("content block frame has no `index`".to_owned())
        })
    }

    fn usage(v: &serde_json::Value) -> Result<Option<Usage>, ProviderError> {
        if v.is_null() {
            return Ok(None);
        }
        let wire: WireUsage = serde_json::from_value(v.clone())
            .map_err(|e| ProviderError::BadRequest(format!("usage is not usage: {e}")))?;
        Ok(Some(wire.into_usage()))
    }
}
