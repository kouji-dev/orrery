//! The SDK's one job, checked on the host: the author builds a tree, and a
//! valid arena comes out.

use orrery_guest::ui;

#[test]
fn hides_the_arena() {
    // Not an index in sight — which is the whole test.
    let tree = ui::section(
        "report",
        vec![
            ui::text("two things happened"),
            ui::table(&["module", "reason"], &[vec!["core".into(), "changed".into()]]),
        ],
    );

    let arena = ui::flatten(&tree);
    assert_eq!(arena.root, 0);
    assert_eq!(arena.nodes.len(), 3);

    // Parent first, depth first, children strictly after their parent: the
    // invariants the host checks on arrival.
    assert_eq!(arena.nodes[0].children.as_slice(), &[1, 2]);
    for (i, node) in arena.nodes.iter().enumerate() {
        for &child in node.children.as_slice() {
            assert!(child as usize > i, "child {child} does not follow parent {i}");
        }
    }
    // Only the stack carries children.
    assert!(arena.nodes[1].children.as_slice().is_empty());
    assert!(arena.nodes[2].children.as_slice().is_empty());

    // The kind discriminant agrees with the payload's tag, which the host
    // rejects an arena for getting wrong.
    assert!(arena.nodes[0].payload.contains("\"t\":\"stack\""));
    assert!(arena.nodes[1].payload.contains("\"t\":\"text\""));
    assert!(arena.nodes[2].payload.contains("\"t\":\"table\""));
}

#[test]
fn a_custom_carries_exactly_one_fallback() {
    let tree = ui::custom("demo.thing", "{\"n\":1}", ui::text("a thing"));
    let arena = ui::flatten(&tree);
    assert_eq!(arena.nodes.len(), 2);
    assert_eq!(arena.nodes[0].children.as_slice(), &[1]);
}

#[test]
fn strings_are_escaped() {
    let arena = ui::flatten(&ui::text("a \"quote\" and a \\ and a \n"));
    let payload = &arena.nodes[0].payload;
    assert!(payload.contains(r#"\"quote\""#), "{payload}");
    assert!(payload.contains(r"\\"), "{payload}");
    assert!(payload.contains(r"\n"), "{payload}");
    // And nothing raw got through: a payload with a real newline in it is not
    // JSON, and the host would reject the whole arena.
    assert!(!payload.contains('\n'), "{payload}");
}

#[test]
fn deep_nesting_still_points_forward() {
    let mut node = ui::text("leaf");
    for i in 0..32 {
        node = ui::section(&format!("level {i}"), vec![node]);
    }
    let arena = ui::flatten(&node);
    assert_eq!(arena.nodes.len(), 33);
    for (i, n) in arena.nodes.iter().enumerate() {
        for &child in n.children.as_slice() {
            assert!(child as usize > i);
        }
    }
}
