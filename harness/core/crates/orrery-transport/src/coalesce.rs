//! One coalescer per connected client.
//!
//! Two clients at different speeds must not share one: a terminal repainting at
//! 30 Hz and a browser tab that has been backgrounded for a minute want
//! different frame rates, and the moment they share a buffer the fast one is
//! paying for the slow one. So the coalescer lives on the connection, and the
//! only thing the session owns is the frames themselves.
//!
//! **Rendering never applies backpressure to the agent loop.** Merging here is
//! how a slow client degrades its own view instead of the run.

use std::time::Duration;

use orrery_agui::{AguiEvent, Frame, PatchOp};

/// Merges frames within one tick.
///
/// A tick of [`Duration::ZERO`] disables merging entirely, which is what an
/// in-process client that draws every frame asks for.
#[derive(Debug)]
pub struct Coalescer {
    tick: Duration,
    pending: Vec<Frame>,
}

impl Coalescer {
    /// A coalescer that merges over `tick`. About 33 ms is a screen refresh.
    #[must_use]
    pub fn new(tick: Duration) -> Self {
        Self {
            tick,
            pending: Vec::new(),
        }
    }

    /// How long this client is willing to wait for more.
    #[must_use]
    pub fn tick(&self) -> Duration {
        self.tick
    }

    /// Whether anything is waiting to be drained.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Buffer one frame.
    pub fn push(&mut self, frame: Frame) {
        self.pending.push(frame);
    }

    /// Merge and hand over everything buffered.
    pub fn drain(&mut self) -> Vec<Frame> {
        let taken = std::mem::take(&mut self.pending);
        if self.tick.is_zero() {
            return taken;
        }
        merge(taken)
    }
}

/// Merge a run of frames.
///
/// Two rules, both restricted to **consecutive** frames so that nothing is ever
/// reordered past something it might depend on:
///
/// - consecutive `TEXT_MESSAGE_CONTENT` on one message become one;
/// - consecutive `STATE_DELTA`s become one, and inside it a `replace` or
///   `remove` drops every earlier op on the same surface or below it, because
///   the later op already contains them.
#[must_use]
pub fn merge(frames: Vec<Frame>) -> Vec<Frame> {
    let mut out: Vec<Frame> = Vec::with_capacity(frames.len());
    for frame in frames {
        match (&frame.event, out.last_mut()) {
            (
                AguiEvent::TextMessageContent { message_id, delta },
                Some(Frame {
                    event:
                        AguiEvent::TextMessageContent {
                            message_id: prev_id,
                            delta: prev_delta,
                        },
                    seq: prev_seq,
                    merged_from,
                }),
            ) if prev_id == message_id => {
                prev_delta.push_str(delta);
                *merged_from = Some(merged_from.unwrap_or(*prev_seq));
                *prev_seq = frame.seq;
            }
            (
                AguiEvent::StateDelta { delta },
                Some(Frame {
                    event: AguiEvent::StateDelta { delta: prev_delta },
                    seq: prev_seq,
                    merged_from,
                }),
            ) => {
                for op in delta {
                    absorb(prev_delta, op.clone());
                }
                *merged_from = Some(merged_from.unwrap_or(*prev_seq));
                *prev_seq = frame.seq;
            }
            _ => out.push(frame),
        }
    }
    out
}

/// Add one op to a pending list, dropping what it makes redundant.
fn absorb(ops: &mut Vec<PatchOp>, op: PatchOp) {
    match &op {
        // An append onto a path we are already appending to is string
        // concatenation, which is the hot path this whole module exists for.
        PatchOp::Append { path, value } => {
            if let Some(PatchOp::Append {
                path: prev,
                value: prev_value,
            }) = ops.last_mut()
                && prev == path
            {
                prev_value.push_str(value);
                return;
            }
            ops.push(op);
        }
        // A whole-surface write contains every earlier write to that surface or
        // to a field inside it. `Set` then `Replace` collapses to the `Replace`.
        PatchOp::Replace { path, .. } | PatchOp::Remove { path } => {
            ops.retain(|prev| !covers(path, prev.path()));
            ops.push(op);
        }
        PatchOp::Add { .. } => ops.push(op),
    }
}

/// Whether a write at `outer` subsumes a write at `inner`.
fn covers(outer: &str, inner: &str) -> bool {
    inner == outer || (inner.starts_with(outer) && inner.as_bytes().get(outer.len()) == Some(&b'/'))
}
