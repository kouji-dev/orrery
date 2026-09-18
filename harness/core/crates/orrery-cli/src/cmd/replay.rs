//! `orrery replay` - re-emit a stored session as events.
//!
//! # Cold replay re-encodes
//!
//! Plan 08 open question 3 decided it and this is where it lands. The sqlite
//! backend persists **one** `turn.settled` event per turn: the rich stream a
//! client sees — the text deltas, the tool-call starts, the argument fragments —
//! is produced by the encoder at the hub while the turn runs, and is not
//! written anywhere. Replaying it therefore means re-encoding the *rows*, which
//! is what this module does: `SessionStore::turns` gives the rows back with
//! their kinds intact, and the same [`Publisher`] a live turn narrates through
//! publishes them into the same [`Hub`].
//!
//! The alternative — persisting the encoder stream beside the turns — was
//! rejected: it doubles every write, it puts a rendering concern in the one
//! store that cannot degrade, and it makes the durable record of a session
//! depend on which AG-UI version wrote it. Turns are the record; frames are a
//! view of it.
//!
//! # No model, no key, no network
//!
//! A past session is on disk. Nothing here builds a kernel or a provider, which
//! is why `replay` takes no `--provider` and works in a checkout with no
//! configuration at all.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 8.

use std::io::Write as _;

use orrery_agui::Frame;
use orrery_client_json::JsonRenderer;
use orrery_client_ratatui::{App, VecSource};
use orrery_proto::{ContentBlock, SessionId, TurnId, Usage};
use orrery_session::{TurnKind, TurnRow};
use orrery_transport::Hub;

use crate::args::Cli;
use crate::exit::{Exit, fail};
use crate::render::HeadlessScrollback;
use crate::session::Publisher;
use crate::ui::Ui;

/// How wide to draw when there is no terminal to ask.
const HEADLESS_WIDTH: u16 = 80;

/// Replay a stored session into the selected renderer.
pub fn dispatch(cli: &Cli, session: &str) -> ! {
    let session: SessionId = session
        .parse()
        .unwrap_or_else(|e| fail(Exit::Usage, format!("`{session}` is not a session id: {e}")));
    let Some(store) = crate::cmd::session::store(cli) else {
        fail(
            Exit::Usage,
            format!(
                "no sessions in {}",
                crate::cmd::session::state_dir(cli).display()
            ),
        );
    };

    let rt = crate::cmd::session::runtime();
    let handle = rt
        .block_on(store.open(session))
        .unwrap_or_else(|e| fail(Exit::Usage, e));
    let rows = rt
        .block_on(store.turns(handle.root))
        .unwrap_or_else(|e| fail(Exit::Kernel, e));

    let frames = encode(&handle.profile, &rows);
    match Ui::resolve(cli.ui, cli.json, crate::ui::stdout_is_tty()) {
        Ui::Json => emit_json(&frames),
        // Ink attaches to a running kernel over an endpoint; there is none here,
        // and pretending otherwise would spawn a client that waits forever.
        Ui::Ink => fail(
            Exit::Usage,
            "`--ui ink` attaches to a running kernel; `replay` has none. \
             Use `--json`, or the built-in renderer.",
        ),
        Ui::Ratatui => rt.block_on(draw(frames)),
    }
    Exit::Ok.exit()
}

/// Turn the rows back into frames, in the order a live turn would have emitted
/// them.
///
/// A **`User` row starts a turn**: that is the one boundary the rows carry
/// unambiguously, because a prompt is the only thing that opens one. Everything
/// after it — the assistant's prose, the tool calls it asked for, the results
/// that came back — belongs to that turn until the next prompt.
///
/// The user's own text is not re-emitted, for the same reason a live run does
/// not emit it: the person typed it, and the frames are what came *back*.
fn encode(profile: &str, rows: &[TurnRow]) -> Vec<Frame> {
    // Sized from the rows so the ring cannot swallow the front of a long
    // session: `attach(None)` is an error, not a truncation, if it does.
    let hub = Hub::with_capacity(profile, rows.len().saturating_mul(16).max(1024));
    let publisher = Publisher::new(hub.clone());

    let mut open: Option<(TurnId, Usage)> = None;
    for row in rows {
        match &row.kind {
            TurnKind::User { input } => {
                if let Some((turn, usage)) = open.take() {
                    publisher.turn_settled(turn, usage);
                }
                publisher.turn_started(row.id);
                open = Some((row.id, Usage::default()));
                for block in &input.attachments {
                    replay_block(&publisher, block);
                }
            }
            TurnKind::Assistant { content, usage } => {
                let (_, total) = open.get_or_insert_with(|| {
                    publisher.turn_started(row.id);
                    (row.id, Usage::default())
                });
                accumulate(total, usage);
                for block in content {
                    replay_block(&publisher, block);
                }
            }
            TurnKind::ToolResult { call, outcome, .. } => {
                if open.is_none() {
                    publisher.turn_started(row.id);
                    open = Some((row.id, Usage::default()));
                }
                publisher.tool_settled(*call, outcome.clone());
            }
            // A summary, a join and a recall all read back as prose, which is
            // exactly what `materialise` hands a model. Replay shows the person
            // the same thing rather than inventing a frame for each.
            TurnKind::Summary { text, .. } => publisher.text(text),
            TurnKind::BranchResult { child, outcome } => {
                publisher.text(&format!("[branch {child} {}]", outcome.tag()));
            }
            TurnKind::Recalled { provider, entries } => {
                for entry in entries {
                    publisher.text(&format!("[{provider}:{}] {}\n", entry.key, entry.text));
                }
            }
            // `TurnKind` is `#[non_exhaustive]`. A row kind this build does not
            // know how to draw is skipped rather than guessed at.
            other => tracing::debug!(kind = other.tag(), "replay skipped an unknown row"),
        }
    }
    if let Some((turn, usage)) = open.take() {
        publisher.turn_settled(turn, usage);
    }

    hub.attach(None)
        .unwrap_or_else(|e| fail(Exit::Kernel, format!("could not replay the session: {e}")))
}

/// One content block, as the frames that carried it.
fn replay_block(publisher: &Publisher, block: &ContentBlock) {
    match block {
        ContentBlock::Text { text } | ContentBlock::Thinking { text } => publisher.text(text),
        ContentBlock::ToolUse { call, name, input } => {
            publisher.tool_started(*call, name);
            if let Ok(args) = serde_json::to_string(input) {
                publisher.tool_args(*call, &args);
            }
        }
        ContentBlock::ToolResult { call, outcome } => {
            publisher.tool_settled(*call, outcome.clone());
        }
        // An image has no frame of its own in this build; saying so beats
        // pretending the turn had nothing in it.
        ContentBlock::Image { media_type, .. } => {
            publisher.text(&format!("[image {media_type}]"));
        }
        _ => {}
    }
}

/// Add one turn's usage to the running total for the turn being replayed.
fn accumulate(total: &mut Usage, add: &Usage) {
    total.input_tokens = total.input_tokens.saturating_add(add.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(add.output_tokens);
    total.cache_hits = total.cache_hits.saturating_add(add.cache_hits);
    total.micro_usd = match (total.micro_usd, add.micro_usd) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (a, b) => a.or(b),
    };
}

/// The frames, one JSON object per line, exactly as `run --json` writes them.
fn emit_json(frames: &[Frame]) {
    let stdout = std::io::stdout();
    let mut out = JsonRenderer::new(stdout.lock());
    for frame in frames {
        if let Err(e) = out.emit(frame) {
            fail(Exit::Kernel, format!("could not write events: {e}"));
        }
    }
}

/// The frames, drawn by the built-in renderer.
async fn draw(frames: Vec<Frame>) {
    let mut source = VecSource::new(frames);
    if crate::ui::stdout_is_tty() {
        if let Ok(mut terminal) = crate::term::setup() {
            let _guard = crate::term::guard();
            let width = terminal.size().map_or(HEADLESS_WIDTH, |s| s.width);
            let mut app = App::new(width);
            let mut sink =
                orrery_client_ratatui::terminal::TerminalScrollback::new(&mut terminal, width);
            let drawn = app.run(&mut source, &mut sink).await;
            crate::term::restore();
            if let Err(e) = drawn {
                fail(Exit::Kernel, e);
            }
            return;
        }
    }

    let mut app = App::new(HEADLESS_WIDTH);
    let mut sink = HeadlessScrollback::new(HEADLESS_WIDTH);
    if let Err(e) = app.run(&mut source, &mut sink).await {
        fail(Exit::Kernel, e);
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in &sink.lines {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
}
