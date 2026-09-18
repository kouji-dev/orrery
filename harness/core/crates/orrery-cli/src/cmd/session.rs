//! `orrery session` - list, show and remove stored sessions.
//!
//! # No kernel, no provider
//!
//! These commands open the session database and nothing else: history is a
//! file, and asking what is in it must not need a model. That is why they build
//! a store directly rather than a [`Session`](crate::session::Session), and why
//! their tests pass no `--provider`.
//!
//! `list` is [`SessionStore::list_sessions`], which landed with this command —
//! before it the trait could only answer about a session you could already
//! name. `show` is one `materialise` on the root branch.
//!
//! Implementation plan: `harness/docs/plans/02-session-store.md` (the store) and
//! `harness/docs/plans/17-cli.md` task 7 (the command).

use std::path::PathBuf;
use std::sync::Arc;

use orrery_proto::{ContentBlock, SessionId, TokenBudget};
use orrery_session::algebra::CharsOverFour;
use orrery_session::{SessionStore, SessionSummary};

use crate::args::{Cli, SessionCommand};
use crate::cmd::layers;
use crate::exit::{Exit, fail};

/// Dispatch a `session` subcommand.
pub fn dispatch(cli: &Cli, command: &SessionCommand) -> ! {
    match command {
        SessionCommand::List => list(cli),
        SessionCommand::Show { id } => show(cli, id),
        SessionCommand::Rm { .. } => fail(
            Exit::Usage,
            "`session rm` is not implemented in this build: `SessionStore` has no delete, \
             and plan 02 open question 2 parks retention for the config phase. \
             See harness/docs/plans/02-session-store.md",
        ),
    }
}

/// Where the database is, given the flags. Defaults to `<workspace>/.orrery`,
/// the same default `cmd::setup` uses, so `run` and `session list` agree
/// without either reading the other's code.
fn state_dir(cli: &Cli) -> PathBuf {
    cli.state_dir.clone().unwrap_or_else(|| {
        let workspace = cli.workspace.clone().unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|e| fail(Exit::Usage, format!("no workspace: {e}")))
        });
        workspace.join(".orrery")
    })
}

/// Open the store, or exit. Answering "which sessions are there" must not
/// *create* a database, so a store that is not there yet is `None` rather than
/// an empty file written into somebody's workspace.
fn store(cli: &Cli) -> Option<Arc<dyn SessionStore>> {
    let dir = state_dir(cli);
    if !dir.join("sessions.db").exists() {
        return None;
    }
    match orrery_harness::features::open_store(&dir) {
        Ok(store) => Some(store),
        Err(e) => fail(Exit::Kernel, format!("{}: {e}", dir.display())),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| fail(Exit::Kernel, format!("no runtime: {e}")))
}

/// One line per session, newest first.
fn list(cli: &Cli) -> ! {
    let Some(store) = store(cli) else {
        eprintln!("orrery: no sessions in {}", state_dir(cli).display());
        Exit::Ok.exit();
    };
    let sessions = runtime()
        .block_on(store.list_sessions())
        .unwrap_or_else(|e| fail(Exit::Kernel, e));

    if sessions.is_empty() {
        eprintln!("orrery: no sessions in {}", state_dir(cli).display());
        Exit::Ok.exit();
    }
    let json = layers::wants_json(cli);
    for s in &sessions {
        if json {
            println!("{}", summary_json(s));
        } else {
            println!(
                "{}  {:>4} turns  {}  {}",
                s.session,
                s.turns,
                s.profile,
                s.workspace
            );
        }
    }
    Exit::Ok.exit()
}

fn summary_json(s: &SessionSummary) -> serde_json::Value {
    serde_json::json!({
        "session": s.session.to_string(),
        "workspace": s.workspace,
        "profile": s.profile,
        "created_at": s.created_at,
        "turns": s.turns,
    })
}

/// The session's header and its transcript.
fn show(cli: &Cli, id: &str) -> ! {
    let session: SessionId = id
        .parse()
        .unwrap_or_else(|e| fail(Exit::Usage, format!("`{id}` is not a session id: {e}")));
    let Some(store) = store(cli) else {
        fail(
            Exit::Usage,
            format!("no sessions in {}", state_dir(cli).display()),
        );
    };

    let rt = runtime();
    let handle = rt
        .block_on(store.open(session))
        .unwrap_or_else(|e| fail(Exit::Usage, e));
    let view = rt
        .block_on(store.materialise(
            handle.root,
            TokenBudget {
                max: u64::MAX,
                reserve: 0,
            },
            &CharsOverFour,
        ))
        .unwrap_or_else(|e| fail(Exit::Kernel, e));

    if layers::wants_json(cli) {
        println!(
            "{}",
            serde_json::json!({
                "session": handle.session.to_string(),
                "workspace": handle.workspace,
                "profile": handle.profile,
                "branches": handle.branches.len(),
                "watermark": view.watermark.map(|s| s.0),
                "messages": view.messages,
            })
        );
        Exit::Ok.exit();
    }

    println!("session {}", handle.session);
    println!("workspace {}", handle.workspace);
    println!("profile   {}", handle.profile);
    println!("branches  {}", handle.branches.len());
    if let Some(mark) = view.watermark {
        println!("compacted up to {}", mark.0);
    }
    println!();
    for message in &view.messages {
        let role = format!("{:?}", message.role).to_lowercase();
        for block in &message.content {
            match block {
                ContentBlock::Text { text } => println!("{role}: {text}"),
                ContentBlock::ToolUse { name, call, .. } => {
                    println!("{role}: [tool {name} {call}]");
                }
                ContentBlock::ToolResult { call, .. } => println!("{role}: [result {call}]"),
                other => println!("{role}: [{}]", block_word(other)),
            }
        }
    }
    Exit::Ok.exit()
}

fn block_word(block: &ContentBlock) -> &'static str {
    match block {
        ContentBlock::Text { .. } => "text",
        ContentBlock::ToolUse { .. } => "tool",
        ContentBlock::ToolResult { .. } => "result",
        _ => "block",
    }
}
