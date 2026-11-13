use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::Duration,
};

use reqwest::{
    header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE},
    StatusCode,
};
use serde::{Deserialize, Serialize};
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

const STALL_TIMEOUT: Duration = Duration::from_secs(25);
const MAX_FAILURES: u8 = 5;

#[derive(Default)]
pub struct UpdateState(Mutex<Option<Update>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current_version: String,
    version: String,
    body: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum DownloadEvent {
    Progress { downloaded: u64, total: Option<u64> },
    Reconnecting { attempt: u8, delay_seconds: u64 },
    Finished,
}

#[derive(Deserialize, Serialize, PartialEq)]
struct CacheInfo {
    url: String,
    signature: String,
}

fn cache_paths(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?
        .join("updater");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok((dir.join("update.part"), dir.join("update.json")))
}

fn prepare_cache(app: &AppHandle, update: &Update) -> Result<PathBuf, String> {
    let (part, metadata) = cache_paths(app)?;
    let expected = CacheInfo {
        url: update.download_url.to_string(),
        signature: update.signature.clone(),
    };
    let matches = fs::read(&metadata)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CacheInfo>(&bytes).ok())
        .is_some_and(|stored| stored == expected);
    if !matches {
        let _ = fs::remove_file(&part);
        fs::write(
            &metadata,
            serde_json::to_vec(&expected).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(part)
}

#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Option<UpdateInfo>, String> {
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| error.to_string())?;
    let update = updater.check().await.map_err(|error| error.to_string())?;
    let info = update.as_ref().map(|value| UpdateInfo {
        current_version: value.current_version.clone(),
        version: value.version.clone(),
        body: value.body.clone(),
    });
    *state.0.lock().map_err(|error| error.to_string())? = update;
    Ok(info)
}

fn current_update(state: &State<'_, UpdateState>) -> Result<Update, String> {
    state
        .0
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or_else(|| "没有可下载的更新。".to_owned())
}

fn range_total(value: &str, offset: u64) -> Option<Option<u64>> {
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.parse::<u64>().ok()?;
    let end = end.parse::<u64>().ok()?;
    if start != offset || end < start {
        return None;
    }
    let total = if total == "*" {
        None
    } else {
        total.parse().ok()
    };
    if total.is_some_and(|length| end >= length) {
        return None;
    }
    Some(total)
}

async fn download_once(
    update: &Update,
    part: &PathBuf,
    on_event: &Channel<DownloadEvent>,
) -> Result<(), String> {
    let offset = fs::metadata(part)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let client = reqwest::Client::builder()
        .connect_timeout(STALL_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client
        .get(update.download_url.clone())
        .header(ACCEPT_ENCODING, "identity");
    if offset > 0 {
        request = request.header(RANGE, format!("bytes={offset}-"));
    }
    let mut response = tokio::time::timeout(STALL_TIMEOUT, request.send())
        .await
        .map_err(|_| "连接更新服务器超时".to_owned())?
        .map_err(|error| error.to_string())?;
    if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
        fs::write(part, []).map_err(|error| format!("无法保存更新文件：{error}"))?;
        return Err("服务器要求重新获取更新文件".to_owned());
    }
    if response.status().is_client_error()
        && response.status() != StatusCode::REQUEST_TIMEOUT
        && response.status() != StatusCode::TOO_MANY_REQUESTS
    {
        return Err(format!("更新服务器拒绝下载（{}）", response.status()));
    }
    if !response.status().is_success() {
        return Err(format!("更新服务器返回 {}", response.status()));
    }

    let (start, total) = if response.status() == StatusCode::PARTIAL_CONTENT {
        let value = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .ok_or("服务器未提供有效的续传范围")?;
        (
            offset,
            range_total(value, offset).ok_or("服务器返回的续传范围不匹配")?,
        )
    } else {
        (0, response.content_length())
    };
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(start > 0)
        .truncate(start == 0)
        .open(part)
        .map_err(|error| format!("无法保存更新文件：{error}"))?;
    let mut downloaded = start;
    let _ = on_event.send(DownloadEvent::Progress { downloaded, total });
    loop {
        let chunk = tokio::time::timeout(STALL_TIMEOUT, response.chunk())
            .await
            .map_err(|_| "下载长时间没有收到数据".to_owned())?
            .map_err(|error| error.to_string())?;
        let Some(chunk) = chunk else {
            break;
        };
        file.write_all(&chunk)
            .map_err(|error| format!("无法保存更新文件：{error}"))?;
        downloaded += chunk.len() as u64;
        let _ = on_event.send(DownloadEvent::Progress { downloaded, total });
    }
    file.flush()
        .map_err(|error| format!("无法保存更新文件：{error}"))?;
    if total.is_some_and(|length| downloaded != length) {
        return Err("更新文件尚未下载完整".to_owned());
    }
    Ok(())
}

#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
    on_event: Channel<DownloadEvent>,
) -> Result<(), String> {
    let update = current_update(&state)?;
    let part = prepare_cache(&app, &update)?;
    if let Ok(bytes) = fs::read(&part) {
        if update.verify(&bytes).is_ok() {
            let length = bytes.len() as u64;
            let _ = on_event.send(DownloadEvent::Progress {
                downloaded: length,
                total: Some(length),
            });
            let _ = on_event.send(DownloadEvent::Finished);
            return Ok(());
        }
    }
    let mut failures = 0;
    loop {
        let before = fs::metadata(&part)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        match download_once(&update, &part, &on_event).await {
            Ok(()) => break,
            Err(error) => {
                if error.starts_with("无法保存更新文件") || error.starts_with("更新服务器拒绝下载")
                {
                    return Err(error);
                }
                let after = fs::metadata(&part)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                failures = if after > before { 1 } else { failures + 1 };
                if failures >= MAX_FAILURES {
                    return Err(format!("网络持续无响应，已保存下载进度。{error}"));
                }
                let delay_seconds = [2, 4, 8, 15][(failures - 1) as usize];
                let _ = on_event.send(DownloadEvent::Reconnecting {
                    attempt: failures,
                    delay_seconds,
                });
                tokio::time::sleep(Duration::from_secs(delay_seconds)).await;
            }
        }
    }
    let bytes = fs::read(&part).map_err(|error| error.to_string())?;
    if let Err(error) = update.verify(&bytes) {
        let _ = fs::remove_file(&part);
        return Err(format!("更新文件校验失败：{error}"));
    }
    let _ = on_event.send(DownloadEvent::Finished);
    Ok(())
}

#[tauri::command]
pub fn install_update(app: AppHandle, state: State<'_, UpdateState>) -> Result<(), String> {
    let update = current_update(&state)?;
    let (part, _) = cache_paths(&app)?;
    let bytes = fs::read(part).map_err(|error| error.to_string())?;
    update
        .restart_after_install(true)
        .install(&bytes)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::range_total;

    #[test]
    fn range_must_start_at_cached_offset_and_fit_total() {
        assert_eq!(range_total("bytes 1024-2047/4096", 1024), Some(Some(4096)));
        assert_eq!(range_total("bytes 0-2047/4096", 1024), None);
        assert_eq!(range_total("bytes 1024-4096/4096", 1024), None);
        assert_eq!(range_total("bytes 1024-1000/4096", 1024), None);
    }
}
