//! Every surface variant round-trips through its fixture, and `custom` cannot
//! be spelled without a fallback.

mod surface {
    use orrery_proto::{Status, Surface, SurfaceId, SurfaceKind, SurfacePatch};

    /// One fixture per variant of `SurfaceKind`. Adding a variant without
    /// adding a fixture fails `covers_every_variant` below.
    const VARIANTS: [&str; 12] = [
        "text", "table", "tree", "diff", "progress", "stream", "task", "question", "form", "stack",
        "markdown", "custom",
    ];

    fn fixture(name: &str) -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
        let file = format!("{path}surface-{name}.json");
        let raw = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("missing fixture {file}: {e}"));
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{file} is not JSON: {e}"))
    }

    #[test]
    fn every_variant_round_trips() {
        for name in VARIANTS {
            let json = fixture(name);
            let surface: Surface = serde_json::from_value(json.clone())
                .unwrap_or_else(|e| panic!("surface-{name}.json did not deserialize: {e}"));
            let back = serde_json::to_value(&surface).unwrap();
            assert_eq!(back, json, "surface-{name}.json did not round-trip");
            surface
                .validate()
                .unwrap_or_else(|e| panic!("surface-{name}.json failed validation: {e}"));
        }
    }

    #[test]
    fn covers_every_variant() {
        for name in VARIANTS {
            let surface: Surface = serde_json::from_value(fixture(name)).unwrap();
            let tag = serde_json::to_value(&surface.kind).unwrap();
            assert_eq!(tag["t"], serde_json::json!(name));
        }
    }

    #[test]
    fn custom_requires_fallback() {
        let no_fallback = serde_json::json!({
            "kind": { "t": "custom", "kind": "buildgraph.graph", "payload": {} }
        });
        assert!(
            serde_json::from_value::<Surface>(no_fallback).is_err(),
            "`custom` without a fallback must not deserialize"
        );
    }

    #[test]
    fn custom_kind_must_be_namespaced() {
        let bare = serde_json::json!({
            "kind": {
                "t": "custom",
                "kind": "graph",
                "payload": {},
                "fallback": { "kind": { "t": "text", "value": "a graph" } }
            }
        });
        let surface: Surface = serde_json::from_value(bare).unwrap();
        assert!(
            surface.validate().is_err(),
            "`custom.kind` must be `<ext>.<name>`"
        );
    }

    #[test]
    fn validation_reaches_into_children() {
        let nested = serde_json::json!({
            "kind": {
                "t": "stack",
                "dir": "column",
                "collapsed": false,
                "children": [{
                    "kind": {
                        "t": "custom",
                        "kind": "nope",
                        "payload": {},
                        "fallback": { "kind": { "t": "text", "value": "x" } }
                    }
                }]
            }
        });
        let surface: Surface = serde_json::from_value(nested).unwrap();
        assert!(
            surface.validate().is_err(),
            "validation must recurse into a stack"
        );
    }

    #[test]
    fn patches_are_op_tagged() {
        let id = SurfaceId::new();
        let patch = SurfacePatch::Append {
            id,
            text: "chunk".into(),
        };
        let json = serde_json::to_value(&patch).unwrap();
        assert_eq!(json["op"], serde_json::json!("append"));
        assert_eq!(serde_json::from_value::<SurfacePatch>(json).unwrap(), patch);

        let remove = SurfacePatch::Remove { id };
        assert_eq!(
            serde_json::to_value(&remove).unwrap()["op"],
            serde_json::json!("remove")
        );
    }

    #[test]
    fn status_is_optional_and_kebab() {
        let surface = Surface {
            id: None,
            status: Some(Status::Running),
            kind: SurfaceKind::Text {
                value: "hi".into(),
                style: None,
            },
        };
        let json = serde_json::to_value(&surface).unwrap();
        assert_eq!(json["status"], serde_json::json!("running"));
        assert!(json.get("id").is_none(), "an absent id must not be written");
    }
}
