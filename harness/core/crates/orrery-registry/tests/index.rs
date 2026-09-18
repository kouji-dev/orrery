//! Plan 15 task 1: the index document's own invariants.

mod common;

use orrery_registry::index::{Entry, EntrySource, Index, SCHEMA};
use orrery_registry::{RegistryError, Timestamp};

fn one_entry() -> Entry {
    Entry {
        id: "buildgraph".to_owned(),
        version: "1.2.0".parse().unwrap(),
        source: EntrySource::CratesIo {
            name: "orrery-ext-buildgraph".to_owned(),
        },
        sha256: "a".repeat(64),
        sig: "org-2026:".to_owned() + &"b".repeat(128),
        requires: vec!["read($WORKSPACE/**)".to_owned(), "spawn(java)".to_owned()],
    }
}

#[test]
fn parses_and_round_trips() {
    let index = common::index_of(vec![one_entry()]);
    let text = index.to_toml().unwrap();
    let back = Index::parse(&text, "index.toml").unwrap();
    assert_eq!(back, index, "the document did not survive a round trip");

    // And the shape the plan writes parses as itself.
    let literal = r#"
schema  = 1
issued  = "2026-09-18T00:00:00Z"
expires = "2026-12-18T00:00:00Z"

[[extension]]
id      = "buildgraph"
version = "1.2.0"
source  = { kind = "crates-io", name = "orrery-ext-buildgraph" }
sha256  = "aaaa"
sig     = "bbbb"
requires = ["read($WORKSPACE/**)", "spawn(java)"]
"#;
    let parsed = Index::parse(literal, "index.toml").unwrap();
    assert_eq!(parsed.extensions.len(), 1);
    assert_eq!(parsed.extensions[0].id, "buildgraph");
    assert_eq!(
        parsed.extensions[0].source,
        EntrySource::CratesIo {
            name: "orrery-ext-buildgraph".to_owned()
        }
    );
}

#[test]
fn unknown_schema_is_refused() {
    let text = r#"
schema  = 2
issued  = "2026-09-18T00:00:00Z"
expires = "2026-12-18T00:00:00Z"
"#;
    let err = Index::parse(text, "index.toml").unwrap_err();
    let RegistryError::UnknownSchema {
        found, supported, ..
    } = err
    else {
        panic!("expected UnknownSchema, got {err:?}");
    };
    assert_eq!((found, supported), (2, SCHEMA));
}

#[test]
fn expired_index_is_refused() {
    let index = common::index_of(vec![one_entry()]);
    let after = Timestamp::parse("now", "2027-01-01T00:00:00Z").unwrap();
    let err = index
        .check_fresh(&after, "index.toml", common::INDEX_URL)
        .unwrap_err();
    let RegistryError::Expired { index: url, .. } = &err else {
        panic!("expected Expired, got {err:?}");
    };
    assert_eq!(url, common::INDEX_URL, "the message must say where to refetch");

    // And a fresh one is not refused.
    index
        .check_fresh(&common::now(), "index.toml", common::INDEX_URL)
        .expect("in force");
}

#[test]
fn a_local_time_expiry_is_refused() {
    // Not a nicety: an index several people read must mean one instant to all
    // of them, and `2026-12-18T00:00:00+09:00` does not.
    let err = Timestamp::parse("expires", "2026-12-18T00:00:00+09:00").unwrap_err();
    assert!(matches!(err, RegistryError::BadTimestamp { .. }), "{err:?}");
}

#[test]
fn timestamps_order_by_instant_not_by_string() {
    let earlier = Timestamp::parse("a", "2026-09-18T09:00:00Z").unwrap();
    let later = Timestamp::parse("b", "2026-09-18T12:00:00Z").unwrap();
    assert!(earlier < later);
}

#[test]
fn every_aspect_round_trips() {
    use orrery_registry::index::{aspect_from_str, aspect_str, parse_requirement};
    for name in [
        "tool", "mcp", "skill", "ext", "mode", "read", "write", "spawn", "net", "creds", "ui",
        "render", "mem.read", "mem.write",
    ] {
        let aspect = aspect_from_str(name).unwrap_or_else(|| panic!("{name} is not an aspect"));
        assert_eq!(aspect_str(aspect), name);
        let cap = parse_requirement(&format!("{name}(x)"), "test").unwrap();
        assert_eq!(cap.aspect, aspect);
        assert_eq!(cap.scope, vec!["x".to_owned()]);
    }
    assert!(aspect_from_str("nonsense").is_none());
}
