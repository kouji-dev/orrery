//! Three streams, three `tracing` layers (section 4.12).
//!
//! The load ledger, the audit and the telemetry stream are separate files with
//! separate retention, because they answer separate questions and one of them is
//! evidence. They are told apart by target prefix — `orrery.load`,
//! `orrery.audit`, `orrery.telemetry` — so a crate emits into a stream by
//! naming it, with no registry of event kinds to keep in step.
//!
//! Typed events go through [`crate::AuditSink`] directly. This module is for the
//! `tracing` events that other crates already emit: it captures them as JSON
//! lines in the same file, so one stream holds both.

use std::fmt::Write as _;
use std::sync::Arc;

use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// One of the three streams.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stream {
    /// What loaded, what degraded and what was skipped.
    Load,
    /// Decisions, calls and consent. The evidence stream.
    Audit,
    /// Counts and timings. Lossy on purpose.
    Telemetry,
}

impl Stream {
    /// The `tracing` target prefix that routes into this stream.
    #[must_use]
    pub const fn target(self) -> &'static str {
        match self {
            Stream::Load => "orrery.load",
            Stream::Audit => "orrery.audit",
            Stream::Telemetry => "orrery.telemetry",
        }
    }

    /// The stream a `tracing` target belongs to, if any.
    ///
    /// A target is in a stream when it *is* the prefix or begins with the prefix
    /// and a dot, so `orrery.audit.policy` is audit and `orrery.auditor` is not.
    #[must_use]
    pub fn of_target(target: &str) -> Option<Self> {
        [Stream::Load, Stream::Audit, Stream::Telemetry]
            .into_iter()
            .find(|s| {
                let p = s.target();
                target == p || target.strip_prefix(p).is_some_and(|r| r.starts_with('.'))
            })
    }

    /// The default file name for this stream.
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            Stream::Load => "load.jsonl",
            Stream::Audit => "audit.jsonl",
            Stream::Telemetry => "telemetry.jsonl",
        }
    }
}

/// A `tracing` layer that writes one stream, as JSON lines, to a sink.
///
/// Events outside its stream are ignored, so the three layers can be installed
/// side by side on one subscriber.
#[derive(Debug)]
pub struct StreamLayer {
    stream: Stream,
    lines: Arc<dyn LineSink>,
}

impl StreamLayer {
    /// A layer for one stream over a line sink.
    #[must_use]
    pub fn new(stream: Stream, lines: Arc<dyn LineSink>) -> Self {
        Self { stream, lines }
    }

    /// Which stream this layer serves.
    #[must_use]
    pub const fn stream(&self) -> Stream {
        self.stream
    }
}

/// Somewhere a rendered line can go. Append-only, like everything else here.
pub trait LineSink: Send + Sync + std::fmt::Debug {
    /// Append one already-rendered JSON line, without its newline.
    fn line(&self, line: &str);
}

impl<S> Layer<S> for StreamLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if Stream::of_target(event.metadata().target()) != Some(self.stream) {
            return;
        }
        let mut visitor = JsonFields::default();
        event.record(&mut visitor);
        let mut line = String::new();
        let _ = write!(
            line,
            "{{\"at_ms\":{},\"stream\":\"{}\",\"target\":{},\"level\":\"{}\",\"fields\":{{{}}}}}",
            crate::event::now_ms(),
            self.stream.target(),
            serde_json::Value::String(event.metadata().target().to_owned()),
            event.metadata().level(),
            visitor.0
        );
        self.lines.line(&line);
    }
}

#[derive(Default)]
struct JsonFields(String);

impl JsonFields {
    fn push(&mut self, field: &Field, value: serde_json::Value) {
        if !self.0.is_empty() {
            self.0.push(',');
        }
        let _ = write!(
            self.0,
            "{}:{}",
            serde_json::Value::String(field.name().to_owned()),
            value
        );
    }
}

impl Visit for JsonFields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.push(field, serde_json::Value::String(format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.push(field, serde_json::Value::String(value.to_owned()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push(field, serde_json::Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push(field, serde_json::Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push(field, serde_json::Value::Bool(value));
    }
}

/// A line sink that keeps lines in memory, for tests.
#[derive(Debug, Default)]
pub struct MemoryLines(parking_lot::Mutex<Vec<String>>);

impl MemoryLines {
    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything written so far.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.0.lock().clone()
    }
}

impl LineSink for MemoryLines {
    fn line(&self, line: &str) {
        self.0.lock().push(line.to_owned());
    }
}

impl LineSink for crate::sink::FileSink {
    fn line(&self, line: &str) {
        self.append_line(line);
    }
}
