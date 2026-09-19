//! `orrery mcp` - the MCP servers a workspace declares, and what they offer.
//!
//! # Why this command exists
//!
//! `orrery-mcp` was built and tested and sat outside the binary's dependency
//! closure entirely, which is why section 8 phase 7 read PARTIAL: a person could
//! declare `[mcp_servers.github]` in their config and had no way to ask whether
//! the harness had seen it, whether it starts, or what it would contribute.
//! That is three questions, and they are the three subcommands.
//!
//! # Discovery starts nothing
//!
//! §4.11's lifecycle rule is that a server is **discovered at session start and
//! connected when first needed**, so `mcp list` starts no process: it reports
//! what the single discovery pass found and what configuration declared, and
//! every server reads `discovered`. `mcp tools` is the one that connects, and it
//! says so by connecting only to the server it was asked about.
//!
//! # No network
//!
//! Only `stdio` servers can be started from here. An `http` server is listed and
//! refused with a sentence, because reaching one is a network call and a network
//! call needs a broker, a grant and a session — which is `orrery run`, not an
//! inspection command.
//!
//! Implementation plan: `harness/docs/plans/13-skills-mcp.md`, rendered by
//! `harness/docs/plans/17-cli.md` task 11.

use std::collections::BTreeMap;

use orrery_mcp::{ServerSpec, Servers, StdioSpec};
use orrery_proto::Layer;

use crate::args::{Cli, McpCommand};
use crate::cmd::layers;
use crate::exit::{Exit, fail};

/// Dispatch an `mcp` subcommand.
pub fn dispatch(cli: &Cli, command: &McpCommand) -> ! {
    match command {
        McpCommand::List => list(cli),
        McpCommand::Tools { server } => tools(cli, server),
    }
}

/// Every declared server, with the layer that declared it and its health.
fn list(cli: &Cli) -> ! {
    let specs = declared(cli);
    if specs.is_empty() {
        eprintln!("orrery: no MCP servers declared. Add `[mcp_servers.<name>]` to a config layer.");
        Exit::Ok.exit();
    }
    // Discovery, and nothing more: `Servers::discover` starts no process, which
    // is the §4.11 rule this command has to respect to be worth reading.
    let servers = Servers::discover(specs);
    let json = layers::wants_json(cli);
    for spec in servers.discovered() {
        // A name that is not a legal ext id is a config error, and it is the
        // one thing worth failing over here: everything downstream — the
        // namespace, the policy rule, the audit line — is that id.
        let ext = orrery_mcp::ext_id(&spec.name).unwrap_or_else(|e| fail(Exit::Usage, e));
        let health = servers
            .health(&spec.name)
            .map_or_else(|| "unknown".to_owned(), |h| format!("{h:?}").to_lowercase());
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "server": spec.name,
                    "ext": ext.to_string(),
                    "layer": format!("{:?}", spec.layer).to_lowercase(),
                    "transport": transport_word(spec),
                    "health": health,
                })
            );
        } else {
            println!(
                "{:<20} {:<8} {:<10} {}",
                spec.name,
                transport_word(spec),
                health,
                ext
            );
        }
    }
    debug_assert_eq!(servers.started(), 0, "listing starts no processes");
    Exit::Ok.exit()
}

/// Connect to one server and print what it offers.
fn tools(cli: &Cli, server: &str) -> ! {
    let specs = declared(cli);
    let servers = Servers::discover(specs);
    if servers.spec(server).is_none() {
        fail(
            Exit::Usage,
            format!("no MCP server `{server}` is declared. `orrery mcp list` shows what is."),
        );
    }

    let rt = crate::cmd::session::runtime();
    let client = rt
        .block_on(servers.connect(server))
        .unwrap_or_else(|e| fail(Exit::Kernel, e));
    let tools = rt
        .block_on(client.list_tools())
        .unwrap_or_else(|e| fail(Exit::Kernel, e));

    let json = layers::wants_json(cli);
    for tool in &tools {
        let r#ref =
            orrery_mcp::tool_ref(server, &tool.name).unwrap_or_else(|e| fail(Exit::Kernel, e));
        if json {
            println!(
                "{}",
                serde_json::json!({
                    // The namespaced ref, because that is the name a policy rule
                    // and an audit record will both use. There is no MCP-shaped
                    // table anywhere; an MCP tool is a `ToolRef` like any other.
                    "ref": r#ref.to_string(),
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                })
            );
        } else {
            println!(
                "{:<40} {}",
                r#ref,
                tool.description.lines().next().unwrap_or_default()
            );
        }
    }
    if tools.is_empty() {
        eprintln!("orrery: `{server}` offers no tools");
    }
    Exit::Ok.exit()
}

/// The servers configuration declares, newest layer first.
///
/// Read out of the resolved layers rather than out of a running session,
/// because asking what is declared must work with no model in sight — the same
/// rule `permissions explain` and `config explain` follow.
fn declared(cli: &Cli) -> Vec<ServerSpec> {
    declared_in(&layers::resolve(cli))
}

/// The same, off layers somebody has already resolved.
///
/// `cmd::setup` calls this: what a turn can reach and what `orrery mcp list`
/// prints have to be the one set, read once, or the listing is theatre - which
/// is exactly what it was, because `orrery-harness` depended on `orrery-mcp`
/// for nothing and the run path had never heard of any of these servers.
pub(crate) fn declared_in(resolved: &orrery_config::ResolvedConfig) -> Vec<ServerSpec> {
    let mut out = Vec::new();
    let mut seen = BTreeMap::new();

    for key in resolved.values.keys_under("mcp_servers") {
        // `mcp_servers.<name>.<field>`: take the name, once.
        let mut parts = key.split('.').skip(1);
        let Some(name) = parts.next() else { continue };
        if seen.contains_key(name) {
            continue;
        }
        let slot = resolved
            .values
            .winner(&format!("mcp_servers.{name}.command"));
        let Some(command) = slot.and_then(|s| s.value.as_str()) else {
            // Declared without a command. Reported rather than skipped: a
            // server nobody can start is exactly what this command is for.
            eprintln!("orrery: `{name}` declares no `command`");
            continue;
        };
        let args: Vec<String> = resolved
            .values
            .winner(&format!("mcp_servers.{name}.args"))
            .and_then(|s| s.value.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let layer = slot.map_or(Layer::Project, |s| s.origin.layer);

        let mut stdio = StdioSpec::new(command);
        stdio.args = args;
        stdio.cwd = Some(resolved.root.clone());
        out.push(ServerSpec::stdio(name, stdio, layer));
        seen.insert(name.to_owned(), ());
    }
    out
}

/// How a server is reached, in one word.
fn transport_word(spec: &ServerSpec) -> &'static str {
    match spec.transport {
        orrery_mcp::TransportSpec::Stdio(_) => "stdio",
        _ => "http",
    }
}
