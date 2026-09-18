//! Orrery as an MCP server: the visible set for one scope, and nothing else.
//!
//! # Exposing cannot leak what the scope could not call itself
//!
//! `tools/list` here is [`Registry::visible`](orrery_tools::Registry::visible)
//! for the scope, exactly — the same function the model's own tool list comes
//! from. So a tool outside the scope is not listed, and because
//! [`Registry::dispatch`] refuses a reference the scope cannot see, it is not
//! callable either: the listing is not the security boundary, and is not being
//! asked to be.
//!
//! Inbound `tools/call` runs the same seven dispatch steps a local call does,
//! policy check included. There is no second door.
//!
//! # Authentication
//!
//! There is none here, and that is deliberate. Open question 4, decided:
//! **share plan 08's answer, do not invent a second one.** A listener is
//! [`serve`]d over a byte stream the caller supplies; over stdio the caller is
//! whoever started the process, and over a socket the answer to "who is that"
//! belongs to the same layer that answers it for plan 08's HTTP listener.
//! [`serve`] takes an already-authenticated stream and says so in its
//! signature rather than growing a token check that would then disagree with
//! the one next door.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_jsonrpc::{Framing, Handler, Peer, RpcError};
use orrery_proto::{AgentScope, CallId, Outcome, Subject};
use orrery_tools::{CallCtx, Registry, Resolution, ToolBudget};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::client::{INITIALIZE, PROTOCOL_VERSION, TOOLS_CALL, TOOLS_LIST};

/// What Orrery calls itself when it is the server.
pub const SERVER_NAME: &str = "orrery";

/// One exposed handle: a scope, the registry behind it, and the budget inbound
/// calls run under.
///
/// Holds the scope by value because an exposure is *for* a scope — there is no
/// method that changes it, so an exposure cannot be widened after the fact.
pub struct McpServerHandle {
    registry: Arc<Registry>,
    scope: AgentScope,
    budget: ToolBudget,
    subject: Subject,
}

impl std::fmt::Debug for McpServerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServerHandle")
            .field("agent", &self.scope.agent)
            .field("tools", &self.registry.visible(&self.scope).len())
            .finish_non_exhaustive()
    }
}

impl McpServerHandle {
    /// Expose the visible set for one scope.
    #[must_use]
    pub fn new(registry: Arc<Registry>, scope: AgentScope, budget: ToolBudget) -> Self {
        let subject = Subject::SubAgent(scope.agent.clone());
        Self {
            registry,
            scope,
            budget,
            subject,
        }
    }

    /// Act as somebody other than the scope's own sub-agent.
    #[must_use]
    pub fn as_subject(mut self, subject: Subject) -> Self {
        self.subject = subject;
        self
    }

    /// What this exposure lists: `visible(scope)`, in MCP's shape.
    #[must_use]
    pub fn tools(&self) -> Vec<Value> {
        self.registry
            .visible(&self.scope)
            .into_iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema,
                })
            })
            .collect()
    }

    /// Run one inbound call, through the ordinary dispatch path.
    async fn call_tool(&self, params: &Value) -> Result<Value, RpcError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::invalid_params("`name` is required"))?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        // Resolution is scoped: a name outside the scope is `Unknown`, which is
        // the same answer the model would get. Nothing here reveals that the
        // tool exists elsewhere.
        let resolution = self.registry.resolve(name, &self.scope);
        let Some(r#ref) = resolution.r#ref().cloned() else {
            let Resolution::Unknown { did_you_mean, .. } = resolution else {
                unreachable!("only `Unknown` has no reference")
            };
            return Ok(error_result(&format!(
                "no such tool: `{name}`{}",
                if did_you_mean.is_empty() {
                    String::new()
                } else {
                    format!(" (did you mean {}?)", did_you_mean.join(", "))
                }
            )));
        };

        let ctx = CallCtx::new(
            CallId::new(),
            self.subject.clone(),
            self.scope.clone(),
            self.budget,
        );
        let outcome = self
            .registry
            .dispatch(&r#ref, arguments, ctx)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;
        Ok(outcome_to_result(&outcome))
    }
}

#[async_trait]
impl Handler for McpServerHandle {
    async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        match method {
            INITIALIZE => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
            })),
            TOOLS_LIST => Ok(json!({ "tools": self.tools() })),
            TOOLS_CALL => self.call_tool(&params).await,
            other => Err(RpcError::method_not_found(other)),
        }
    }
}

/// Serve MCP over an **already-authenticated** stream.
///
/// The returned [`Peer`] keeps the connection alive; dropping it ends it.
pub fn serve<R, W>(handle: Arc<McpServerHandle>, reader: R, writer: W) -> Peer
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    Peer::spawn(reader, writer, Framing::LineDelimited, handle)
}

/// An [`Outcome`] as MCP's `tools/call` result.
///
/// A denial comes back as `isError: true` with the reason, because that is
/// MCP's vocabulary for "the call happened and did not work". It is **not** a
/// JSON-RPC error: a refusal is a value on this side of the wire, and turning
/// it into a protocol error would make the caller's error handler and its
/// result handler describe the same refusal in two different ways.
#[must_use]
pub fn outcome_to_result(outcome: &Outcome) -> Value {
    match outcome {
        Outcome::Ok { value, .. } => json!({
            "content": [text_block(&render(value.as_ref()))],
            "isError": false,
        }),
        Outcome::Truncated {
            bytes_emitted,
            limit,
            ..
        } => json!({
            "content": [text_block(&format!(
                "the result was cut short at {limit} bytes ({bytes_emitted} produced)"
            ))],
            "isError": false,
        }),
        Outcome::Denied { rule, reason } => json!({
            "content": [text_block(reason)],
            "isError": true,
            "_meta": { "orrery/rule": rule.to_string() },
        }),
        Outcome::Unloaded { ext } => error_result(&format!("`{ext}` is not loaded")),
        Outcome::Cancelled { .. } => error_result("cancelled"),
        Outcome::Failed { code, message } => error_result(&format!("{code}: {message}")),
        // `Outcome` is `#[non_exhaustive]`: an outcome this crate has not been
        // taught about is reported, not silently called a success.
        _ => error_result("the call produced an outcome this server cannot render"),
    }
}

fn render(value: Option<&Value>) -> String {
    match value {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn text_block(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

fn error_result(message: &str) -> Value {
    json!({ "content": [text_block(message)], "isError": true })
}

#[cfg(test)]
mod tests {
    use super::{error_result, outcome_to_result};
    use orrery_proto::Outcome;

    #[test]
    fn a_denial_is_an_is_error_result_not_a_protocol_error() {
        let rule = orrery_proto::RuleId::new();
        let rendered = outcome_to_result(&Outcome::Denied {
            rule,
            reason: "no".to_owned(),
        });
        assert_eq!(rendered["isError"], true);
        assert_eq!(rendered["content"][0]["text"], "no");
        assert_eq!(rendered["_meta"]["orrery/rule"], rule.to_string());
    }

    #[test]
    fn an_ok_with_nothing_to_say_still_has_a_content_block() {
        let rendered = outcome_to_result(&Outcome::ok());
        assert_eq!(rendered["isError"], false);
        assert_eq!(rendered["content"][0]["type"], "text");
    }

    #[test]
    fn an_error_result_is_shaped_like_a_result() {
        let rendered = error_result("gone");
        assert_eq!(rendered["isError"], true);
    }
}
