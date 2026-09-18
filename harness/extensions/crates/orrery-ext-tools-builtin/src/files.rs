//! `read`, `write`, `edit` — the three that touch a file.
//!
//! None of them opens one. Every byte moves through
//! [`BrokerFacade`](orrery_ext_api::BrokerFacade), which is what makes the
//! ceiling real and the write revertible.

use orrery_ext_api::{CallCtx, ReadRequest, WriteRequest};
use orrery_proto::Outcome;
use serde_json::Value;

use crate::{bad_input, string_arg, text_outcome};

/// What one read is allowed to pull: the call's ceiling, or less if asked.
fn ceiling(ctx: &CallCtx, input: &Value) -> u64 {
    let asked = input.get("limit").and_then(Value::as_u64);
    match asked {
        Some(n) => n.min(ctx.budget.output_bytes),
        None => ctx.budget.output_bytes,
    }
}

/// Read a file, bounded while reading.
pub(crate) async fn read(input: Value, ctx: &CallCtx) -> Result<Outcome, Outcome> {
    let path = string_arg(&input, "path")?;
    let limit = ceiling(ctx, &input);
    let chunk = ctx
        .broker
        .read(ReadRequest::new(&path, limit))
        .await
        .map_err(orrery_ext_api::BrokerError::into_outcome)?;

    let text = String::from_utf8_lossy(&chunk.bytes).into_owned();
    if chunk.eof {
        return Ok(text_outcome(ctx, text));
    }
    // There was more. Say so as a value the model can act on — asking for the
    // next slice — rather than as an error somebody has to render.
    let bytes_emitted = chunk.bytes.len() as u64;
    Ok(Outcome::Truncated {
        surface: Some(ctx.ui.markdown(text, false)),
        bytes_emitted,
        limit,
    })
}

/// Replace a file's contents, all-or-nothing.
pub(crate) async fn write(input: Value, ctx: &CallCtx) -> Result<Outcome, Outcome> {
    let path = string_arg(&input, "path")?;
    let content = string_arg(&input, "content")?;
    let bytes = content.len() as u64;
    ctx.broker
        .write(WriteRequest::new(&path, content.into_bytes()))
        .await
        .map_err(orrery_ext_api::BrokerError::into_outcome)?;
    Ok(text_outcome(ctx, format!("wrote {bytes} bytes to {path}")))
}

/// Replace one run of text in a file with another.
pub(crate) async fn edit(input: Value, ctx: &CallCtx) -> Result<Outcome, Outcome> {
    let path = string_arg(&input, "path")?;
    let old = string_arg(&input, "old_text")?;
    let new = string_arg(&input, "new_text")?;
    let all = input
        .get("replace_all")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let chunk = ctx
        .broker
        .read(ReadRequest::new(&path, ctx.budget.output_bytes))
        .await
        .map_err(orrery_ext_api::BrokerError::into_outcome)?;
    if !chunk.eof {
        // Rewriting a file we have only seen the first slice of would delete the
        // rest of it. Refuse, and say why in a way a person can act on.
        return Err(bad_input(format!(
            "`{path}` is longer than this call's ceiling of {} bytes; \
             editing it would drop the rest",
            ctx.budget.output_bytes
        )));
    }
    let before = String::from_utf8_lossy(&chunk.bytes).into_owned();
    let hits = before.matches(&old).count();
    if hits == 0 {
        return Err(bad_input(format!("`{path}` does not contain that text")));
    }
    if hits > 1 && !all {
        return Err(bad_input(format!(
            "that text appears {hits} times in `{path}`; \
             pass `replace_all` or give a longer, unique `old_text`"
        )));
    }
    let after = if all {
        before.replace(&old, &new)
    } else {
        before.replacen(&old, &new, 1)
    };

    ctx.broker
        .write(WriteRequest::new(&path, after.into_bytes()))
        .await
        .map_err(orrery_ext_api::BrokerError::into_outcome)?;
    Ok(text_outcome(
        ctx,
        format!(
            "replaced {} occurrence(s) in {path}",
            if all { hits } else { 1 }
        ),
    ))
}
