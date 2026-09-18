//! `orrery attach` - a renderer against a running kernel.
//!
//! This is the command that makes "no privileged client" checkable: the
//! built-in TUI reaches the kernel over a named pipe, through the same
//! handshake, the same frames and the same control RPC an external client uses.
//! Nothing it does is unavailable to `node dist/index.js`.
//!
//! # Both endpoints `serve` prints
//!
//! AG-UI's transport used to have one way in — `POST /run` — whose SSE body
//! carries the frames of that run and nothing else, so an `attach` over
//! `http://…` with nothing to submit would have sat on a connection that never
//! delivered anything, and it was refused by name. `orrery-transport` now
//! serves `GET /events`: a passive subscribe over the same hub, the same
//! frames, the same `since`. Both endpoints therefore work here, and below
//! [`connect`] a renderer cannot tell a pipe from an SSE body. Only `inproc:`
//! is refused, because that is this process's own kernel and there is nothing
//! to connect to.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 4.

use std::io::Write;

use orrery_agui::Frame;
use orrery_client::{AguiSession, Endpoint};
use orrery_client_json::JsonRenderer;
use orrery_client_ratatui::{App, FrameSource};
use orrery_proto::{ReqId, Request, Seq, SessionId, UserInput};
use orrery_transport::frame::Hello;
use orrery_transport::listener::pipe::{self, PipeClient};

use crate::args::Cli;
use crate::exit::{Exit, fail};
use crate::render::{self, HeadlessScrollback};
use crate::ui::Ui;

/// How wide to draw when there is no terminal to ask.
const HEADLESS_WIDTH: u16 = 80;

/// Attach the selected renderer to an endpoint, replaying from `since`.
pub fn dispatch(cli: &Cli, endpoint: &str, since: Option<u64>, submit: Option<&str>) -> ! {
    let endpoint = endpoint
        .parse::<Endpoint>()
        .unwrap_or_else(|e| fail(Exit::Usage, e));
    if matches!(endpoint, Endpoint::Inproc) {
        fail(
            Exit::Usage,
            "`inproc:` is this process's own kernel; run `orrery` with no subcommand instead.",
        );
    }

    let ui = Ui::resolve(cli.ui, cli.json, crate::ui::stdout_is_tty());
    if matches!(ui, Ui::Ink) {
        fail(
            Exit::Usage,
            "the Ink client attaches on its own: run it with `ORRERY_ENDPOINT` set to \
             the `http://` endpoint `orrery serve` printed.",
        );
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => fail(Exit::Kernel, format!("could not start a runtime: {e}")),
    };

    let submit = submit.map(str::to_owned);
    let code = runtime.block_on(async move {
        let mut source = connect(&endpoint, since, submit.as_deref()).await;
        match ui {
            Ui::Json => drain_json(&mut source).await,
            Ui::Ratatui => drain_ratatui(&mut source).await,
            // Refused above, before anything was connected.
            Ui::Ink => Exit::Usage,
        }
    });
    code.exit()
}

/// Connect, replay from `since`, and submit if there is something to submit.
///
/// One function for both byte transports, because "no privileged client" is a
/// claim about code paths rather than about features.
async fn connect(endpoint: &Endpoint, since: Option<u64>, submit: Option<&str>) -> Source {
    match endpoint {
        Endpoint::Pipe { name } => {
            let mut client = match pipe::connect(
                name,
                Hello {
                    since,
                    ..Hello::default()
                },
            )
            .await
            {
                Ok(client) => client,
                Err(e) => fail(
                    Exit::Kernel,
                    format!("could not reach `pipe:{name}` ({e}). Is `orrery serve` still running?"),
                ),
            };
            if let Some(prompt) = submit {
                let request = Request::TurnSubmit {
                    id: ReqId::new(),
                    // The kernel serves one session; it answers on the one it
                    // has, whichever id a client names.
                    session: SessionId::new(),
                    input: UserInput::text(prompt),
                };
                if let Err(e) = client.send(&request).await {
                    fail(Exit::Kernel, format!("could not submit the turn: {e}"));
                }
            }
            Source::Pipe(Box::new(client))
        }
        Endpoint::Http { .. } => {
            let mut session = match AguiSession::connect(endpoint).await {
                Ok(session) => session,
                Err(e) => fail(
                    Exit::Kernel,
                    format!("could not reach `{endpoint}` ({e}). Is `orrery serve` still running?"),
                ),
            };
            // Subscribe *before* submitting, so a turn started here is not
            // half-missed in the window between the two calls.
            if let Err(e) = session.subscribe(since.map(Seq)).await {
                fail(
                    Exit::Kernel,
                    format!("could not subscribe to `{endpoint}` ({e})"),
                );
            }
            if let Some(prompt) = submit {
                if let Err(e) = session.attach(SessionId::new(), since.map(Seq)).await {
                    fail(Exit::Kernel, format!("could not attach to `{endpoint}`: {e}"));
                }
                if let Err(e) = session.submit(UserInput::text(prompt)).await {
                    fail(Exit::Kernel, format!("could not submit the turn: {e}"));
                }
            }
            Source::Http(Box::new(session))
        }
        Endpoint::Inproc => unreachable!("refused in `dispatch`"),
    }
}

/// A connected renderer's frames, whichever transport carried them.
enum Source {
    /// A named pipe or UDS.
    Pipe(Box<PipeClient<interprocess::local_socket::tokio::Stream>>),
    /// HTTP + SSE, subscribed through `GET /events`.
    Http(Box<AguiSession>),
}

#[async_trait::async_trait]
impl FrameSource for Source {
    async fn next_frame(&mut self) -> Option<Result<Frame, orrery_client::ClientError>> {
        match self {
            Source::Pipe(client) => client.next_frame().await.map(Ok),
            Source::Http(session) => session.next_frame().await,
        }
    }
}

impl Source {
    /// The next frame, or `None` when the stream ends.
    ///
    /// A gap is reported and then stepped over: the frame that revealed it is
    /// still good, and a renderer draws what arrives.
    async fn next(&mut self) -> Option<Frame> {
        loop {
            match FrameSource::next_frame(self).await? {
                Ok(frame) => return Some(frame),
                Err(e) => eprintln!("orrery: {e}"),
            }
        }
    }
}

/// Every frame as a line of JSON, until the run ends.
async fn drain_json(client: &mut Source) -> Exit {
    let stdout = std::io::stdout();
    let mut out = JsonRenderer::new(stdout.lock());
    while let Some(frame) = client.next().await {
        let done = render::is_run_finished(&frame);
        if let Err(e) = out.emit(&frame) {
            fail(Exit::Kernel, format!("could not write events: {e}"));
        }
        if done {
            break;
        }
    }
    let _ = out.into_inner().flush();
    Exit::Ok
}

/// The built-in TUI, or its headless twin when there is no terminal.
async fn drain_ratatui(client: &mut Source) -> Exit {
    if crate::ui::stdout_is_tty() {
        if let Ok(mut terminal) = crate::term::setup() {
            let _guard = crate::term::guard();
            let width = terminal.size().map_or(HEADLESS_WIDTH, |s| s.width);
            let mut app = App::new(width);
            let mut sink =
                orrery_client_ratatui::terminal::TerminalScrollback::new(&mut terminal, width);
            let drawn = app.run(client, &mut sink).await;
            crate::term::restore();
            if let Err(e) = drawn {
                fail(Exit::Kernel, e);
            }
            return Exit::Ok;
        }
    }

    // No terminal. Draw anyway, into a buffer, and print what the TUI would
    // have shown — a piped `attach` is still meant to show the turn.
    let mut app = App::new(HEADLESS_WIDTH);
    let mut sink = HeadlessScrollback::new(HEADLESS_WIDTH);
    while let Some(frame) = client.next().await {
        let done = render::is_run_finished(&frame);
        app.apply(&frame);
        if let Err(e) = app.flush_scrollback(&mut sink) {
            fail(Exit::Kernel, e);
        }
        if done {
            break;
        }
    }
    if let Err(e) = app.flush_scrollback(&mut sink) {
        fail(Exit::Kernel, e);
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in &sink.lines {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
    Exit::Ok
}
