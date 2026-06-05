use crate::git::service::GitService;
use crate::projects::service::ProjectService;

mod core;
mod git;
mod projects;

use core::database::Database;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;
            let pool = Database::get(app);
            let project_service = ProjectService::new(pool, GitService::new());
            app.manage(project_service);
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(core::logger::plugin())
        .invoke_handler(tauri::generate_handler![
            projects::commands::project_list,
            projects::commands::project_create,
            projects::commands::project_remove,
            projects::commands::project_detect_git,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
