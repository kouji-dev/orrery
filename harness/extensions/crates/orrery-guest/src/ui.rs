//! Surfaces, as a tree. The arena is this module's problem, not yours.
//!
//! You build a [`Node`] with the constructors here and hand it back from your
//! tool. [`flatten`] turns it into the arena the world carries — parent-first,
//! depth-first, with every child index strictly greater than its parent's —
//! and you never type an index.

use crate::bindings::orrery::extension::surfaces as wit;

pub use wit::{NodeKind, NodeStatus};

/// One surface, as a normal tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// What kind of node this is.
    pub kind: NodeKind,
    /// A stable handle, when a later patch will address this node.
    pub id: Option<String>,
    /// How far along it is.
    pub status: Option<NodeStatus>,
    /// This node's own JSON, without its recursive fields.
    pub payload: String,
    /// Its children. Only a stack and a custom have any.
    pub children: Vec<Node>,
}

impl Node {
    /// Give this node a handle, so a later patch can address it.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Say how far along it is.
    #[must_use]
    pub fn with_status(mut self, status: NodeStatus) -> Self {
        self.status = Some(status);
        self
    }

    fn leaf(kind: NodeKind, payload: String) -> Self {
        Self {
            kind,
            id: None,
            status: None,
            payload,
            children: Vec::new(),
        }
    }
}

/// Escape a string for JSON. The SDK carries no serde: one escaper is cheaper
/// in component bytes than a derive, and this is the only JSON it writes.
#[must_use]
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_list(items: &[&str]) -> String {
    let mut out = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(item));
    }
    out.push(']');
    out
}

/// A run of text.
#[must_use]
pub fn text(value: &str) -> Node {
    Node::leaf(
        NodeKind::Text,
        format!("{{\"t\":\"text\",\"value\":{}}}", json_string(value)),
    )
}

/// Markdown. `complete` is false while it is still streaming, so a client knows
/// not to trust a half-open code fence.
#[must_use]
pub fn markdown(value: &str, complete: bool) -> Node {
    Node::leaf(
        NodeKind::Markdown,
        format!(
            "{{\"t\":\"markdown\",\"value\":{},\"complete\":{complete}}}",
            json_string(value)
        ),
    )
}

/// Rows under headers. Every row is expected to be `columns.len()` long.
#[must_use]
pub fn table(columns: &[&str], rows: &[Vec<String>]) -> Node {
    let mut body = String::from("[");
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            body.push(',');
        }
        body.push('[');
        for (j, cell) in row.iter().enumerate() {
            if j > 0 {
                body.push(',');
            }
            body.push_str(&format!("{{\"text\":{}}}", json_string(cell)));
        }
        body.push(']');
    }
    body.push(']');
    Node::leaf(
        NodeKind::Table,
        format!(
            "{{\"t\":\"table\",\"columns\":{},\"rows\":{body}}}",
            json_list(columns)
        ),
    )
}

/// Something taking a while. `done`/`total` absent means indeterminate.
#[must_use]
pub fn progress(label: &str, done: Option<u64>, total: Option<u64>) -> Node {
    let mut payload = format!("{{\"t\":\"progress\",\"label\":{}", json_string(label));
    if let Some(done) = done {
        payload.push_str(&format!(",\"done\":{done}"));
    }
    if let Some(total) = total {
        payload.push_str(&format!(",\"total\":{total}"));
    }
    payload.push('}');
    Node::leaf(NodeKind::Progress, payload)
}

/// Children, top to bottom.
#[must_use]
pub fn column(children: Vec<Node>) -> Node {
    stack("column", None, children)
}

/// Children, left to right.
#[must_use]
pub fn row(children: Vec<Node>) -> Node {
    stack("row", None, children)
}

/// A titled group.
#[must_use]
pub fn section(title: &str, children: Vec<Node>) -> Node {
    stack("column", Some(title), children)
}

fn stack(dir: &str, title: Option<&str>, children: Vec<Node>) -> Node {
    let mut payload = format!("{{\"t\":\"stack\",\"dir\":\"{dir}\",\"collapsed\":false");
    if let Some(title) = title {
        payload.push_str(&format!(",\"title\":{}", json_string(title)));
    }
    payload.push('}');
    Node {
        kind: NodeKind::Stack,
        id: None,
        status: None,
        payload,
        children,
    }
}

/// Your own renderer, with the fallback a client without it draws instead.
///
/// The fallback is **mandatory**, and `kind` must be `<ext>.<name>` — a custom
/// surface nobody can draw is a blank hole in a transcript, so neither the type
/// nor the host will let you make one.
#[must_use]
pub fn custom(kind: &str, payload_json: &str, fallback: Node) -> Node {
    Node {
        kind: NodeKind::Custom,
        id: None,
        status: None,
        payload: format!(
            "{{\"t\":\"custom\",\"kind\":{},\"payload\":{payload_json}}}",
            json_string(kind)
        ),
        children: vec![fallback],
    }
}

/// Flatten a tree into the arena the world carries.
///
/// The guarantees the host checks on arrival, which this function is what
/// satisfies: parent-first and depth-first, every child index strictly greater
/// than its parent's, and children only on a stack or a custom.
#[must_use]
pub fn flatten(root: &Node) -> wit::Surface {
    let mut nodes = Vec::new();
    push(root, &mut nodes);
    wit::Surface { nodes, root: 0 }
}

/// Reserve this node's slot, write its descendants, then fill the slot in —
/// which is what makes the ordering parent-first while still letting a parent
/// record where its children landed.
fn push(node: &Node, nodes: &mut Vec<wit::SurfaceNode>) -> u32 {
    let me = nodes.len() as u32;
    nodes.push(wit::SurfaceNode {
        kind: NodeKind::Text,
        id: None,
        status: None,
        payload: String::new(),
        children: Vec::new(),
    });
    let children: Vec<u32> = node.children.iter().map(|c| push(c, nodes)).collect();
    nodes[me as usize] = wit::SurfaceNode {
        kind: node.kind,
        id: node.id.clone(),
        status: node.status,
        payload: node.payload.clone(),
        children,
    };
    me
}
