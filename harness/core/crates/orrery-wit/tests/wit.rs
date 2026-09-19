//! `wit-check`: the `.wit` parses, and the frozen shapes in it match the Rust
//! side that rebuilds them.
//!
//! Run by `cargo xtask wit-check`.

use orrery_wit::arena::NodeKind;
use wit_parser::{Resolve, TypeDefKind};

/// Windows checkouts normalise line endings; the comparison must not care.
const CRLF: &str = "\r\n";
/// The newline a normalised comparison uses.
const LF: &str = "\n";

fn resolve() -> (Resolve, wit_parser::PackageId) {
    let mut resolve = Resolve::default();
    let id = resolve
        .push_str("orrery-extension.wit", orrery_wit::WORLD)
        .expect("harness/wit/orrery-extension.wit parses");
    (resolve, id)
}

#[test]
fn the_world_parses() {
    let (resolve, pkg) = resolve();
    let package = &resolve.packages[pkg];
    assert_eq!(package.name.to_string(), orrery_wit::PACKAGE);
    assert!(
        package.worlds.contains_key("orrery-extension"),
        "the world is named `orrery-extension`: {:?}",
        package.worlds.keys().collect::<Vec<_>>()
    );
}

#[test]
fn the_embedded_world_matches_the_file_on_disk() {
    let on_disk = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../wit/orrery-extension.wit"),
    )
    .expect("the .wit is where the crate says it is");
    assert_eq!(
        on_disk.replace("\r\n", "\n"),
        orrery_wit::WORLD.replace("\r\n", "\n"),
        "the embedded world and the file have drifted"
    );
}

/// The guest SDK ships its own copy, and `cargo package` is why.
///
/// `orrery-guest` is the crate every community wasm-extension author depends
/// on, so it has to package. `wit_bindgen::generate!` used to point at
/// `../../../wit` — outside the crate directory, which `cargo package` does not
/// put in the tarball, so the published crate could not build at all.
///
/// The fix is a vendored copy under `orrery-guest/wit/`, and this test is what
/// keeps it from being a second source of truth: it must be the canonical file,
/// byte for byte. Edit `harness/wit/orrery-extension.wit` and copy it over;
/// `cargo xtask wit-check` fails until you do.
#[test]
fn the_guest_sdks_vendored_copy_is_the_canonical_file() {
    let vendored = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../extensions/crates/orrery-guest/wit/orrery-extension.wit");
    let copy = std::fs::read_to_string(&vendored).unwrap_or_else(|e| {
        panic!(
            "the guest SDK's vendored `.wit` is missing: {e}; \
             copy harness/wit/orrery-extension.wit to {}",
            vendored.display()
        )
    });
    assert_eq!(
        copy.replace(CRLF, LF),
        orrery_wit::WORLD.replace(CRLF, LF),
        "the guest SDK's vendored `.wit` has drifted from harness/wit/orrery-extension.wit"
    );
}

/// The freeze, checked from the other side: the WIT enum and the Rust enum name
/// the same twelve kinds. A variant added to one and not the other is how a
/// guest and a host come to disagree about what a node is.
#[test]
fn node_kind_matches_the_rust_enum() {
    let (resolve, pkg) = resolve();
    let iface = resolve.packages[pkg].interfaces["surfaces"];
    let ty = resolve.interfaces[iface].types["node-kind"];
    let TypeDefKind::Enum(e) = &resolve.types[ty].kind else {
        panic!("node-kind is an enum");
    };
    let in_wit: Vec<&str> = e.cases.iter().map(|c| c.name.as_str()).collect();

    let in_rust = [
        NodeKind::Text,
        NodeKind::Table,
        NodeKind::Tree,
        NodeKind::Diff,
        NodeKind::Progress,
        NodeKind::Stream,
        NodeKind::Task,
        NodeKind::Question,
        NodeKind::Form,
        NodeKind::Stack,
        NodeKind::Markdown,
        NodeKind::Custom,
    ]
    .map(NodeKind::tag);

    assert_eq!(in_wit, in_rust.to_vec(), "node-kind drifted");
}

#[test]
fn a_surface_node_has_exactly_the_frozen_fields() {
    let (resolve, pkg) = resolve();
    let iface = resolve.packages[pkg].interfaces["surfaces"];
    let ty = resolve.interfaces[iface].types["surface-node"];
    let TypeDefKind::Record(r) = &resolve.types[ty].kind else {
        panic!("surface-node is a record");
    };
    let fields: Vec<&str> = r.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(fields, ["kind", "id", "status", "payload", "children"]);
}

/// The two structural rules the world exists to enforce, asserted on the world
/// itself rather than trusted to a comment.
#[test]
fn no_import_carries_a_capability_token() {
    let (resolve, pkg) = resolve();
    let iface = resolve.packages[pkg].interfaces["broker"];
    for (name, func) in &resolve.interfaces[iface].functions {
        for (param, _) in &func.params {
            let p = param.to_lowercase();
            assert!(
                !p.contains("token") && !p.contains("cap") && !p.contains("grant"),
                "broker.{name} takes `{param}`: the guest must never hold a token"
            );
        }
    }
}

#[test]
fn every_broker_function_can_fail_with_a_value() {
    let (resolve, pkg) = resolve();
    let iface = resolve.packages[pkg].interfaces["broker"];
    assert_eq!(resolve.interfaces[iface].functions.len(), 5);
    for (name, func) in &resolve.interfaces[iface].functions {
        let result = func.result.expect("every broker function returns a result");
        let wit_parser::Type::Id(id) = result else {
            panic!("broker.{name} returns a bare type, not a result");
        };
        assert!(
            matches!(resolve.types[id].kind, TypeDefKind::Result(_)),
            "broker.{name} must return `result<_, error>`: a denial is a value, \
             never a trap"
        );
    }
}
