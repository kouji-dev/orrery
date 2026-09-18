//! The in-process listener: the kernel is a server even with nobody on a socket.
//!
//! No serialisation, and no *option* of serialisation — [`Channel`] is generic
//! over a payload with no `Serialize` bound anywhere in sight, and the
//! in-process client is instantiated at [`orrery_proto::Event`] directly. A
//! round trip that never names serde cannot quietly start using it.
//!
//! That is the whole point of the in-process transport being a transport at
//! all: a CLI that skips the wire still goes through the same request and event
//! types as a browser tab, so there is no second code path to drift.

use tokio::sync::mpsc;

use crate::error::TransportError;

/// An in-process connection, carrying `T` as itself.
#[derive(Debug)]
pub struct Channel<T> {
    tx: mpsc::UnboundedSender<T>,
    rx: mpsc::UnboundedReceiver<T>,
}

/// Both ends of one in-process connection.
#[must_use]
pub fn pair<T>() -> (Channel<T>, Channel<T>) {
    let (a_tx, a_rx) = mpsc::unbounded_channel();
    let (b_tx, b_rx) = mpsc::unbounded_channel();
    (
        Channel { tx: a_tx, rx: b_rx },
        Channel { tx: b_tx, rx: a_rx },
    )
}

impl<T> Channel<T> {
    /// Send a value. Never blocks: rendering does not push back on the loop.
    ///
    /// # Errors
    ///
    /// [`TransportError::Closed`] when the other end is gone.
    pub fn send(&self, value: T) -> Result<(), TransportError> {
        self.tx.send(value).map_err(|_| TransportError::Closed)
    }

    /// Receive the next value, or `None` when the other end is gone.
    pub async fn recv(&mut self) -> Option<T> {
        self.rx.recv().await
    }

    /// Receive without waiting.
    pub fn try_recv(&mut self) -> Option<T> {
        self.rx.try_recv().ok()
    }

    /// Whether nothing is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rx.is_empty()
    }
}
