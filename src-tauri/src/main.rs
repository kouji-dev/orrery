// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The same exe doubles as a CLI: a recognised subcommand (e.g. `orrery hook
    // --event PreToolUse`) runs it and exits before any Tauri init; a bare launch
    // starts the desktop app.
    if orrery_lib::cli::invoked_as_cli() {
        orrery_lib::cli::run();
        return;
    }
    orrery_lib::run()
}
