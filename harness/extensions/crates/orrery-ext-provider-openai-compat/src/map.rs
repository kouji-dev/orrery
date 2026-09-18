//! Chat-completion chunks onto [`ModelEvent`].
//!
//! The mapper is stateful because the wire format is: a tool call arrives as a
//! `tool_calls` array whose entries are keyed by `index`, the `id` and `name`
//! appear only on the first fragment, and every later fragment is arguments
//! with nothing to say which call they belong to except that index.
//!
//! # Two things this refuses to guess
//!
//! - An unknown `finish_reason` is an error, not `EndTurn`. "The turn ended
//!   normally" is exactly the lie that hides a truncation.
//! - A tool-call fragment for an index that was never opened is an error, not a
//!   new call. A compatible server that emits one is broken, and inventing a
//!   [`CallId`] for it would put a call in the transcript the model never made.

use std::collections::HashMap;

use orrery_proto::{CallId, Usage};
use orrery_provider::{ModelEvent, ProviderError, StopReason};
use serde::Deserialize;

use crate::sse::SseEvent;

/// The sentinel an OpenAI-compatible stream ends with.
const DONE: &str = "[DONE]";

/// One `finish_reason` string onto [`StopReason`].
///
/// # Errors
///
/// [`ProviderError::BadRequest`] for a reason this build has never heard of.
pub fn stop_reason(raw: &str) -> Result<StopReason, ProviderError> {
    Ok(match raw {
        "stop" => StopReason::EndTurn,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        // vllm reports a stop *sequence* hit as `stop` with `stop_reason` set;
        // ollama uses this spelling. Both mean the same thing to the kernel.
        "stop_sequence" => StopReason::StopSequence,
        "content_filter" => StopReason::Refusal,
        other => {
            return Err(ProviderError::BadRequest(format!(
                "unknown finish_reason `{other}`: this build cannot say how the turn ended"
            )));
        }
    })
}

#[derive(Debug, Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    prompt_tokens_details: Option<PromptDetails>,
}

#[derive(Debug, Deserialize)]
struct PromptDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
}

impl WireUsage {
    fn into_usage(self) -> Usage {
        let cache_hits = self
            .prompt_tokens_details
            .and_then(|d| d.cached_tokens)
            .unwrap_or(0);
        Usage {
            // `prompt_tokens` already includes the cached ones on this wire,
            // unlike Anthropic's, so they are not summed a second time.
            input_tokens: self.prompt_tokens.unwrap_or(0),
            output_tokens: self.completion_tokens.unwrap_or(0),
            cache_hits,
            // Pricing is not the provider's to know: the kernel holds the rate
            // card, because it is the thing that compares a spend to a budget
            // across providers.
            micro_usd: None,
        }
    }
}

/// Turns chat-completion chunks into provider-neutral events.
#[derive(Debug, Default)]
pub struct EventMapper {
    /// `tool_calls[].index` to the call minted for it.
    calls: HashMap<u64, CallId>,
    /// Which indices are still open, so `[DONE]` can close them.
    open: Vec<u64>,
    started: bool,
    finished: bool,
}

impl EventMapper {
    /// A mapper that has seen nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Map one frame.
    ///
    /// # Errors
    ///
    /// [`ProviderError`] when the frame carries an `error` object, or when it
    /// is not the JSON its shape promises.
    pub fn frame(&mut self, ev: &SseEvent) -> Result<Vec<ModelEvent>, ProviderError> {
        let data = ev.data.trim();
        if data.is_empty() {
            return Ok(Vec::new());
        }
        if data == DONE {
            return Ok(self.close());
        }
        let v: serde_json::Value = serde_json::from_str(data).map_err(|e| {
            ProviderError::BadRequest(format!("chat completion chunk is not JSON: {e}"))
        })?;

        // A compatible server signals a mid-stream failure by putting an
        // `error` object in a chunk rather than by closing the socket.
        if let Some(error) = v.get("error").filter(|e| !e.is_null()) {
            return Err(crate::classify_wire_error(
                error["type"].as_str().unwrap_or_default(),
                error["code"].as_str().unwrap_or_default(),
                error["message"].as_str().unwrap_or_default(),
            ));
        }

        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            out.push(ModelEvent::Started {
                id: v["id"].as_str().unwrap_or_default().to_owned(),
            });
        }

        if let Some(choices) = v["choices"].as_array() {
            for choice in choices {
                self.choice(choice, &mut out)?;
            }
        }

        // Usage rides on the terminal chunk when `stream_options.include_usage`
        // was accepted, and on every chunk on some servers. The kernel sums, so
        // emitting it whenever it appears is correct.
        if let Some(u) = Self::usage(&v["usage"])? {
            out.push(ModelEvent::Usage { usage: u });
        }
        Ok(out)
    }

    /// What is left when the stream ends: any tool call the server left open.
    ///
    /// Called for `[DONE]` and again when the byte stream ends, so a server
    /// that omits the sentinel still closes its calls. The second call is a
    /// no-op, which is why `finished` exists.
    pub fn close(&mut self) -> Vec<ModelEvent> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        let mut out = Vec::new();
        for index in std::mem::take(&mut self.open) {
            if let Some(call) = self.calls.get(&index).copied() {
                out.push(ModelEvent::ToolUseEnd { call });
            }
        }
        out
    }

    /// Whether a `finish_reason` has already been seen. A server that both
    /// finishes and then sends a usage-only chunk is normal.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn choice(
        &mut self,
        choice: &serde_json::Value,
        out: &mut Vec<ModelEvent>,
    ) -> Result<(), ProviderError> {
        let delta = &choice["delta"];
        if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
            out.push(ModelEvent::TextDelta {
                text: text.to_owned(),
            });
        }
        // Exposed reasoning. Not in the OpenAI schema; every local server that
        // serves a reasoning model spells it one of these two ways.
        for key in ["reasoning_content", "reasoning"] {
            if let Some(text) = delta[key].as_str().filter(|t| !t.is_empty()) {
                out.push(ModelEvent::ThinkingDelta {
                    text: text.to_owned(),
                });
            }
        }

        if let Some(calls) = delta["tool_calls"].as_array() {
            for entry in calls {
                self.tool_call(entry, out)?;
            }
        }

        if let Some(raw) = choice["finish_reason"].as_str().filter(|r| !r.is_empty()) {
            let stop = stop_reason(raw)?;
            // Close the calls before saying the turn ended, so a consumer never
            // sees `Done` with a call still open.
            out.extend(self.close());
            out.push(ModelEvent::Done { stop });
        }
        Ok(())
    }

    fn tool_call(
        &mut self,
        entry: &serde_json::Value,
        out: &mut Vec<ModelEvent>,
    ) -> Result<(), ProviderError> {
        // `index` is how fragments are correlated. A server that omits it can
        // only ever be describing one call, so zero is the honest default.
        let index = entry["index"].as_u64().unwrap_or(0);
        let name = entry["function"]["name"].as_str();

        let call = match self.calls.get(&index).copied() {
            Some(call) => call,
            None => {
                let Some(name) = name else {
                    return Err(ProviderError::BadRequest(format!(
                        "tool-call fragment for index {index}, which was never opened with a name"
                    )));
                };
                // The server's `call_…` id is not a uuid and `CallId` is. We
                // mint our own and use it on the way back out too, so both
                // sides of a request match without a mapping table that could
                // go stale between passes.
                let call = CallId::new();
                self.calls.insert(index, call);
                self.open.push(index);
                out.push(ModelEvent::ToolUseStart {
                    call,
                    name: name.to_owned(),
                });
                call
            }
        };

        if let Some(fragment) = entry["function"]["arguments"]
            .as_str()
            .filter(|f| !f.is_empty())
        {
            out.push(ModelEvent::ToolUseDelta {
                call,
                json_fragment: fragment.to_owned(),
            });
        }
        Ok(())
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
