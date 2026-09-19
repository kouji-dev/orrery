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

/// Which streams a query reads.
///
/// **Not an `Option<Stream>`.** It was, and that is how `orrery ledger` came to
/// hide a decision: it named [`Stream::Audit`], a refusal to load is written to
/// [`Stream::Load`], and no command in the binary named `Load` at all. A
/// caller that wants "every decision" now says so, and which streams that means
/// is [`Stream::is_decision`]'s answer rather than a list each caller keeps.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Streams {
    /// Every record in the file, whatever stream it belongs to.
    #[default]
    All,
    /// Every stream that carries decisions. What `orrery ledger` reads.
    Decisions,
    /// Exactly one, for an operator who is asking about retention rather than
    /// about what happened.
    Only(Stream),
}

impl Streams {
    /// Whether records of this stream are selected.
    #[must_use]
    pub fn contains(self, stream: Stream) -> bool {
        match self {
            Streams::All => true,
            Streams::Decisions => stream.is_decision(),
            Streams::Only(one) => one == stream,
        }
    }
}

/// What to select out of a stream.
///
/// Every field but [`Query::streams`] is an `Option`, and `None` means "do not
/// filter on this". The default selects everything.
#[derive(Clone, Debug, Default)]
pub struct Query {
    /// Which streams to read. `orrery ledger` passes [`Streams::Decisions`],
    /// `orrery telemetry` passes [`Streams::Only(Stream::Telemetry)`](Streams::Only).
    pub streams: Streams,
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
        if !self.streams.contains(record.event.stream()) {
            return false;
        }
        if let Some(subject) = &self.subject {
            match subject_of(&record.event) {
                Some(found) if &found == subject => {}
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
/// Not every event has a subject — a model request does not — and a
/// `--subject` filter therefore *excludes* them rather than matching
/// everything. That is the right way round: asking "what did `ext:git` do"
/// should not hand back the whole stream.
///
/// An extension load **does** name somebody: the extension. It used to be left
/// out here, which meant `orrery ledger --subject ext:git` could not show the
/// refusal that stopped `git` loading — the one record about `ext:git` in the
/// whole file.
#[must_use]
pub fn subject_of(event: &AuditEvent) -> Option<Subject> {
    match event {
        AuditEvent::CapabilityDecision { subject, .. } | AuditEvent::SubAgentSpawn {
            parent: subject,
            ..
        } => Some(subject.clone()),
        AuditEvent::ExtensionLoad { ext, .. } => Some(Subject::Ext(ext.clone())),
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
                streams: Streams::Only(Stream::Audit),
                ..Query::default()
            },
        );
        let telemetry = scan_str(
            &text,
            &Query {
                streams: Streams::Only(Stream::Telemetry),
                ..Query::default()
            },
        );
        assert_eq!(ledger.records.len(), 2);
        assert_eq!(telemetry.records.len(), 1);
    }

    /// `orrery ledger` reads every stream that carries a decision, and a
    /// refusal to load is one. Telemetry is not.
    #[test]
    fn decisions_are_every_stream_that_carries_one() {
        let sink = MemorySink::new();
        sink.append(AuditEvent::ExtensionLoad {
            ext: "git".parse().expect("an ext id"),
            status: "skipped".to_owned(),
            contributions: Vec::new(),
            problems: vec!["managed.toml refuses unpinned extensions".to_owned()],
        });
        sink.append(AuditEvent::ModelRequest {
            model: "fixture".to_owned(),
            input_tokens: 1,
            output_tokens: 1,
        });
        sink.append(AuditEvent::ConsentAnswer {
            prompt: orrery_proto::PromptId::new(),
            answer: "allow-once".to_owned(),
        });
        let text = sink.to_jsonl();

        let decisions = scan_str(
            &text,
            &Query {
                streams: Streams::Decisions,
                ..Query::default()
            },
        );
        assert_eq!(
            decisions.records.len(),
            2,
            "the load and the consent, not the count: {:#?}",
            decisions.records
        );
        assert!(
            decisions
                .records
                .iter()
                .any(|r| matches!(r.event, AuditEvent::ExtensionLoad { .. })),
            "the refusal is one of them"
        );

        // And the load stream is reachable on its own, for retention questions.
        let only = scan_str(
            &text,
            &Query {
                streams: Streams::Only(Stream::Load),
                ..Query::default()
            },
        );
        assert_eq!(only.records.len(), 1);
    }

    /// An extension load names the extension, so a subject filter finds the
    /// one record in the file that is about it.
    #[test]
    fn a_load_is_findable_by_the_extension_it_is_about() {
        let sink = MemorySink::new();
        sink.append(AuditEvent::ExtensionLoad {
            ext: "git".parse().expect("an ext id"),
            status: "skipped".to_owned(),
            contributions: Vec::new(),
            problems: Vec::new(),
        });
        let found = scan_str(
            &sink.to_jsonl(),
            &Query {
                subject: Some(Subject::Ext("git".parse().expect("an ext id"))),
                ..Query::default()
            },
        );
        assert_eq!(found.records.len(), 1);
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
