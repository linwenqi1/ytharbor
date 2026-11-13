mod commands;
mod douyin;
mod downloader;
mod metadata;
mod models;
mod progress;
mod storage;
mod updater;

use downloader::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            app.manage(AppState::new(app.handle().clone()));
            app.manage(updater::UpdateState::default());
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
            updater::check_update,
            updater::download_update,
            updater::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running YtHarbor");
}
