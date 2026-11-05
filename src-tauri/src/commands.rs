use crate::{downloader::AppState, metadata, models::{AppSettings, DownloadRequest, DownloadTask, VideoMetadata}, storage};
use std::fs;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn parse_video(app: AppHandle, url: String) -> Result<VideoMetadata, String> {
    metadata::validate_url(&url)?;
    tauri::async_runtime::spawn_blocking(move || metadata::parse(&app, &url)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn start_download(state: State<'_, AppState>, request: DownloadRequest) -> Result<DownloadTask, String> {
    state.enqueue(request)
}

#[tauri::command]
pub fn cancel_download(app: AppHandle, state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    state.cancel(&app, &task_id)
}

#[tauri::command]
pub fn get_tasks(state: State<'_, AppState>) -> Result<Vec<DownloadTask>, String> {
    state.tasks()
}

#[tauri::command]
pub fn get_history(app: AppHandle) -> Result<Vec<DownloadTask>, String> {
    storage::load_history(&app)
}

#[tauri::command]
pub fn clear_history(app: AppHandle) -> Result<(), String> {
    storage::clear_history(&app)
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<AppSettings, String> {
    storage::load_settings(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<AppSettings, String> {
    if settings.output_directory.trim().is_empty() { return Err("下载目录不能为空".to_owned()); }
    fs::create_dir_all(&settings.output_directory).map_err(|error| format!("无法使用下载目录：{error}"))?;
    if !matches!(settings.default_quality.as_str(), "1080p" | "720p" | "480p" | "360p") { return Err("默认画质无效".to_owned()); }
    let template = settings.filename_template.trim();
    if template.is_empty() || template.len() > 160 || template.contains("..") || template.contains('/') || template.contains('\\') {
        return Err("文件名模板不能为空，且不能包含路径字符".to_owned());
    }
    if !template.contains("%(ext)s") { return Err("文件名模板需要包含 %(ext)s".to_owned()); }
    storage::store_settings(&app, &settings)?;
    Ok(settings)
}
