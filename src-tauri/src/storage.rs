use crate::models::{AppSettings, DownloadTask};
use std::{fs, path::{Path, PathBuf}};
use tauri::{AppHandle, Manager};

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app.path().app_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&path).map_err(|error| format!("无法创建应用数据目录：{error}"))?;
    Ok(path)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    if !path.exists() { return Ok(None); }
    let content = fs::read_to_string(path).map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    serde_json::from_str(&content).map(Some).map_err(|error| format!("数据格式损坏：{error}"))
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let content = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| format!("保存失败：{error}"))
}

pub fn load_settings(app: &AppHandle) -> Result<AppSettings, String> {
    let path = data_dir(app)?.join("settings.json");
    if let Some(settings) = read_json(&path)? { return Ok(settings); }
    let downloads = app.path().download_dir().unwrap_or_else(|_| data_dir(app).unwrap_or_default().join("downloads"));
    let settings = AppSettings {
        output_directory: downloads.to_string_lossy().to_string(),
        default_quality: "1080p".to_owned(),
        filename_template: "%(title)s [%(id)s].%(ext)s".to_owned(),
    };
    write_json(&path, &settings)?;
    Ok(settings)
}

pub fn store_settings(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    write_json(&data_dir(app)?.join("settings.json"), settings)
}

pub fn load_history(app: &AppHandle) -> Result<Vec<DownloadTask>, String> {
    Ok(read_json(&data_dir(app)?.join("history.json"))?.unwrap_or_default())
}

pub fn append_history(app: &AppHandle, task: &DownloadTask) -> Result<(), String> {
    let path = data_dir(app)?.join("history.json");
    let mut history: Vec<DownloadTask> = read_json(&path)?.unwrap_or_default();
    history.retain(|item| item.id != task.id);
    history.insert(0, task.clone());
    history.truncate(200);
    write_json(&path, &history)
}

pub fn clear_history(app: &AppHandle) -> Result<(), String> {
    write_json(&data_dir(app)?.join("history.json"), &Vec::<DownloadTask>::new())
}
