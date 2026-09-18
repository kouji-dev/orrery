//! The default view bindings: assistant text, tool started and settled, consent prompts and errors.
//!
//! Implementation plan: `harness/docs/plans/09-surfaces.md`
//!
//! # Why this is an extension and not a built-in
//!
//! Unbound loop events are hidden. A client with nothing bound would therefore
//! show nothing at all, which is broken rather than minimal — so *something*
//! has to ship the floor. The tempting shortcut is to install it inside the
//! kernel, and then there is one view path for the five bindings everybody
//! needs and a different one for everybody else's.
//!
//! So the floor loads the way a third-party bundle does: an `orrery.toml`, an
//! [`ExtensionManifest`](orrery_ext_api::ExtensionManifest) parsed by the same
//! parser, a load through [`orrery_ext_api::testing`], an entry in the ledger,
//! and a deny rule that switches it off. Proving the mechanism on the one case
//! that most tempts a shortcut is the point of the exercise — and this crate
//! depends on nothing a community author could not depend on.
//!
//! The binding *content* lives in [`orrery_ext_api::floor`], next to the
//! vocabulary it is written in so the two cannot drift. Nothing in the kernel
//! calls it; this extension is the only thing that does.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use async_trait::async_trait;
use orrery_ext_api::{
    CallCtx, HostError, NativeExtension, ToolDef, ViewBinding, ViewRegistry, floor, floor_kinds,
};
use orrery_proto::Outcome;
use serde_json::Value;

/// This extension's `orrery.toml`, compiled in.
pub const MANIFEST: &str = include_str!("../orrery.toml");

/// The floor bindings, shipped as an extension.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultViews;

impl DefaultViews {
    /// The bindings this extension contributes.
    ///
    /// Assistant text, tool started, tool settled, consent and errors — in the
    /// order the manifest promises them.
    #[must_use]
    pub fn views(&self) -> Vec<ViewBinding> {
        floor()
    }

    /// A registry with the floor installed, which is what a session gets when
    /// this extension loads and nothing else has bound anything.
    #[must_use]
    pub fn registry(&self) -> ViewRegistry {
        let mut registry = ViewRegistry::new();
        registry.bind_all(self.views());
        registry
    }

    /// The names this extension promises in its manifest, as the ledger spells
    /// them.
    #[must_use]
    pub fn promised() -> Vec<String> {
        floor_kinds().iter().map(ToString::to_string).collect()
    }
}

#[async_trait]
impl NativeExtension for DefaultViews {
    fn manifest(&self) -> &str {
        MANIFEST
    }

    fn manifest_path(&self) -> &str {
        "harness/extensions/crates/orrery-ext-views-default/orrery.toml"
    }

    /// None. This extension contributes views, and a view is not a tool: it is
    /// never offered to the model and never called with an input.
    fn tools(&self) -> Vec<ToolDef> {
        Vec::new()
    }

    async fn call(&self, tool: &str, _input: Value, _ctx: &CallCtx) -> Result<Outcome, HostError> {
        Err(HostError::NoSuchTool {
            ext: orrery_proto::ExtId::new("views-default").expect("a literal ext id"),
            tool: tool.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use orrery_ext_api::ExtensionManifest;

    use super::{DefaultViews, MANIFEST};

    /// What the manifest promises is what the code contributes. A collection
    /// member the manifest names and the code does not have costs exactly
    /// itself at load time — better to catch it here.
    #[test]
    fn the_manifest_and_the_code_agree() {
        let manifest = ExtensionManifest::from_toml_str(MANIFEST, "orrery.toml").expect("parses");
        let promised = manifest.provides.views.clone();
        assert_eq!(
            promised,
            DefaultViews::promised(),
            "the manifest's `views` list is the floor, in order"
        );
        let bound: Vec<String> = DefaultViews
            .views()
            .iter()
            .map(|b| b.event.to_string())
            .collect();
        assert_eq!(bound, promised);
    }
}
