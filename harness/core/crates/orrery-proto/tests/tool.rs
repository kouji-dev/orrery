//! The one `ToolDescriptor`: a wire type, and proto is its home.

use orrery_proto::ToolDescriptor;

fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "ripgrep.search".to_owned(),
        description: "Search the workspace".to_owned(),
        input_schema: serde_json::json!({ "type": "object" }),
        atomic: false,
    }
}

#[test]
fn round_trips_as_json() {
    let json = serde_json::to_value(descriptor()).expect("serialises");
    assert_eq!(json["name"], "ripgrep.search");
    assert_eq!(json["atomic"], false);
    let back: ToolDescriptor = serde_json::from_value(json).expect("deserialises");
    assert_eq!(back, descriptor());
}

#[test]
fn round_trips_over_cbor() {
    let mut bytes = Vec::new();
    ciborium::into_writer(&descriptor(), &mut bytes).expect("encodes");
    let back: ToolDescriptor = ciborium::from_reader(&bytes[..]).expect("decodes");
    assert_eq!(back, descriptor());
}

#[test]
fn has_a_schema() {
    let schema = serde_json::to_value(schemars::schema_for!(ToolDescriptor)).expect("serialises");
    let props = &schema["properties"];
    for field in ["name", "description", "input_schema", "atomic"] {
        assert!(props.get(field).is_some(), "schema is missing {field}");
    }
}
