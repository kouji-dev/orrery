//! MCP servers on the **run path**.
//!
//! # Why this module exists at all
//!
//! `orrery-mcp` was built, tested and exported, `orrery-cli` depended on it for
//! `orrery mcp list | tools` — and `orrery-harness` depended on it for nothing.
//! So an inspection command performed a real handshake and listed
//! `mcp.fixture.echo`, and a real turn calling that name answered "no-such-tool:
//! there is no tool called `mcp.fixture.echo` in this agent's tool set". The
//! subsystem existed; the product did not have it. That is the same defect as
//! the inert `[permissions]` and the inert `maxUsd`, and it has one shape: an
//! inspection command sees the subsystem and the run path does not.
//!
//! # No second namespacing scheme, and no second door
//!
//! An MCP server is extension id `mcp.<server>` (plan 13), so its tools are
//! ordinary [`ToolRef`]s registered in the ordinary [`Registry`] behind the
//! ordinary policy check. [`RoutingHost`] is the one new thing, and it is four
//! lines: the registry has one host, and an `mcp.*` reference belongs to the
//! MCP host while everything else belongs to the extension table. Nothing here
//! dispatches; `Registry::dispatch` does, with its policy check, exactly as it
//! does for a local tool.
//!
//! # Discovery, connection, and the one honest deviation
//!
//! §4.11 says a server is discovered at session start and connected when first
//! needed. [`Servers::discover`] still starts nothing, and the MCP host still
//! reconnects a dead server on the next call. But a tool the model was never
//! offered cannot be "needed", so **the tool list is asked for while the
//! session is being assembled** — one `initialize` plus one `tools/list` per
//! declared server. A server that will not start degrades: it is recorded and
//! the harness opens without it, because somebody else's server failing is not
//! this harness failing.
//!
//! # `list_changed` cannot grow this session, and says so
//!
//! [`Registry`] is `Arc`-shared and immutable once the kernel holds it, so a
//! tool announced mid-session cannot be added to it. That is the conservative
//! direction and the plan's own rule — "a tool set that grows after the
//! manifest was approved is exactly what the manifest exists to prevent" — so
//! [`watch_for_growth`] re-resolves against what was admitted and **records the
//! refusal** rather than letting the growth pass unseen. Never silently
//! available, in either direction.
//!
//! Implementation plan: `harness/docs/plans/13-skills-mcp.md`

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use orrery_audit::{Audit, AuditEvent};
use orrery_mcp::{ListChangedWatch, McpHost, Servers};
use orrery_proto::{Outcome, Subject, ToolRef};
use orrery_tools::{CallCtx, PolicyCheck, Registry, ToolError, ToolHost};

/// The registry's host when MCP servers are declared.
///
/// One host, two destinations, chosen by the one bit that already means this:
/// `ExtId::is_mcp`.
pub struct RoutingHost {
    table: Arc<dyn ToolHost>,
    mcp: Arc<McpHost>,
}

impl std::fmt::Debug for RoutingHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutingHost")
            .field("mcp", &self.mcp)
            .finish_non_exhaustive()
    }
}

impl RoutingHost {
    /// Route `mcp.*` to `mcp`, and everything else to `table`.
    #[must_use]
    pub fn new(table: Arc<dyn ToolHost>, mcp: Arc<McpHost>) -> Self {
        Self { table, mcp }
    }
}

#[async_trait]
impl ToolHost for RoutingHost {
    async fn call(
        &self,
        r#ref: &ToolRef,
        input: serde_json::Value,
        ctx: &CallCtx,
    ) -> Result<Outcome, ToolError> {
        if r#ref.ext.is_mcp() {
            self.mcp.call(r#ref, input, ctx).await
        } else {
            self.table.call(r#ref, input, ctx).await
        }
    }
}

/// Connect every declared server once and put what it offers in the registry.
///
/// Returns the fully-qualified names that were admitted, which is what
/// [`watch_for_growth`] compares a later `tools/list` against.
///
/// Nothing here fails the build: a server that will not start is marked failed
/// and recorded, and the session opens without its tools.
pub async fn install(
    registry: &mut Registry,
    servers: &Arc<Servers>,
    subject: &Subject,
    policy: &dyn PolicyCheck,
    audit: &Audit,
) -> BTreeSet<String> {
    let mut admitted = BTreeSet::new();
    let specs: Vec<(String, orrery_proto::Layer)> = servers
        .discovered()
        .iter()
        .map(|s| (s.name.clone(), s.layer))
        .collect();

    for (name, layer) in specs {
        let client = match servers.connect(&name).await {
            Ok(client) => client,
            Err(e) => {
                servers.mark_failed(&name, e.to_string());
                record_problem(audit, &name, "unreachable", &e.to_string());
                continue;
            }
        };
        let tools = match client.list_tools().await {
            Ok(tools) => tools,
            Err(e) => {
                servers.mark_failed(&name, e.to_string());
                record_problem(audit, &name, "unreachable", &e.to_string());
                continue;
            }
        };
        match orrery_mcp::register(registry, &name, layer, &tools, subject, policy, audit) {
            Ok(reconciliation) => {
                for decision in &reconciliation.decisions {
                    if decision.is_available() {
                        admitted.insert(decision.r#ref().to_string());
                    }
                }
            }
            Err(e) => record_problem(audit, &name, "unloadable", &e.to_string()),
        }
    }
    admitted
}

/// Watch for `tools/list_changed` and record what it would have added.
///
/// The registry cannot grow after the kernel holds it, so this admits nothing.
/// What it does is make the growth **visible**: the stream says which tools
/// appeared and that this session refused them, instead of a server quietly
/// offering something nobody agreed to and nobody could see.
pub fn watch_for_growth(
    servers: Arc<Servers>,
    watch: Arc<ListChangedWatch>,
    audit: Audit,
    admitted: BTreeSet<String>,
) {
    tokio::spawn(async move {
        loop {
            watch.changed().await;
            let names: Vec<String> = servers
                .discovered()
                .iter()
                .map(|s| s.name.clone())
                .collect();
            for server in names {
                let Ok(client) = servers.connect(&server).await else {
                    continue;
                };
                let Ok(tools) = client.list_tools().await else {
                    continue;
                };
                for tool in &tools {
                    let Ok(r#ref) = orrery_mcp::tool_ref(&server, &tool.name) else {
                        continue;
                    };
                    if admitted.contains(&r#ref.to_string()) {
                        continue;
                    }
                    let reason = format!(
                        "`{ref}` appeared after this session's tool set was approved, so it \
                         was not added to it; start a new session to pick it up",
                        r#ref = r#ref
                    );
                    tracing::warn!(
                        target: "orrery.mcp.register",
                        tool = %r#ref,
                        "an MCP server grew mid-session; the new tool was refused, not admitted"
                    );
                    record_problem(&audit, &server, "tool-refused", &reason);
                }
            }
        }
    });
}

/// One line in the stream about a server that could not contribute.
fn record_problem(audit: &Audit, server: &str, status: &str, problem: &str) {
    let Ok(ext) = orrery_mcp::ext_id(server) else {
        return;
    };
    tracing::warn!(
        target: "orrery.mcp.health",
        server,
        status,
        problem,
        "an MCP server did not contribute what it declared"
    );
    audit.append(AuditEvent::ExtensionLoad {
        ext,
        status: status.to_owned(),
        contributions: Vec::new(),
        problems: vec![problem.to_owned()],
    });
}
