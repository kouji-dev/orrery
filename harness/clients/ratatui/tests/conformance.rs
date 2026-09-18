//! Task 7: every scenario, through this renderer.
//!
//! The same fixtures the SDK and the `json` client run. A scenario only the SDK
//! checks is a scenario that has stopped describing the clients.

use orrery_client::conformance::{fixtures_dir, load_all, run};
use orrery_client_ratatui::app::App;
use orrery_client_ratatui::scrollback::{Recording, lines_of};
use orrery_client_ratatui::testing::frames_of;
use orrery_client_ratatui::widgets;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// Every scenario renders: the store agrees with the fixture, nothing panics,
/// and the screen it produces is snapshotted.
#[test]
fn every_scenario_renders() {
    let scenarios = load_all(&fixtures_dir()).expect("the fixtures load");
    assert!(
        scenarios.len() >= 16,
        "the conformance set is the contract; it should not shrink: {}",
        scenarios.len()
    );
    for scenario in &scenarios {
        // First the store: if this renderer disagreed with the SDK about what
        // the events mean, the picture would be wrong for a reason that has
        // nothing to do with drawing.
        run(scenario).unwrap_or_else(|e| panic!("{e}"));

        let mut app = App::new(60);
        let mut sink = Recording::new(60);
        for frame in frames_of(scenario) {
            app.apply(&frame);
            app.flush_scrollback(&mut sink).expect("prints");
        }
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 14));
        app.draw(&mut buf);

        let mut screen = String::new();
        if !sink.blocks().is_empty() {
            screen.push_str("── scrollback ──\n");
            screen.push_str(&sink.text());
            screen.push('\n');
        }
        screen.push_str("── live ──\n");
        screen.push_str(&lines_of(&buf).join("\n"));
        insta::assert_snapshot!(scenario.name.clone(), screen);
    }
}

/// Every core surface has a widget.
///
/// `SurfaceKind` is `#[non_exhaustive]`, so no match written in this crate can
/// be exhaustive — the compiler requires a `_` arm and there is no attribute
/// that removes it. The plan asked for "adding a variant breaks the build
/// here"; this is the closest thing that is true, and it is not weaker in
/// practice: the variant tags come out of `SurfaceKind`'s own derived JSON
/// schema, so a new variant is in the schema the moment it is declared, and
/// this test is red in the same commit that declares it.
#[test]
fn every_core_surface_has_a_widget() {
    let schema = serde_json::to_value(schemars::schema_for!(orrery_proto::SurfaceKind))
        .expect("the schema serialises");
    // The root's own `oneOf` and nothing below it: `Field`'s `FieldKind` is
    // internally tagged too and lives in the same document, and a `choice`
    // field is not a core surface.
    let variants = schema
        .get("oneOf")
        .and_then(serde_json::Value::as_array)
        .expect("SurfaceKind's schema is a tagged union");
    let mut tags: Vec<String> = variants
        .iter()
        .filter_map(|v| tag_values(v.get("properties")?.get("t")?))
        .flatten()
        .collect();
    tags.sort();
    tags.dedup();
    assert!(
        tags.len() >= 12,
        "the tags did not come out of the schema as expected: {tags:?}"
    );

    let missing: Vec<&String> = tags
        .iter()
        .filter(|tag| !widgets::HANDLED.contains(&tag.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "SurfaceKind grew {missing:?} and this renderer has no widget for it. \
         Every core surface must render: one that cannot is not a renderer. \
         Add the widget in src/widgets/, add its tag to widgets::HANDLED, and \
         add a snapshot."
    );

    let stale: Vec<&&str> = widgets::HANDLED
        .iter()
        .filter(|tag| !tags.iter().any(|t| t == *tag))
        .collect();
    assert!(
        stale.is_empty(),
        "HANDLED names {stale:?}, which SurfaceKind no longer has"
    );
}

/// Pull the tag values out of one variant's `t` property.
fn tag_values(value: &serde_json::Value) -> Option<Vec<String>> {
    let map = value.as_object()?;
    if let Some(one) = map.get("const").and_then(|v| v.as_str()) {
        return Some(vec![one.to_owned()]);
    }
    let list = map.get("enum")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
    )
}

/// A custom surface always draws its fallback in the TUI, in every scenario
/// that has one. §6.3, asserted rather than asserted-in-a-comment.
#[test]
fn custom_surfaces_always_show_their_fallback() {
    let scenario = load_all(&fixtures_dir())
        .expect("the fixtures load")
        .into_iter()
        .find(|s| s.name == "custom-with-fallback")
        .expect("the custom scenario is in the set");
    let mut app = App::new(60);
    let mut sink = Recording::new(60);
    for frame in frames_of(&scenario) {
        app.apply(&frame);
        app.flush_scrollback(&mut sink).expect("prints");
    }
    assert!(
        sink.text().contains("proto -> agui -> kernel"),
        "the fallback is what the TUI shows: {}",
        sink.text()
    );
}
