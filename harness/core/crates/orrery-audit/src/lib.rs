//! The append-only structured stream: three tracing layers, redaction in schema, file and OTLP sinks.
//!
//! One stream answers one question: **which rule allowed this**. Every
//! capability decision is recorded with the rule that produced it, every tool
//! call with a digest of its input, every extension load with what it did and
//! did not contribute.
//!
//! # Redaction is in the schema, not the deployment
//!
//! [`AuditEvent`] holds a [`Digest`] or a [`ContentRef`] wherever a raw value
//! would otherwise sit. A deployment cannot switch that off, and a new variant
//! cannot forget it, because there is no field to put a secret in. Credential
//! values have nowhere to appear at all: the broker resolves a credential by
//! name at the point of use and never returns it, so there is no string to
//! redact.
//!
//! # Append-only
//!
//! [`AuditSink`] has exactly one method. Nothing here removes, rewrites or
//! truncates a record.
//!
//! Implementation plan: `harness/docs/plans/07-policy-broker-audit.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod error;
pub mod event;
pub mod layer;
pub mod redact;
pub mod sink;

pub use error::AuditError;
pub use event::{AuditEvent, AuditRecord, CallOutcome, Verdict};
pub use layer::{LineSink, MemoryLines, Stream, StreamLayer};
pub use redact::{ContentRef, Digest};
pub use sink::{AuditSink, FileSink, MemorySink, NullSink, Rotation};

use std::sync::Arc;

/// The handle every other crate holds: a shared, append-only sink.
pub type Audit = Arc<dyn AuditSink>;

/// An audit that records nothing, for a caller that has not been given one.
///
/// Deliberately a real sink rather than an `Option<Audit>`: a call site that can
/// skip the audit is a call site that will.
#[must_use]
pub fn null() -> Audit {
    Arc::new(NullSink)
}

/// An in-memory audit, for tests and for `permissions explain`.
#[must_use]
pub fn memory() -> Arc<MemorySink> {
    Arc::new(MemorySink::new())
}
