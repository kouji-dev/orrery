//! Translation #9: a `Surface` crosses the WIT boundary as a flat arena.
//!
//! These tests are the freeze. Every guest language binds against the shape
//! asserted here, so a change to it is a breaking change to the world.

use orrery_proto::surface::{
    Cell, Choice, DiffLine, DiffLineKind, Field, FieldKind, Hunk, StackDir, Status, Surface,
    SurfaceKind, TaskItem, TextStyle, TreeNode,
};
use orrery_wit::arena::{ArenaError, NodeKind, SurfaceArena, SurfaceNode, flatten, rebuild};
use proptest::prelude::*;

// --- strategies -----------------------------------------------------------

fn leaf_kind() -> impl Strategy<Value = SurfaceKind> {
    prop_oneof![
        (".{0,12}", proptest::option::of(Just(TextStyle::Code)))
            .prop_map(|(value, style)| SurfaceKind::Text { value, style }),
        (
            proptest::collection::vec("[a-z]{1,4}", 0..3),
            proptest::collection::vec(
                proptest::collection::vec(
                    ".{0,4}".prop_map(|text| Cell { text, style: None }),
                    0..3
                ),
                0..3
            )
        )
            .prop_map(|(columns, rows)| SurfaceKind::Table { columns, rows }),
        proptest::collection::vec("[a-z]{1,4}", 0..3).prop_map(|labels| SurfaceKind::Tree {
            nodes: labels
                .into_iter()
                .map(|label| TreeNode {
                    label,
                    id: None,
                    expanded: false,
                    children: Vec::new(),
                })
                .collect(),
        }),
        ("[a-z/]{1,8}", proptest::collection::vec("[a-z]{0,4}", 0..3)).prop_map(|(path, lines)| {
            SurfaceKind::Diff {
                path,
                hunks: vec![Hunk {
                    old_start: 1,
                    old_lines: lines.len() as u64,
                    new_start: 1,
                    new_lines: lines.len() as u64,
                    lines: lines
                        .into_iter()
                        .map(|text| DiffLine {
                            kind: DiffLineKind::Context,
                            text,
                        })
                        .collect(),
                }],
            }
        }),
        ("[a-z ]{0,8}", proptest::option::of(0u64..100)).prop_map(|(label, done)| {
            SurfaceKind::Progress {
                label,
                done,
                total: None,
            }
        }),
        "[a-z]{1,6}".prop_map(|id| SurfaceKind::Stream { id }),
        proptest::collection::vec("[a-z]{1,4}", 0..3).prop_map(|ids| SurfaceKind::Task {
            items: ids
                .into_iter()
                .map(|id| TaskItem {
                    label: id.clone(),
                    id,
                    status: Status::Pending,
                })
                .collect(),
        }),
        ("[a-z ?]{1,8}", proptest::collection::vec("[a-z]{1,4}", 0..3)).prop_map(
            |(prompt, values)| SurfaceKind::Question {
                prompt,
                choices: values
                    .into_iter()
                    .map(|value| Choice {
                        label: value.clone(),
                        value,
                    })
                    .collect(),
                multi: false,
                free: true,
                default: None,
                deadline_ms: None,
            }
        ),
        proptest::collection::vec("[a-z]{1,4}", 0..3).prop_map(|names| SurfaceKind::Form {
            fields: names
                .into_iter()
                .map(|name| Field {
                    label: name.clone(),
                    name,
                    kind: FieldKind::Text {},
                    required: false,
                    default: None,
                })
                .collect(),
            submit: "go".into(),
        }),
        ("[a-z#* ]{0,10}", any::<bool>())
            .prop_map(|(value, complete)| SurfaceKind::Markdown { value, complete }),
    ]
}

/// Leaves, then `stack` and `custom` layered on top — the two recursive shapes
/// that WIT cannot express and that the arena therefore has to carry.
fn any_surface() -> impl Strategy<Value = Surface> {
    let leaf = (
        leaf_kind(),
        proptest::option::of(Just(Status::Running)),
        proptest::option::of(any::<u128>()),
    )
        .prop_map(|(kind, status, id)| Surface {
            id: id
                .map(uuid::Uuid::from_u128)
                .map(orrery_proto::ids::SurfaceId::from_uuid),
            status,
            kind,
        });

    leaf.prop_recursive(5, 48, 4, |inner| {
        prop_oneof![
            (
                proptest::collection::vec(inner.clone(), 0..4),
                proptest::option::of("[a-z ]{1,6}"),
                any::<bool>(),
            )
                .prop_map(|(children, title, collapsed)| Surface::new(SurfaceKind::Stack {
                    dir: StackDir::Column,
                    title,
                    collapsed,
                    children,
                })),
            (inner, "[a-z]{1,4}", "[a-z]{1,4}").prop_map(|(fallback, ext, name)| Surface::new(
                SurfaceKind::Custom {
                    kind: format!("{ext}.{name}"),
                    payload: serde_json::json!({ "n": 1, "s": name }),
                    fallback: Box::new(fallback),
                }
            )),
        ]
    })
}

// --- the freeze -----------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// `flatten` then `rebuild` is the identity, deep stacks and nested custom
    /// fallbacks included.
    #[test]
    fn round_trips(surface in any_surface()) {
        let arena = flatten(&surface);
        let back = rebuild(&arena).expect("a flattened surface rebuilds");
        prop_assert_eq!(back, surface);
    }

    /// The arena is acyclic *by construction*: every child index is strictly
    /// greater than its parent's. Guests rely on this to walk without a visited
    /// set, so it is part of the frozen contract.
    #[test]
    fn children_always_point_forward(surface in any_surface()) {
        let arena = flatten(&surface);
        for (i, node) in arena.nodes.iter().enumerate() {
            for &child in &node.children {
                prop_assert!(child as usize > i, "child {} does not follow parent {}", child, i);
            }
        }
    }
}

#[test]
fn rejects_cycles() {
    // Hand-built: node 0 is a stack whose child is itself.
    let arena = SurfaceArena {
        nodes: vec![SurfaceNode {
            kind: NodeKind::Stack,
            id: None,
            status: None,
            payload: r#"{"t":"stack","dir":"column","collapsed":false}"#.into(),
            children: vec![0],
        }],
        root: 0,
    };
    assert!(matches!(
        rebuild(&arena),
        Err(ArenaError::BackwardChild { .. })
    ));
}

#[test]
fn rejects_longer_cycles() {
    let stack = |child: u32| SurfaceNode {
        kind: NodeKind::Stack,
        id: None,
        status: None,
        payload: r#"{"t":"stack","dir":"column","collapsed":false}"#.into(),
        children: vec![child],
    };
    let arena = SurfaceArena {
        nodes: vec![stack(1), stack(0)],
        root: 0,
    };
    assert!(matches!(
        rebuild(&arena),
        Err(ArenaError::BackwardChild { .. })
    ));
}

#[test]
fn rejects_dangling_indices() {
    let arena = SurfaceArena {
        nodes: vec![SurfaceNode {
            kind: NodeKind::Stack,
            id: None,
            status: None,
            payload: r#"{"t":"stack","dir":"column","collapsed":false}"#.into(),
            children: vec![7],
        }],
        root: 0,
    };
    assert!(matches!(
        rebuild(&arena),
        Err(ArenaError::DanglingChild { index: 7, .. })
    ));
}

#[test]
fn rejects_a_dangling_root() {
    let arena = SurfaceArena {
        nodes: Vec::new(),
        root: 0,
    };
    assert!(matches!(rebuild(&arena), Err(ArenaError::DanglingRoot { .. })));
}

#[test]
fn rejects_a_kind_tag_that_disagrees_with_the_payload() {
    // The enum discriminant is a convenience for guests that do not want to
    // parse JSON to switch. It must agree with the payload's tag or the two
    // halves of the node say different things.
    let arena = SurfaceArena {
        nodes: vec![SurfaceNode {
            kind: NodeKind::Table,
            id: None,
            status: None,
            payload: r#"{"t":"text","value":"hi"}"#.into(),
            children: vec![],
        }],
        root: 0,
    };
    assert!(matches!(
        rebuild(&arena),
        Err(ArenaError::KindDisagreesWithPayload { .. })
    ));
}

#[test]
fn custom_must_carry_exactly_one_fallback_child() {
    let arena = SurfaceArena {
        nodes: vec![SurfaceNode {
            kind: NodeKind::Custom,
            id: None,
            status: None,
            payload: r#"{"t":"custom","kind":"x.y","payload":null}"#.into(),
            children: vec![],
        }],
        root: 0,
    };
    assert!(matches!(
        rebuild(&arena),
        Err(ArenaError::CustomNeedsOneFallback { .. })
    ));
}

#[test]
fn a_leaf_carries_no_children() {
    let arena = SurfaceArena {
        nodes: vec![
            SurfaceNode {
                kind: NodeKind::Text,
                id: None,
                status: None,
                payload: r#"{"t":"text","value":"hi"}"#.into(),
                children: vec![1],
            },
            SurfaceNode {
                kind: NodeKind::Text,
                id: None,
                status: None,
                payload: r#"{"t":"text","value":"there"}"#.into(),
                children: vec![],
            },
        ],
        root: 0,
    };
    assert!(matches!(
        rebuild(&arena),
        Err(ArenaError::LeafWithChildren { .. })
    ));
}

/// A guest that only ever emits a table should not have to know about ids,
/// statuses or the JSON envelope. This asserts the *shape* a guest sees.
#[test]
fn a_table_flattens_to_one_childless_node() {
    let surface = Surface::new(SurfaceKind::Table {
        columns: vec!["module".into(), "reason".into()],
        rows: vec![vec![
            Cell {
                text: "core".into(),
                style: None,
            },
            Cell {
                text: "changed".into(),
                style: None,
            },
        ]],
    });
    let arena = flatten(&surface);
    assert_eq!(arena.nodes.len(), 1);
    assert_eq!(arena.root, 0);
    assert_eq!(arena.nodes[0].kind, NodeKind::Table);
    assert!(arena.nodes[0].children.is_empty());
    assert_eq!(rebuild(&arena).unwrap(), surface);
}

/// Depth-first, parent before child: the order the arena is written in is part
/// of the contract too, because a guest streaming nodes relies on it.
#[test]
fn nodes_are_written_parent_first_depth_first() {
    let leaf = |v: &str| Surface::new(SurfaceKind::Text {
        value: v.into(),
        style: None,
    });
    let surface = Surface::new(SurfaceKind::Stack {
        dir: StackDir::Column,
        title: None,
        collapsed: false,
        children: vec![
            Surface::new(SurfaceKind::Stack {
                dir: StackDir::Row,
                title: None,
                collapsed: false,
                children: vec![leaf("a"), leaf("b")],
            }),
            leaf("c"),
        ],
    });
    let arena = flatten(&surface);
    let kinds: Vec<NodeKind> = arena.nodes.iter().map(|n| n.kind).collect();
    assert_eq!(
        kinds,
        vec![
            NodeKind::Stack,
            NodeKind::Stack,
            NodeKind::Text,
            NodeKind::Text,
            NodeKind::Text
        ]
    );
    assert_eq!(arena.nodes[0].children, vec![1, 4]);
    assert_eq!(arena.nodes[1].children, vec![2, 3]);
}
