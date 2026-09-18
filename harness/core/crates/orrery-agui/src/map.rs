//! The names and pointers the mapping table stands on.
//!
//! Kept apart from [`crate::encode`] so that the *vocabulary* — what a custom
//! event is called, what a surface's JSON Pointer looks like — is one short
//! file that a TypeScript reader can check against their own without reading
//! the encoder's state machine.

use orrery_proto::SurfaceId;

/// The `Custom` name a consent prompt rides under.
pub const CONSENT_REQUEST: &str = "orrery.consent.request";

/// The `Custom` name a consent resolution rides under.
///
/// Emitted on replay for a prompt whose deadline lapsed while nobody was
/// attached: the client is told the answer, not asked the question.
pub const CONSENT_RESOLVED: &str = "orrery.consent.resolved";

/// The `Custom` name a turn's cancellation rides under.
pub const TURN_CANCELLED: &str = "orrery.turn.cancelled";

/// The root the shared state's surfaces hang off.
pub const SURFACES_ROOT: &str = "/surfaces";

/// Escape one JSON Pointer reference token (RFC 6901 §3).
#[must_use]
pub fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// The pointer to a whole surface.
#[must_use]
pub fn surface_pointer(id: SurfaceId) -> String {
    format!("{SURFACES_ROOT}/{}", escape(&id.to_string()))
}

/// The pointer to a field inside a surface.
#[must_use]
pub fn field_pointer(id: SurfaceId, path: &[String]) -> String {
    let mut out = surface_pointer(id);
    for seg in path {
        out.push('/');
        out.push_str(&escape(seg));
    }
    out
}
