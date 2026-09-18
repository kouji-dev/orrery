//! The append-only structured stream: three tracing layers, redaction in schema, file and OTLP sinks.
//!
//! Implementation plan: `harness/docs/plans/07-policy-broker-audit.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]
