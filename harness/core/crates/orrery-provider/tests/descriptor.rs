//! `orrery-provider` does not own a `ToolDescriptor`; it re-exports proto's.

use std::sync::Arc;

use orrery_provider::{ModelRequest, ToolDescriptor};

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

    let mut req = ModelRequest::new("m", Arc::from([]), 16);
    req.tools = Arc::from([here.clone()]);
    assert_eq!(req.tools[0], here);
}
