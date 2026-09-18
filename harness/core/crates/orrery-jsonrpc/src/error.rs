//! What a call can come back as, other than an answer.

use crate::message::ErrorObject;

/// A call that did not return a result.
#[non_exhaustive]
#[derive(Clone, Debug, thiserror::Error)]
pub enum RpcError {
    /// The far side refused, in JSON-RPC's own vocabulary.
    #[error("{message} ({code})")]
    Rpc {
        /// The code.
        code: i64,
        /// What it said.
        message: String,
        /// Anything else it said.
        data: Option<serde_json::Value>,
    },
    /// This one call was cancelled. The connection is fine and the other calls
    /// on it are unaffected — that distinction is the entire reason this crate
    /// exists rather than the ADE's `recv_timeout` client.
    #[error("cancelled")]
    Cancelled,
    /// The connection is gone. Every pending call gets this, once.
    #[error("the connection is closed")]
    Closed,
    /// The far side answered with something that is not a JSON-RPC frame, or
    /// not one we can use.
    #[error("{0}")]
    Protocol(String),
}

impl RpcError {
    /// JSON-RPC's own code for "I do not have that method".
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// JSON-RPC's own code for "those are not the arguments".
    pub const INVALID_PARAMS: i64 = -32602;
    /// JSON-RPC's own code for "it broke".
    pub const INTERNAL_ERROR: i64 = -32603;

    /// "I do not have that method."
    #[must_use]
    pub fn method_not_found(method: &str) -> Self {
        RpcError::Rpc {
            code: Self::METHOD_NOT_FOUND,
            message: format!("no such method: `{method}`"),
            data: None,
        }
    }

    /// "Those are not the arguments."
    #[must_use]
    pub fn invalid_params(why: impl Into<String>) -> Self {
        RpcError::Rpc {
            code: Self::INVALID_PARAMS,
            message: why.into(),
            data: None,
        }
    }

    /// "It broke."
    #[must_use]
    pub fn internal(why: impl Into<String>) -> Self {
        RpcError::Rpc {
            code: Self::INTERNAL_ERROR,
            message: why.into(),
            data: None,
        }
    }

    /// How this goes back over the wire.
    #[must_use]
    pub fn to_object(&self) -> ErrorObject {
        match self {
            RpcError::Rpc {
                code,
                message,
                data,
            } => ErrorObject {
                code: *code,
                message: message.clone(),
                data: data.clone(),
            },
            RpcError::Cancelled => ErrorObject {
                code: -32800, // LSP's RequestCancelled, and MCP uses it too.
                message: "cancelled".to_owned(),
                data: None,
            },
            RpcError::Closed => ErrorObject {
                code: Self::INTERNAL_ERROR,
                message: "the connection is closed".to_owned(),
                data: None,
            },
            RpcError::Protocol(why) => ErrorObject {
                code: Self::INTERNAL_ERROR,
                message: why.clone(),
                data: None,
            },
        }
    }
}

impl From<ErrorObject> for RpcError {
    fn from(object: ErrorObject) -> Self {
        RpcError::Rpc {
            code: object.code,
            message: object.message,
            data: object.data,
        }
    }
}
