use crate::agents::service::AgentService;
use crate::git::service::GitService;
use crate::projects::service::ProjectService;
use crate::tickets::service::TicketService;

mod agents;
mod appicon;
pub mod cli;
mod core;
mod cost;
mod fs;
mod git;
mod hooks;
mod metrics;
mod perf;
mod projects;
mod tickets;
mod runtime;
mod search;
mod settings;
mod update;
mod watch;

use core::database::Database;
use runtime::jobobj::JobGuard;
use runtime::RuntimeService;
use tauri::{Manager, RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Open at 80% of the CURRENT monitor (clamped to a sane floor), then
            // re-center — the config ships a fixed 1600×800 that overflows small
            // laptops and looks tiny on large displays. Runtime, because 80% needs
            // the live screen size; a failed monitor query leaves the config size.
            if let Some(win) = app.get_webview_window("main") {
                if let Ok(Some(monitor)) = win.current_monitor() {
                    let screen = monitor.size(); // physical px
                    let w = ((screen.width as f64 * 0.8).round() as u32).max(1000);
                    let h = ((screen.height as f64 * 0.8).round() as u32).max(600);
                    let _ = win.set_size(tauri::PhysicalSize::new(w, h));
                    let _ = win.center();
                }
            }
            // A7.7: record our WebView2 browser-process pid. WebView2 shares one
            // browser family per user-data-dir, so with a second Orrery instance
            // running our UI processes parent under THAT instance — the process
            // tree attaches this family to our root explicitly instead of
            // trusting the ppid walk.
            let webview_family = metrics::WebviewFamily::default();
            app.manage(webview_family.clone());
            #[cfg(windows)]
            if let Some(win) = app.get_webview_window("main") {
                let fam = webview_family.clone();
                let _ = win.with_webview(move |pv| unsafe {
                    let controller = pv.controller();
                    if let Ok(core) = controller.CoreWebView2() {
                        let mut pid = 0u32;
                        if core.BrowserProcessId(&mut pid).is_ok() && pid != 0 {
                            fam.0.lock().unwrap().push(pid);
                        }
                    }
                });
            }
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
            // settings live in the same DB; the agent service consults them at
            // spawn time (branch template + worktree root)
            let settings_service = settings::SettingsService::new(pool.clone());
            let worktree_root = app
                .path()
                .app_data_dir()
                .expect("no app data dir")
                .join("worktrees");
            let ticket_service = TicketService::new(pool.clone());
            let agent_service =
                AgentService::new(pool, git, worktree_root, settings_service.clone());
            // Snapshot in-flight agents BEFORE reconciling: reset_running flips
            // them to idle, and the auto-resume flow (agents_interrupted, a
            // one-shot drain) needs to know who was interrupted by the restart.
            let interrupted = agent_service.running_ids().unwrap_or_default();
            // reconcile stale state from a previous run (crash/force-quit): no PTY
            // process survives a restart, so any in-flight agent drops to idle.
            let _ = agent_service.reset_running();
            app.manage(crate::agents::service::InterruptedAgents::new(interrupted));
            // A0.7 Phase 1: wire the emit-telemetry funnel — telemetry dir,
            // the persisted raw-trace toggle, and a notifier that (a) tells the
            // UI on `telemetry://trace` and (b) persists an auto-disable back
            // into settings so the toggle never lies after a restart.
            {
                let telemetry_dir = app
                    .path()
                    .app_data_dir()
                    .expect("no app data dir")
                    .join("telemetry");
                let raw_on = settings_service
                    .get()
                    .map(|s| s.telemetry_raw_trace)
                    .unwrap_or(false);
                let notify_app = app.handle().clone();
                let notify_settings = settings_service.clone();
                core::emit::init(
                    telemetry_dir,
                    raw_on,
                    Box::new(move |active, reason| {
                        // Auto-disable (cap hit) → write the toggle off in the DB.
                        // User-driven changes already persisted via settings_set.
                        if !active && reason != "user" {
                            if let Ok(mut s) = notify_settings.get() {
                                if s.telemetry_raw_trace {
                                    s.telemetry_raw_trace = false;
                                    let _ = notify_settings.set(&s);
                                }
                            }
                        }
                        let _ = core::emit::emit_tracked(
                            &notify_app,
                            "telemetry://trace",
                            &serde_json::json!({ "active": active, "reason": reason }),
                        );
                    }),
                );
            }
            app.manage(settings_service);
            app.manage(project_service);
            app.manage(ticket_service);
            app.manage(agent_service);
            app.manage(crate::watch::WatchService::new());
            app.manage(crate::search::SearchService::new());
            app.manage(RuntimeService::new());
            // loopback bridge for native agent hooks (permission round-trip + status)
            match hooks::HookBridge::start(app.handle().clone()) {
                Ok(bridge) => {
                    app.manage(bridge);
                }
                Err(e) => log::error!("hook bridge failed to start: {e}"),
            }
            // Install orrery's status/needs-input hooks GLOBALLY (merged into the
            // user's real config). Harmless for non-orrery runs — the hook only
            // brokers when the ORRERY_* env is present. Best-effort: never abort
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
            // Sampler + last snapshot live in shared managed state so the one-shot
            // `system_metrics` command reuses the warm sampler / fresh snapshot
            // instead of paying a cold double-sweep + sleep per call.
            let shared_sampler = metrics::SharedSampler::new();
            app.manage(shared_sampler.clone());
            let metrics_app = app.handle().clone();
            std::thread::spawn(move || {
                use tauri::Manager;
                let app_pid = std::process::id();
                shared_sampler.warm_up(); // first sample's cpu% would otherwise be 0
                let mut tick: u32 = 0;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    tick = tick.wrapping_add(1);
                    // Why adaptive: even the scoped refresh costs real syscalls,
                    // and resource gauges are the LEAST important computation in
                    // the app — they must never compete with agents or the UI.
                    // 5s while agents run, every 4th tick (20s) when idle; back
                    // to 5s within one tick of an agent starting.
                    let active = metrics_app
                        .try_state::<RuntimeService>()
                        .is_some_and(|rt| rt.any_running());
                    if !active && tick % 4 != 0 {
                        continue;
                    }
                    // timed so the recurring sweep shows in the perf table —
                    // its cost is otherwise invisible (only the one-shot
                    // command path was measured) and it's a steady-state
                    // baseline-CPU suspect.
                    crate::perf::timed("system_metrics_push", || {
                        let (Some(runtime), Some(agents)) = (
                            metrics_app.try_state::<RuntimeService>(),
                            metrics_app.try_state::<AgentService>(),
                        ) else {
                            return; // services not ready yet (shouldn't happen post-setup)
                        };
                        let pids = runtime.pids();
                        let labels = metrics::commands::agent_labels(&agents);
                        let m = shared_sampler.refresh_and_sample(app_pid, &pids, &labels);
                        let _ = crate::core::emit::emit_tracked(&metrics_app, "system://metrics", &m);
                    });
                }
            });

            // Perf exec-aggregate push loop (perf://stats every 2s; see src/perf).
            perf::spawn_push_loop(app.handle().clone());
            perf::spawn_ui_probe(app.handle().clone()); // UI-thread queue-delay gauge (ui_thread_lag row)

            // Cost push loop: shell out to `ccusage` and emit the global total on
            // `system://cost`. `available:false` is emitted when ccusage can't run
            // — the UI hides the readout. This loop is the ONLY thing that runs
            // ccusage: the one-shot `system_cost` command just peeks the shared
            // CostCache (A1.8 — no second node process, no thread parked behind
            // the loop's in-flight startup run).
            // The cache stays managed even with the kill switch off — the
            // registered `system_cost` command takes State<CostCache> and an
            // unmanaged state would panic the invoke; cold peek returns None.
            let cost_cache = cost::CostCache::new();
            app.manage(cost_cache.clone());
            let cost_app = app.handle().clone();
            if cost::COST_FEATURES_ENABLED {
            std::thread::spawn(move || {
                loop {
                    // timed: measured ~2.2s per cycle (npx resolution + node +
                    // full transcript scan). At the old 60s period that was
                    // ~3.7% of a core forever — a daily cost total does not
                    // need minute freshness, so 5 minutes. First emit is still
                    // immediate (loop body runs before the first sleep) — and
                    // deduped against the startup one-shot via the cache.
                    let snap = crate::perf::timed("system_cost_push", || {
                        cost_cache.get(cost::COST_FRESH)
                    });
                    let _ = crate::core::emit::emit_tracked(&cost_app, "system://cost", &snap);
                    std::thread::sleep(std::time::Duration::from_secs(300));
                }
            });
            } else {
                let _ = cost_app; // loop consumer — unused while the switch is off
            }

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(core::logger::plugin())
        .invoke_handler(tauri::generate_handler![
            projects::commands::project_list,
            projects::commands::project_create,
            projects::commands::project_update,
            projects::commands::project_init_git,
            projects::commands::project_remove,
            projects::commands::project_detect_git,
            projects::commands::project_commits,
            tickets::commands::ticket_list,
            tickets::commands::ticket_create,
            tickets::commands::ticket_update,
            tickets::commands::ticket_remove,
            tickets::commands::ticket_set_status,
            tickets::commands::comment_list,
            tickets::commands::comment_add,
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
            agents::commands::runtime_subscribe,
            agents::commands::runtime_snapshot,
            agents::commands::agents_interrupted,
            agents::commands::detect_tools,
            agents::commands::verify_tool_path,
            agents::commands::agent_commit_diff,
            agents::commands::agent_commit_file_diff,
            agents::commands::agent_range_files,
            agents::commands::agent_range_file_diff,
            agents::commands::agent_blame,
            agents::commands::agent_working_blame,
            agents::commands::agent_file_history,
            agents::commands::agent_merge,
            agents::commands::agent_conflicts,
            agents::commands::agent_conflict_resolve,
            agents::commands::agent_merge_abort,
            agents::commands::agent_merge_continue,
            agents::commands::agent_session_state,
            search::commands::search_start,
            search::commands::search_cancel,
            search::commands::search_files,
            settings::commands::settings_get,
            settings::commands::settings_set,
            metrics::commands::system_metrics,
            metrics::commands::process_tree,
            core::emit::telemetry_emits,
            core::emit::telemetry_trace_state,
            cost::commands::system_cost,
            appicon::set_window_icon,
            update::update_check,
            update::update_install,
            core::commands::open_log,
            core::commands::open_path,
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
