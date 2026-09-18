//! Where `seq` comes from, and the one place it comes from.

use std::sync::atomic::{AtomicU64, Ordering};

use orrery_agui::{AguiEvent, Frame};

/// Hands out a session's sequence numbers.
///
/// **Once, per session, at the differ's output — never per connection.** A
/// per-connection counter would make `since` mean a different thing to every
/// client, which would quietly break the one thing `seq` exists for: a client
/// that lost its connection asking for exactly what it missed.
///
/// It sits downstream of the encoder, so the unit it counts is an AG-UI frame
/// rather than a kernel event. One kernel event can expand to two frames
/// (`tool.settled` is `TOOL_CALL_END` and `TOOL_CALL_RESULT`), and each gets its
/// own number — otherwise contiguity would not be arithmetic.
#[derive(Debug, Default)]
pub struct SeqAuthority {
    next: AtomicU64,
}

impl SeqAuthority {
    /// A fresh authority. The first frame is `seq = 1`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// An authority resuming a session that already reached `last`.
    #[must_use]
    pub fn resuming(last: u64) -> Self {
        Self {
            next: AtomicU64::new(last + 1),
        }
    }

    /// Stamp one event.
    pub fn stamp(&self, event: AguiEvent) -> Frame {
        Frame::new(self.next.fetch_add(1, Ordering::SeqCst), event)
    }

    /// The number the next frame will get.
    #[must_use]
    pub fn peek(&self) -> u64 {
        self.next.load(Ordering::SeqCst)
    }

    /// The last number handed out, if any.
    #[must_use]
    pub fn last(&self) -> Option<u64> {
        self.peek().checked_sub(1).filter(|n| *n > 0)
    }
}
