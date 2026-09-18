//! What the loop selects over, and what it sends back.
//!
//! The frames arrive from *somewhere*: an [`AguiSession`](orrery_client::AguiSession)
//! in the binary, a vector in a test. The loop cannot tell the difference, which
//! is the point — this client attaches the same way an external one does, so the
//! in-memory case is a transport and not a shortcut.

use async_trait::async_trait;
use orrery_agui::Frame;
use orrery_client::ClientError;

/// Somewhere frames come from.
#[async_trait]
pub trait FrameSource: Send {
    /// The next frame, or `None` when the session ends.
    async fn next_frame(&mut self) -> Option<Result<Frame, ClientError>>;
}

#[async_trait]
impl FrameSource for orrery_client::AguiSession {
    async fn next_frame(&mut self) -> Option<Result<Frame, ClientError>> {
        orrery_client::AguiSession::next_frame(self).await
    }
}

/// Frames already in hand: a fixture, a replay.
#[derive(Clone, Debug, Default)]
pub struct VecSource {
    frames: std::collections::VecDeque<Frame>,
}

impl VecSource {
    /// A source that yields these frames, then ends.
    #[must_use]
    pub fn new(frames: Vec<Frame>) -> Self {
        Self {
            frames: frames.into(),
        }
    }

    /// How many are left.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.frames.len()
    }
}

#[async_trait]
impl FrameSource for VecSource {
    async fn next_frame(&mut self) -> Option<Result<Frame, ClientError>> {
        self.frames.pop_front().map(Ok)
    }
}

/// What the client wants the kernel to do.
///
/// The renderer never calls the kernel itself: it produces these and the loop
/// hands them to the session, which owns `seq` and the pending queue. A dropped
/// connection then resumes the turn rather than losing it.
#[derive(Clone, Debug, PartialEq)]
pub enum Outgoing {
    /// Start a turn with this text.
    Submit(String),
    /// Stop the turn in flight. `^C`, which is not an exit.
    Cancel,
    /// Answer a consent prompt.
    Consent {
        /// Which prompt.
        prompt: String,
        /// What was decided.
        answer: orrery_proto::ConsentAnswerKind,
    },
    /// What a surface produced: a question's choice, a form's values.
    Intent {
        /// Which surface.
        surface: String,
        /// What it produced.
        value: serde_json::Value,
    },
    /// A frame was lost; re-attach from here.
    Reattach {
        /// The last `seq` this client is sure of.
        since: u64,
    },
    /// `^D`. The loop stops.
    Exit,
}
