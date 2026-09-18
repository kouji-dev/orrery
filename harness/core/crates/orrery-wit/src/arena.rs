//! Translation #9 — a [`Surface`] as a flat arena.
//!
//! # Why
//!
//! WIT has no recursive types. [`Surface`] has two recursive shapes:
//! `stack.children: Vec<Surface>` and `custom.fallback: Box<Surface>`. Neither
//! survives a `.wit` definition, so across the wasm boundary a surface is a
//! **flat arena**: a `list<surface-node>` plus child indices, rebuilt host-side
//! into the real tree.
//!
//! # The frozen shape
//!
//! Every guest language binds against this, so changing it later is the one
//! genuinely expensive mistake available here. What is frozen:
//!
//! 1. A node is `{ kind, id, status, payload, children }`.
//! 2. `kind` is a flat enum over the twelve [`SurfaceKind`] variants. It is a
//!    convenience for guests that want to switch without parsing JSON, and it
//!    **must agree** with the tag inside `payload`.
//! 3. `payload` is the node's own JSON — the serde encoding of its
//!    [`SurfaceKind`] with the recursive fields removed. It keeps serde's `"t"`
//!    tag, so a host can deserialise it after putting the children back.
//! 4. `children` are indices into `nodes` and are **strictly greater than the
//!    node's own index**. The arena is therefore acyclic by construction and a
//!    guest can walk it without a visited set.
//! 5. Nodes are written **parent first, depth first**, so a guest may stream
//!    them and a reader always sees a parent before its children.
//! 6. Only `stack` (any number) and `custom` (exactly one, the fallback) carry
//!    children. Every other kind carries none.
//!
//! [`rebuild`] enforces all of it. It is the trust boundary: the arena arrives
//! from guest memory, so nothing about it is assumed.

use std::collections::BTreeMap;

use orrery_proto::ids::SurfaceId;
use orrery_proto::surface::{Status, Surface, SurfaceKind};
use serde::{Deserialize, Serialize};

/// How deep a rebuilt tree may nest.
///
/// Child indices strictly increase, so an arena cannot loop; it can still be a
/// 100k-node chain, and [`rebuild`] recurses. This bounds the stack instead of
/// trusting the guest not to be a chain.
pub const MAX_DEPTH: usize = 128;

/// Which kind of node this is.
///
/// One variant per [`SurfaceKind`]. The names are serde's kebab-case tags, and
/// the same names appear in the `.wit` `node-kind` enum.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeKind {
    /// [`SurfaceKind::Text`].
    Text,
    /// [`SurfaceKind::Table`].
    Table,
    /// [`SurfaceKind::Tree`].
    Tree,
    /// [`SurfaceKind::Diff`].
    Diff,
    /// [`SurfaceKind::Progress`].
    Progress,
    /// [`SurfaceKind::Stream`].
    Stream,
    /// [`SurfaceKind::Task`].
    Task,
    /// [`SurfaceKind::Question`].
    Question,
    /// [`SurfaceKind::Form`].
    Form,
    /// [`SurfaceKind::Stack`] — carries its children.
    Stack,
    /// [`SurfaceKind::Markdown`].
    Markdown,
    /// [`SurfaceKind::Custom`] — carries exactly one child, its fallback.
    Custom,
}

impl NodeKind {
    /// The serde tag this kind is written as.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            NodeKind::Text => "text",
            NodeKind::Table => "table",
            NodeKind::Tree => "tree",
            NodeKind::Diff => "diff",
            NodeKind::Progress => "progress",
            NodeKind::Stream => "stream",
            NodeKind::Task => "task",
            NodeKind::Question => "question",
            NodeKind::Form => "form",
            NodeKind::Stack => "stack",
            NodeKind::Markdown => "markdown",
            NodeKind::Custom => "custom",
        }
    }

    /// The kind a serde tag names, if it names one.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "text" => NodeKind::Text,
            "table" => NodeKind::Table,
            "tree" => NodeKind::Tree,
            "diff" => NodeKind::Diff,
            "progress" => NodeKind::Progress,
            "stream" => NodeKind::Stream,
            "task" => NodeKind::Task,
            "question" => NodeKind::Question,
            "form" => NodeKind::Form,
            "stack" => NodeKind::Stack,
            "markdown" => NodeKind::Markdown,
            "custom" => NodeKind::Custom,
            _ => return None,
        })
    }

    /// How many children this kind is allowed to carry.
    const fn child_rule(self) -> ChildRule {
        match self {
            NodeKind::Stack => ChildRule::Any,
            NodeKind::Custom => ChildRule::ExactlyOne,
            _ => ChildRule::None,
        }
    }
}

enum ChildRule {
    None,
    ExactlyOne,
    Any,
}

/// One node of the arena. Mirrors `surfaces.surface-node` in the `.wit`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceNode {
    /// What kind of node this is.
    pub kind: NodeKind,
    /// The handle a patch addresses this surface by, as its string form.
    pub id: Option<String>,
    /// How far along it is.
    pub status: Option<Status>,
    /// This node's own JSON, without its recursive fields.
    ///
    /// JSON crosses as a `string`; see `bench/json_cost` for what that costs.
    pub payload: String,
    /// Indices into [`SurfaceArena::nodes`], each strictly greater than this
    /// node's own index.
    pub children: Vec<u32>,
}

/// A whole surface as an arena. Mirrors `surfaces.surface` in the `.wit`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceArena {
    /// Every node, parent first and depth first.
    pub nodes: Vec<SurfaceNode>,
    /// Where to start. Index 0 is not special, but [`flatten`] always writes 0.
    pub root: u32,
}

/// An arena that does not describe a surface.
///
/// Every variant is something a guest could send, so every one is a value the
/// host reports rather than a panic.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArenaError {
    /// `root` is not an index into `nodes`.
    #[error("root index {root} is out of range: the arena has {len} nodes")]
    DanglingRoot {
        /// The offending index.
        root: u32,
        /// How many nodes there are.
        len: usize,
    },
    /// A child index is out of range.
    #[error("node {parent} names child {index}, but the arena has {len} nodes")]
    DanglingChild {
        /// Which node named it.
        parent: usize,
        /// The offending index.
        index: u32,
        /// How many nodes there are.
        len: usize,
    },
    /// A child index does not point forward, so the arena could loop.
    #[error(
        "node {parent} names child {index}, which does not follow it; \
         child indices must be strictly greater so the arena cannot loop"
    )]
    BackwardChild {
        /// Which node named it.
        parent: usize,
        /// The offending index.
        index: u32,
    },
    /// A kind that carries no children carries some.
    #[error("node {node} is `{kind}`, which carries no children, but it names {count}")]
    LeafWithChildren {
        /// Which node.
        node: usize,
        /// What it says it is.
        kind: &'static str,
        /// How many it named.
        count: usize,
    },
    /// A `custom` node did not carry exactly one fallback.
    #[error(
        "node {node} is `custom`, which must carry exactly one child — its \
         fallback — but it names {count}"
    )]
    CustomNeedsOneFallback {
        /// Which node.
        node: usize,
        /// How many it named.
        count: usize,
    },
    /// The `kind` discriminant and the payload's tag say different things.
    #[error("node {node} says it is `{kind}` but its payload is tagged `{tag}`")]
    KindDisagreesWithPayload {
        /// Which node.
        node: usize,
        /// What the discriminant said.
        kind: &'static str,
        /// What the payload said.
        tag: String,
    },
    /// The payload was not a JSON object.
    #[error("node {node}: {reason}")]
    BadPayload {
        /// Which node.
        node: usize,
        /// What was wrong with it.
        reason: String,
    },
    /// The tree nests deeper than [`MAX_DEPTH`].
    #[error("the arena nests deeper than {MAX_DEPTH} levels")]
    TooDeep,
    /// The id string is not a surface id.
    #[error("node {node}: `{value}` is not a surface id")]
    BadId {
        /// Which node.
        node: usize,
        /// What it said.
        value: String,
    },
}

// --- flatten --------------------------------------------------------------

/// Write a surface out as an arena.
///
/// Infallible: every [`Surface`] has an arena, and the arena it produces always
/// satisfies [`rebuild`].
#[must_use]
pub fn flatten(surface: &Surface) -> SurfaceArena {
    let mut nodes = Vec::new();
    push(surface, &mut nodes);
    SurfaceArena { nodes, root: 0 }
}

/// Reserve this node's slot, write its descendants, then fill the slot in.
///
/// Reserving first is what makes the ordering parent-first while still letting
/// the parent record the indices its children landed on.
fn push(surface: &Surface, nodes: &mut Vec<SurfaceNode>) -> u32 {
    let me = u32::try_from(nodes.len()).expect("an arena never reaches u32::MAX nodes");
    nodes.push(PLACEHOLDER);

    let (kind, children) = split(&surface.kind);
    let child_indices: Vec<u32> = children.iter().map(|child| push(child, nodes)).collect();

    nodes[me as usize] = SurfaceNode {
        kind,
        id: surface.id.map(|id| id.to_string()),
        status: surface.status,
        payload: serde_json::to_string(&strip(&surface.kind))
            .expect("a SurfaceKind always serialises"),
        children: child_indices,
    };
    me
}

const PLACEHOLDER: SurfaceNode = SurfaceNode {
    kind: NodeKind::Text,
    id: None,
    status: None,
    payload: String::new(),
    children: Vec::new(),
};

/// This kind's discriminant and the children it owns.
fn split(kind: &SurfaceKind) -> (NodeKind, Vec<&Surface>) {
    match kind {
        SurfaceKind::Text { .. } => (NodeKind::Text, Vec::new()),
        SurfaceKind::Table { .. } => (NodeKind::Table, Vec::new()),
        SurfaceKind::Tree { .. } => (NodeKind::Tree, Vec::new()),
        SurfaceKind::Diff { .. } => (NodeKind::Diff, Vec::new()),
        SurfaceKind::Progress { .. } => (NodeKind::Progress, Vec::new()),
        SurfaceKind::Stream { .. } => (NodeKind::Stream, Vec::new()),
        SurfaceKind::Task { .. } => (NodeKind::Task, Vec::new()),
        SurfaceKind::Question { .. } => (NodeKind::Question, Vec::new()),
        SurfaceKind::Form { .. } => (NodeKind::Form, Vec::new()),
        SurfaceKind::Markdown { .. } => (NodeKind::Markdown, Vec::new()),
        SurfaceKind::Stack { children, .. } => (NodeKind::Stack, children.iter().collect()),
        SurfaceKind::Custom { fallback, .. } => (NodeKind::Custom, vec![fallback.as_ref()]),
        // `SurfaceKind` is `#[non_exhaustive]`. A variant added upstream
        // without a case here would silently lose its payload, so stop loudly:
        // this is a compile-time omission, not a runtime input.
        other => unreachable!("SurfaceKind variant not covered by the arena: {other:?}"),
    }
}

/// This kind's own JSON, with the recursive fields removed.
///
/// Going through `serde_json::Value` rather than hand-writing twelve encoders
/// means a field added to a variant upstream travels automatically; only a new
/// *recursive* field needs work here, and [`split`]'s `unreachable!` is what
/// catches a new variant.
fn strip(kind: &SurfaceKind) -> serde_json::Value {
    let mut value = serde_json::to_value(kind).expect("a SurfaceKind always serialises");
    if let Some(map) = value.as_object_mut() {
        match kind {
            SurfaceKind::Stack { .. } => {
                map.remove("children");
            }
            SurfaceKind::Custom { .. } => {
                map.remove("fallback");
            }
            _ => {}
        }
    }
    value
}

// --- rebuild --------------------------------------------------------------

/// Rebuild the real tree from an arena that arrived from guest memory.
///
/// # Errors
///
/// [`ArenaError`] for any arena that does not satisfy the frozen shape: a
/// dangling or backward index, a kind carrying children it may not, a `kind`
/// discriminant disagreeing with its payload, a payload that is not the JSON of
/// that kind, or a tree deeper than [`MAX_DEPTH`].
pub fn rebuild(arena: &SurfaceArena) -> Result<Surface, ArenaError> {
    let len = arena.nodes.len();
    if arena.root as usize >= len {
        return Err(ArenaError::DanglingRoot {
            root: arena.root,
            len,
        });
    }

    // Check the whole arena first, so a malformed node far from the root is
    // still reported rather than lurking behind a branch nobody walked.
    for (i, node) in arena.nodes.iter().enumerate() {
        for &child in &node.children {
            if child as usize >= len {
                return Err(ArenaError::DanglingChild {
                    parent: i,
                    index: child,
                    len,
                });
            }
            if (child as usize) <= i {
                return Err(ArenaError::BackwardChild {
                    parent: i,
                    index: child,
                });
            }
        }
        match node.kind.child_rule() {
            ChildRule::None if !node.children.is_empty() => {
                return Err(ArenaError::LeafWithChildren {
                    node: i,
                    kind: node.kind.tag(),
                    count: node.children.len(),
                });
            }
            ChildRule::ExactlyOne if node.children.len() != 1 => {
                return Err(ArenaError::CustomNeedsOneFallback {
                    node: i,
                    count: node.children.len(),
                });
            }
            _ => {}
        }
    }

    build(arena, arena.root as usize, 0)
}

fn build(arena: &SurfaceArena, index: usize, depth: usize) -> Result<Surface, ArenaError> {
    if depth >= MAX_DEPTH {
        return Err(ArenaError::TooDeep);
    }
    let node = &arena.nodes[index];

    let mut payload: serde_json::Value =
        serde_json::from_str(&node.payload).map_err(|e| ArenaError::BadPayload {
            node: index,
            reason: e.to_string(),
        })?;
    let map = payload
        .as_object_mut()
        .ok_or_else(|| ArenaError::BadPayload {
            node: index,
            reason: "a node payload is a JSON object".to_owned(),
        })?;

    let tag = map
        .get("t")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ArenaError::BadPayload {
            node: index,
            reason: "a node payload carries a `t` tag".to_owned(),
        })?
        .to_owned();
    if NodeKind::from_tag(&tag) != Some(node.kind) {
        return Err(ArenaError::KindDisagreesWithPayload {
            node: index,
            kind: node.kind.tag(),
            tag,
        });
    }

    // Put the recursive fields back, then let serde do the rest.
    match node.kind {
        NodeKind::Stack => {
            let children = node
                .children
                .iter()
                .map(|&c| build(arena, c as usize, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            map.insert(
                "children".to_owned(),
                serde_json::to_value(children).expect("a Surface always serialises"),
            );
        }
        NodeKind::Custom => {
            let fallback = build(arena, node.children[0] as usize, depth + 1)?;
            map.insert(
                "fallback".to_owned(),
                serde_json::to_value(fallback).expect("a Surface always serialises"),
            );
        }
        _ => {}
    }

    let kind: SurfaceKind =
        serde_json::from_value(payload).map_err(|e| ArenaError::BadPayload {
            node: index,
            reason: e.to_string(),
        })?;

    let id = match &node.id {
        None => None,
        Some(raw) => Some(raw.parse::<SurfaceId>().map_err(|_| ArenaError::BadId {
            node: index,
            value: raw.clone(),
        })?),
    };

    Ok(Surface {
        id,
        status: node.status,
        kind,
    })
}

// --- measurement ----------------------------------------------------------

/// What one encoding of a surface costs on the wire.
///
/// Used by `tests/json_cost.rs` to put a number on the flagged
/// "JSON crosses as a `string`" question rather than leaving it open.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Cost {
    /// How many bytes the encoding takes.
    pub bytes: usize,
    /// How many separate JSON documents had to be parsed or written.
    pub json_documents: usize,
}

/// The cost of an arena whose payloads are JSON strings — what the frozen WIT
/// actually does.
#[must_use]
pub fn cost_as_json_strings(arena: &SurfaceArena) -> Cost {
    // The canonical ABI writes each node's fields separately; the bytes that
    // cross are the field bytes, not a serialisation of the whole record.
    let bytes = arena
        .nodes
        .iter()
        .map(|n| {
            n.payload.len()
                + n.id.as_ref().map_or(0, String::len)
                + n.children.len() * size_of::<u32>()
                + size_of::<u32>() * 2
        })
        .sum::<usize>()
        + size_of::<u32>();
    Cost {
        bytes,
        json_documents: arena.nodes.len(),
    }
}

/// The cost of the hypothetical fully-typed variant: the same data, but with
/// every payload field expressed in WIT instead of JSON-in-a-string.
///
/// Approximated by measuring the payload's *values* without their JSON syntax —
/// keys, quotes, braces and commas are exactly what typing it away removes.
#[must_use]
pub fn cost_as_typed_fields(arena: &SurfaceArena) -> Cost {
    let bytes = arena
        .nodes
        .iter()
        .map(|n| {
            let value: serde_json::Value = serde_json::from_str(&n.payload).unwrap_or_default();
            value_bytes(&value)
                + n.id.as_ref().map_or(0, String::len)
                + n.children.len() * size_of::<u32>()
                + size_of::<u32>() * 2
        })
        .sum::<usize>()
        + size_of::<u32>();
    Cost {
        bytes,
        json_documents: 0,
    }
}

/// The bytes a typed encoding would spend on this value: strings as their
/// contents, numbers and bools as their machine width, keys not at all.
fn value_bytes(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Null => 0,
        serde_json::Value::Bool(_) => 1,
        serde_json::Value::Number(_) => 8,
        serde_json::Value::String(s) => s.len() + size_of::<u32>(),
        serde_json::Value::Array(items) => {
            size_of::<u32>() + items.iter().map(value_bytes).sum::<usize>()
        }
        serde_json::Value::Object(map) => {
            // The discriminant survives typing; the keys do not.
            size_of::<u32>() + map.values().map(value_bytes).sum::<usize>()
        }
    }
}

/// A small helper for the measurement: how many nodes of each kind an arena has.
#[must_use]
pub fn kind_census(arena: &SurfaceArena) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    for node in &arena.nodes {
        *out.entry(node.kind.tag()).or_insert(0) += 1;
    }
    out
}
