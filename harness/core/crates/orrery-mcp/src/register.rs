//! An MCP server is an extension id. There is no second namespacing scheme.
//!
//! `mcp.jira`'s `create_issue` is a [`ToolRef`] like any other — `ext:
//! "mcp.jira"`, `name: "create_issue"` — so it inherits namespacing, layer
//! precedence, ambiguity resolution, the visible set, the policy check and the
//! audit **unchanged**. [`ExtId`] already reserves exactly one dotted form for
//! this, and [`ToolRef`]'s parser already splits on the last dot so that
//! `mcp.jira.create_issue` means what it looks like. Nothing here re-implements
//! any of that; this module only decides what to put in the table.
//!
//! # `list_changed` does not silently extend the session
//!
//! [`reconcile`] is the manifest-integrity rule. A server that grows a tool
//! mid-session has each **new** tool re-resolved against policy and either
//! admitted — and recorded, in the audit stream and in the returned
//! [`Reconciliation`] — or refused and left out of the registry. A tool set that
//! grows after the manifest was approved is exactly what the manifest exists to
//! prevent.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use orrery_audit::{Audit, AuditEvent};
use orrery_jsonrpc::Handler;
use orrery_proto::{ExtId, Layer, Subject, ToolRef};
use orrery_tools::{PolicyCheck, Registry, ToolSpec};

use crate::client::McpTool;
use crate::error::McpError;

/// The namespace prefix an MCP server registers under.
pub const PREFIX: &str = "mcp.";

/// The extension id a server registers as.
///
/// # Errors
///
/// [`McpError::BadName`] when the name is not a legal [`ExtId`] segment — it is
/// half of every tool name the model will see, so it is validated here rather
/// than producing an unparseable `ToolRef` later.
pub fn ext_id(server: &str) -> Result<ExtId, McpError> {
    ExtId::new(format!("{PREFIX}{server}")).map_err(|e| McpError::BadName {
        name: server.to_owned(),
        message: e.to_string(),
    })
}

/// The reference one of a server's tools registers under.
///
/// # Errors
///
/// [`McpError::BadName`], as [`ext_id`].
pub fn tool_ref(server: &str, tool: &str) -> Result<ToolRef, McpError> {
    Ok(ToolRef {
        ext: ext_id(server)?,
        name: tool.to_owned(),
    })
}

/// What a tool the server offered turned into.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admission {
    /// It was already in the registry from this server. Nothing changed.
    Unchanged {
        /// Which tool.
        r#ref: ToolRef,
    },
    /// It is new, policy did not categorically refuse it for this subject, and
    /// it is now in the registry.
    Admitted {
        /// Which tool.
        r#ref: ToolRef,
    },
    /// It is new and policy refuses it for this subject, so it was **not**
    /// registered. The model is never offered it and no call can reach it.
    Refused {
        /// Which tool.
        r#ref: ToolRef,
        /// Why, in words a person can act on.
        reason: String,
    },
}

impl Admission {
    /// Which tool this is about.
    #[must_use]
    pub fn r#ref(&self) -> &ToolRef {
        match self {
            Admission::Unchanged { r#ref }
            | Admission::Admitted { r#ref }
            | Admission::Refused { r#ref, .. } => r#ref,
        }
    }

    /// Whether this tool is callable after the reconciliation.
    #[must_use]
    pub fn is_available(&self) -> bool {
        !matches!(self, Admission::Refused { .. })
    }
}

/// What one pass of [`reconcile`] decided, in the order the server listed them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reconciliation {
    /// Every tool the server offered and what became of it.
    pub decisions: Vec<Admission>,
}

impl Reconciliation {
    /// The tools that are new this pass and were let in.
    #[must_use]
    pub fn admitted(&self) -> Vec<&ToolRef> {
        self.decisions
            .iter()
            .filter_map(|d| match d {
                Admission::Admitted { r#ref } => Some(r#ref),
                _ => None,
            })
            .collect()
    }

    /// The tools that are new this pass and were kept out.
    #[must_use]
    pub fn refused(&self) -> Vec<&ToolRef> {
        self.decisions
            .iter()
            .filter_map(|d| match d {
                Admission::Refused { r#ref, .. } => Some(r#ref),
                _ => None,
            })
            .collect()
    }

    /// Whether anything at all changed.
    #[must_use]
    pub fn grew(&self) -> bool {
        self.decisions
            .iter()
            .any(|d| matches!(d, Admission::Admitted { .. }))
    }
}

/// Put a freshly connected server's tools into the registry.
///
/// The first pass and every later one are the **same function**: an initial
/// `tools/list` and a `notifications/tools/list_changed` are the same event as
/// far as the manifest is concerned, so there is no first-time path that skips
/// the check.
///
/// # Errors
///
/// [`McpError::BadName`] when the server's name is not a legal namespace.
pub fn register(
    registry: &mut Registry,
    server: &str,
    layer: Layer,
    tools: &[McpTool],
    subject: &Subject,
    policy: &dyn PolicyCheck,
    audit: &Audit,
) -> Result<Reconciliation, McpError> {
    reconcile(registry, server, layer, tools, subject, policy, audit)
}

/// Re-resolve a server's tool list against policy and the registry.
///
/// Every tool the server now offers is looked at:
///
/// - already registered from this server → [`Admission::Unchanged`];
/// - new, and [`PolicyCheck::categorically_denies`] says no for this subject →
///   [`Admission::Refused`], and it is **not** registered;
/// - new, and policy does not categorically refuse it → registered, and
///   [`Admission::Admitted`].
///
/// Either way the decision is appended to the audit stream, so "the tool set
/// grew mid-session" is answerable from the stream alone rather than from
/// somebody's memory of a turn.
///
/// # Errors
///
/// [`McpError::BadName`] when the server's name is not a legal namespace.
pub fn reconcile(
    registry: &mut Registry,
    server: &str,
    layer: Layer,
    tools: &[McpTool],
    subject: &Subject,
    policy: &dyn PolicyCheck,
    audit: &Audit,
) -> Result<Reconciliation, McpError> {
    let ext = ext_id(server)?;
    let mut decisions = Vec::with_capacity(tools.len());

    for tool in tools {
        let r#ref = ToolRef {
            ext: ext.clone(),
            name: tool.name.clone(),
        };

        if registry.entry(&r#ref).is_some() {
            decisions.push(Admission::Unchanged { r#ref });
            continue;
        }

        if policy.categorically_denies(subject, &r#ref) {
            let reason = format!(
                "policy refuses `{ref}` for `{subject}`, so it was not added to the session's \
                 tool set",
                r#ref = r#ref
            );
            tracing::warn!(
                target: "orrery.mcp.register",
                tool = %r#ref,
                %subject,
                "an MCP tool was refused rather than silently admitted"
            );
            audit.append(AuditEvent::ExtensionLoad {
                ext: ext.clone(),
                status: "tool-refused".to_owned(),
                contributions: Vec::new(),
                problems: vec![reason.clone()],
            });
            decisions.push(Admission::Refused { r#ref, reason });
            continue;
        }

        registry.register(
            &ext,
            layer,
            ToolSpec::new(&tool.name)
                .described(&tool.description)
                .with_schema(tool.input_schema.clone()),
        );
        audit.append(AuditEvent::ExtensionLoad {
            ext: ext.clone(),
            status: "tool-admitted".to_owned(),
            contributions: vec![r#ref.to_string()],
            problems: Vec::new(),
        });
        decisions.push(Admission::Admitted { r#ref });
    }

    Ok(Reconciliation { decisions })
}

#[cfg(test)]
mod tests {
    use super::{ext_id, tool_ref};

    #[test]
    fn a_server_is_an_extension_id() {
        assert_eq!(ext_id("jira").unwrap().as_str(), "mcp.jira");
        assert!(ext_id("jira").unwrap().is_mcp());
    }

    #[test]
    fn the_qualified_name_round_trips() {
        let r#ref = tool_ref("jira", "create_issue").unwrap();
        assert_eq!(r#ref.to_string(), "mcp.jira.create_issue");
        assert_eq!(
            "mcp.jira.create_issue"
                .parse::<orrery_proto::ToolRef>()
                .unwrap(),
            r#ref
        );
    }

    #[test]
    fn a_name_that_is_not_a_namespace_is_refused_here_not_later() {
        assert!(ext_id("Jira Cloud").is_err());
        assert!(ext_id("a.b").is_err());
    }
}

/// Notices `notifications/tools/list_changed` and wakes whoever is watching.
///
/// Installed as the connection's [`Handler`] — see
/// [`Servers::with_handler`](crate::health::Servers::with_handler) — so that a
/// server announcing a new tool set reaches [`reconcile`] rather than being
/// discovered the next time somebody happens to list. The notification only
/// ever *causes* a re-resolution; it never adds a tool by itself.
#[derive(Debug, Default)]
pub struct ListChangedWatch {
    seen: AtomicUsize,
    notify: tokio::sync::Notify,
}

impl ListChangedWatch {
    /// A watch that has seen nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many `tools/list_changed` notifications have arrived.
    #[must_use]
    pub fn seen(&self) -> usize {
        self.seen.load(Ordering::SeqCst)
    }

    /// Wait for the next one.
    ///
    /// A notification that arrived before anybody was waiting still wakes the
    /// next waiter: [`Notify::notify_one`](tokio::sync::Notify::notify_one)
    /// leaves a permit behind, where `notify_waiters` would have been a race
    /// nobody could win from the outside.
    pub async fn changed(&self) {
        self.notify.notified().await;
    }
}

#[async_trait]
impl Handler for ListChangedWatch {
    async fn notify(&self, method: &str, _params: serde_json::Value) {
        if method == crate::client::TOOLS_LIST_CHANGED {
            self.seen.fetch_add(1, Ordering::SeqCst);
            tracing::info!(
                target: "orrery.mcp.register",
                "an MCP server says its tool set moved; re-resolving against policy"
            );
            self.notify.notify_one();
        }
    }
}
