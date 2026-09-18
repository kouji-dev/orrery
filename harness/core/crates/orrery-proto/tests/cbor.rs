//! Every fixture survives a trip through CBOR.
//!
//! This is the test that fails loudly if anyone adds a **newtype variant** to
//! an internally tagged enum. Serde can fake internal tagging for a newtype
//! variant in JSON by flattening a map, and cannot do it at all for a
//! non-map payload — so a change that looks fine in JSON silently stops
//! serialising over the binary transport. Running every fixture through
//! `ciborium` catches it at the point it is introduced instead of at the point
//! a client first fails to parse a frame.

use std::path::Path;

use orrery_proto::{Event, Request, Surface};

fn fixtures(prefix: &str) -> Vec<(String, serde_json::Value)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut out: Vec<(String, serde_json::Value)> = std::fs::read_dir(dir)
        .expect("fixtures/ is missing")
        .map(|e| e.expect("unreadable fixture").path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix))
        .map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            let raw = std::fs::read_to_string(&p).unwrap();
            (
                name,
                serde_json::from_str(&raw).expect("fixture is not JSON"),
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!out.is_empty(), "no fixtures matched `{prefix}`");
    out
}

/// JSON → `T` → CBOR bytes → `T` → JSON, and the two JSONs must be equal.
fn round_trip<T>(name: &str, json: &serde_json::Value)
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let typed: T = serde_json::from_value(json.clone())
        .unwrap_or_else(|e| panic!("{name}: JSON did not deserialize: {e}"));

    let mut bytes = Vec::new();
    ciborium::into_writer(&typed, &mut bytes)
        .unwrap_or_else(|e| panic!("{name}: did not serialise to CBOR: {e}"));

    let back: T = ciborium::from_reader(bytes.as_slice())
        .unwrap_or_else(|e| panic!("{name}: did not deserialize from CBOR: {e}"));

    let again = serde_json::to_value(&back).unwrap();
    assert_eq!(&again, json, "{name}: CBOR round-trip changed the value");
}

mod cbor {
    use super::{Event, Request, Surface, fixtures, round_trip};

    #[test]
    fn frames_round_trip() {
        for (name, json) in fixtures("frame-request-") {
            round_trip::<Request>(&name, &json);
        }
        for (name, json) in fixtures("frame-event-") {
            round_trip::<Event>(&name, &json);
        }
    }

    #[test]
    fn surfaces_round_trip() {
        for (name, json) in fixtures("surface-") {
            round_trip::<Surface>(&name, &json);
        }
    }
}
