mod commands;
mod douyin;
mod downloader;
mod metadata;
mod models;
mod progress;
mod storage;

use downloader::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(AppState::new(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::parse_video,
            commands::start_download,
            commands::cancel_download,
            commands::get_tasks,
            commands::get_history,
            commands::clear_history,
            commands::get_settings,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running YtHarbor");
}
