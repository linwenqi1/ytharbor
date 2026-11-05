use crate::models::{FormatOption, MediaKind, VideoMetadata};
use serde::Deserialize;
use std::{collections::BTreeMap, ffi::OsString, path::PathBuf, process::Command};
use tauri::{AppHandle, Manager};
use url::Url;

#[derive(Debug, Deserialize)]
struct RawMetadata {
    id: Option<String>,
    webpage_url: Option<String>,
    title: Option<String>,
    uploader: Option<String>,
    channel: Option<String>,
    duration: Option<f64>,
    thumbnail: Option<String>,
    formats: Option<Vec<RawFormat>>,
}

#[derive(Debug, Deserialize)]
struct RawFormat {
    format_id: Option<String>,
    ext: Option<String>,
    height: Option<u32>,
    vcodec: Option<String>,
    filesize: Option<u64>,
    filesize_approx: Option<u64>,
}

pub fn validate_url(value: &str) -> Result<String, String> {
    let parsed = Url::parse(value).map_err(|_| "链接格式不正确".to_owned())?;
    if !matches!(parsed.scheme(), "http" | "https") { return Err("仅支持 http/https 视频链接".to_owned()); }
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let supported = host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com")
        || host == "bilibili.com" || host.ends_with(".bilibili.com") || host == "b23.tv";
    if !supported { return Err("目前仅支持 Bilibili 和 YouTube 链接".to_owned()); }
    Ok(if host.contains("bilibili") || host == "b23.tv" { "Bilibili" } else { "YouTube" }.to_owned())
}

pub fn parse(app: &AppHandle, url: &str) -> Result<VideoMetadata, String> {
    let platform = validate_url(url)?;
    let mut command = Command::new(yt_dlp_binary(app));
    command.args(["--dump-single-json", "--no-playlist", "--skip-download", "--no-warnings"]);
    if let Some(runtime) = deno_binary(app) {
        command.arg("--js-runtimes").arg(format!("deno:{}", runtime.to_string_lossy()));
    }
    let output = command.args(["--", url]).output().map_err(|error| format!("无法启动 yt-dlp：{error}"))?;
    if !output.status.success() {
        return Err(clean_error(&String::from_utf8_lossy(&output.stderr)));
    }
    let raw: RawMetadata = serde_json::from_slice(&output.stdout).map_err(|error| format!("无法读取视频信息：{error}"))?;
    let mut by_height: BTreeMap<u32, (String, String, Option<u64>)> = BTreeMap::new();
    for format in raw.formats.unwrap_or_default() {
        let Some(height) = format.height else { continue };
        if format.vcodec.as_deref() == Some("none") { continue; }
        let id = format.format_id.unwrap_or_else(|| "bestvideo".to_owned());
        let ext = format.ext.unwrap_or_else(|| "mp4".to_owned());
        let size = format.filesize.or(format.filesize_approx);
        let replace = by_height.get(&height).is_none_or(|existing| existing.2.unwrap_or(0) < size.unwrap_or(0));
        if replace { by_height.insert(height, (id, ext, size)); }
    }
    let mut formats: Vec<FormatOption> = by_height.into_iter().rev().map(|(height, (id, ext, filesize))| FormatOption {
        id, label: format!("{height}p"), height: Some(height), extension: ext, filesize, kind: MediaKind::Video,
    }).collect();
    formats.truncate(6);
    formats.push(FormatOption { id: "bestaudio".to_owned(), label: "最佳音质".to_owned(), height: None, extension: "m4a".to_owned(), filesize: None, kind: MediaKind::Audio });
    if formats.len() == 1 { return Err("没有找到可下载的视频格式".to_owned()); }
    Ok(VideoMetadata {
        id: raw.id.unwrap_or_default(), url: raw.webpage_url.unwrap_or_else(|| url.to_owned()), platform,
        title: raw.title.unwrap_or_else(|| "未命名视频".to_owned()),
        author: raw.uploader.or(raw.channel).unwrap_or_default(), duration: raw.duration,
        thumbnail: raw.thumbnail.map(|url| url.replacen("http://", "https://", 1)), formats,
    })
}

pub fn yt_dlp_binary(app: &AppHandle) -> OsString {
    if let Some(value) = std::env::var_os("YTHARBOR_YTDLP") { return value; }
    let exe = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };
    let mut candidates = Vec::<PathBuf>::new();
    if let Ok(dir) = app.path().resource_dir() { candidates.push(dir.join(exe)); candidates.push(dir.join("binaries").join(exe)); }
    if let Ok(current) = std::env::current_exe() { if let Some(dir) = current.parent() { candidates.push(dir.join(exe)); } }
    let source_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    candidates.push(source_dir.join(exe));
    if let Some(triple) = current_target_triple() {
        let source_name = if cfg!(windows) { format!("yt-dlp-{triple}.exe") } else { format!("yt-dlp-{triple}") };
        candidates.push(source_dir.join(source_name));
    }
    candidates.into_iter().find(|path| path.is_file()).map(Into::into).unwrap_or_else(|| OsString::from(exe))
}

fn current_target_triple() -> Option<String> {
    let arch = match std::env::consts::ARCH { "x86_64" => "x86_64", "aarch64" => "aarch64", _ => return None };
    let suffix = match std::env::consts::OS {
        "windows" => "pc-windows-msvc",
        "linux" => "unknown-linux-gnu",
        "macos" => "apple-darwin",
        _ => return None,
    };
    Some(format!("{arch}-{suffix}"))
}

pub fn ffmpeg_directory(app: &AppHandle) -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("YTHARBOR_FFMPEG_DIR") { return Some(value.into()); }
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let mut candidates = Vec::new();
    if let Ok(dir) = app.path().resource_dir() { candidates.push(dir.join("ffmpeg")); candidates.push(dir.join("resources").join("ffmpeg")); }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("ffmpeg"));
    candidates.into_iter().find(|path| path.join(exe).is_file())
}

pub fn deno_binary(app: &AppHandle) -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("YTHARBOR_DENO") { return Some(value.into()); }
    let exe = if cfg!(windows) { "deno.exe" } else { "deno" };
    let mut candidates = Vec::new();
    if let Ok(dir) = app.path().resource_dir() { candidates.push(dir.join("deno").join(exe)); candidates.push(dir.join("resources").join("deno").join(exe)); }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("deno").join(exe));
    candidates.into_iter().find(|path| path.is_file())
}

fn clean_error(stderr: &str) -> String {
    let useful = stderr.lines().rev().find(|line| line.contains("ERROR:")).unwrap_or(stderr).trim();
    if useful.is_empty() { "视频解析失败，请检查链接或网络连接".to_owned() } else { useful.trim_start_matches("ERROR:").trim().to_owned() }
}
