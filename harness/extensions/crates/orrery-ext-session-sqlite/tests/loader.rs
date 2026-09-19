//! The extension goes in through the loader, not only through `build`.
//!
//! `build` is the direct constructor: an embedder that already knows it wants
//! SQLite calls it and gets a store. That is not the same as being an
//! *extension*, and until this crate implemented [`NativeExtension`] it was not
//! one — its `orrery.toml` was never parsed, so nothing in the ledger knew the
//! `session` singleton had a holder and no deny rule could name it.
//!
//! These tests drive the loader's half: the manifest the host would parse, and
//! the contributions the table would record.

use orrery_ext_api::{ExtensionManifest, NativeExtension, RuntimeKind};
use orrery_ext_session_sqlite::SqliteSessions;

#[test]
fn the_manifest_parses_through_the_real_parser() {
    let manifest = ExtensionManifest::from_toml_str(
        SqliteSessions.manifest(),
        SqliteSessions.manifest_path(),
    )
    .expect("the shipped orrery.toml parses");
    assert_eq!(manifest.name.as_str(), "sqlite");
    assert_eq!(manifest.runtime, RuntimeKind::Native);
}

#[test]
fn it_claims_the_session_singleton() {
    let manifest =
        ExtensionManifest::from_toml_str(SqliteSessions.manifest(), SqliteSessions.manifest_path())
            .expect("parses");
    assert_eq!(
        manifest.provides.session.as_deref(),
        Some("sqlite"),
        "the `session` singleton slot is what this extension is for"
    );
}

/// A session store is not a tool. It is never offered to the model and never
/// called with an input, so the tool list is empty on purpose — the same shape
/// `orrery-ext-views-default` has.
#[test]
fn it_contributes_no_tools() {
    assert!(SqliteSessions.tools().is_empty());
}
