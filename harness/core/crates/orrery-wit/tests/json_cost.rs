//! Plan 14, task 7 — what "JSON crosses as a `string`" actually costs.
//!
//! `00-overview.md` flagged it and left it open. This puts a number on it, on
//! the worst realistic shape: a delta-heavy extension emitting 1000 small
//! surface updates. Run with `--nocapture` to see the table; the assertions are
//! the regression guard.

use std::time::Instant;

use orrery_proto::surface::{Cell, StackDir, Surface, SurfaceKind, TextStyle};
use orrery_wit::arena::{cost_as_json_strings, cost_as_typed_fields, flatten, rebuild};

/// One update from a delta-heavy extension: a small stack with a line of text
/// and a two-cell row. This is the hot path — a progress tick, a log line —
/// not a big report, because big reports are where the encoding does not matter.
fn one_update(i: usize) -> Surface {
    Surface::new(SurfaceKind::Stack {
        dir: StackDir::Row,
        title: None,
        collapsed: false,
        children: vec![
            Surface::new(SurfaceKind::Text {
                value: format!("step {i}"),
                style: Some(TextStyle::Muted),
            }),
            Surface::new(SurfaceKind::Table {
                columns: vec!["module".into(), "reason".into()],
                rows: vec![vec![
                    Cell {
                        text: format!("crate-{}", i % 17),
                        style: None,
                    },
                    Cell {
                        text: "changed".into(),
                        style: None,
                    },
                ]],
            }),
        ],
    })
}

const UPDATES: usize = 1000;

#[test]
fn json_as_string_is_measured_not_assumed() {
    let surfaces: Vec<Surface> = (0..UPDATES).map(one_update).collect();

    let mut json = 0usize;
    let mut typed = 0usize;
    let mut documents = 0usize;
    for s in &surfaces {
        let arena = flatten(s);
        let a = cost_as_json_strings(&arena);
        let b = cost_as_typed_fields(&arena);
        json += a.bytes;
        typed += b.bytes;
        documents += a.json_documents;
    }

    // Round-trip cost, both ways, over the same 1000 updates.
    let started = Instant::now();
    let arenas: Vec<_> = surfaces.iter().map(flatten).collect();
    let flatten_ns = started.elapsed().as_nanos() / UPDATES as u128;
    let started = Instant::now();
    for arena in &arenas {
        rebuild(arena).expect("a flattened surface rebuilds");
    }
    let rebuild_ns = started.elapsed().as_nanos() / UPDATES as u128;

    let overhead = (json as f64 / typed as f64 - 1.0) * 100.0;
    println!("--- plan 14 task 7: JSON-as-string over {UPDATES} delta updates ---");
    println!("json-as-string : {json} bytes ({documents} JSON documents)");
    println!("typed-in-wit   : {typed} bytes (hypothetical)");
    println!("overhead       : {overhead:.1}%");
    println!("flatten        : {flatten_ns} ns/update");
    println!("rebuild        : {rebuild_ns} ns/update");

    // The decision this test records: the overhead is bounded and small enough
    // that typing the payloads in WIT is not worth freezing twelve more records
    // into the contract. If this trips, the payloads grew a shape the estimate
    // does not model and the question is open again — see the plan.
    assert!(
        overhead < 120.0,
        "JSON-as-string overhead is {overhead:.1}%, which is past the point \
         where typing the hot payloads in WIT pays for itself; re-open plan 14 \
         open question 1 before the contract freezes"
    );
    assert!(json > 0 && typed > 0);
}

/// The other half of the cost: a typed encoding would not need a JSON parse per
/// node. Assert the parse is the part worth knowing about, and that it stays in
/// the microsecond range for a whole update rather than the millisecond one.
#[test]
fn a_whole_update_round_trips_in_microseconds() {
    let surface = one_update(7);
    let arena = flatten(&surface);
    let started = Instant::now();
    for _ in 0..UPDATES {
        let a = flatten(&surface);
        rebuild(&a).expect("a flattened surface rebuilds");
    }
    let per = started.elapsed().as_micros() as f64 / UPDATES as f64;
    println!("round trip: {per:.2} us/update over {} nodes", arena.nodes.len());
    assert!(
        per < 200.0,
        "a {}-node update round-trips in {per:.2}us; that is the JSON parse \
         becoming the cost, not the boundary",
        arena.nodes.len()
    );
}
