//! The other direction: a guest calling back into the broker.
//!
//! This is the whole reason the RPC crate has a bidirectional peer rather than
//! a request/response client. A guest's `ctx.proc.run(...)` is a request
//! travelling *up* the same connection its `tool/call` came down, while that
//! call is still in flight.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_ext_api::{
    BrokerError, BrokerFacade, NetRequest, ReadRequest, SpawnRequest, WriteRequest,
};
use orrery_jsonrpc::{Handler, RpcError};
use orrery_proto::ExtId;
use serde_json::Value;

use crate::protocol;

/// Answers a guest's broker calls, and nothing else.
///
/// Every method a guest may name is on this list. There is no fall-through, no
/// "pass anything starting with `fs/`" — a method the host does not implement
/// is `method not found`, which is what keeps the guest's reachable surface
/// equal to the broker's.
pub struct BrokerBridge {
    ext: ExtId,
    broker: Arc<dyn BrokerFacade>,
}

impl std::fmt::Debug for BrokerBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrokerBridge")
            .field("ext", &self.ext.as_str())
            .finish_non_exhaustive()
    }
}

impl BrokerBridge {
    /// A bridge for one extension.
    #[must_use]
    pub fn new(ext: ExtId, broker: Arc<dyn BrokerFacade>) -> Self {
        Self { ext, broker }
    }
}

/// A denial travels to the guest as a JSON-RPC error carrying the rule, so the
/// guest's own `catch` can tell "not allowed" from "it broke".
fn to_rpc(error: BrokerError) -> RpcError {
    match error {
        BrokerError::Denied { rule, reason } => RpcError::Rpc {
            code: -32001,
            message: reason,
            data: Some(serde_json::json!({ "denied": true, "rule": rule.to_string() })),
        },
        BrokerError::Cancelled { .. } => RpcError::Cancelled,
        other => RpcError::internal(other.to_string()),
    }
}

fn bad_params(e: serde_json::Error) -> RpcError {
    RpcError::invalid_params(e.to_string())
}

#[async_trait]
impl Handler for BrokerBridge {
    async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        match method {
            protocol::BROKER_READ => {
                let p: protocol::ReadParams = serde_json::from_value(params).map_err(bad_params)?;
                let chunk = self
                    .broker
                    .read(ReadRequest::new(&p.path, p.limit).at(p.offset))
                    .await
                    .map_err(to_rpc)?;
                Ok(serde_json::to_value(protocol::ReadReply {
                    text: String::from_utf8_lossy(&chunk.bytes).into_owned(),
                    eof: chunk.eof,
                    total: chunk.total,
                })
                .expect("a read reply always serialises"))
            }
            protocol::BROKER_WRITE => {
                let p: protocol::WriteParams =
                    serde_json::from_value(params).map_err(bad_params)?;
                let mut request = WriteRequest::new(&p.path, p.text.into_bytes());
                request.atomic = p.atomic;
                self.broker.write(request).await.map_err(to_rpc)?;
                Ok(Value::Null)
            }
            protocol::BROKER_SPAWN => {
                let p: protocol::SpawnParams =
                    serde_json::from_value(params).map_err(bad_params)?;
                let mut request = SpawnRequest::new(&p.program, p.args);
                request.cwd = p.cwd.map(Into::into);
                request.timeout_ms = p.timeout_ms;
                let out = self.broker.spawn(request).await.map_err(to_rpc)?;
                Ok(serde_json::to_value(protocol::SpawnReply {
                    status: out.status,
                    stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                    truncated: out.truncated,
                })
                .expect("a spawn reply always serialises"))
            }
            protocol::BROKER_FETCH => {
                let request: NetRequestWire = serde_json::from_value(params).map_err(bad_params)?;
                let response = self
                    .broker
                    .fetch(NetRequest {
                        method: request.method,
                        url: request.url,
                        headers: request.headers,
                        body: request.body.map(String::into_bytes),
                    })
                    .await
                    .map_err(to_rpc)?;
                Ok(serde_json::json!({
                    "status": response.status,
                    "headers": response.headers,
                    "body": String::from_utf8_lossy(&response.body),
                }))
            }
            protocol::BROKER_CREDENTIAL => {
                let p: protocol::CredentialParams =
                    serde_json::from_value(params).map_err(bad_params)?;
                let value = self.broker.credential(&p.name).await.map_err(to_rpc)?;
                Ok(serde_json::to_value(protocol::CredentialReply { value })
                    .expect("a credential reply always serialises"))
            }
            other => Err(RpcError::method_not_found(other)),
        }
    }

    async fn notify(&self, method: &str, _params: Value) {
        tracing::debug!(
            target: "orrery.host.rpc",
            ext = %self.ext,
            method,
            "notification from a guest"
        );
    }
}

/// `broker/fetch`'s params, with a text body.
#[derive(serde::Deserialize)]
struct NetRequestWire {
    #[serde(default = "get")]
    method: String,
    url: String,
    #[serde(default)]
    headers: Vec<(String, String)>,
    #[serde(default)]
    body: Option<String>,
}

fn get() -> String {
    "GET".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{BrokerBridge, to_rpc};
    use orrery_ext_api::{BrokerError, DeniesEverything};
    use orrery_jsonrpc::{Handler, RpcError};
    use std::sync::Arc;

    #[tokio::test]
    async fn a_method_the_broker_does_not_have_is_not_found() {
        let bridge = BrokerBridge::new("x".parse().unwrap(), Arc::new(DeniesEverything));
        let answer = bridge
            .request("fs/readFileSync", serde_json::json!({}))
            .await;
        assert!(
            matches!(answer, Err(RpcError::Rpc { code, .. }) if code == RpcError::METHOD_NOT_FOUND)
        );
    }

    #[tokio::test]
    async fn a_denial_reaches_the_guest_as_a_denial() {
        let bridge = BrokerBridge::new("x".parse().unwrap(), Arc::new(DeniesEverything));
        let answer = bridge
            .request(
                crate::protocol::BROKER_SPAWN,
                serde_json::json!({ "program": "java", "args": [] }),
            )
            .await;
        match answer {
            Err(RpcError::Rpc { data, .. }) => {
                assert_eq!(data.unwrap()["denied"], true);
            }
            other => panic!("expected a denial, got {other:?}"),
        }
    }

    #[test]
    fn a_cancellation_is_not_dressed_up_as_an_internal_error() {
        let cancelled = to_rpc(BrokerError::Cancelled {
            reason: orrery_proto::CancelReason::User,
        });
        assert!(matches!(cancelled, RpcError::Cancelled));
    }
}
