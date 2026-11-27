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

#[derive(Default)]
struct QueueState {
    tasks: HashMap<String, DownloadTask>,
    controls: HashMap<String, Arc<AtomicBool>>,
    running_id: Option<String>,
}

impl QueueState {
    fn insert(&mut self, task: DownloadTask) {
        self.controls
            .insert(task.id.clone(), Arc::new(AtomicBool::new(false)));
        self.tasks.insert(task.id.clone(), task);
    }

    fn cancel(&mut self, id: &str) -> Result<Option<DownloadTask>, String> {
        let control = self
            .controls
            .get(id)
            .cloned()
            .ok_or_else(|| "任务不存在或已经结束".to_owned())?;
        control.store(true, Ordering::Relaxed);
        if self.running_id.as_deref() == Some(id) {
            return Ok(None);
        }
        self.controls.remove(id);
        Ok(self.finish(id, TaskStatus::Cancelled, None, None))
    }

    fn claim_for_run(&mut self, id: &str) -> Option<Arc<AtomicBool>> {
        let control = self.controls.get(id)?.clone();
        if control.load(Ordering::Relaxed) {
            return None;
        }
        let still_pending = self
            .tasks
            .get(id)
            .is_some_and(|task| task.status == TaskStatus::Pending);
        if !still_pending {
            return None;
        }
        self.running_id = Some(id.to_owned());
        Some(control)
    }

    fn finish(
        &mut self,
        id: &str,
        status: TaskStatus,
        output: Option<String>,
        error: Option<String>,
    ) -> Option<DownloadTask> {
        self.controls.remove(id);
        if self.running_id.as_deref() == Some(id) {
            self.running_id = None;
        }
        let task = self.tasks.get_mut(id)?;
        if is_terminal_status(&task.status) {
            return None;
        }
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
    }
}

struct Inner {
    state: Mutex<QueueState>,
}

pub struct AppState {
    inner: Arc<Inner>,
    queue: mpsc::Sender<QueuedDownload>,
}

impl AppState {
    pub fn new(app: AppHandle) -> Self {
        let inner = Arc::new(Inner {
            state: Mutex::new(QueueState::default()),
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
        self.inner
            .state
            .lock()
            .map_err(lock_error)?
            .insert(task.clone());
        self.queue
            .send(QueuedDownload { id, request })
            .map_err(|_| "下载队列已停止".to_owned())?;
        Ok(task)
    }

    pub fn cancel(&self, app: &AppHandle, id: &str) -> Result<(), String> {
        let finished = self
            .inner
            .state
            .lock()
            .map_err(lock_error)?
            .cancel(id)?;
        if let Some(task) = finished {
            let _ = storage::append_history(app, &task);
            let _ = app.emit("download://updated", task);
        }
        Ok(())
    }

    pub fn tasks(&self) -> Result<Vec<DownloadTask>, String> {
        let mut tasks: Vec<_> = self
            .inner
            .state
            .lock()
            .map_err(lock_error)?
            .tasks
            .values()
            .cloned()
            .collect();
        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(tasks)
    }
}

fn queue_worker(app: AppHandle, inner: Arc<Inner>, receiver: mpsc::Receiver<QueuedDownload>) {
    while let Ok(item) = receiver.recv() {
        let Some(cancel) = inner
            .state
            .lock()
            .ok()
            .and_then(|mut state| state.claim_for_run(&item.id))
        else {
            continue;
        };
        let id = item.id.clone();
        run_download(&app, &inner, item, cancel);
        if let Ok(mut state) = inner.state.lock() {
            state.controls.remove(&id);
            if state.running_id.as_deref() == Some(id.as_str()) {
                state.running_id = None;
            }
        }
    }
}

fn run_download(app: &AppHandle, inner: &Arc<Inner>, item: QueuedDownload, cancel: Arc<AtomicBool>) {
    if cancel.load(Ordering::Relaxed) {
        finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
        return;
    }
    if metadata::is_douyin_url(&item.request.url) {
        run_douyin_download(app, inner, item, cancel);
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
    let template = variant_output_template(&template, &item.request);
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

    if cancel.load(Ordering::Relaxed) {
        finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
        return;
    }

    let spawned = metadata::yt_dlp_command(app)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            if cancel.load(Ordering::Relaxed) {
                finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
            } else {
                finish_task(
                    app,
                    inner,
                    &item.id,
                    TaskStatus::Failed,
                    None,
                    Some(format!("无法启动 yt-dlp：{error}")),
                );
            }
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
    } else if cancel.load(Ordering::Relaxed) {
        finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
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
}

fn run_douyin_download(
    app: &AppHandle,
    inner: &Arc<Inner>,
    item: QueuedDownload,
    cancel: Arc<AtomicBool>,
) {
    if cancel.load(Ordering::Relaxed) {
        finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
        return;
    }
    if item.request.kind == crate::models::MediaKind::Audio {
        finish_task(
            app,
            inner,
            &item.id,
            TaskStatus::Failed,
            None,
            Some("抖音原生下载暂仅支持视频；音频提取将在后续版本支持".to_owned()),
        );
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
            if cancel.load(Ordering::Relaxed) {
                finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
            } else {
                finish_task(app, inner, &item.id, TaskStatus::Failed, None, Some(error));
            }
            return;
        }
    };
    if cancel.load(Ordering::Relaxed) {
        finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
        return;
    }
    let template = storage::load_settings(app)
        .map(|value| value.filename_template)
        .unwrap_or_else(|_| "%(title)s [%(id)s].%(ext)s".to_owned());
    let template = variant_output_template(&template, &item.request);
    let output_path = native_output_path(&item.request, &template, &download.id);
    if let Some(existing_path) =
        find_existing_native_output(&item.request, &output_path, &download.id)
    {
        finish_task(
            app,
            inner,
            &item.id,
            TaskStatus::Existing,
            Some(existing_path.to_string_lossy().to_string()),
            None,
        );
        return;
    }
    let partial_path = output_path.with_file_name(format!(
        "{}.part",
        output_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    ));
    let mut file = match open_partial_file(&partial_path) {
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
            return;
        }
    };
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
        return;
    }
    if cancel.load(Ordering::Relaxed) {
        drop(file);
        let _ = fs::remove_file(&partial_path);
        finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
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
}

fn open_partial_file(path: &Path) -> std::io::Result<fs::File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
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

const PART_SUFFIX_LEN: usize = ".part".len();
const MAX_FILENAME_BYTES: usize = 255 - PART_SUFFIX_LEN;
const MAX_FILENAME_UTF16: usize = 255 - PART_SUFFIX_LEN;
const WINDOWS_MAX_PATH_UTF16: usize = 259;

fn native_output_path(request: &DownloadRequest, template: &str, video_id: &str) -> PathBuf {
    let base_template = template
        .replace("%(id)s", video_id)
        .replace("%(ext)s", "mp4");
    // Existing yt-dlp templates can contain many advanced tokens. Native
    // resolvers intentionally support the documented core fields; fall back to
    // a predictable filename if an unsupported token remains.
    let effective_template = if base_template.replace("%(title)s", "").contains("%(") {
        format!("%(title)s [{video_id}] [{}].mp4", download_variant(request))
    } else {
        base_template
    };

    let mut filename = sanitize_filename(&effective_template.replace("%(title)s", &request.title));
    let full_path = Path::new(&request.output_directory).join(&filename);
    let (max_bytes, max_utf16) = filename_limits(&request.output_directory);
    if fits_filename_limits(&filename, max_bytes, max_utf16) || full_path.is_file() {
        return full_path;
    }

    if effective_template.contains("%(title)s") {
        let clean_title = clean_filename_chars(&request.title);
        let without_title = sanitize_filename(&effective_template.replace("%(title)s", ""));
        let occurrences = effective_template.matches("%(title)s").count().max(1);
        let overhead_bytes = without_title.len() + occurrences;
        let overhead_utf16 = without_title.encode_utf16().count() + occurrences;
        let title_max_bytes = max_bytes.saturating_sub(overhead_bytes) / occurrences;
        let title_max_utf16 = max_utf16.saturating_sub(overhead_utf16) / occurrences;
        let short_title = truncate_to_limits(&clean_title, title_max_bytes, title_max_utf16);
        filename = sanitize_filename(&effective_template.replace("%(title)s", &short_title));
    }

    let (component_bytes, component_utf16) = component_limits();
    filename = clamp_component_length(&filename, component_bytes, component_utf16);
    Path::new(&request.output_directory).join(filename)
}

fn find_existing_native_output(
    request: &DownloadRequest,
    output_path: &Path,
    video_id: &str,
) -> Option<PathBuf> {
    if output_path.is_file() {
        return Some(output_path.to_path_buf());
    }
    if video_id.is_empty() {
        return None;
    }
    let suffix = format!("[{video_id}] [{}].mp4", download_variant(request));
    let expected_name = output_path.file_name()?.to_string_lossy();
    if !ends_with_filename_suffix(&expected_name, &suffix) {
        return None;
    }
    let entries = fs::read_dir(&request.output_directory).ok()?;
    let mut matches: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| ends_with_filename_suffix(&name.to_string_lossy(), &suffix))
        })
        .collect();
    matches.sort();
    matches.into_iter().next()
}

fn ends_with_filename_suffix(name: &str, suffix: &str) -> bool {
    if cfg!(windows) {
        name.to_ascii_lowercase()
            .ends_with(&suffix.to_ascii_lowercase())
    } else {
        name.ends_with(suffix)
    }
}

fn component_limits() -> (usize, usize) {
    if cfg!(windows) {
        (usize::MAX, MAX_FILENAME_UTF16)
    } else {
        (MAX_FILENAME_BYTES, usize::MAX)
    }
}

fn filename_limits(directory: &str) -> (usize, usize) {
    if cfg!(windows) {
        let dir_utf16 = directory
            .trim_end_matches(['\\', '/'])
            .encode_utf16()
            .count();
        let max_utf16 = WINDOWS_MAX_PATH_UTF16
            .saturating_sub(dir_utf16 + 1 + PART_SUFFIX_LEN)
            .min(MAX_FILENAME_UTF16);
        (usize::MAX, max_utf16)
    } else {
        (MAX_FILENAME_BYTES, usize::MAX)
    }
}

fn fits_filename_limits(filename: &str, max_bytes: usize, max_utf16: usize) -> bool {
    filename.len() <= max_bytes && filename.encode_utf16().count() <= max_utf16
}

fn truncate_to_limits(value: &str, max_bytes: usize, max_utf16: usize) -> String {
    let mut out = String::new();
    let mut bytes = 0;
    let mut utf16 = 0;
    for ch in value.chars() {
        let ch_bytes = ch.len_utf8();
        let ch_utf16 = ch.len_utf16();
        if ch_bytes > max_bytes.saturating_sub(bytes)
            || ch_utf16 > max_utf16.saturating_sub(utf16)
        {
            break;
        }
        out.push(ch);
        bytes += ch_bytes;
        utf16 += ch_utf16;
    }
    out.trim_end_matches([' ', '.']).to_owned()
}

fn clamp_component_length(filename: &str, max_bytes: usize, max_utf16: usize) -> String {
    if filename.len() <= max_bytes && filename.encode_utf16().count() <= max_utf16 {
        return filename.to_owned();
    }
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, format!(".{ext}")),
        _ => (filename, String::new()),
    };
    let stem_max_bytes = max_bytes.saturating_sub(ext.len());
    let stem_max_utf16 = max_utf16.saturating_sub(ext.encode_utf16().count());
    let short_stem = truncate_to_limits(stem, stem_max_bytes, stem_max_utf16);
    if short_stem.is_empty() {
        format!("douyin-video{ext}")
    } else {
        sanitize_filename(&format!("{short_stem}{ext}"))
    }
}

fn clean_filename_chars(value: &str) -> String {
    let clean: String = value
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
    clean
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches([' ', '.'])
        .to_owned()
}

fn sanitize_filename(value: &str) -> String {
    let mut clean = clean_filename_chars(value);
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

fn variant_output_template(template: &str, request: &DownloadRequest) -> String {
    let variant = download_variant(request);
    if template.contains("%(quality)s") {
        return template.replace("%(quality)s", &variant);
    }
    let Some(extension_at) = template.rfind("%(ext)s") else {
        return template.to_owned();
    };
    let (prefix, suffix) = template.split_at(extension_at);
    if let Some(stem) = prefix.strip_suffix('.') {
        format!("{stem} [{variant}].{suffix}")
    } else {
        format!("{prefix} [{variant}]{suffix}")
    }
}

fn download_variant(request: &DownloadRequest) -> String {
    match request.kind {
        crate::models::MediaKind::Video => {
            let label = sanitize_variant(&request.format_label);
            if label.is_empty() {
                format!("format-{}", sanitize_variant(&request.format_id))
            } else {
                label
            }
        }
        crate::models::MediaKind::Audio => format!(
            "audio-{}",
            request.audio_format.as_deref().unwrap_or("audio")
        ),
    }
}

fn sanitize_variant(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
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

fn is_terminal_status(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Completed | TaskStatus::Existing | TaskStatus::Failed | TaskStatus::Cancelled
    )
}

fn update_task<F: FnOnce(&mut DownloadTask)>(
    app: &AppHandle,
    inner: &Arc<Inner>,
    id: &str,
    change: F,
) {
    let task = inner.state.lock().ok().and_then(|mut state| {
        let task = state.tasks.get_mut(id)?;
        if is_terminal_status(&task.status) {
            return None;
        }
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
    let task = inner
        .state
        .lock()
        .ok()
        .and_then(|mut state| state.finish(id, status, output, error));
    if let Some(task) = task {
        let _ = storage::append_history(app, &task);
        let _ = app.emit("download://updated", task);
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
        classify_successful_output, existing_output_path, find_existing_native_output,
        native_output_path, open_partial_file, validate_request, variant_output_template,
        OutputTracker, QueueState,
    };
    use crate::models::{DownloadRequest, DownloadTask, MediaKind, TaskStatus};
    use std::{fs, io::Write, sync::atomic::Ordering};

    fn task(id: &str) -> DownloadTask {
        DownloadTask {
            id: id.to_owned(),
            url: "https://www.douyin.com/video/7699483289760959786".to_owned(),
            title: "test".to_owned(),
            thumbnail: None,
            format_label: "1080p".to_owned(),
            kind: MediaKind::Video,
            status: TaskStatus::Pending,
            percent: None,
            speed: None,
            eta: None,
            output_path: None,
            error: None,
            created_at: 1,
            updated_at: 1,
        }
    }

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
    fn adds_the_selected_quality_to_legacy_templates() {
        let mut request = request("30016");
        request.format_label = "360p".to_owned();

        assert_eq!(
            variant_output_template("%(title)s [%(id)s].%(ext)s", &request),
            "%(title)s [%(id)s] [360p].%(ext)s"
        );
    }

    #[test]
    fn gives_different_qualities_different_output_templates() {
        let mut low = request("30016");
        low.format_label = "360p".to_owned();
        let mut high = request("30080");
        high.format_label = "1080p".to_owned();
        let template = "%(title)s [%(id)s].%(ext)s";

        assert_ne!(
            variant_output_template(template, &low),
            variant_output_template(template, &high)
        );
    }

    #[test]
    fn gives_douyin_qualities_stable_distinct_paths() {
        let mut low = request("douyin:720p");
        low.format_label = "720p".to_owned();
        let mut high = request("douyin:1080p");
        high.format_label = "1080p".to_owned();
        let base_template = "%(title)s [%(id)s].%(ext)s";
        let low_template = variant_output_template(base_template, &low);
        let high_template = variant_output_template(base_template, &high);

        let first_low = native_output_path(&low, &low_template, "7699483289760959786");
        let second_low = native_output_path(&low, &low_template, "7699483289760959786");
        let high_path = native_output_path(&high, &high_template, "7699483289760959786");

        assert_eq!(first_low, second_low);
        assert_ne!(first_low, high_path);
        assert!(first_low.to_string_lossy().contains("[720p]"));
        assert!(high_path.to_string_lossy().contains("[1080p]"));
    }

    #[test]
    fn replaces_the_quality_placeholder_for_video_and_audio() {
        let mut video = request("30080");
        video.format_label = "1080p".to_owned();
        let mut audio = request("bestaudio");
        audio.kind = MediaKind::Audio;
        audio.audio_format = Some("mp3".to_owned());
        let template = "%(title)s [%(id)s] [%(quality)s].%(ext)s";

        assert_eq!(
            variant_output_template(template, &video),
            "%(title)s [%(id)s] [1080p].%(ext)s"
        );
        assert_eq!(
            variant_output_template(template, &audio),
            "%(title)s [%(id)s] [audio-mp3].%(ext)s"
        );
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

    #[test]
    fn overwrites_stale_partial_file_on_retry() {
        let directory =
            std::env::temp_dir().join(format!("ytharbor-part-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("test directory should be created");
        let part_path = directory.join("video.mp4.part");
        fs::write(&part_path, b"stale partial data").expect("stale part file should be created");

        let mut file =
            open_partial_file(&part_path).expect("stale part file should be overwritten");
        file.write_all(b"fresh")
            .expect("fresh data should be written");
        drop(file);

        assert_eq!(
            fs::read(&part_path).expect("part file should be readable"),
            b"fresh"
        );
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    #[test]
    fn cancelling_pending_task_prevents_worker_from_claiming_it() {
        let mut state = QueueState::default();
        state.insert(task("task-1"));

        let cancelled = state
            .cancel("task-1")
            .expect("pending task cancel should succeed")
            .expect("pending cancel should transition immediately");

        assert_eq!(cancelled.status, TaskStatus::Cancelled);
        assert!(state.claim_for_run("task-1").is_none());
        assert!(state.cancel("task-1").is_err());
    }

    #[test]
    fn cancelling_claimed_task_signals_worker_and_preserves_control_until_finish() {
        let mut state = QueueState::default();
        state.insert(task("task-2"));

        let cancel_flag = state
            .claim_for_run("task-2")
            .expect("pending task should be claimable");
        assert!(!cancel_flag.load(Ordering::Relaxed));

        let immediate = state
            .cancel("task-2")
            .expect("running task cancel should succeed");
        assert!(immediate.is_none());
        assert!(cancel_flag.load(Ordering::Relaxed));

        let finished = state
            .finish("task-2", TaskStatus::Cancelled, None, None)
            .expect("running task should transition on finish");
        assert_eq!(finished.status, TaskStatus::Cancelled);
        assert!(state.running_id.is_none());
        assert!(state.controls.is_empty());
    }

    #[test]
    fn cancelling_after_finish_is_rejected_and_preserves_completed_status() {
        let mut state = QueueState::default();
        state.insert(task("task-3"));
        let _ = state.claim_for_run("task-3").expect("task should claim");

        let completed = state
            .finish(
                "task-3",
                TaskStatus::Completed,
                Some("C:\\Downloads\\out.mp4".to_owned()),
                None,
            )
            .expect("task should finish as completed");
        assert_eq!(completed.status, TaskStatus::Completed);
        assert!(state.cancel("task-3").is_err());
        assert!(state
            .finish("task-3", TaskStatus::Cancelled, None, None)
            .is_none());
        assert_eq!(
            state.tasks.get("task-3").expect("task should remain").status,
            TaskStatus::Completed
        );
    }

    #[test]
    fn truncates_long_douyin_titles_while_preserving_id_quality_and_extension() {
        let long_title = "超长抖音文案测试🎉#话题标签 ".repeat(30);
        let mut low = request("douyin:720p");
        low.title = long_title.clone();
        low.format_label = "720p".to_owned();
        let mut high = request("douyin:1080p");
        high.title = long_title;
        high.format_label = "1080p".to_owned();
        let base_template = "%(title)s [%(id)s] [%(quality)s].%(ext)s";

        let low_path = native_output_path(
            &low,
            &variant_output_template(base_template, &low),
            "7699483289760959786",
        );
        let high_path = native_output_path(
            &high,
            &variant_output_template(base_template, &high),
            "7699483289760959786",
        );

        let low_name = low_path
            .file_name()
            .expect("low filename should exist")
            .to_string_lossy();
        let high_name = high_path
            .file_name()
            .expect("high filename should exist")
            .to_string_lossy();

        assert_ne!(low_path, high_path);
        if cfg!(windows) {
            assert!(low_name.encode_utf16().count() + ".part".len() <= 255);
            assert!(high_name.encode_utf16().count() + ".part".len() <= 255);
        } else {
            assert!(low_name.len() + ".part".len() <= 255);
            assert!(high_name.len() + ".part".len() <= 255);
        }
        assert!(low_name.ends_with("[7699483289760959786] [720p].mp4"));
        assert!(high_name.ends_with("[7699483289760959786] [1080p].mp4"));
    }

    #[test]
    fn preserves_legacy_filename_for_titles_with_dots_and_medium_chinese_length() {
        let mut req = request("douyin:1080p");
        // 90 Chinese chars (> 255 UTF-8 bytes) ending with "..." — valid on Windows NTFS
        // and must keep the exact pre-truncation filename for existing-file detection.
        req.title = format!("{}...", "抖音中文标题测试字符".repeat(9));
        req.format_label = "1080p".to_owned();
        let template = variant_output_template("%(title)s [%(id)s] [%(quality)s].%(ext)s", &req);

        let path = native_output_path(&req, &template, "7699483289760959786");
        let name = path
            .file_name()
            .expect("filename should exist")
            .to_string_lossy();

        if cfg!(windows) {
            assert_eq!(
                name,
                format!("{} [7699483289760959786] [1080p].mp4", req.title)
            );
        } else {
            assert!(name.len() + ".part".len() <= 255);
            assert!(name.ends_with("[7699483289760959786] [1080p].mp4"));
        }
    }

    #[test]
    fn fits_long_douyin_filename_in_deep_directory_and_creates_file() {
        let base = std::env::temp_dir().join(format!("ytharbor-long-{}", uuid::Uuid::new_v4()));
        let deep = base
            .join("nested-downloads-directory-segment-one")
            .join("nested-downloads-directory-segment-two")
            .join("nested-downloads-directory-segment-three");
        fs::create_dir_all(&deep).expect("deep directory should be created");

        let mut req = request("douyin:1080p");
        req.output_directory = deep.to_string_lossy().to_string();
        req.title = "这是一段非常非常长的抖音视频标题包含很多话题标签".repeat(12);
        req.format_label = "1080p".to_owned();
        let template = variant_output_template("%(title)s [%(id)s] [%(quality)s].%(ext)s", &req);

        let output_path = native_output_path(&req, &template, "7699483289760959786");
        let partial_path = output_path.with_file_name(format!(
            "{}.part",
            output_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));

        if cfg!(windows) {
            assert!(
                partial_path.to_string_lossy().encode_utf16().count() <= 259,
                "partial path should fit Windows MAX_PATH: {}",
                partial_path.display()
            );
        }
        assert!(output_path
            .file_name()
            .expect("filename should exist")
            .to_string_lossy()
            .ends_with("[7699483289760959786] [1080p].mp4"));

        let mut file =
            open_partial_file(&partial_path).expect("partial file should be creatable on disk");
        file.write_all(b"mp4-data")
            .expect("partial file should be writable");
        drop(file);
        fs::rename(&partial_path, &output_path).expect("partial file should rename to final path");
        assert!(output_path.is_file());

        fs::remove_dir_all(base).expect("test directory should be removed");
    }

    #[test]
    fn finds_existing_douyin_file_by_id_and_quality_suffix_when_title_differs() {
        let directory = std::env::temp_dir().join(format!(
            "ytharbor-douyin-existing-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("test directory should be created");

        let existing_1080 = directory.join("旧标题截断版本 [7699483289760959786] [1080p].mp4");
        let existing_720 = directory.join("旧标题截断版本 [7699483289760959786] [720p].mp4");
        let stale_part = directory.join("新标题完整版本 [7699483289760959786] [1440p].mp4.part");
        fs::write(&existing_1080, b"video").expect("1080p file should be created");
        fs::write(&existing_720, b"video").expect("720p file should be created");
        fs::write(&stale_part, b"part").expect("part file should be created");

        let mut req = request("douyin:1080p");
        req.output_directory = directory.to_string_lossy().to_string();
        req.title = "完全不同的新标题".to_owned();
        req.format_label = "1080p".to_owned();
        let template = variant_output_template("%(title)s [%(id)s] [%(quality)s].%(ext)s", &req);
        let expected_path = native_output_path(&req, &template, "7699483289760959786");

        assert_eq!(
            find_existing_native_output(&req, &expected_path, "7699483289760959786"),
            Some(existing_1080)
        );

        let mut req_1440 = req.clone();
        req_1440.format_id = "douyin:1440p".to_owned();
        req_1440.format_label = "1440p".to_owned();
        let template_1440 =
            variant_output_template("%(title)s [%(id)s] [%(quality)s].%(ext)s", &req_1440);
        let expected_1440 = native_output_path(&req_1440, &template_1440, "7699483289760959786");
        assert_eq!(
            find_existing_native_output(&req_1440, &expected_1440, "7699483289760959786"),
            None
        );

        fs::remove_dir_all(directory).expect("test directory should be removed");
    }
}
