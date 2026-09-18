//! Reading the stream back, for an operator.
//!
//! # Why this is not a second schema
//!
//! Everything here deserialises [`AuditRecord`] and filters it. There is no
//! index, no summary table and no derived store, because a second
//! representation of the evidence stream is a second thing that can disagree
//! with it. A query is a scan of the JSONL the [`FileSink`](crate::FileSink)
//! wrote, in the order it wrote it.
//!
//! # Reading is not writing
//!
//! Nothing in this module is `mut`, and nothing here opens a file for anything
//! but reading. [`AuditSink`](crate::AuditSink) remains the only way a record
//! gets into the stream, and it only appends.
//!
//! # A malformed line is reported, not swallowed
//!
//! A truncated tail — a process killed mid-write — must not make the whole
//! stream unreadable, and must not quietly vanish either. [`Scan::skipped`]
//! counts the lines that would not parse, so an operator surface can say "487
//! records, 1 unreadable line" instead of either crashing or lying.

use std::path::Path;

use orrery_proto::Subject;

use crate::error::AuditError;
use crate::event::{AuditEvent, AuditRecord};
use crate::layer::Stream;

/// What to select out of a stream.
///
/// Every field is an `Option`, and `None` means "do not filter on this". The
/// default selects everything, which is what `orrery ledger` with no flags is.
#[derive(Clone, Debug, Default)]
pub struct Query {
    /// Only records belonging to this stream. `orrery ledger` passes
    /// [`Stream::Audit`], `orrery telemetry` passes [`Stream::Telemetry`].
    pub stream: Option<Stream>,
    /// Only decisions and spawns by this subject, spelled the way
    /// [`Subject`] displays: `agent`, `ext:<id>`, `agent:<name>`.
    pub subject: Option<Subject>,
    /// Only records naming this rule — matched against the rule id **and**
    /// against the rule as written, because an operator reading a config file
    /// has the text in front of them and not the id.
    pub rule: Option<String>,
    /// At most this many records, counted from the **end** of the stream: the
    /// interesting part of an audit is what just happened.
    pub limit: Option<usize>,
}

impl Query {
    /// Whether one record is selected.
    #[must_use]
    pub fn matches(&self, record: &AuditRecord) -> bool {
        if let Some(stream) = self.stream {
            if record.event.stream() != stream {
                return false;
            }
        }
        if let Some(subject) = &self.subject {
            match subject_of(&record.event) {
                Some(found) if found == subject => {}
                _ => return false,
            }
        }
        if let Some(rule) = &self.rule {
            if !names_rule(&record.event, rule) {
                return false;
            }
        }
        true
    }
}

/// What a scan found.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    /// The records that matched, oldest first.
    pub records: Vec<AuditRecord>,
    /// How many records were read before filtering.
    pub read: usize,
    /// How many lines would not parse. A truncated tail is the usual reason.
    pub skipped: usize,
}

/// Read one JSONL stream and select from it.
///
/// A file that is not there is an empty scan rather than an error: "nothing has
/// been audited yet" is an answer, and making the caller distinguish it from a
/// broken path only moves the `if` somewhere worse.
///
/// # Errors
///
/// [`AuditError::Sink`] when the file exists and cannot be read.
pub fn scan(path: impl AsRef<Path>, query: &Query) -> Result<Scan, AuditError> {
    let path = path.as_ref();
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Scan::default()),
        Err(e) => return Err(AuditError::sink(path, e)),
    };
    Ok(scan_str(&text, query))
}

/// The same, over a stream already in memory. What the tests drive, and what
/// [`MemorySink::to_jsonl`](crate::MemorySink::to_jsonl) feeds.
#[must_use]
pub fn scan_str(text: &str, query: &Query) -> Scan {
    let mut out = Scan::default();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<AuditRecord>(line) {
            Ok(record) => {
                out.read += 1;
                if query.matches(&record) {
                    out.records.push(record);
                }
            }
            Err(_) => out.skipped += 1,
        }
    }
    if let Some(limit) = query.limit {
        let drop = out.records.len().saturating_sub(limit);
        out.records.drain(..drop);
    }
    out
}

/// Who did it, for the variants that name somebody.
///
/// Not every event has a subject — a model request and an extension load do
/// not — and a `--subject` filter therefore *excludes* them rather than
/// matching everything. That is the right way round: asking "what did `ext:git`
/// do" should not hand back the whole stream.
#[must_use]
pub fn subject_of(event: &AuditEvent) -> Option<&Subject> {
    match event {
        AuditEvent::CapabilityDecision { subject, .. } | AuditEvent::SubAgentSpawn {
            parent: subject,
            ..
        } => Some(subject),
        _ => None,
    }
}

/// Whether an event names this rule, by id or by the text it was written as.
#[must_use]
pub fn names_rule(event: &AuditEvent, wanted: &str) -> bool {
    let AuditEvent::CapabilityDecision {
        rule, rule_text, ..
    } = event
    else {
        return false;
    };
    rule.as_ref().is_some_and(|r| r.to_string() == wanted)
        || rule_text
            .as_deref()
            .is_some_and(|t| t.contains(wanted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Verdict;
    use crate::sink::{AuditSink, MemorySink};
    use orrery_proto::RuleId;

    fn stream_with_three() -> (String, RuleId, RuleId) {
        let allow = RuleId::new();
        let deny = RuleId::new();
        let sink = MemorySink::new();
        sink.append(AuditEvent::CapabilityDecision {
            subject: Subject::Agent,
            request: "write(./src/main.rs)".to_owned(),
            verdict: Verdict::Allow,
            rule: Some(allow),
            rule_text: Some("allow = [\"write(./**)\"]".to_owned()),
            layer: None,
            reason: None,
        });
        sink.append(AuditEvent::CapabilityDecision {
            subject: Subject::Ext("git".parse().expect("an ext id")),
            request: "net(domain: github.com)".to_owned(),
            verdict: Verdict::Deny,
            rule: Some(deny),
            rule_text: Some("deny = [\"net(*)\"]".to_owned()),
            layer: None,
            reason: None,
        });
        sink.append(AuditEvent::ModelRequest {
            model: "fixture".to_owned(),
            input_tokens: 10,
            output_tokens: 3,
        });
        (sink.to_jsonl(), allow, deny)
    }

    /// No filter is everything, and the counts say what was read.
    #[test]
    fn an_empty_query_selects_everything() {
        let found = scan_str(&stream_with_three().0, &Query::default());
        assert_eq!(found.read, 3);
        assert_eq!(found.records.len(), 3);
        assert_eq!(found.skipped, 0);
    }

    /// `orrery ledger` and `orrery telemetry` are the same scan, split by the
    /// event's own `stream()`.
    #[test]
    fn the_two_streams_are_disjoint() {
        let (text, _allow, _deny) = stream_with_three();
        let ledger = scan_str(
            &text,
            &Query {
                stream: Some(Stream::Audit),
                ..Query::default()
            },
        );
        let telemetry = scan_str(
            &text,
            &Query {
                stream: Some(Stream::Telemetry),
                ..Query::default()
            },
        );
        assert_eq!(ledger.records.len(), 2);
        assert_eq!(telemetry.records.len(), 1);
    }

    /// A subject filter picks that subject, and leaves out the events that
    /// have no subject at all.
    #[test]
    fn a_subject_filter_excludes_what_has_no_subject() {
        let found = scan_str(
            &stream_with_three().0,
            &Query {
                subject: Some(Subject::Agent),
                ..Query::default()
            },
        );
        assert_eq!(found.records.len(), 1);
        assert!(matches!(
            found.records[0].event,
            AuditEvent::CapabilityDecision { .. }
        ));
    }

    /// A rule can be asked for by id or by the words in the config file.
    #[test]
    fn a_rule_matches_by_id_or_by_text() {
        let (text, _allow, _deny) = stream_with_three();
        let by_id = scan_str(
            &text,
            &Query {
                rule: Some(_deny.to_string()),
                ..Query::default()
            },
        );
        assert_eq!(by_id.records.len(), 1);

        let by_text = scan_str(
            &text,
            &Query {
                rule: Some("write(./**)".to_owned()),
                ..Query::default()
            },
        );
        assert_eq!(by_text.records.len(), 1);
    }

    /// A limit keeps the **latest** records: an audit is read from the end.
    #[test]
    fn a_limit_keeps_the_tail() {
        let found = scan_str(
            &stream_with_three().0,
            &Query {
                limit: Some(1),
                ..Query::default()
            },
        );
        assert_eq!(found.records.len(), 1);
        assert!(matches!(
            found.records[0].event,
            AuditEvent::ModelRequest { .. }
        ));
    }

    /// A killed process leaves half a line. It is counted, not swallowed, and
    /// it does not take the readable records with it.
    #[test]
    fn a_truncated_tail_is_counted() {
        let mut text = stream_with_three().0;
        text.push_str("{\"seq\":9,\"at_ms\":1,\"t\":\"model.re");
        let found = scan_str(&text, &Query::default());
        assert_eq!(found.records.len(), 3, "the whole lines still read");
        assert_eq!(found.skipped, 1, "and the broken one is reported");
    }

    /// Nothing audited yet is an answer, not an error.
    #[test]
    fn a_missing_file_is_an_empty_scan() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let found = scan(dir.path().join("nothing.jsonl"), &Query::default())
            .expect("a missing file is not an error");
        assert!(found.records.is_empty());
        assert_eq!(found.read, 0);
    }
}
