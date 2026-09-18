//! The script a skill bundles, as a committed fixture.
//!
//! Real skills bundle shell or Python; this is Rust so that the `scripts::`
//! tests behave the same on every platform the harness is built on, and so that
//! there is no chance whatsoever of a test reaching the network.
//!
//! It speaks the effect channel described in `orrery_skills::scripts`: one JSON
//! object per line on stdout asks the host to do something, anything else is
//! plain output. The script itself touches nothing — which is the point. What
//! it asks for is the grant's business, not this program's.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        // `write <path> <contents>`
        Some("write") => {
            println!("writing {}", args[1]);
            println!(
                "{}",
                write_request(&args[1], args.get(2).map_or("", String::as_str))
            );
        }
        // `write-two <inside> <outside>` — one write the caller expects to be
        // inside the grant, one it expects to be refused.
        Some("write-two") => {
            println!("{}", write_request(&args[1], "inside"));
            println!("{}", write_request(&args[2], "outside"));
        }
        // Never returns. The budget is what stops it.
        Some("spin") => loop {
            std::hint::spin_loop();
        },
        _ => println!("hello from a skill script"),
    }
}

/// `{"effect":"write","path":...,"contents":...}`, without a serde dependency:
/// a fixture binary should not drag the crate's dependency graph around with it.
fn write_request(path: &str, contents: &str) -> String {
    format!(
        "{{\"effect\":\"write\",\"path\":{},\"contents\":{}}}",
        quote(path),
        quote(contents)
    )
}

/// The JSON string escapes that matter here. Windows paths are full of
/// backslashes, so this is not decoration.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str(r#"\""#),
            '\\' => out.push_str(r"\\"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
