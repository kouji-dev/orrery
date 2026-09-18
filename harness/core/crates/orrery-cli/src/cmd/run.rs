//! `orrery run` - one turn, print the final text, exit.
//!
//! The non-interactive mode §5.5 is about. Two shapes, one kernel:
//!
//! - the default prints the assistant's final text on stdout, and
//! - `--json` prints the AG-UI frames the turn produced, one per line.
//!
//! **stdout is data, stderr is narration.** Every log line goes to stderr, so
//! `-vv` does not stop `--json` stdout parsing as JSONL.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 3.

use std::sync::Arc;
use std::time::Duration;

use orrery_client_json::JsonRenderer;
use orrery_kernel::TurnOutcome;
use orrery_proto::TurnId;
use tokio_util::sync::CancellationToken;

use crate::args::Cli;
use crate::exit::{Exit, fail};
use crate::render;
use crate::session::{Completed, Session};
use crate::ui::Ui;

/// Run one turn against the resolved profile.
pub fn dispatch(cli: &Cli, prompt: &str) -> ! {
    let session = crate::cmd::session(cli);
    let json = Ui::for_run(cli.ui, cli.json) == Ui::Json;

    let harness = session.harness();
    let publisher = session.publisher();
    let hub = session.hub();
    let turn = TurnId::new();
    let prompt = prompt.to_owned();

    let completed = session.harness().block_on(async move {
        // Subscribe *before* submitting, so nothing the turn emits between the
        // two is lost. A tick of zero means no coalescing: `run` is not drawing
        // at a frame budget, and a merged frame would be a frame a `--json`
        // reader never saw.
        let mut stream = hub.subscribe(Duration::ZERO);
        let running = tokio::spawn(Session::submit(
            harness,
            Arc::clone(&publisher),
            turn,
            prompt,
            CancellationToken::new(),
        ));
        if json {
            let stdout = std::io::stdout();
            let mut out = JsonRenderer::new(stdout.lock());
            if let Err(e) = render::json_until_run_finished(&mut stream, &mut out).await {
                fail(Exit::Kernel, format!("could not write events: {e}"));
            }
        }
        running.await
    });

    let completed = match completed {
        Ok(Ok(completed)) => completed,
        Ok(Err(e)) => fail(Exit::Kernel, e),
        Err(e) => fail(Exit::Kernel, format!("the turn panicked: {e}")),
    };

    if !json {
        report(&completed);
    }
    Exit::from_turn(&completed.outcome, &completed.tools).exit()
}

/// What a person sees when they did not ask for events.
///
/// The final text on stdout; anything that is not the answer on stderr, so
/// `orrery run -p … > answer.txt` contains the answer and nothing else.
fn report(completed: &Completed) {
    match &completed.outcome {
        TurnOutcome::Completed { text, .. } => println!("{text}"),
        TurnOutcome::StoppedByBudget { kind, .. } => {
            eprintln!("orrery: stopped by a ceiling ({kind:?})");
        }
        TurnOutcome::Cancelled { reason, .. } => eprintln!("orrery: cancelled ({reason:?})"),
        TurnOutcome::NeedsLogin { reason } => eprintln!("orrery: sign in first — {reason}"),
        TurnOutcome::Failed { code, message, .. } => eprintln!("orrery: {code}: {message}"),
        other => eprintln!("orrery: the turn ended in a way this build does not print: {other:?}"),
    }
}
