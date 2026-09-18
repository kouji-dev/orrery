//! Task 3: one widget per core surface, and its degradation.
//!
//! Every case renders a surface taken from the conformance fixtures into a
//! plain [`Buffer`] and snapshots the cells. Two snapshots per widget: the
//! ordinary terminal, and the constrained one — narrow and without colour —
//! because the degradation table in the plan is a promise and a promise nobody
//! asserts is a paragraph.

use orrery_client::conformance::{Scenario, fixtures_dir, load};
use orrery_client::{SurfaceStore, SurfaceView};
use orrery_client_ratatui::scrollback::lines_of;
use orrery_client_ratatui::testing::{as_surface, frames_of};
use orrery_client_ratatui::theme::Theme;
use orrery_client_ratatui::widgets;
use orrery_proto::Surface;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn scenario(name: &str) -> Scenario {
    load(&fixtures_dir().join(format!("{name}.jsonl"))).expect("the fixture loads")
}

/// Replay a fixture and take the surface it is about.
fn surface_from(name: &str, id: &str) -> Surface {
    let mut store = SurfaceStore::new();
    for frame in frames_of(&scenario(name)) {
        store.apply(&frame);
    }
    let view: &SurfaceView = store
        .state()
        .turns
        .iter()
        .flat_map(|t| t.surfaces.iter())
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("{name} has no surface {id}"));
    as_surface(view)
}

fn draw(surface: &Surface, width: u16, theme: Theme) -> String {
    let height = widgets::measure(surface, width).max(1);
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    widgets::render(surface, buf.area, &mut buf, &theme);
    lines_of(&buf).join("\n")
}

/// The ordinary terminal, and the constrained one.
fn both(name: &str, surface: &Surface) {
    insta::assert_snapshot!(format!("{name}"), draw(surface, 60, Theme::colour()));
    insta::assert_snapshot!(
        format!("{name}-degraded"),
        draw(surface, 24, Theme::monochrome())
    );
}

#[test]
fn text_snapshot() {
    let surface = Surface::new(orrery_proto::SurfaceKind::Text {
        value: "the workspace has three crates: proto, provider and kernel".into(),
        style: Some(orrery_proto::TextStyle::Muted),
    });
    both("text", &surface);
}

#[test]
fn markdown_snapshot() {
    both("markdown", &surface_from("streaming-markdown", "msg-1"));
}

#[test]
fn table_snapshot() {
    both("table", &surface_from("table-then-resort", "tbl-1"));
}

#[test]
fn table_elides_middle_columns_when_narrow() {
    // The documented degradation: drop from the middle, never wrap a cell.
    let surface = Surface::new(orrery_proto::SurfaceKind::Table {
        columns: vec!["crate".into(), "lines".into(), "owner".into(), "status".into()],
        rows: vec![vec![
            orrery_proto::Cell {
                text: "orrery-proto".into(),
                style: None,
            },
            orrery_proto::Cell {
                text: "2358".into(),
                style: None,
            },
            orrery_proto::Cell {
                text: "kernel-team".into(),
                style: None,
            },
            orrery_proto::Cell {
                text: "green".into(),
                style: None,
            },
        ]],
    });
    let narrow = draw(&surface, 24, Theme::monochrome());
    assert!(
        narrow.lines().all(|l| l.chars().count() <= 24),
        "nothing spilled past the pane: {narrow}"
    );
    assert!(
        narrow.contains("crate") && narrow.contains("status"),
        "the outer columns are the ones a reader scans, so they survive: {narrow}"
    );
    insta::assert_snapshot!("table-elided", narrow);
}

#[test]
fn tree_snapshot() {
    both("tree", &surface_from("tree-surface", "tree-1"));
}

#[test]
fn diff_snapshot() {
    both("diff", &surface_from("diff-surface", "diff-1"));
}

#[test]
fn diff_marks_every_line_without_colour() {
    let surface = surface_from("diff-surface", "diff-1");
    let mono = draw(&surface, 60, Theme::monochrome());
    let body: Vec<&str> = mono
        .lines()
        .filter(|l| !l.starts_with("──") && !l.starts_with("@@"))
        .collect();
    assert!(
        body.iter()
            .all(|l| l.starts_with('+') || l.starts_with('-') || l.starts_with(' ') || l.is_empty()),
        "every line carries its marker with no colour to lean on: {body:?}"
    );
}

#[test]
fn progress_snapshot() {
    both("progress", &surface_from("progress-surface", "prog-1"));
}

#[test]
fn progress_is_indeterminate_without_a_total() {
    let surface = Surface::new(orrery_proto::SurfaceKind::Progress {
        label: "thinking".into(),
        done: None,
        total: None,
    });
    let drawn = draw(&surface, 60, Theme::colour());
    assert!(
        !drawn.contains('['),
        "no bar when there is nothing to measure: {drawn}"
    );
    insta::assert_snapshot!("progress-indeterminate", drawn);
}

#[test]
fn stream_snapshot() {
    both("stream", &surface_from("stream-surface", "stream-1"));
}

#[test]
fn stream_tail_is_bounded() {
    let mut tail = widgets::stream::Tail::new(3);
    for n in 0..100 {
        tail.push(&format!("line {n}\n"));
    }
    assert_eq!(tail.len(), 3, "the head is dropped, not the tail");
    assert_eq!(tail.lines(), ["line 97", "line 98", "line 99"]);
    tail.push("a partial");
    assert_eq!(
        tail.lines().last(),
        Some(&"a partial"),
        "an unterminated chunk is still shown"
    );
}

#[test]
fn task_snapshot() {
    both("task", &surface_from("task-surface", "task-1"));
}

#[test]
fn task_pins_the_active_item() {
    let items: Vec<orrery_proto::TaskItem> = (0..20)
        .map(|n| orrery_proto::TaskItem {
            id: format!("i{n}"),
            label: format!("step {n}"),
            status: if n == 15 {
                orrery_proto::Status::Running
            } else {
                orrery_proto::Status::Pending
            },
        })
        .collect();
    let surface = Surface::new(orrery_proto::SurfaceKind::Task { items });
    let mut buf = Buffer::empty(Rect::new(0, 0, 40, 5));
    widgets::render(&surface, buf.area, &mut buf, &Theme::colour());
    let drawn = lines_of(&buf).join("\n");
    assert!(
        drawn.contains("step 15"),
        "the item being worked on survives truncation: {drawn}"
    );
}

#[test]
fn question_snapshot() {
    both("question", &surface_from("question-surface", "q-1"));
}

#[test]
fn form_snapshot() {
    // The fixture's final form has five fields, one past INLINE_MAX, so this
    // snapshot IS the sequential degradation.
    both("form", &surface_from("form-surface", "form-1"));
}

#[test]
fn form_short_enough_is_drawn_inline() {
    let surface = Surface::new(orrery_proto::SurfaceKind::Form {
        fields: vec![
            orrery_proto::Field {
                name: "branch".into(),
                label: "Branch".into(),
                kind: orrery_proto::FieldKind::Text {},
                required: true,
                default: Some("main".into()),
            },
            orrery_proto::Field {
                name: "token".into(),
                label: "Token".into(),
                kind: orrery_proto::FieldKind::Secret {},
                required: true,
                default: Some("hunter2".into()),
            },
        ],
        submit: "Create".into(),
    });
    let drawn = draw(&surface, 60, Theme::colour());
    assert!(
        drawn.contains("Branch") && drawn.contains("Token"),
        "two fields fit, so both are shown: {drawn}"
    );
    assert!(
        !drawn.contains("hunter2"),
        "a secret is never echoed, not even from its default: {drawn}"
    );
    insta::assert_snapshot!("form-inline", drawn);
}

#[test]
fn stack_snapshot() {
    both("stack", &surface_from("tool-call", "call-1"));
}

#[test]
fn custom_snapshot() {
    both("custom", &surface_from("custom-with-fallback", "dag-1"));
}

#[test]
fn custom_always_draws_its_fallback() {
    let surface = surface_from("custom-with-fallback", "dag-1");
    let drawn = draw(&surface, 60, Theme::colour());
    assert!(
        drawn.contains("proto -> agui -> kernel"),
        "the fallback is what a TUI shows, always: {drawn}"
    );
    assert!(
        drawn.contains("buildgraph.dag"),
        "and it says whose kind it stood in for, so a lazy fallback is visible"
    );
}
