//! A guest component that probes the boundary from the inside.
//!
//! It is the other half of every test in `orrery-host-wasm`: the host can only
//! assert that it *configured* a sandbox, but this crate is what actually tries
//! to get out of one. It is written against `harness/wit/orrery-extension.wit`
//! and nothing else — no Orrery SDK — so it also demonstrates that the world is
//! usable with `wit-bindgen` alone.

wit_bindgen::generate!({
    path: "../../../../../../wit",
    world: "orrery-extension",
});

use exports::orrery::extension::tools::Guest;
use orrery::extension::broker;
use orrery::extension::surfaces::{NodeKind, Surface, SurfaceNode};

/// One `text` node, which is the shape every probe reports in.
fn text(value: &str) -> Surface {
    Surface {
        nodes: vec![SurfaceNode {
            kind: NodeKind::Text,
            id: None,
            status: None,
            payload: format!(
                "{{\"t\":\"text\",\"value\":{}}}",
                json_string(value)
            ),
            children: vec![],
        }],
        root: 0,
    }
}

/// Minimal JSON string escaping — the guest has no serde and does not need one.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// How a broker error reads back to the host, as a stable tag.
fn tag(e: &broker::Error) -> String {
    match e {
        broker::Error::Denied(why) => format!("denied:{why}"),
        broker::Error::Budget(why) => format!("budget:{why}"),
        broker::Error::Io(why) => format!("io:{why}"),
        broker::Error::Cancelled => "cancelled".to_owned(),
    }
}

/// A loop nothing but an epoch deadline ends.
///
/// `black_box` is load-bearing: without it LLVM proves a counting loop
/// terminates, deletes it, and the "infinite loop" test silently stops testing
/// anything. Ask how this crate is built before trusting a spin.
fn spin() -> ! {
    let mut n: u64 = 0;
    loop {
        n = std::hint::black_box(n).wrapping_add(1);
    }
}

struct Probe;

impl Guest for Probe {
    fn call(name: String, input: String) -> Result<Surface, String> {
        match name.as_str() {
            // --- the boundary ---------------------------------------------
            //
            // With zero preopens there is no directory to resolve a path
            // against, so every one of these must fail. If any of them
            // succeeds, the sandbox has a hole.
            "probe-fs" => {
                let mut findings = Vec::new();
                for path in ["/", ".", "/etc/hosts", "C:\\Windows\\win.ini"] {
                    match std::fs::read_to_string(path) {
                        Ok(_) => findings.push(format!("READ {path}")),
                        Err(e) => findings.push(format!("denied {path}: {}", e.kind() as u8)),
                    }
                }
                match std::fs::read_dir(".") {
                    Ok(_) => findings.push("LISTED .".to_owned()),
                    Err(_) => findings.push("no cwd".to_owned()),
                }
                match std::fs::write("escape.txt", b"x") {
                    Ok(()) => findings.push("WROTE escape.txt".to_owned()),
                    Err(_) => findings.push("no write".to_owned()),
                }
                Ok(text(&findings.join(" | ")))
            }

            // --- ceilings --------------------------------------------------
            "spin" => spin(),
            "grow" => {
                // Ask for far more than any ceiling. The allocator's grow
                // fails, Rust aborts, and the guest traps.
                let mut held: Vec<Vec<u8>> = Vec::new();
                loop {
                    held.push(vec![7u8; 4 * 1024 * 1024]);
                    if held.len() > 4096 {
                        return Ok(text("impossible"));
                    }
                }
            }

            // --- the broker ------------------------------------------------
            //
            // Note what these prove: the guest *handles* the error. A denial
            // arrives as a value it can branch on, so it reaches this line at
            // all, which a trap would not.
            "run-proc" => {
                let opts = broker::RunOpts {
                    timeout_ms: 1_000,
                    max_output_bytes: 4096,
                };
                match broker::run_proc("echo", &["hi".to_owned()], opts) {
                    Ok(out) => Ok(text(&format!(
                        "ran: exit={} stdout={} truncated={}",
                        out.exit_code,
                        String::from_utf8_lossy(&out.stdout).trim(),
                        out.truncated
                    ))),
                    // Handled, not propagated: the point of the test.
                    Err(e) => Ok(text(&format!("handled {}", tag(&e)))),
                }
            }
            "read-file" => {
                let max: u64 = input.trim().parse().unwrap_or(8);
                match broker::read_file("fixture.txt", max) {
                    Ok(out) => Ok(text(&format!(
                        "read {} bytes truncated={} body={}",
                        out.bytes.len(),
                        out.truncated,
                        String::from_utf8_lossy(&out.bytes)
                    ))),
                    Err(e) => Ok(text(&format!("handled {}", tag(&e)))),
                }
            }
            "write-file" => {
                match broker::write_file("out.txt", input.as_bytes(), true) {
                    Ok(()) => Ok(text("wrote")),
                    Err(e) => Ok(text(&format!("handled {}", tag(&e)))),
                }
            }
            "fetch" => {
                let opts = broker::FetchOpts {
                    method: "GET".to_owned(),
                    headers: vec![],
                    body: None,
                    timeout_ms: 1_000,
                    max_response_bytes: 4096,
                };
                match broker::fetch(&input, &opts) {
                    Ok(out) => Ok(text(&format!("status {}", out.status))),
                    Err(e) => Ok(text(&format!("handled {}", tag(&e)))),
                }
            }
            "use-credential" => {
                let usage = broker::CredUsage {
                    host: "example.invalid".to_owned(),
                    purpose: "test".to_owned(),
                };
                match broker::use_credential(&input, &usage) {
                    Ok(()) => Ok(text("attached")),
                    Err(e) => Ok(text(&format!("handled {}", tag(&e)))),
                }
            }

            // --- cancellation ---------------------------------------------
            //
            // Blocked in an import; when the broker frees it with `cancelled`,
            // the guest spins and the epoch deadline finishes the job. Both
            // halves happen, and the host asserts both.
            "blocked-then-spin" => {
                let opts = broker::RunOpts {
                    timeout_ms: 60_000,
                    max_output_bytes: 16,
                };
                let outcome = match broker::run_proc("sleep", &["60".to_owned()], opts) {
                    Ok(_) => "ran".to_owned(),
                    Err(e) => tag(&e),
                };
                // Report what the import said by writing it where the host can
                // see it even after the trap: a second broker call, which is
                // itself allowed to fail.
                let _ = broker::write_file("cancel-witness.txt", outcome.as_bytes(), true);
                spin()
            }

            // --- the arena -------------------------------------------------
            //
            // The shape task 6 compares between languages.
            "table" => Ok(Surface {
                nodes: vec![SurfaceNode {
                    kind: NodeKind::Table,
                    id: None,
                    status: None,
                    payload: "{\"t\":\"table\",\"columns\":[\"module\",\"reason\"],\
                              \"rows\":[[{\"text\":\"core\"},{\"text\":\"changed\"}]]}"
                        .to_owned(),
                    children: vec![],
                }],
                root: 0,
            }),
            "stack" => Ok(Surface {
                nodes: vec![
                    SurfaceNode {
                        kind: NodeKind::Stack,
                        id: None,
                        status: None,
                        payload: "{\"t\":\"stack\",\"dir\":\"column\",\"collapsed\":false}"
                            .to_owned(),
                        children: vec![1, 2],
                    },
                    SurfaceNode {
                        kind: NodeKind::Text,
                        id: None,
                        status: None,
                        payload: "{\"t\":\"text\",\"value\":\"one\"}".to_owned(),
                        children: vec![],
                    },
                    SurfaceNode {
                        kind: NodeKind::Text,
                        id: None,
                        status: None,
                        payload: "{\"t\":\"text\",\"value\":\"two\"}".to_owned(),
                        children: vec![],
                    },
                ],
                root: 0,
            }),
            "echo" => Ok(text(&input)),

            // A malformed arena, so the host's rebuild has something hostile to
            // reject: a child index that points backwards.
            "bad-arena" => Ok(Surface {
                nodes: vec![SurfaceNode {
                    kind: NodeKind::Stack,
                    id: None,
                    status: None,
                    payload: "{\"t\":\"stack\",\"dir\":\"column\",\"collapsed\":false}".to_owned(),
                    children: vec![0],
                }],
                root: 0,
            }),

            "fail" => Err(format!("the guest refused: {input}")),
            other => Err(format!("no such tool: {other}")),
        }
    }
}

export!(Probe);
