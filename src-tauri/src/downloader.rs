use crate::{
    douyin, metadata,
    models::{DownloadRequest, DownloadTask, TaskStatus},
    progress, storage,
};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};

struct QueuedDownload {
    id: String,
    request: DownloadRequest,
}

#[derive(Default)]
struct OutputTracker {
    candidate_path: Option<PathBuf>,
    final_path: Option<PathBuf>,
    existing_paths: Vec<PathBuf>,
}

struct Inner {
    tasks: Mutex<HashMap<String, DownloadTask>>,
    controls: Mutex<HashMap<String, Arc<AtomicBool>>>,
    running_id: Mutex<Option<String>>,
}

pub struct AppState {
    inner: Arc<Inner>,
    queue: mpsc::Sender<QueuedDownload>,
}

impl AppState {
    pub fn new(app: AppHandle) -> Self {
        let inner = Arc::new(Inner {
            tasks: Mutex::new(HashMap::new()),
            controls: Mutex::new(HashMap::new()),
            running_id: Mutex::new(None),
        });
        let (queue, receiver) = mpsc::channel();
        let worker_inner = Arc::clone(&inner);
        thread::spawn(move || queue_worker(app, worker_inner, receiver));
        Self { inner, queue }
    }

    pub fn enqueue(&self, request: DownloadRequest) -> Result<DownloadTask, String> {
        metadata::validate_url(&request.url)?;
        validate_request(&request)?;
        fs::create_dir_all(&request.output_directory)
            .map_err(|error| format!("无法创建下载目录：{error}"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        let task = DownloadTask {
            id: id.clone(),
            url: request.url.clone(),
            title: request.title.clone(),
            thumbnail: request.thumbnail.clone(),
            format_label: request.format_label.clone(),
            kind: request.kind,
            status: TaskStatus::Pending,
            percent: None,
            speed: None,
            eta: None,
            output_path: None,
            error: None,
            created_at: now,
            updated_at: now,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.inner
            .tasks
            .lock()
            .map_err(lock_error)?
            .insert(id.clone(), task.clone());
        self.inner
            .controls
            .lock()
            .map_err(lock_error)?
            .insert(id.clone(), cancel);
        self.queue
            .send(QueuedDownload { id, request })
            .map_err(|_| "下载队列已停止".to_owned())?;
        Ok(task)
    }

    pub fn cancel(&self, app: &AppHandle, id: &str) -> Result<(), String> {
        let control = self
            .inner
            .controls
            .lock()
            .map_err(lock_error)?
            .get(id)
            .cloned()
            .ok_or_else(|| "任务不存在或已经结束".to_owned())?;
        control.store(true, Ordering::Relaxed);
        let is_running = self.inner.running_id.lock().map_err(lock_error)?.as_deref() == Some(id);
        if !is_running {
            finish_task(app, &self.inner, id, TaskStatus::Cancelled, None, None);
            self.inner.controls.lock().map_err(lock_error)?.remove(id);
        }
        Ok(())
    }

    pub fn tasks(&self) -> Result<Vec<DownloadTask>, String> {
        let mut tasks: Vec<_> = self
            .inner
            .tasks
            .lock()
            .map_err(lock_error)?
            .values()
            .cloned()
            .collect();
        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(tasks)
    }
}

fn queue_worker(app: AppHandle, inner: Arc<Inner>, receiver: mpsc::Receiver<QueuedDownload>) {
    while let Ok(item) = receiver.recv() {
        let cancelled = inner
            .controls
            .lock()
            .ok()
            .and_then(|items| items.get(&item.id).cloned())
            .is_none_or(|flag| flag.load(Ordering::Relaxed));
        let still_pending = inner
            .tasks
            .lock()
            .ok()
            .and_then(|tasks| tasks.get(&item.id).cloned())
            .is_some_and(|task| task.status == TaskStatus::Pending);
        if cancelled || !still_pending {
            continue;
        }
        if let Ok(mut running) = inner.running_id.lock() {
            *running = Some(item.id.clone());
        }
        run_download(&app, &inner, item);
        if let Ok(mut running) = inner.running_id.lock() {
            *running = None;
        }
    }
}

fn run_download(app: &AppHandle, inner: &Arc<Inner>, item: QueuedDownload) {
    if metadata::is_douyin_url(&item.request.url) {
        run_douyin_download(app, inner, item);
        return;
    }
    update_task(app, inner, &item.id, |task| {
        task.status = TaskStatus::Downloading
    });
    let mut args = vec![
        "--newline".to_owned(), "--no-playlist".to_owned(), "--progress".to_owned(),
        "--encoding".to_owned(), "utf-8".to_owned(),
        "--progress-template".to_owned(), "download:ytharbor-progress:%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s".to_owned(),
        "--print".to_owned(), "video:ytharbor-candidate:%(filename)s".to_owned(),
        "--print".to_owned(), "after_move:ytharbor-file:%(filepath)s".to_owned(),
        "--no-quiet".to_owned(),
        "-P".to_owned(), item.request.output_directory.clone(),
    ];
    let template = storage::load_settings(app)
        .map(|value| value.filename_template)
        .unwrap_or_else(|_| "%(title)s [%(id)s].%(ext)s".to_owned());
    args.extend(["-o".to_owned(), template]);
    match item.request.kind {
        crate::models::MediaKind::Video => {
            args.extend([
                "-f".to_owned(),
                format!(
                    "{}+bestaudio/{}",
                    item.request.format_id, item.request.format_id
                ),
                "--merge-output-format".to_owned(),
                "mp4".to_owned(),
            ]);
        }
        crate::models::MediaKind::Audio => {
            args.extend([
                "-f".to_owned(),
                "bestaudio/best".to_owned(),
                "--extract-audio".to_owned(),
                "--audio-format".to_owned(),
                item.request
                    .audio_format
                    .clone()
                    .unwrap_or_else(|| "mp3".to_owned()),
            ]);
        }
    }
    if let Some(path) = metadata::ffmpeg_directory(app) {
        args.extend([
            "--ffmpeg-location".to_owned(),
            path.to_string_lossy().to_string(),
        ]);
    }
    if let Some(runtime) = metadata::deno_binary(app) {
        args.extend([
            "--js-runtimes".to_owned(),
            format!("deno:{}", runtime.to_string_lossy()),
        ]);
    }
    args.extend(["--".to_owned(), item.request.url.clone()]);

    let spawned = metadata::yt_dlp_command(app)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            finish_task(
                app,
                inner,
                &item.id,
                TaskStatus::Failed,
                None,
                Some(format!("无法启动 yt-dlp：{error}")),
            );
            cleanup(inner, &item.id);
            return;
        }
    };
    let (line_tx, line_rx) = mpsc::channel::<String>();
    let mut out_thread = child
        .stdout
        .take()
        .map(|stdout| spawn_reader(stdout, line_tx.clone()));
    let mut err_thread = child
        .stderr
        .take()
        .map(|stderr| spawn_reader(stderr, line_tx));
    let cancel = inner
        .controls
        .lock()
        .ok()
        .and_then(|items| items.get(&item.id).cloned())
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let mut recent = VecDeque::<String>::with_capacity(24);
    let mut output = OutputTracker::default();

    let exit_status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            // Do not synchronously join the pipe readers here. A network or
            // child-process failure can leave an inherited pipe open, which
            // used to delay the visible cancellation indefinitely. Dropping
            // the handles detaches these short-lived readers; they exit once
            // their pipes close and can no longer mutate task state.
            drop(out_thread.take());
            drop(err_thread.take());
            finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
            cleanup(inner, &item.id);
            return;
        }
        match line_rx.recv_timeout(Duration::from_millis(120)) {
            Ok(line) => process_line(app, inner, &item, &line, &mut output, &mut recent),
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                finish_task(
                    app,
                    inner,
                    &item.id,
                    TaskStatus::Failed,
                    None,
                    Some(format!("下载进程异常：{error}")),
                );
                cleanup(inner, &item.id);
                return;
            }
        }
    };
    if let Some(handle) = out_thread.take() {
        let _ = handle.join();
    }
    if let Some(handle) = err_thread.take() {
        let _ = handle.join();
    }
    while let Ok(line) = line_rx.try_recv() {
        process_line(app, inner, &item, &line, &mut output, &mut recent);
    }
    if exit_status.success() {
        match classify_successful_output(&item.request, &output) {
            Ok((status, path)) => finish_task(app, inner, &item.id, status, path, None),
            Err(error) => finish_task(app, inner, &item.id, TaskStatus::Failed, None, Some(error)),
        }
    } else {
        let message = recent
            .iter()
            .rev()
            .find(|line| line.contains("ERROR:"))
            .or_else(|| recent.back())
            .cloned()
            .unwrap_or_else(|| "下载失败，请检查网络后重试".to_owned());
        finish_task(
            app,
            inner,
            &item.id,
            TaskStatus::Failed,
            None,
            Some(message.trim_start_matches("ERROR:").trim().to_owned()),
        );
    }
    cleanup(inner, &item.id);
}

fn run_douyin_download(app: &AppHandle, inner: &Arc<Inner>, item: QueuedDownload) {
    if item.request.kind == crate::models::MediaKind::Audio {
        finish_task(
            app,
            inner,
            &item.id,
            TaskStatus::Failed,
            None,
            Some("抖音原生下载暂仅支持视频；音频提取将在后续版本支持".to_owned()),
        );
        cleanup(inner, &item.id);
        return;
    }
    update_task(app, inner, &item.id, |task| {
        task.status = TaskStatus::Downloading
    });

    // Resolve immediately before opening the stream. This produces a fresh
    // signed CDN URL instead of persisting a short-lived URL in task history.
    let download = match douyin::open_download(&item.request.url, &item.request.format_id) {
        Ok(download) => download,
        Err(error) => {
            finish_task(app, inner, &item.id, TaskStatus::Failed, None, Some(error));
            cleanup(inner, &item.id);
            return;
        }
    };
    let template = storage::load_settings(app)
        .map(|value| value.filename_template)
        .unwrap_or_else(|_| "%(title)s [%(id)s].%(ext)s".to_owned());
    let output_path = native_output_path(&item.request, &template, &download.id);
    let partial_path = output_path.with_file_name(format!(
        "{}.part",
        output_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    ));
    let mut file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial_path)
    {
        Ok(file) => file,
        Err(error) => {
            finish_task(
                app,
                inner,
                &item.id,
                TaskStatus::Failed,
                None,
                Some(format!("无法创建下载文件：{error}")),
            );
            cleanup(inner, &item.id);
            return;
        }
    };
    let cancel = inner
        .controls
        .lock()
        .ok()
        .and_then(|items| items.get(&item.id).cloned())
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let total = download.total_bytes;
    let started = Instant::now();
    let mut last_update = Instant::now() - Duration::from_secs(1);
    let mut written = 0_u64;
    let stream_result = douyin::stream_download(&download, |chunk| {
        if cancel.load(Ordering::Relaxed) {
            return Err("下载已取消".to_owned());
        }
        file.write_all(chunk).map_err(|error| format!("无法写入下载文件：{error}"))?;
        written += chunk.len() as u64;
        if last_update.elapsed() >= Duration::from_millis(180) {
            update_native_progress(app, inner, &item.id, written, total, started.elapsed());
            last_update = Instant::now();
        }
        Ok(())
    });
    if let Err(error) = stream_result {
        drop(file);
        let _ = fs::remove_file(&partial_path);
        if cancel.load(Ordering::Relaxed) {
            finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
        } else {
            finish_task(app, inner, &item.id, TaskStatus::Failed, None, Some(error));
        }
        cleanup(inner, &item.id);
        return;
    }
    if cancel.load(Ordering::Relaxed) {
        drop(file);
        let _ = fs::remove_file(&partial_path);
        finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
        cleanup(inner, &item.id);
        return;
    }
    if let Err(error) = file.flush() {
        drop(file);
        let _ = fs::remove_file(&partial_path);
        finish_task(
            app,
            inner,
            &item.id,
            TaskStatus::Failed,
            None,
            Some(format!("无法完成抖音下载：{error}")),
        );
        cleanup(inner, &item.id);
        return;
    }
    drop(file);
    if let Err(error) = fs::rename(&partial_path, &output_path) {
        let _ = fs::remove_file(&partial_path);
        finish_task(
            app,
            inner,
            &item.id,
            TaskStatus::Failed,
            None,
            Some(format!("无法完成抖音下载：{error}")),
        );
        cleanup(inner, &item.id);
        return;
    }
    update_native_progress(app, inner, &item.id, written, total, started.elapsed());
    finish_task(
        app,
        inner,
        &item.id,
        TaskStatus::Completed,
        Some(output_path.to_string_lossy().to_string()),
        None,
    );
    cleanup(inner, &item.id);
}

fn update_native_progress(
    app: &AppHandle,
    inner: &Arc<Inner>,
    id: &str,
    written: u64,
    total: Option<u64>,
    elapsed: Duration,
) {
    let seconds = elapsed.as_secs_f64().max(0.001);
    let speed = written as f64 / seconds;
    let eta = total.and_then(|total| {
        (speed > 0.0).then(|| ((total.saturating_sub(written)) as f64 / speed).ceil() as u64)
    });
    let percent =
        total.map(|total| (written as f64 / total.max(1) as f64 * 100.0).clamp(0.0, 100.0));
    update_task(app, inner, id, |task| {
        task.status = TaskStatus::Downloading;
        task.percent = percent;
        task.speed = Some(speed);
        task.eta = eta;
    });
}

fn native_output_path(request: &DownloadRequest, template: &str, video_id: &str) -> PathBuf {
    let rendered = template
        .replace("%(title)s", &request.title)
        .replace("%(id)s", video_id)
        .replace("%(ext)s", "mp4");
    // Existing yt-dlp templates can contain many advanced tokens. Native
    // resolvers intentionally support the documented core fields; fall back to
    // a predictable filename if an unsupported token remains.
    let filename = if rendered.contains("%(") {
        format!("{} [{}].mp4", request.title, video_id)
    } else {
        rendered
    };
    unique_path(
        Path::new(&request.output_directory),
        &sanitize_filename(&filename),
    )
}

fn sanitize_filename(value: &str) -> String {
    let mut clean: String = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                ' '
            } else {
                character
            }
        })
        .collect();
    clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    clean = clean.trim_matches([' ', '.']).to_owned();
    if clean.is_empty() {
        clean = "douyin-video.mp4".to_owned();
    }
    let stem = clean.strip_suffix(".mp4").unwrap_or(&clean);
    if matches!(
        stem.to_ascii_lowercase().as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    ) {
        clean = format!("video-{clean}");
    }
    clean
}

fn unique_path(directory: &Path, filename: &str) -> PathBuf {
    let candidate = directory.join(filename);
    if available_native_path(&candidate) {
        return candidate;
    }
    let stem = candidate.file_stem().unwrap_or_default().to_string_lossy();
    let extension = candidate
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    for index in 2..10_000 {
        let candidate = directory.join(format!("{stem}-{index}{extension}"));
        if available_native_path(&candidate) {
            return candidate;
        }
    }
    directory.join(format!("{stem}-{}{}", uuid::Uuid::new_v4(), extension))
}

fn available_native_path(path: &Path) -> bool {
    let partial = path.with_file_name(format!("{}.part", path.file_name().unwrap_or_default().to_string_lossy()));
    !path.exists() && !partial.exists()
}

fn process_line(
    app: &AppHandle,
    inner: &Arc<Inner>,
    item: &QueuedDownload,
    line: &str,
    output: &mut OutputTracker,
    recent: &mut VecDeque<String>,
) {
    if let Some(parsed) = progress::parse_progress(line) {
        update_task(app, inner, &item.id, |task| {
            task.status = TaskStatus::Downloading;
            task.percent = parsed.percent.or(task.percent);
            task.speed = parsed.speed;
            task.eta = parsed.eta;
        });
    } else if let Some(path) = line.strip_prefix("ytharbor-candidate:") {
        output.candidate_path = Some(absolute_output_path(
            &item.request.output_directory,
            path.trim(),
        ));
    } else if let Some(path) = line.strip_prefix("ytharbor-file:") {
        output.final_path = Some(absolute_output_path(
            &item.request.output_directory,
            path.trim(),
        ));
    } else if let Some(path) = existing_output_path(&item.request.output_directory, line) {
        output.existing_paths.push(path);
    } else if ["[Merger]", "[ExtractAudio]", "[VideoConvertor]", "[Fixup"]
        .iter()
        .any(|marker| line.contains(marker))
    {
        update_task(app, inner, &item.id, |task| {
            task.status = TaskStatus::Processing;
            task.percent = Some(100.0);
            task.speed = None;
            task.eta = None;
        });
    }
    if !line.trim().is_empty() {
        if recent.len() == 24 {
            recent.pop_front();
        }
        recent.push_back(line.trim().to_owned());
    }
}

fn absolute_output_path(directory: &str, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        Path::new(directory).join(path)
    }
}

fn existing_output_path(directory: &str, line: &str) -> Option<PathBuf> {
    line.strip_prefix("[download] ")
        .and_then(|value| value.strip_suffix(" has already been downloaded"))
        .map(|path| absolute_output_path(directory, path.trim()))
}

fn resolve_candidate_output(request: &DownloadRequest, candidate: &Path) -> Option<PathBuf> {
    let expected_extension = match request.kind {
        crate::models::MediaKind::Video => Some("mp4"),
        crate::models::MediaKind::Audio => request.audio_format.as_deref(),
    };
    if let Some(extension) = expected_extension {
        let expected = candidate.with_extension(extension);
        if expected.is_file() {
            return Some(expected);
        }
    }
    candidate.is_file().then(|| candidate.to_path_buf())
}

fn classify_successful_output(
    request: &DownloadRequest,
    output: &OutputTracker,
) -> Result<(TaskStatus, Option<String>), String> {
    let resolved = output
        .final_path
        .as_ref()
        .filter(|path| path.is_file())
        .cloned()
        .or_else(|| {
            output
                .candidate_path
                .as_deref()
                .and_then(|path| resolve_candidate_output(request, path))
        });
    if let Some(path) = resolved {
        let status = if output
            .existing_paths
            .iter()
            .any(|existing| same_file(existing, &path))
        {
            TaskStatus::Existing
        } else {
            TaskStatus::Completed
        };
        return Ok((status, Some(path.to_string_lossy().to_string())));
    }
    if !output.existing_paths.is_empty() {
        return Err("文件已存在，但无法定位现有文件".to_owned());
    }
    Ok((TaskStatus::Completed, None))
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn spawn_reader<R: std::io::Read + Send + 'static>(
    reader: R,
    sender: mpsc::Sender<String>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            let _ = sender.send(line);
        }
    })
}

fn update_task<F: FnOnce(&mut DownloadTask)>(
    app: &AppHandle,
    inner: &Arc<Inner>,
    id: &str,
    change: F,
) {
    let task = inner.tasks.lock().ok().and_then(|mut tasks| {
        let task = tasks.get_mut(id)?;
        change(task);
        task.updated_at = now_ms();
        Some(task.clone())
    });
    if let Some(task) = task {
        let _ = app.emit("download://updated", task);
    }
}

fn finish_task(
    app: &AppHandle,
    inner: &Arc<Inner>,
    id: &str,
    status: TaskStatus,
    output: Option<String>,
    error: Option<String>,
) {
    let task = inner.tasks.lock().ok().and_then(|mut tasks| {
        let task = tasks.get_mut(id)?;
        task.status = status;
        task.output_path = output;
        task.error = error;
        task.updated_at = now_ms();
        if matches!(task.status, TaskStatus::Completed | TaskStatus::Existing) {
            task.percent = Some(100.0);
        }
        task.speed = None;
        task.eta = None;
        Some(task.clone())
    });
    if let Some(task) = task {
        let _ = storage::append_history(app, &task);
        let _ = app.emit("download://updated", task);
    }
}

fn cleanup(inner: &Arc<Inner>, id: &str) {
    if let Ok(mut controls) = inner.controls.lock() {
        controls.remove(id);
    }
}

fn validate_request(request: &DownloadRequest) -> Result<(), String> {
    if request.output_directory.trim().is_empty() {
        return Err("请先选择下载目录".to_owned());
    }
    let yt_dlp_format = request.format_id.chars().all(|c| c.is_ascii_alphanumeric() || ".+_-".contains(c));
    let douyin_format = request.format_id.strip_prefix("douyin:").is_some_and(|quality| {
        quality == "best" || quality.strip_suffix('p').is_some_and(|height| !height.is_empty() && height.bytes().all(|byte| byte.is_ascii_digit()))
    });
    if request.format_id.len() > 100 || !(yt_dlp_format || douyin_format) {
        return Err("下载格式无效".to_owned());
    }
    if request.kind == crate::models::MediaKind::Audio
        && !matches!(request.audio_format.as_deref(), Some("mp3" | "m4a"))
    {
        return Err("音频格式无效".to_owned());
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn lock_error<T>(_: std::sync::PoisonError<T>) -> String {
    "任务状态暂时不可用".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        classify_successful_output, existing_output_path, validate_request, OutputTracker,
    };
    use crate::models::{DownloadRequest, MediaKind, TaskStatus};
    use std::fs;

    fn request(format_id: &str) -> DownloadRequest {
        DownloadRequest {
            url: "https://www.douyin.com/video/7699483289760959786".to_owned(),
            title: "test".to_owned(),
            thumbnail: None,
            format_id: format_id.to_owned(),
            format_label: "test".to_owned(),
            kind: MediaKind::Video,
            output_directory: "C:\\Downloads".to_owned(),
            audio_format: None,
        }
    }

    #[test]
    fn accepts_scoped_douyin_quality_ids() {
        assert!(validate_request(&request("douyin:1440p")).is_ok());
        assert!(validate_request(&request("douyin:best")).is_ok());
    }

    #[test]
    fn rejects_unscoped_format_ids_with_colons() {
        assert!(validate_request(&request("best:unexpected")).is_err());
    }

    #[test]
    fn marks_the_final_file_as_existing_when_yt_dlp_skips_it() {
        let directory =
            std::env::temp_dir().join(format!("ytharbor-existing-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("test directory should be created");
        let path = directory.join("中文视频.mp4");
        fs::write(&path, []).expect("test file should be created");
        let line = format!("[download] {} has already been downloaded", path.display());
        let existing = existing_output_path(directory.to_string_lossy().as_ref(), &line)
            .expect("existing path should be parsed");
        let output = OutputTracker {
            candidate_path: Some(path.clone()),
            final_path: Some(path.clone()),
            existing_paths: vec![existing],
        };

        let (status, output_path) = classify_successful_output(&request("test"), &output)
            .expect("existing output should be classified");

        assert_eq!(status, TaskStatus::Existing);
        assert_eq!(
            output_path.as_deref(),
            Some(path.to_string_lossy().as_ref())
        );
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn keeps_completed_when_only_an_intermediate_file_was_reused() {
        let directory =
            std::env::temp_dir().join(format!("ytharbor-completed-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("test directory should be created");
        let final_path = directory.join("video.mp4");
        fs::write(&final_path, []).expect("test file should be created");
        let output = OutputTracker {
            candidate_path: Some(final_path.clone()),
            final_path: Some(final_path),
            existing_paths: vec![directory.join("video.f123.mp4")],
        };

        let (status, _) = classify_successful_output(&request("test"), &output)
            .expect("completed output should be classified");

        assert_eq!(status, TaskStatus::Completed);
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }
}
