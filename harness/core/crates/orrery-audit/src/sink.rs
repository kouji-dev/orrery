//! Where the stream goes. Append, and nothing else.
//!
//! Every sink here offers exactly one mutating operation — [`AuditSink::append`]
//! — taking `&self`. There is no `remove`, no `truncate`, no `records_mut`, and
//! no handle that would let a caller reach the backing store. That is the whole
//! design: an audit a caller can edit is not an audit.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::error::AuditError;
use crate::event::{AuditEvent, AuditRecord};

/// Somewhere an [`AuditEvent`] can be appended.
///
/// Append-only by construction: this is the only method, it takes `&self`, and
/// no implementor in this crate exposes its storage mutably.
///
/// ```compile_fail
/// use orrery_audit::MemorySink;
/// let sink = MemorySink::new();
/// // There is no way to take an earlier record back out and change it.
/// sink.records_mut().clear();
/// ```
pub trait AuditSink: Send + Sync + std::fmt::Debug {
    /// Append one event. Failures are swallowed on purpose: a session must not
    /// die because a disk filled, and [`FileSink::errors`] counts what was lost.
    fn append(&self, event: AuditEvent);
}

/// A sink that keeps everything in memory. For tests, and for `explain`.
#[derive(Debug, Default)]
pub struct MemorySink {
    records: Mutex<Vec<AuditRecord>>,
    next: AtomicU64,
}

impl MemorySink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything appended so far, in order. A copy: the sink keeps the original.
    #[must_use]
    pub fn records(&self) -> Vec<AuditRecord> {
        self.records.lock().clone()
    }

    /// How many events are in the stream.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.lock().len()
    }

    /// Whether nothing has been appended.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The stream as JSONL, exactly as a [`FileSink`] would have written it.
    #[must_use]
    pub fn to_jsonl(&self) -> String {
        let mut out = String::new();
        for r in self.records.lock().iter() {
            if let Ok(line) = serde_json::to_string(r) {
                out.push_str(&line);
                out.push('\n');
            }
        }
        out
    }
}

impl AuditSink for MemorySink {
    fn append(&self, event: AuditEvent) {
        let seq = self.next.fetch_add(1, Ordering::SeqCst);
        self.records.lock().push(AuditRecord::new(seq, event));
    }
}

/// A sink that drops everything. The default when nothing is configured.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl AuditSink for NullSink {
    fn append(&self, _event: AuditEvent) {}
}

/// How big a file may get and how many old ones to keep.
///
/// Open question 3 of the plan, decided: **size-based rotation with a retention
/// count**. A long session must not fill a disk, and a time-based scheme needs a
/// policy about idle days that nobody wants to reason about.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Rotation {
    /// Roll over once the live file passes this many bytes. Zero disables it.
    pub max_bytes: u64,
    /// How many rolled files to keep beside it.
    pub keep: usize,
}

impl Default for Rotation {
    fn default() -> Self {
        Self {
            max_bytes: 16 * 1024 * 1024,
            keep: 4,
        }
    }
}

/// A JSONL file on disk, one record per line, opened for append.
#[derive(Debug)]
pub struct FileSink {
    path: PathBuf,
    rotation: Rotation,
    state: Mutex<FileState>,
    next: AtomicU64,
    errors: AtomicU64,
}

#[derive(Debug)]
struct FileState {
    /// `None` between dropping the old handle and opening the new one: Windows
    /// will not rename a file that is still open.
    file: Option<std::fs::File>,
    bytes: u64,
}

impl FileSink {
    /// Open (or create) the file with the default rotation.
    ///
    /// # Errors
    ///
    /// When the file cannot be created or opened for append.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AuditError> {
        Self::with_rotation(path, Rotation::default())
    }

    /// Open (or create) the file with an explicit rotation.
    ///
    /// # Errors
    ///
    /// When the file cannot be created or opened for append.
    pub fn with_rotation(path: impl AsRef<Path>, rotation: Rotation) -> Result<Self, AuditError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| AuditError::sink(&path, e))?;
            }
        }
        let file = Self::open_append(&path).map_err(|e| AuditError::sink(&path, e))?;
        let bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path,
            rotation,
            state: Mutex::new(FileState {
                file: Some(file),
                bytes,
            }),
            next: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        })
    }

    /// The file being written.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many records could not be written. Never panics, never blocks a turn.
    #[must_use]
    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    fn open_append(path: &Path) -> std::io::Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    }

    /// Close the live file, shift the rolled ones along, drop what falls past
    /// the retention count, and open a fresh live file.
    fn rotate(&self, state: &mut FileState) -> std::io::Result<()> {
        // Windows will not rename an open file, so let go of the handle first.
        state.file = None;

        for n in (1..=self.rotation.keep).rev() {
            let from = if n == 1 {
                self.path.clone()
            } else {
                self.rolled(n - 1)
            };
            let to = self.rolled(n);
            if from.exists() {
                let _ = std::fs::rename(&from, &to);
            }
        }
        let _ = std::fs::remove_file(self.rolled(self.rotation.keep + 1));

        state.file = Some(Self::open_append(&self.path)?);
        state.bytes = 0;
        Ok(())
    }

    fn rolled(&self, n: usize) -> PathBuf {
        let mut name = self.path.as_os_str().to_os_string();
        name.push(format!(".{n}"));
        PathBuf::from(name)
    }
}

impl FileSink {
    /// Append one already-rendered JSON line. The `tracing` layers use this;
    /// typed events go through [`AuditSink::append`].
    pub(crate) fn append_line(&self, rendered: &str) {
        let mut line = String::with_capacity(rendered.len() + 1);
        line.push_str(rendered);
        line.push('\n');

        let mut state = self.state.lock();
        if self.rotation.max_bytes > 0
            && state.bytes >= self.rotation.max_bytes
            && self.rotate(&mut state).is_err()
        {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
        let written = match state.file.as_mut() {
            Some(file) => file.write_all(line.as_bytes()).and_then(|()| file.flush()),
            None => Err(std::io::Error::other("audit sink is not open")),
        };
        match written {
            Ok(()) => state.bytes += line.len() as u64,
            Err(_) => {
                self.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl AuditSink for FileSink {
    fn append(&self, event: AuditEvent) {
        let seq = self.next.fetch_add(1, Ordering::SeqCst);
        match serde_json::to_string(&AuditRecord::new(seq, event)) {
            Ok(line) => self.append_line(&line),
            Err(_) => {
                self.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
