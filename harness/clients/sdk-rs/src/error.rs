//! What a client is told when something goes wrong.

/// Anything that can go wrong on the client's side of the wire.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The endpoint string is not one of the three forms.
    #[error(
        "`{0}` is not an endpoint: expected `inproc:`, `pipe:<name>` or `http://[token@]host:port`"
    )]
    BadEndpoint(String),

    /// The connection is gone.
    #[error("the connection is closed")]
    Closed,

    /// The kernel refused, or could not answer.
    #[error("the kernel said no: {0}")]
    Refused(String),

    /// A frame did not decode.
    #[error("frame: {0}")]
    Codec(String),

    /// A gap was detected and the client has not re-attached yet.
    ///
    /// Carried rather than panicked on: a gap is a cue to re-attach with
    /// `since`, and a renderer that treats it as fatal loses a turn it could
    /// have recovered.
    #[error("frames {expected}..{got} were lost; re-attach with since = {}", expected - 1)]
    Gap {
        /// The `seq` that should have come next.
        expected: u64,
        /// The `seq` that did.
        got: u64,
    },

    /// The transport failed.
    #[error("transport: {0}")]
    Transport(#[from] orrery_transport::TransportError),

    /// HTTP failed.
    #[error("http: {0}")]
    Http(String),

    /// Anything the operating system said.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
