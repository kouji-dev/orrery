use crate::agents::service::AgentService;
use crate::git::service::GitService;
use crate::projects::service::ProjectService;

mod agents;
pub mod cli;
mod core;
mod fs;
mod git;
mod hooks;
mod projects;
mod runtime;
mod watch;

use core::database::Database;
use runtime::RuntimeService;
use tauri::{Manager, RunEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let pool = Database::get(app);
            let git = GitService::new();
            // projects table is created first so agents can reference it later
            let project_service = ProjectService::new(pool.clone(), git.clone());
            let worktree_root = app
                .path()
                .app_data_dir()
                .expect("no app data dir")
                .join("worktrees");
            let agent_service = AgentService::new(pool, git, worktree_root);
            // reconcile stale state from a previous run (crash/force-quit): no PTY
            // process survives a restart, so any in-flight agent drops to idle.
            let _ = agent_service.reset_running();
            app.manage(project_service);
            app.manage(agent_service);
            app.manage(crate::watch::WatchService::new());
            app.manage(RuntimeService::new());
            // loopback bridge for native agent hooks (permission round-trip + status)
            match hooks::HookBridge::start(app.handle().clone()) {
                Ok(bridge) => {
                    app.manage(bridge);
                }
                Err(e) => log::error!("hook bridge failed to start: {e}"),
            }
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(core::logger::plugin())
        .invoke_handler(tauri::generate_handler![
            projects::commands::project_list,
            projects::commands::project_create,
            projects::commands::project_update,
            projects::commands::project_init_git,
            projects::commands::project_remove,
            projects::commands::project_detect_git,
            projects::commands::project_commits,
            agents::commands::agent_list,
            agents::commands::agent_spawn,
            agents::commands::agent_update,
            agents::commands::agent_remove,
            agents::commands::agent_tree,
            agents::commands::agent_dir,
            agents::commands::agent_changes,
            agents::commands::agent_commit,
            agents::commands::agent_discard,
            agents::commands::agent_merge,
            agents::commands::agent_diff,
            agents::commands::agent_watch,
            agents::commands::agent_start,
            agents::commands::agent_stop,
            agents::commands::agent_input,
            agents::commands::agent_resize,
            agents::commands::detect_tools,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|handle, event| {
            // on shutdown (for any reason we can intercept): kill every PTY
            // process and mark in-flight agents idle so a relaunch is clean.
            if let RunEvent::Exit = event {
                if let Some(rt) = handle.try_state::<RuntimeService>() {
                    rt.stop_all();
                }
                if let Some(svc) = handle.try_state::<AgentService>() {
                    let _ = svc.reset_running();
                }
            }
        });
}
