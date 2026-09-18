//! The differ: the append fast path, the skipped subtree, the cost guard and —
//! the one that matters most — that a patch sequence reconstructs its target.

use orrery_proto::{
    Cell, Choice, DiffLine, DiffLineKind, Field, FieldKind, Hunk, StackDir, Status, Surface,
    SurfaceId, SurfaceKind, SurfacePatch, TaskItem, TextStyle, TreeNode,
};
use orrery_surface::{COST_GUARD, apply, diff};
use proptest::prelude::*;

fn id() -> SurfaceId {
    SurfaceId::new()
}

fn text(value: &str) -> Surface {
    Surface::new(SurfaceKind::Text {
        value: value.to_owned(),
        style: None,
    })
}

fn markdown(value: &str, complete: bool) -> Surface {
    Surface::new(SurfaceKind::Markdown {
        value: value.to_owned(),
        complete,
    })
}

fn stack(children: Vec<Surface>) -> Surface {
    Surface::new(SurfaceKind::Stack {
        dir: StackDir::Column,
        title: None,
        collapsed: false,
        children,
    })
}

fn with_id(mut surface: Surface, at: u128) -> Surface {
    surface.id = Some(SurfaceId::from_uuid(uuid_from(at)));
    surface
}

fn uuid_from(at: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(at)
}

fn table(rows: &[(&str, &str)]) -> Surface {
    Surface::new(SurfaceKind::Table {
        columns: vec!["crate".into(), "lines".into()],
        rows: rows
            .iter()
            .map(|(a, b)| {
                vec![
                    Cell {
                        text: (*a).to_owned(),
                        style: None,
                    },
                    Cell {
                        text: (*b).to_owned(),
                        style: Some(TextStyle::Muted),
                    },
                ]
            })
            .collect(),
    })
}

fn patch(prev: &Surface, next: &Surface) -> Vec<SurfacePatch> {
    let mut out = Vec::new();
    diff(prev, next, id(), &mut out);
    out
}

/// A markdown surface growing by one word is one `Append` carrying only the
/// new word — the reason the op exists.
#[test]
fn append_fast_path() {
    let prev = markdown("the workspace has", false);
    let next = markdown("the workspace has three", false);
    let out = patch(&prev, &next);

    assert_eq!(out.len(), 1, "one op, not a new tree: {out:?}");
    let SurfacePatch::Append { text, .. } = &out[0] else {
        panic!("expected an append, got {:?}", out[0]);
    };
    assert_eq!(text, " three", "only the suffix travels");
}

/// Text takes the same path. A rewrite that is not a prefix extension does not.
#[test]
fn append_only_when_it_really_is_an_append() {
    let out = patch(&text("abc"), &text("abcd"));
    assert!(matches!(out.as_slice(), [SurfacePatch::Append { .. }]));

    let out = patch(&text("abc"), &text("xbc"));
    assert!(
        matches!(out.as_slice(), [SurfacePatch::Set { .. }]),
        "a rewrite is a set, not an append: {out:?}"
    );
}

/// A hundred children, one changed: one `Set`, and the other ninety-nine
/// subtrees are never walked.
#[test]
fn unchanged_subtree_is_skipped() {
    let children: Vec<Surface> = (0..100).map(|i| text(&format!("child {i}"))).collect();
    let prev = stack(children.clone());
    let mut changed = children;
    changed[42] = text("child forty-two, revised");
    let next = stack(changed);

    let mut out = Vec::new();
    let cost = diff(&prev, &next, id(), &mut out);

    assert_eq!(out.len(), 1, "one op: {out:?}");
    assert!(matches!(out[0], SurfacePatch::Set { .. }));
    assert_eq!(cost.subtrees_skipped, 99, "ninety-nine hashes matched");
    assert_eq!(
        cost.nodes_walked, 2,
        "the root and the one child that changed, nothing else"
    );
}

/// A different variant is a whole new surface.
#[test]
fn discriminant_change_replaces() {
    let out = patch(&text("hello"), &markdown("hello", true));
    assert!(
        matches!(out.as_slice(), [SurfacePatch::Replace { .. }]),
        "{out:?}"
    );
}

/// Reordering children that carry stable ids touches the slots that moved, and
/// nothing else. A wholesale replacement here is what makes a re-sort expensive.
#[test]
fn stack_children_match_by_id() {
    let a = with_id(text("alpha"), 1);
    let b = with_id(text("beta"), 2);
    let c = with_id(text("gamma"), 3);
    let prev = stack(vec![a.clone(), b.clone(), c.clone()]);
    let next = stack(vec![c, b, a]);

    let out = patch(&prev, &next);
    assert_eq!(
        out.len(),
        2,
        "two slots moved, the middle one held: {out:?}"
    );
    for op in &out {
        let SurfacePatch::Set { path, .. } = op else {
            panic!("a move is a set at the slot, not {op:?}");
        };
        assert_eq!(path[0], "kind");
        assert_eq!(path[1], "children");
    }

    let mut rebuilt = prev;
    for op in &out {
        apply(&mut rebuilt, op).expect("the moves apply");
    }
    assert_eq!(rebuilt, next);
}

/// A table whose every row moved costs more in `Set`s than it does as one
/// `Replace`, so the guard collapses it.
#[test]
fn cost_guard_collapses() {
    let rows: Vec<(String, String)> = (0..20)
        .map(|i| (format!("crate-{i:02}"), format!("{}", 1000 + i)))
        .collect();
    let borrowed: Vec<(&str, &str)> = rows.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let prev = table(&borrowed);
    let mut resorted = borrowed.clone();
    resorted.reverse();
    let next = table(&resorted);

    let mut out = Vec::new();
    let cost = diff(&prev, &next, id(), &mut out);

    assert_eq!(out.len(), 1, "one op, not twenty: {out:?}");
    assert!(
        matches!(out[0], SurfacePatch::Replace { .. }),
        "the guard collapses to a replace: {:?}",
        out[0]
    );
    assert!(cost.collapsed);
    assert!(
        cost.patch_bytes as f64 > COST_GUARD * cost.replace_bytes as f64,
        "the guard fired because the ops were dear: {cost:?}"
    );
    let one = serde_json::to_vec(&out[0]).expect("serialises");
    assert!(
        one.len() <= cost.replace_bytes,
        "and what went out is the replace, not more"
    );
}

/// One changed cell is one op, and the guard does not fire on it.
#[test]
fn cost_guard_leaves_a_cheap_diff_alone() {
    let prev = table(&[("orrery-proto", "2358"), ("orrery-agui", "612")]);
    let next = table(&[("orrery-proto", "2358"), ("orrery-agui", "613")]);

    let mut out = Vec::new();
    let cost = diff(&prev, &next, id(), &mut out);
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(matches!(out[0], SurfacePatch::Set { .. }));
    assert!(!cost.collapsed);
}

// ---------------------------------------------------------------------------
// The property that matters most.
// ---------------------------------------------------------------------------

fn arb_style() -> impl Strategy<Value = Option<TextStyle>> {
    prop_oneof![
        Just(None),
        Just(Some(TextStyle::Plain)),
        Just(Some(TextStyle::Muted)),
        Just(Some(TextStyle::Error)),
    ]
}

fn arb_status() -> impl Strategy<Value = Option<Status>> {
    prop_oneof![
        Just(None),
        Just(Some(Status::Running)),
        Just(Some(Status::Done)),
        Just(Some(Status::Failed)),
    ]
}

fn arb_cell() -> impl Strategy<Value = Cell> {
    ("[a-z ]{0,6}", arb_style()).prop_map(|(text, style)| Cell { text, style })
}

fn arb_leaf() -> impl Strategy<Value = SurfaceKind> {
    prop_oneof![
        ("[a-z ]{0,20}", arb_style()).prop_map(|(value, style)| SurfaceKind::Text { value, style }),
        ("[a-z `\n]{0,20}", any::<bool>())
            .prop_map(|(value, complete)| SurfaceKind::Markdown { value, complete }),
        "[a-z]{1,6}".prop_map(|id| SurfaceKind::Stream { id }),
        (
            prop::collection::vec("[a-z]{1,4}", 0..3),
            prop::collection::vec(prop::collection::vec(arb_cell(), 0..3), 0..3)
        )
            .prop_map(|(columns, rows)| SurfaceKind::Table { columns, rows }),
        prop::collection::vec(("[a-z]{1,4}", any::<bool>()), 0..3).prop_map(|nodes| {
            SurfaceKind::Tree {
                nodes: nodes
                    .into_iter()
                    .map(|(label, expanded)| TreeNode {
                        label,
                        id: None,
                        expanded,
                        children: Vec::new(),
                    })
                    .collect(),
            }
        }),
        (
            "[a-z/.]{1,8}",
            prop::collection::vec(("[a-z ]{0,5}", any::<bool>()), 0..3)
        )
            .prop_map(|(path, lines)| SurfaceKind::Diff {
                path,
                hunks: vec![Hunk {
                    old_start: 1,
                    old_lines: lines.len() as u64,
                    new_start: 1,
                    new_lines: lines.len() as u64,
                    lines: lines
                        .into_iter()
                        .map(|(text, add)| DiffLine {
                            kind: if add {
                                DiffLineKind::Add
                            } else {
                                DiffLineKind::Context
                            },
                            text,
                        })
                        .collect(),
                }],
            }),
        (
            "[a-z ]{1,8}",
            prop::option::of(0u64..100),
            prop::option::of(0u64..100)
        )
            .prop_map(|(label, done, total)| SurfaceKind::Progress { label, done, total }),
        prop::collection::vec(("[a-z]{1,4}", "[a-z ]{0,6}"), 0..3).prop_map(|items| {
            SurfaceKind::Task {
                items: items
                    .into_iter()
                    .map(|(id, label)| TaskItem {
                        id,
                        label,
                        status: Status::Pending,
                    })
                    .collect(),
            }
        }),
        (
            "[a-z ?]{1,10}",
            prop::collection::vec("[a-z]{1,4}", 0..3),
            any::<bool>(),
            any::<bool>()
        )
            .prop_map(|(prompt, choices, multi, free)| SurfaceKind::Question {
                prompt,
                choices: choices
                    .into_iter()
                    .map(|value| Choice {
                        label: value.clone(),
                        value,
                    })
                    .collect(),
                multi,
                free,
                default: None,
                deadline_ms: None,
            }),
        (prop::collection::vec("[a-z]{1,4}", 0..3), "[a-z]{1,6}").prop_map(|(fields, submit)| {
            SurfaceKind::Form {
                fields: fields
                    .into_iter()
                    .map(|name| Field {
                        label: name.clone(),
                        name,
                        kind: FieldKind::Text {},
                        required: false,
                        default: None,
                    })
                    .collect(),
                submit,
            }
        }),
    ]
}

fn arb_surface() -> impl Strategy<Value = Surface> {
    let leaf = (arb_leaf(), arb_status()).prop_map(|(kind, status)| Surface {
        id: None,
        status,
        kind,
    });
    leaf.prop_recursive(3, 12, 3, |inner| {
        prop_oneof![
            (
                prop::collection::vec(inner.clone(), 0..3),
                any::<bool>(),
                prop::option::of("[a-z ]{1,5}")
            )
                .prop_map(|(children, collapsed, title)| Surface::new(
                    SurfaceKind::Stack {
                        dir: StackDir::Column,
                        title,
                        collapsed,
                        children,
                    }
                )),
            inner.prop_map(|fallback| Surface::new(SurfaceKind::Custom {
                kind: "test.thing".into(),
                payload: serde_json::json!({ "n": 1 }),
                fallback: Box::new(fallback),
            })),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Applying `diff(prev, next)` to `prev` yields `next`. Every other
    /// property of the differ is an optimisation; this one is correctness.
    #[test]
    fn patches_reconstruct(prev in arb_surface(), next in arb_surface()) {
        let surface = SurfaceId::new();
        let mut out = Vec::new();
        diff(&prev, &next, surface, &mut out);

        let mut rebuilt = prev.clone();
        for op in &out {
            apply(&mut rebuilt, op)
                .unwrap_or_else(|e| panic!("patch {op:?} did not apply: {e}"));
        }
        prop_assert_eq!(&rebuilt, &next, "patches: {:?}", out);
    }

    /// And the same surface twice is no patches at all.
    #[test]
    fn nothing_changed_is_no_ops(surface in arb_surface()) {
        let mut out = Vec::new();
        diff(&surface, &surface, SurfaceId::new(), &mut out);
        prop_assert!(out.is_empty(), "{:?}", out);
    }
}
