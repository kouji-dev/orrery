//! `bash` — a shell command, contained by whoever creates the process.
//!
//! The tool does not spawn anything. It hands a [`SpawnRequest`] to the broker,
//! and the broker starts the process inside a job object (Windows) or its own
//! process group (unix), pumps its output against the ceiling and kills the
//! whole tree when the wall clock runs out. A `spawn` rule decided whether to
//! ask; **this** is where what can run is decided.

use orrery_ext_api::{CallCtx, SpawnRequest};
use orrery_proto::Outcome;
use serde_json::Value;

use crate::{string_arg, text_outcome};

/// The shell, and the flag that makes it take one string.
fn shell() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("/bin/sh", "-c")
    }
}

/// Run a command and report what it said.
pub(crate) async fn run(input: Value, ctx: &CallCtx) -> Result<Outcome, Outcome> {
    let command = string_arg(&input, "command")?;
    let (program, flag) = shell();

    let mut request = SpawnRequest::new(program, [flag.to_owned(), command.clone()]);
    if let Some(cwd) = input.get("cwd").and_then(Value::as_str) {
        request = request.in_dir(cwd);
    }
    request.timeout_ms = input
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .map(|ms| ms.min(ctx.budget.wall_clock_ms));
    request.output_bytes = Some(ctx.budget.output_bytes);

    let out = ctx
        .broker
        .spawn(request)
        .await
        .map_err(orrery_ext_api::BrokerError::into_outcome)?;

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let mut text = stdout;
    if !stderr.is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&stderr);
    }

    if out.truncated {
        let bytes_emitted = text.len() as u64;
        return Ok(Outcome::Truncated {
            surface: Some(ctx.ui.markdown(text, false)),
            bytes_emitted,
            limit: ctx.budget.output_bytes,
        });
    }
    match out.status {
        Some(0) | None => Ok(text_outcome(ctx, text)),
        // A non-zero exit is not a harness failure: the command ran, and what it
        // said is what the model needs to see next.
        Some(code) => Ok(Outcome::Failed {
            code: format!("exit-{code}"),
            message: if text.is_empty() {
                format!("`{command}` exited {code}")
            } else {
                text
            },
        }),
    }
}
