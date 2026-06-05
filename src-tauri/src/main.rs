// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `katrix __hook <EVENT>`: act as an agent hook (forward to the bridge, print
    // the decision) and exit before any Tauri init. Normal launch has no extra args.
    let mut args = std::env::args();
    let _bin = args.next();
    if args.next().as_deref() == Some(katrix_lib::HOOK_SUBCOMMAND) {
        katrix_lib::run_hook(args.next().unwrap_or_default());
        return;
    }
    katrix_lib::run()
}
