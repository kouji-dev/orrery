//! `orrery-tools` does not own a `ToolDescriptor`; it re-exports proto's.

use orrery_tools::ToolDescriptor;

#[test]
fn is_protos_type() {
    // Compiles only if the two names are the same type.
    let from_proto = orrery_proto::ToolDescriptor {
        name: "builtin.read".to_owned(),
        description: "Read a file".to_owned(),
        input_schema: serde_json::json!({ "type": "object" }),
        atomic: true,
    };
    let here: ToolDescriptor = from_proto;
    assert!(here.atomic);
}
