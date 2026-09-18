//! What can go wrong on the way out.

/// A replay that cannot be served from the ring.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    /// The client is asking for frames the ring has already dropped.
    ///
    /// Not a hole and not a silent truncation: the caller has to go to the
    /// session store, which holds every settled turn in full. The ring exists
    /// to spare that lookup for the common case, not to replace it.
    #[error("frames before seq {earliest} are no longer buffered; replay from the session store")]
    TooOld {
        /// The earliest `seq` the ring still holds.
        earliest: u64,
    },
}

/// What can go wrong on a listener.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The connection went away.
    #[error("the connection is closed")]
    Closed,

    /// The client fell so far behind that its buffer was abandoned.
    ///
    /// Rendering never applies backpressure to the agent loop, so the only
    /// thing left to give up is the client's own view. It re-attaches with
    /// `since` and gets the gap.
    #[error("the client fell behind and must re-attach from seq {last_delivered}")]
    Lagged {
        /// The last `seq` this client actually received.
        last_delivered: u64,
    },

    /// A frame would not encode or decode.
    #[error("frame codec: {0}")]
    Codec(String),

    /// The negotiated wire format is not one this build speaks.
    #[error("unknown wire format `{0}`: expected `json` or `cbor`")]
    UnknownFormat(String),

    /// A frame claimed a length this side refuses to allocate.
    #[error("frame of {len} bytes exceeds the {max}-byte limit")]
    FrameTooLarge {
        /// What the header claimed.
        len: u64,
        /// What is allowed.
        max: u64,
    },

    /// The replay could not be served.
    #[error(transparent)]
    Replay(#[from] ReplayError),

    /// Anything the operating system said.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
