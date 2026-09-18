//! Cancelling one call.
//!
//! The notification is LSP's `$/cancelRequest` in spirit, spelled `$/cancel`
//! and carrying `{ "id": <request id> }`. It names **one request**, which is
//! the whole point: `turn.cancel` must reach one in-flight tool call and leave
//! the connection, and every other call on it, alone.
//!
//! It is a notification rather than a request because there is nothing useful
//! to answer. The far side may already have replied — the cancel and the reply
//! race, and both orderings are fine: the caller has already stopped waiting,
//! and a late answer to a forgotten id is dropped with a debug line.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::message::Id;

/// The method name a cancellation travels under.
pub const METHOD: &str = "$/cancel";

/// What a cancellation carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Params {
    /// The request to stop.
    pub id: Id,
}

/// The params for cancelling one request.
#[must_use]
pub fn params(id: &Id) -> Value {
    serde_json::to_value(Params { id: id.clone() }).expect("a request id always serialises")
}

/// The request a cancellation names, if this is one.
#[must_use]
pub fn requested(method: &str, params: &Value) -> Option<Id> {
    if method != METHOD {
        return None;
    }
    serde_json::from_value::<Params>(params.clone())
        .ok()
        .map(|p| p.id)
}

#[cfg(test)]
mod tests {
    use super::{METHOD, params, requested};
    use crate::message::Id;

    #[test]
    fn a_cancellation_names_one_request() {
        let id = Id::Num(7);
        let payload = params(&id);
        assert_eq!(payload["id"], 7);
        assert_eq!(requested(METHOD, &payload), Some(id));
    }

    #[test]
    fn anything_else_is_not_a_cancellation() {
        assert_eq!(requested("tool/call", &params(&Id::Num(1))), None);
        assert_eq!(requested(METHOD, &serde_json::json!({})), None);
    }
}
