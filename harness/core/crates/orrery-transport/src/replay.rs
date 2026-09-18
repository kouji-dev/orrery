//! The buffer a re-attaching client is served from.
//!
//! # Coalescing and replay do not conflict
//!
//! They read from different places. A slow client loses intermediate
//! **frames**; a re-attaching client is served from here, and from the session
//! store behind it, which has every settled turn in full. The kernel buffers
//! frames only for as long as a connected client might still be behind.

use std::collections::VecDeque;

use orrery_agui::Frame;

use crate::error::ReplayError;

/// A bounded ring of recent frames, in `seq` order.
#[derive(Debug)]
pub struct ReplayRing {
    cap: usize,
    frames: VecDeque<Frame>,
}

impl ReplayRing {
    /// A ring holding at most `cap` frames.
    ///
    /// # Panics
    ///
    /// If `cap` is zero — a ring that holds nothing would turn every
    /// re-attachment into a session-store read, which is exactly what it is
    /// here to avoid.
    #[must_use]
    pub fn new(cap: usize) -> Self {
        assert!(cap > 0, "a replay ring of zero frames buffers nothing");
        Self {
            cap,
            frames: VecDeque::with_capacity(cap.min(1024)),
        }
    }

    /// Add a frame, dropping the oldest when full.
    pub fn push(&mut self, frame: Frame) {
        if self.frames.len() == self.cap {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    /// The earliest `seq` still buffered.
    #[must_use]
    pub fn earliest(&self) -> Option<u64> {
        self.frames.front().map(|f| f.seq)
    }

    /// The latest `seq` buffered.
    #[must_use]
    pub fn latest(&self) -> Option<u64> {
        self.frames.back().map(|f| f.seq)
    }

    /// How many frames are buffered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether nothing is buffered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Everything after `since`, contiguously.
    ///
    /// `since` is **exclusive**: `since = 5` returns 6 onwards. `None` means
    /// from the start of what is buffered.
    ///
    /// # Errors
    ///
    /// [`ReplayError::TooOld`] when the ring has already dropped what is being
    /// asked for. The caller falls back to `SessionStore::events_since`, which
    /// is the authority; this is only the fast path.
    pub fn since(&self, since: Option<u64>) -> Result<Vec<Frame>, ReplayError> {
        let Some(earliest) = self.earliest() else {
            return Ok(Vec::new());
        };
        match since {
            None if earliest > 1 => Err(ReplayError::TooOld { earliest }),
            None => Ok(self.frames.iter().cloned().collect()),
            Some(since) => {
                // The client has seq `since`; it needs `since + 1` onwards. The
                // ring can serve that only if it still holds `since + 1`.
                if earliest > since + 1 {
                    return Err(ReplayError::TooOld { earliest });
                }
                Ok(self
                    .frames
                    .iter()
                    .filter(|f| f.seq > since)
                    .cloned()
                    .collect())
            }
        }
    }
}
