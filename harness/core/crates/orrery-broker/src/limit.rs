//! Ceilings applied **while** the work happens, never after it.
//!
//! The difference is the whole point. "Read the file, then truncate to 4 KB"
//! has already put 100 MB in the heap. `LimitedReader` never pulls more than the
//! ceiling plus one buffer, so **peak** memory is bounded and not just the
//! result.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncRead, AsyncReadExt as _};

use crate::error::BrokerError;

/// How big a chunk is pulled at a time. The "plus one buffer" in the bound.
pub const CHUNK: usize = 8 * 1024;

/// Counts what a reader actually pulled, so a test can assert on the peak
/// rather than on the result.
#[derive(Debug, Default, Clone)]
pub struct PullCounter(Arc<AtomicU64>);

impl PullCounter {
    /// A fresh counter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many bytes have been pulled from the source so far.
    #[must_use]
    pub fn pulled(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    fn add(&self, n: u64) {
        self.0.fetch_add(n, Ordering::SeqCst);
    }
}

/// A reader that stops pulling once the ceiling is reached.
///
/// It reads at most `ceiling + CHUNK` bytes from the source, ever: the last
/// chunk is what proves there was more, and nothing beyond it is requested.
#[derive(Debug)]
pub struct LimitedReader<R> {
    inner: R,
    ceiling: u64,
    counter: PullCounter,
}

impl<R: AsyncRead + Unpin + Send> LimitedReader<R> {
    /// Wrap a source with a byte ceiling.
    pub fn new(inner: R, ceiling: u64) -> Self {
        Self {
            inner,
            ceiling,
            counter: PullCounter::new(),
        }
    }

    /// The counter, so a caller can watch the peak rather than the result.
    #[must_use]
    pub fn counter(&self) -> PullCounter {
        self.counter.clone()
    }

    /// How many bytes have been pulled so far.
    #[must_use]
    pub fn pulled(&self) -> u64 {
        self.counter.pulled()
    }

    /// Read up to the ceiling.
    ///
    /// # Errors
    ///
    /// [`BrokerError::LimitExceeded`] when the source has more than the ceiling
    /// allows, after at most one chunk beyond it has been pulled.
    pub async fn read_to_end(&mut self) -> Result<Vec<u8>, BrokerError> {
        let (bytes, truncated) = self.take_bytes().await?;
        if truncated {
            return Err(BrokerError::LimitExceeded {
                what: "output",
                ceiling: self.ceiling,
                unit: "bytes",
            });
        }
        Ok(bytes)
    }

    /// Read up to the ceiling and say whether there was more.
    ///
    /// For the callers whose contract is "as much as the budget allows". The
    /// bytes come back either way; `true` means the source had more and nothing
    /// beyond one chunk past the ceiling was ever requested from it.
    ///
    /// # Errors
    ///
    /// When the source itself fails.
    pub async fn take_bytes(&mut self) -> Result<(Vec<u8>, bool), BrokerError> {
        let cap = usize::try_from(self.ceiling).unwrap_or(usize::MAX);
        let mut out: Vec<u8> = Vec::new();
        let mut chunk = vec![0u8; CHUNK];
        loop {
            let n = self
                .inner
                .read(&mut chunk)
                .await
                .map_err(|e| BrokerError::io("<reader>", e))?;
            if n == 0 {
                return Ok((out, false));
            }
            self.counter.add(n as u64);
            out.extend_from_slice(&chunk[..n]);
            if out.len() > cap {
                // Stop here. Nothing further is requested from the source, so
                // the peak is bounded by the ceiling plus one chunk.
                out.truncate(cap);
                return Ok((out, true));
            }
        }
    }
}
