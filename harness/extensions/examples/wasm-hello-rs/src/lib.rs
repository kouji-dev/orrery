//! The Rust example: one tool, written with `orrery-guest`.
//!
//! Compare it with `../wasm-hello-go`, which is the same tool written against
//! the raw `.wit` with `wit-bindgen` and no SDK of ours. Both produce the same
//! surface, which is the point: the world is the contract, and an SDK is a
//! convenience on top of it rather than a requirement.

use orrery_guest::{Ctx, ui};

orrery_guest::export_extension! {
    // Note what the author never touches: an arena index. `ui::table` builds a
    // node, the SDK flattens it.
    "hello" => |input: &str, _ctx: &Ctx| -> Result<ui::Node, String> {
        let who = if input.trim().is_empty() { "world" } else { input.trim() };
        Ok(ui::section(
            "hello",
            vec![
                ui::text(&format!("hello, {who}")),
                ui::table(
                    &["language", "sdk"],
                    &[vec!["rust".to_owned(), "orrery-guest".to_owned()]],
                ),
            ],
        ))
    },

    // The fourth writing of one tool. `native-hello`, `node-hello` and
    // `wasm-hello-go` return exactly this, and
    // `orrery-harness/tests/parity.rs` asserts the four are equal. No input,
    // no capability: the comparison is about dispatch, not about policy.
    "parity" => |_input: &str, _ctx: &Ctx| -> Result<ui::Node, String> {
        Ok(ui::table(
            &["key", "value"],
            &[
                vec!["tool".to_owned(), "parity".to_owned()],
                vec!["runtime".to_owned(), "irrelevant".to_owned()],
            ],
        ))
    },

    // A denial is a value. This tool asks for something it was probably not
    // granted, and reports what it was told instead of failing.
    "try-spawn" => |_input: &str, ctx: &Ctx| -> Result<ui::Node, String> {
        match ctx.proc.run("git", &["status", "--porcelain"]) {
            Ok(out) => Ok(ui::text(&format!(
                "git said {} ({} bytes)",
                out.exit_code,
                out.stdout.len()
            ))),
            Err(e) => Ok(ui::text(&format!("not this time: {e}"))),
        }
    },
}
