//! Per-node blake3, so an unchanged subtree costs one comparison.
//!
//! The hash of a node folds in the hashes of its child *surfaces* rather than
//! re-serialising them, so building the tree is one pass over the surface and
//! not one pass per node. That is what makes [`crate::diff`]'s skip O(1) per
//! child instead of O(size of child).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use orrery_proto::{Surface, SurfaceKind};

/// One node's digest.
pub type NodeHash = [u8; 32];

/// How many nodes were hashed, for a test that wants to prove a subtree was
/// never walked.
///
/// Cheap enough to leave switched on: one relaxed increment per node.
#[derive(Clone, Debug, Default)]
pub struct HashCounter(Arc<AtomicU64>);

impl HashCounter {
    /// A counter at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many nodes have been hashed through this counter.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    fn bump(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

/// The digest of one surface, and of every surface nested inside it.
///
/// `children` mirrors the child *surfaces* of the node — a stack's children in
/// order, or a custom surface's single fallback — so a hash tree can be walked
/// alongside the surface it describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashTree {
    /// This node's digest, covering its whole subtree.
    pub hash: NodeHash,
    /// One entry per child surface, in order.
    pub children: Vec<HashTree>,
}

impl HashTree {
    /// The child at `index`, when there is one.
    #[must_use]
    pub fn child(&self, index: usize) -> Option<&HashTree> {
        self.children.get(index)
    }
}

/// Hash a surface and everything under it.
#[must_use]
pub fn hash_tree(surface: &Surface) -> HashTree {
    hash_tree_counted(surface, &HashCounter::new())
}

/// Hash a surface, counting the nodes visited.
#[must_use]
pub fn hash_tree_counted(surface: &Surface, counter: &HashCounter) -> HashTree {
    counter.bump();
    let children: Vec<HashTree> = match &surface.kind {
        SurfaceKind::Stack { children, .. } => children
            .iter()
            .map(|c| hash_tree_counted(c, counter))
            .collect(),
        SurfaceKind::Custom { fallback, .. } => vec![hash_tree_counted(fallback, counter)],
        _ => Vec::new(),
    };

    let mut hasher = blake3::Hasher::new();
    hasher.update(&shallow_bytes(surface));
    for child in &children {
        hasher.update(&child.hash);
    }
    HashTree {
        hash: *hasher.finalize().as_bytes(),
        children,
    }
}

/// Everything about a node except the child surfaces folded in separately.
///
/// Serialising through `serde_json` rather than hand-writing a field walk means
/// a variant added to [`SurfaceKind`] is covered the day it lands, instead of
/// silently hashing to the same value as its neighbour.
fn shallow_bytes(surface: &Surface) -> Vec<u8> {
    let mut value = match serde_json::to_value(surface) {
        Ok(value) => value,
        // A surface that will not serialise cannot be sent either; hashing it
        // to a constant is harmless, because the differ will find every field
        // unequal and emit a replace.
        Err(_) => serde_json::Value::Null,
    };
    if let Some(kind) = value
        .get_mut("kind")
        .and_then(serde_json::Value::as_object_mut)
    {
        kind.remove("children");
        kind.remove("fallback");
    }
    serde_json::to_vec(&value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use orrery_proto::{StackDir, Surface, SurfaceKind, TextStyle};

    use super::{HashCounter, hash_tree, hash_tree_counted};

    fn text(value: &str) -> Surface {
        Surface::new(SurfaceKind::Text {
            value: value.to_owned(),
            style: None,
        })
    }

    fn stack(children: Vec<Surface>) -> Surface {
        Surface::new(SurfaceKind::Stack {
            dir: StackDir::Column,
            title: None,
            collapsed: false,
            children,
        })
    }

    #[test]
    fn the_same_surface_hashes_the_same() {
        assert_eq!(hash_tree(&text("a")).hash, hash_tree(&text("a")).hash);
    }

    #[test]
    fn a_changed_leaf_changes_every_hash_above_it() {
        let before = hash_tree(&stack(vec![text("a"), text("b")]));
        let after = hash_tree(&stack(vec![text("a"), text("c")]));
        assert_ne!(before.hash, after.hash, "the root moved");
        assert_eq!(
            before.children[0].hash, after.children[0].hash,
            "and the untouched child did not"
        );
        assert_ne!(before.children[1].hash, after.children[1].hash);
    }

    #[test]
    fn a_style_is_part_of_the_hash() {
        let styled = Surface::new(SurfaceKind::Text {
            value: "a".into(),
            style: Some(TextStyle::Error),
        });
        assert_ne!(hash_tree(&text("a")).hash, hash_tree(&styled).hash);
    }

    #[test]
    fn hashing_is_one_visit_per_node() {
        let counter = HashCounter::new();
        let _ = hash_tree_counted(&stack(vec![text("a"), stack(vec![text("b")])]), &counter);
        assert_eq!(counter.count(), 4, "root, a, inner stack, b");
    }
}
