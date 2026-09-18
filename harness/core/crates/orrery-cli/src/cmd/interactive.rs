//! Bare `orrery`: a kernel in this process, with a renderer on the hub.
//!
//! Which renderer is [`Ui::resolve`]'s answer — ratatui on a tty, json when
//! stdout is a pipe, Ink when asked. All three are subscribers: the one linked
//! into this binary attaches to the same [`Hub`](orrery_transport::Hub) the
//! external ones reach over a transport, and submits through the same control
//! RPC. That is what "no privileged client" means in code.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` tasks 1, 2 and 10.

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::Duration;

use orrery_client_json::JsonRenderer;
use orrery_client_ratatui::{App, Outgoing};

use crate::args::Cli;
use crate::control::KernelControl;
use crate::exit::{Exit, fail};
use crate::render;
use crate::ui::Ui;

/// Start an interactive session.
pub fn dispatch(cli: &Cli) -> ! {
    match Ui::resolve(cli.ui, cli.json, crate::ui::stdout_is_tty()) {
        Ui::Ink => ink(cli),
        Ui::Json => json(cli),
        Ui::Ratatui => ratatui(cli),
    }
}

/// `--ui ink`: serve the kernel over HTTP and hand the endpoint to Node.
///
/// The Ink client is told where to attach in `ORRERY_ENDPOINT` and nothing
/// else, because that is the entire contract a third-party client has to
/// implement.
fn ink(cli: &Cli) -> ! {
    // Fail on the missing runtime *before* building a kernel: a person who
    // typed `--ui ink` on a box with no Node should get one sentence, not a
    // session that starts and then cannot be drawn.
    if !crate::ui::node_on_path() {
        fail(Exit::Usage, crate::ui::InkError::NoNode);
    }
    let bundle = crate::ui::ink_bundle();
    if !bundle.exists() {
        fail(Exit::Usage, crate::ui::InkError::NotBuilt(bundle));
    }

    let session = crate::cmd::session(cli);
    let control = KernelControl::new(&session);
    let hub = session.hub();
    let id = session.id();
    let url = session.harness().block_on(async {
        match orrery_transport::listener::http::serve(
            std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            hub,
            Arc::clone(&control) as Arc<dyn orrery_transport::listener::ControlHandler>,
            orrery_transport::listener::http::HttpConfig {
                token: None,
                tick: crate::cmd::serve::TICK,
            },
        )
        .await
        {
            Ok(server) => {
                let url = server.base_url();
                std::mem::forget(server);
                url
            }
            Err(e) => fail(Exit::Kernel, format!("could not listen: {e}")),
        }
    });

    let mut child = match crate::ui::spawn_ink(&url, &id.to_string()) {
        Ok(child) => child,
        Err(e) => fail(Exit::Usage, e),
    };
    match child.wait() {
        Ok(status) if status.success() => Exit::Ok.exit(),
        Ok(_) => Exit::TaskFailed.exit(),
        Err(e) => fail(Exit::Kernel, format!("the Ink client would not run: {e}")),
    }
}

/// No terminal: one prompt per line of stdin, one stream of events on stdout.
///
/// The shape a CI job and an adapter want, and the reason the renderer default
/// looks at the tty at all.
fn json(cli: &Cli) -> ! {
    let session = crate::cmd::session(cli);
    let control = KernelControl::new(&session);
    let hub = session.hub();
    let mut code = Exit::Ok;

    for line in std::io::stdin().lock().lines() {
        let prompt = match line {
            Ok(line) => line.trim().to_owned(),
            Err(e) => fail(Exit::Kernel, format!("could not read stdin: {e}")),
        };
        if prompt.is_empty() {
            continue;
        }
        let hub = hub.clone();
        let control = Arc::clone(&control);
        session.harness().block_on(async move {
            let mut stream = hub.subscribe(Duration::ZERO);
            control.submit(prompt);
            let stdout = std::io::stdout();
            let mut out = JsonRenderer::new(stdout.lock());
            if let Err(e) = render::json_until_run_finished(&mut stream, &mut out).await {
                fail(Exit::Kernel, format!("could not write events: {e}"));
            }
            let _ = out.into_inner().flush();
        });
        code = Exit::Ok;
    }
    code.exit()
}

/// The built-in TUI on the in-process transport.
fn ratatui(cli: &Cli) -> ! {
    let session = crate::cmd::session(cli);
    let control = KernelControl::new(&session);
    let hub = session.hub();

    let mut terminal = match crate::term::setup() {
        Ok(terminal) => terminal,
        Err(e) => fail(
            Exit::Kernel,
            format!("could not set the terminal up: {e}. Pipe the output for `--ui json`."),
        ),
    };
    let _guard = crate::term::guard();
    let width = terminal.size().map_or(80, |s| s.width);

    let outcome = session.harness().block_on(async move {
        let mut stream = hub.subscribe(crate::cmd::serve::TICK);
        let mut keys = key_reader();
        let mut app = App::new(width);
        loop {
            tokio::select! {
                batch = stream.next_batch() => {
                    let Some(batch) = batch else { break };
                    for frame in &batch {
                        app.apply(frame);
                    }
                }
                key = keys.recv() => {
                    let Some(key) = key else { break };
                    for out in app.key(key) {
                        match out {
                            Outgoing::Submit(text) => {
                                control.submit(text);
                            }
                            Outgoing::Exit => return Ok(()),
                            // Cancel, consent and intent all ride the same
                            // control RPC an external client uses.
                            Outgoing::Cancel | Outgoing::Consent { .. }
                            | Outgoing::Intent { .. } | Outgoing::Reattach { .. } => {}
                        }
                    }
                }
            }
            let mut sink =
                orrery_client_ratatui::terminal::TerminalScrollback::new(&mut terminal, width);
            app.flush_scrollback(&mut sink)?;
            drop(sink);
            terminal.draw(|f| {
                let area = f.area();
                app.draw(&mut ratatui::buffer::Buffer::empty(area));
            })?;
        }
        Ok::<(), std::io::Error>(())
    });

    crate::term::restore();
    match outcome {
        Ok(()) => Exit::Ok.exit(),
        Err(e) => fail(Exit::Kernel, e),
    }
}

/// Key events, off the blocking reader `crossterm` gives and onto the loop.
fn key_reader() -> tokio::sync::mpsc::UnboundedReceiver<crossterm::event::KeyEvent> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(event) = crossterm::event::read() {
            if let crossterm::event::Event::Key(key) = event
                && tx.send(key).is_err()
            {
                return;
            }
        }
    });
    rx
}
