//! Redaction, in the schema rather than in the deployment.
//!
//! Nothing here is a filter applied on the way out. The event types in
//! [`crate::event`] hold a [`Digest`] or a [`ContentRef`] where a raw value
//! would otherwise sit, so a caller that wanted to log a secret would have to
//! construct a field that does not exist. A deployment cannot turn this off,
//! and a new event variant cannot forget it.
//!
//! Credential *values* have nowhere to appear at all: the broker resolves a
//! credential by name at the point of use and never hands the value back, so
//! there is no string to redact.

use serde::{Deserialize, Serialize};

/// A blake3 digest of something we deliberately do not keep.
///
/// Rendered as `blake3:<hex>`. Stable across processes and platforms: JSON is
/// canonicalised (object keys sorted, no insignificant whitespace) before it is
/// hashed, so two callers that built the same input in a different order agree.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Digest(String);

impl Digest {
    /// Hash raw bytes.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(format!("blake3:{}", blake3::hash(bytes).to_hex()))
    }

    /// Hash a JSON value by its canonical form.
    #[must_use]
    pub fn of_json(value: &serde_json::Value) -> Self {
        let mut buf = String::new();
        canonicalise(value, &mut buf);
        Self::of_bytes(buf.as_bytes())
    }

    /// Hash any serialisable value. Falls back to the digest of `null` for the
    /// types that cannot be serialised at all, because an audit record is never
    /// worth a panic.
    #[must_use]
    pub fn of<T: Serialize>(value: &T) -> Self {
        let json = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
        Self::of_json(&json)
    }

    /// The `blake3:<hex>` form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Canonical JSON: object keys sorted, arrays in order, no whitespace.
fn canonicalise(value: &serde_json::Value, out: &mut String) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::Value::String(k.clone()).to_string());
                out.push(':');
                canonicalise(&map[k], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonicalise(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Content recorded by reference: enough to find it in the session store, and
/// not enough to reconstruct it.
///
/// Memory entries, surface payloads and prompt bodies use this.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentRef {
    /// The id it is stored under.
    pub id: String,
    /// Which store, or which memory scope.
    pub scope: String,
    /// How long the content was, in bytes.
    pub len: usize,
    /// Its digest, so a later read can be checked against what was recorded.
    pub digest: Digest,
}

impl ContentRef {
    /// Describe a body without keeping it.
    #[must_use]
    pub fn new(id: impl Into<String>, scope: impl Into<String>, body: &str) -> Self {
        Self {
            id: id.into(),
            scope: scope.into(),
            len: body.len(),
            digest: Digest::of_bytes(body.as_bytes()),
        }
    }
}
