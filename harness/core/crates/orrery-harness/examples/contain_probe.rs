//! A process tree for `builtin::bash_is_contained` to kill.
//!
//! Two modes:
//!
//! - `spawn <beacon>` — start a copy of itself in `write` mode and then sleep
//!   for a long time. This is the **child** `builtin.bash` starts.
//! - `write <beacon>` — append a byte to `<beacon>` a few times a second,
//!   forever. This is the **grandchild**, and the one a `Child::kill` would
//!   never reach.
//!
//! A beacon that stops growing is the assertion: the containment reached a
//! process the harness never held a handle to.

use std::io::Write as _;

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    let beacon = args.next().unwrap_or_default();

    match mode.as_str() {
        "spawn" => {
            let me = std::env::current_exe().expect("a process knows where it is");
            // Never waited on, on purpose: this process is about to be killed
            // by the containment, and the grandchild has to outlive it or the
            // test proves nothing.
            #[allow(clippy::zombie_processes)]
            let _child = std::process::Command::new(me)
                .arg("write")
                .arg(&beacon)
                .spawn()
                .expect("the grandchild starts");
            // Long enough that only the containment can end this.
            std::thread::sleep(std::time::Duration::from_secs(120));
        }
        "write" => loop {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&beacon)
            {
                let _ = f.write_all(b".");
                let _ = f.flush();
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        },
        other => {
            eprintln!("contain_probe: unknown mode `{other}`");
            std::process::exit(2);
        }
    }
}
