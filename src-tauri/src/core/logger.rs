pub fn plugin<R>() -> tauri::plugin::TauriPlugin<R>
where
    R: tauri::Runtime,
{
    tauri_plugin_log::Builder::new()
        .target(tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::Stdout,
        ))
        .target(tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::Webview,
        ))
        .target(tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::LogDir {
                file_name: Some("orrery".to_string()),
            },
        ))
        .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
        .build()
}
