//! Starting a guest, and holding on to it.

use std::process::Stdio;
use std::sync::Arc;

use orrery_jsonrpc::framing::{StderrRing, pump_stderr};
use orrery_jsonrpc::{Framing, Handler, Peer};
use parking_lot::Mutex;
use tokio::process::{Child, Command};

use crate::contain::Containment;

/// How much of a chatty guest's stderr to keep.
///
/// The end, not the beginning: when a child dies, the reason is the last thing
/// it said.
pub const STDERR_RING_BYTES: usize = 16 * 1024;

/// A running child extension.
pub struct Guest {
    /// The connection to it.
    pub peer: Peer,
    /// The last few KB it complained about.
    pub stderr: Arc<StderrRing>,
    child: Mutex<Child>,
    /// Dropping this kills the child and everything it spawned.
    containment: Option<Containment>,
}

impl std::fmt::Debug for Guest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Guest")
            .field("pid", &self.containment.as_ref().map(Containment::pid))
            .field("connected", &self.peer.is_connected())
            .finish_non_exhaustive()
    }
}

impl Guest {
    /// Start a child and speak to it over its stdio.
    ///
    /// The child is contained **before** anything is sent to it: a guest that
    /// forks something on its first line and then fails must not leave that
    /// something behind.
    ///
    /// # Errors
    ///
    /// [`std::io::Error`] when the program cannot be started, or when the OS
    /// refuses to contain it — in which case the child is killed rather than
    /// kept, because an uncontainable child is worse than no child.
    pub fn start(
        mut command: Command,
        framing: Framing,
        handler: Arc<dyn Handler>,
    ) -> std::io::Result<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        Containment::configure(&mut command);

        let mut child = command.spawn()?;
        let containment = match Containment::capture(&child) {
            Ok(containment) => Some(containment),
            Err(e) => {
                let _ = child.start_kill();
                return Err(e);
            }
        };

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let ring = Arc::new(StderrRing::new(STDERR_RING_BYTES));
        tokio::spawn(pump_stderr(stderr, ring.clone()));

        Ok(Self {
            peer: Peer::spawn(stdout, stdin, framing, handler),
            stderr: ring,
            child: Mutex::new(child),
            containment,
        })
    }

    /// Whether the child is still running.
    ///
    /// `false` the moment it exits, whether or not anybody was waiting on it —
    /// which is what lets a crashed extension be reported as crashed rather
    /// than as a call that never answered.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        matches!(self.child.lock().try_wait(), Ok(None))
    }

    /// Its process id, while it has one.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.containment.as_ref().map(Containment::pid)
    }

    /// What it last complained about. The first thing to reach for when a
    /// guest dies without saying why over the protocol.
    #[must_use]
    pub fn stderr_tail(&self) -> String {
        self.stderr.tail()
    }

    /// Kill the child and its whole tree.
    pub fn kill(&mut self) {
        if let Some(containment) = self.containment.take() {
            containment.kill_tree();
        }
        let _ = self.child.lock().start_kill();
    }
}
