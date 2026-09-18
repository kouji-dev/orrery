//! `orrery serve` - kernel only; print the endpoint; keep running.
//!
//! # Two listeners, because two clients
//!
//! With no `--listen`, `serve` opens **both** phase-1 byte transports over one
//! hub and prints both endpoints, most-local first:
//!
//! ```text
//! pipe:orrery-<session>
//! http://127.0.0.1:53124
//! ```
//!
//! Open question 2, decided: one string with a scheme prefix, and the grammar
//! is [`orrery_client::Endpoint`]'s — `attach` parses it with `FromStr` and
//! `ORRERY_ENDPOINT` carries the same string, so there is one spelling and one
//! parser rather than a URL for HTTP and something else for pipes. `--listen`
//! picks one transport: `pipe:<name>`, or a `host:port` for HTTP.
//!
//! Sessions outlive clients: nothing here counts connections, and the process
//! runs until it is stopped.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 4.

use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use orrery_transport::listener::{ControlHandler, http, pipe};

use crate::args::Cli;
use crate::control::KernelControl;
use crate::exit::{Exit, fail};

/// How often a connected client's frames are merged. 33ms is a frame at 30Hz,
/// which is what a terminal can actually show.
pub const TICK: Duration = Duration::from_millis(33);

/// Start a kernel with no client attached.
pub fn dispatch(cli: &Cli, listen: Option<&str>) -> ! {
    let session = crate::cmd::session(cli);
    let control = KernelControl::new(&session);
    let hub = session.hub();
    let id = session.id();

    let endpoints = session.harness().block_on(async {
        let mut endpoints: Vec<String> = Vec::new();
        let want_pipe = listen.is_none_or(|l| l.starts_with("pipe:"));
        let want_http = listen.is_none_or(|l| !l.starts_with("pipe:"));

        if want_pipe {
            let name = listen
                .and_then(|l| l.strip_prefix("pipe:"))
                .map_or_else(|| format!("orrery-{id}"), str::to_owned);
            // `pipe::serve` takes the handler by value; a closure over the
            // `Arc` is how one handler serves both listeners.
            let forward = {
                let control = Arc::clone(&control);
                move |request| control.control(request)
            };
            match pipe::serve(&name, hub.clone(), TICK, forward) {
                // Leaked on purpose: the server lives as long as the process,
                // and dropping the handle would stop accepting.
                Ok(server) => {
                    endpoints.push(format!("pipe:{}", server.name()));
                    std::mem::forget(server);
                }
                Err(e) => fail(Exit::Kernel, format!("could not open the pipe: {e}")),
            }
        }

        if want_http {
            let addr: SocketAddr = match listen {
                Some(l) => match l.trim_start_matches("http://").parse() {
                    Ok(addr) => addr,
                    Err(_) => fail(
                        Exit::Usage,
                        format!("`{l}` is not an address: expected `host:port` or `pipe:<name>`"),
                    ),
                },
                None => SocketAddr::from(([127, 0, 0, 1], 0)),
            };
            match http::serve(
                addr,
                hub.clone(),
                Arc::clone(&control) as Arc<dyn orrery_transport::listener::ControlHandler>,
                http::HttpConfig {
                    token: None,
                    tick: TICK,
                },
            )
            .await
            {
                Ok(server) => {
                    endpoints.push(server.base_url());
                    std::mem::forget(server);
                }
                Err(e) => fail(Exit::Kernel, format!("could not listen on {addr}: {e}")),
            }
        }
        endpoints
    });

    // stdout is data: the endpoints and nothing else, so
    // `ORRERY_ENDPOINT=$(orrery serve | head -1)` is a working line.
    {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        for endpoint in &endpoints {
            let _ = writeln!(out, "{endpoint}");
        }
        let _ = out.flush();
    }
    eprintln!("orrery: session {id}; ^C to stop");

    // Sessions outlive clients.
    session.harness().block_on(std::future::pending::<()>());
    Exit::Ok.exit()
}
