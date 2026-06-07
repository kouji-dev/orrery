use crate::agents::service::AgentService;
use crate::git::service::GitService;
use crate::projects::service::ProjectService;

mod agents;
pub mod cli;
mod core;
mod fs;
mod git;
mod hooks;
mod metrics;
mod projects;
mod runtime;
mod watch;

use core::database::Database;
use runtime::jobobj::JobGuard;
use runtime::RuntimeService;
use tauri::{Manager, RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Install the OS process-tree kill mechanism FIRST so every process
            // we later spawn (agents + their subtrees) inherits it. On Windows
            // this is a kill-on-close Job Object; the guard must live for the
            // whole process lifetime, so we hand it to Tauri managed state and
            // never drop it early. Best-effort: failures are logged, not fatal.
            app.manage(JobGuard::install());
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
            // Install katrix's status/needs-input hooks GLOBALLY (merged into the
            // user's real config). Harmless for non-katrix runs — the hook only
            // brokers when the KATRIX_* env is present. Best-effort: never abort
            // startup if this fails.
            match (app.path().home_dir(), crate::hooks::hook_binary()) {
                (Ok(home), Some(hook_bin)) => {
                    crate::agents::adapters::install_global_hooks(&home, &hook_bin);
                }
                (Err(e), _) => log::warn!("global hook install skipped: no home dir: {e}"),
                (_, None) => log::warn!("global hook install skipped: no hook binary"),
            }

            // Metrics push loop: sample the app + every running agent's process
            // subtree (cpu%/mem) every 3s and emit `system://metrics`. sysinfo
            // derives per-process cpu% from the delta between two refreshes, so we
            // warm up with one refresh before the loop, then refresh+emit each tick.
            let metrics_app = app.handle().clone();
            std::thread::spawn(move || {
                use tauri::{Emitter, Manager};
                let app_pid = std::process::id();
                let mut sampler = metrics::MetricsSampler::new();
                sampler.refresh(); // warm-up: first sample's cpu% would otherwise be 0
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    sampler.refresh();
                    let (Some(runtime), Some(agents)) = (
                        metrics_app.try_state::<RuntimeService>(),
                        metrics_app.try_state::<AgentService>(),
                    ) else {
                        continue; // services not ready yet (shouldn't happen post-setup)
                    };
                    let m = metrics::commands::sample_with_labels(
                        &sampler, app_pid, &runtime, &agents,
                    );
                    let _ = metrics_app.emit("system://metrics", m);
                }
            });

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
            agents::commands::agent_commits,
            agents::commands::agent_commit,
            agents::commands::agent_discard,
            agents::commands::agent_push,
            agents::commands::agent_action,
            agents::commands::agent_diff,
            agents::commands::agent_watch,
            agents::commands::agent_start,
            agents::commands::agent_stop,
            agents::commands::agent_input,
            agents::commands::agent_allow,
            agents::commands::agent_deny,
            agents::commands::agent_decide,
            agents::commands::agent_resize,
            agents::commands::detect_tools,
            metrics::commands::system_metrics,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|handle, event| {
            // Tear down every PTY process (and, on Unix, its process group) and
            // mark in-flight agents idle so a relaunch is clean. Idempotent, so
            // firing on multiple shutdown signals is safe.
            //
            // NOTE: this graceful path only runs for shutdowns we can intercept.
            // A force-kill/crash fires NO RunEvent — that case is covered on
            // Windows by the kill-on-close Job Object installed at startup.
            let teardown = || {
                if let Some(rt) = handle.try_state::<RuntimeService>() {
                    rt.stop_all();
                }
                if let Some(svc) = handle.try_state::<AgentService>() {
                    let _ = svc.reset_running();
                }
            };
            match event {
                // Event loop is exiting (final).
                RunEvent::Exit => teardown(),
                // App is about to exit (e.g. last window closed / programmatic
                // exit). Tear down here too so agents die before we leave the UI.
                RunEvent::ExitRequested { .. } => teardown(),
                // A window-close path: catch it so closing the window also tears
                // agents down, even if the app lingers afterward.
                RunEvent::WindowEvent {
                    event: WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed,
                    ..
                } => teardown(),
                _ => {}
            }
        });
}
