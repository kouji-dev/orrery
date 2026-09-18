//! JSON-RPC 2.0 on the wire, and what it means once it is off it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A request id. A number here, but a peer may send a string and gets a string
/// back, because JSON-RPC allows both and a peer that does not echo the id it
/// was given is a peer nobody can correlate.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    /// The usual.
    Num(u64),
    /// Allowed, and some servers do it.
    Text(String),
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Id::Num(n) => write!(f, "{n}"),
            Id::Text(s) => f.write_str(s),
        }
    }
}

/// An error as JSON-RPC carries it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErrorObject {
    /// The code.
    pub code: i64,
    /// What went wrong, in words.
    pub message: String,
    /// Anything else the far side wanted to say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// One JSON-RPC frame, exactly as it goes on the wire.
///
/// Deliberately **one struct with options** rather than an enum with
/// `#[serde(untagged)]`: untagged deserialisation reports "data did not match
/// any variant", which is useless when a guest is emitting a slightly wrong
/// frame at three in the morning. [`Envelope::classify`] does the same job and
/// can say which field was missing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// Always `"2.0"`.
    #[serde(default = "version")]
    pub jsonrpc: String,
    /// Present on a request and on its response; absent on a notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// Present on a request and a notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// The arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// The answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// The refusal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

fn version() -> String {
    "2.0".to_owned()
}

impl Envelope {
    /// A request.
    #[must_use]
    pub fn request(id: Id, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: version(),
            id: Some(id),
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    /// A notification: no id, and therefore no answer.
    #[must_use]
    pub fn notification(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: version(),
            id: None,
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    /// A successful answer.
    #[must_use]
    pub fn result(id: Id, result: Value) -> Self {
        Self {
            jsonrpc: version(),
            id: Some(id),
            method: None,
            params: None,
            result: Some(result),
            error: None,
        }
    }

    /// A failed answer.
    #[must_use]
    pub fn failure(id: Id, error: ErrorObject) -> Self {
        Self {
            jsonrpc: version(),
            id: Some(id),
            method: None,
            params: None,
            result: None,
            error: Some(error),
        }
    }

    /// What this frame actually is.
    #[must_use]
    pub fn classify(self) -> Incoming {
        match (self.id, self.method, self.error) {
            (Some(id), Some(method), _) => Incoming::Request {
                id,
                method,
                params: self.params.unwrap_or(Value::Null),
            },
            (None, Some(method), _) => Incoming::Notification {
                method,
                params: self.params.unwrap_or(Value::Null),
            },
            (Some(id), None, Some(error)) => Incoming::Failure { id, error },
            (Some(id), None, None) => Incoming::Result {
                id,
                result: self.result.unwrap_or(Value::Null),
            },
            (None, None, error) => Incoming::Junk {
                why: match error {
                    Some(e) => format!("an error with no id: {}", e.message),
                    None => "no id and no method".to_owned(),
                },
            },
        }
    }
}

/// One frame, understood.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    /// Answer me.
    Request {
        /// Which request.
        id: Id,
        /// What is being asked for.
        method: String,
        /// With what.
        params: Value,
    },
    /// Do not answer me.
    Notification {
        /// What happened.
        method: String,
        /// With what.
        params: Value,
    },
    /// The answer to something we asked.
    Result {
        /// Which request.
        id: Id,
        /// The answer.
        result: Value,
    },
    /// The refusal of something we asked.
    Failure {
        /// Which request.
        id: Id,
        /// Why not.
        error: ErrorObject,
    },
    /// Something that is not a JSON-RPC frame at all.
    ///
    /// A variant rather than an error, because one bad frame from a guest must
    /// not tear down a connection that is carrying three good calls.
    Junk {
        /// What is wrong with it.
        why: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{Envelope, ErrorObject, Id, Incoming};
    use serde_json::json;

    #[test]
    fn the_four_shapes_classify() {
        let request = Envelope::request(Id::Num(1), "m", json!({}));
        assert!(matches!(request.classify(), Incoming::Request { .. }));

        let notification = Envelope::notification("m", json!({}));
        assert!(matches!(
            notification.classify(),
            Incoming::Notification { .. }
        ));

        let result = Envelope::result(Id::Num(1), json!(2));
        assert!(matches!(result.classify(), Incoming::Result { .. }));

        let failure = Envelope::failure(
            Id::Num(1),
            ErrorObject {
                code: -1,
                message: "no".into(),
                data: None,
            },
        );
        assert!(matches!(failure.classify(), Incoming::Failure { .. }));
    }

    #[test]
    fn a_null_result_is_a_result_and_not_junk() {
        // `{"jsonrpc":"2.0","id":1,"result":null}` is a perfectly good answer,
        // and serde would give us `result: None` for it either way.
        let raw = r#"{"jsonrpc":"2.0","id":1,"result":null}"#;
        let envelope: Envelope = serde_json::from_str(raw).unwrap();
        assert!(matches!(
            envelope.classify(),
            Incoming::Result { result, .. } if result.is_null()
        ));
    }

    #[test]
    fn junk_is_a_value_not_a_torn_down_connection() {
        let raw = r#"{"jsonrpc":"2.0","params":{}}"#;
        let envelope: Envelope = serde_json::from_str(raw).unwrap();
        assert!(matches!(envelope.classify(), Incoming::Junk { .. }));
    }

    #[test]
    fn a_string_id_comes_back_as_a_string() {
        let raw = r#"{"jsonrpc":"2.0","id":"abc","result":1}"#;
        let envelope: Envelope = serde_json::from_str(raw).unwrap();
        assert_eq!(envelope.id, Some(Id::Text("abc".into())));
    }
}
