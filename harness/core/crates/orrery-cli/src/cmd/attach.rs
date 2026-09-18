//! `orrery attach` - a renderer against a running kernel.
//!
//! This is the command that makes "no privileged client" checkable: the
//! built-in TUI reaches the kernel over a named pipe, through the same
//! handshake, the same frames and the same control RPC an external client uses.
//! Nothing it does is unavailable to `node dist/index.js`.
//!
//! # HTTP is run-scoped
//!
//! AG-UI's transport has one way in — `POST /run` — and its SSE body carries
//! that run's frames. There is no passive subscribe route, so an `attach` over
//! `http://…` with nothing to submit would sit on a connection that never
//! delivers anything. It is refused by name rather than left to hang; the pipe
//! endpoint `serve` prints is the one to attach to.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 4.

use std::io::Write;

use orrery_agui::Frame;
use orrery_client::Endpoint;
use orrery_client_json::JsonRenderer;
use orrery_client_ratatui::{App, FrameSource};
use orrery_proto::{ReqId, Request, SessionId, UserInput};
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
    let name = match &endpoint {
        Endpoint::Pipe { name } => name.clone(),
        Endpoint::Http { .. } => fail(
            Exit::Usage,
            "`attach` needs a pipe endpoint. AG-UI's HTTP transport is run-scoped — \
             frames arrive on the body of a `POST /run` — so there is nothing for a \
             passive attach to subscribe to. Use the `pipe:` endpoint `orrery serve` printed.",
        ),
        Endpoint::Inproc => fail(
            Exit::Usage,
            "`inproc:` is this process's own kernel; run `orrery` with no subcommand instead.",
        ),
    };

    let ui = Ui::resolve(cli.ui, cli.json, crate::ui::stdout_is_tty());
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => fail(Exit::Kernel, format!("could not start a runtime: {e}")),
    };

    let code = runtime.block_on(async move {
        let mut client = match pipe::connect(
            &name,
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
                // The kernel serves one session; it answers on the one it has,
                // whichever id a client names.
                session: SessionId::new(),
                input: UserInput::text(prompt),
            };
            if let Err(e) = client.send(&request).await {
                fail(Exit::Kernel, format!("could not submit the turn: {e}"));
            }
        }
        match ui {
            Ui::Json => drain_json(&mut client).await,
            Ui::Ink => fail(
                Exit::Usage,
                "the Ink client attaches on its own: run it with `ORRERY_ENDPOINT` set to \
                 the `http://` endpoint `orrery serve` printed.",
            ),
            Ui::Ratatui => drain_ratatui(&mut client).await,
        }
    });
    code.exit()
}

/// Every frame as a line of JSON, until the run ends.
async fn drain_json(
    client: &mut PipeClient<interprocess::local_socket::tokio::Stream>,
) -> Exit {
    let stdout = std::io::stdout();
    let mut out = JsonRenderer::new(stdout.lock());
    while let Some(frame) = client.next_frame().await {
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
async fn drain_ratatui(
    client: &mut PipeClient<interprocess::local_socket::tokio::Stream>,
) -> Exit {
    if crate::ui::stdout_is_tty() {
        if let Ok(mut terminal) = crate::term::setup() {
            let _guard = crate::term::guard();
            let width = terminal.size().map_or(HEADLESS_WIDTH, |s| s.width);
            let mut app = App::new(width);
            let mut source = PipeSource { client };
            let mut sink =
                orrery_client_ratatui::terminal::TerminalScrollback::new(&mut terminal, width);
            if let Err(e) = app.run(&mut source, &mut sink).await {
                drop(sink);
                crate::term::restore();
                fail(Exit::Kernel, e);
            }
            return Exit::Ok;
        }
    }

    // No terminal. Draw anyway, into a buffer, and print what the TUI would
    // have shown — a piped `attach` is still meant to show the turn.
    let mut app = App::new(HEADLESS_WIDTH);
    let mut sink = HeadlessScrollback::new(HEADLESS_WIDTH);
    while let Some(frame) = client.next_frame().await {
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

/// A [`FrameSource`] over a connected pipe.
struct PipeSource<'a> {
    client: &'a mut PipeClient<interprocess::local_socket::tokio::Stream>,
}

#[async_trait::async_trait]
impl FrameSource for PipeSource<'_> {
    async fn next_frame(&mut self) -> Option<Result<Frame, orrery_client::ClientError>> {
        self.client.next_frame().await.map(Ok)
    }
}
