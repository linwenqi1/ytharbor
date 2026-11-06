use crate::{metadata, models::{DownloadRequest, DownloadTask, TaskStatus}, progress, storage};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{BufRead, BufReader},
    process::Stdio,
    sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};

struct QueuedDownload {
    id: String,
    request: DownloadRequest,
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
        fs::create_dir_all(&request.output_directory).map_err(|error| format!("无法创建下载目录：{error}"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        let task = DownloadTask {
            id: id.clone(), url: request.url.clone(), title: request.title.clone(), thumbnail: request.thumbnail.clone(),
            format_label: request.format_label.clone(), kind: request.kind, status: TaskStatus::Pending,
            percent: None, speed: None, eta: None, output_path: None, error: None, created_at: now, updated_at: now,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.inner.tasks.lock().map_err(lock_error)?.insert(id.clone(), task.clone());
        self.inner.controls.lock().map_err(lock_error)?.insert(id.clone(), cancel);
        self.queue.send(QueuedDownload { id, request }).map_err(|_| "下载队列已停止".to_owned())?;
        Ok(task)
    }

    pub fn cancel(&self, app: &AppHandle, id: &str) -> Result<(), String> {
        let control = self.inner.controls.lock().map_err(lock_error)?.get(id).cloned().ok_or_else(|| "任务不存在或已经结束".to_owned())?;
        control.store(true, Ordering::Relaxed);
        let is_running = self.inner.running_id.lock().map_err(lock_error)?.as_deref() == Some(id);
        if !is_running {
            finish_task(app, &self.inner, id, TaskStatus::Cancelled, None, None);
            self.inner.controls.lock().map_err(lock_error)?.remove(id);
        }
        Ok(())
    }

    pub fn tasks(&self) -> Result<Vec<DownloadTask>, String> {
        let mut tasks: Vec<_> = self.inner.tasks.lock().map_err(lock_error)?.values().cloned().collect();
        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(tasks)
    }
}

fn queue_worker(app: AppHandle, inner: Arc<Inner>, receiver: mpsc::Receiver<QueuedDownload>) {
    while let Ok(item) = receiver.recv() {
        let cancelled = inner.controls.lock().ok().and_then(|items| items.get(&item.id).cloned()).is_none_or(|flag| flag.load(Ordering::Relaxed));
        let still_pending = inner.tasks.lock().ok().and_then(|tasks| tasks.get(&item.id).cloned()).is_some_and(|task| task.status == TaskStatus::Pending);
        if cancelled || !still_pending { continue; }
        if let Ok(mut running) = inner.running_id.lock() { *running = Some(item.id.clone()); }
        run_download(&app, &inner, item);
        if let Ok(mut running) = inner.running_id.lock() { *running = None; }
    }
}

fn run_download(app: &AppHandle, inner: &Arc<Inner>, item: QueuedDownload) {
    update_task(app, inner, &item.id, |task| task.status = TaskStatus::Downloading);
    let mut args = vec![
        "--newline".to_owned(), "--no-playlist".to_owned(), "--progress".to_owned(),
        "--progress-template".to_owned(), "download:ytharbor-progress:%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s".to_owned(),
        "--print".to_owned(), "after_move:ytharbor-file:%(filepath)s".to_owned(),
        "-P".to_owned(), item.request.output_directory.clone(),
    ];
    let template = storage::load_settings(app).map(|value| value.filename_template).unwrap_or_else(|_| "%(title)s [%(id)s].%(ext)s".to_owned());
    args.extend(["-o".to_owned(), template]);
    match item.request.kind {
        crate::models::MediaKind::Video => {
            args.extend(["-f".to_owned(), format!("{}+bestaudio/{}", item.request.format_id, item.request.format_id), "--merge-output-format".to_owned(), "mp4".to_owned()]);
        }
        crate::models::MediaKind::Audio => {
            args.extend(["-f".to_owned(), "bestaudio/best".to_owned(), "--extract-audio".to_owned(), "--audio-format".to_owned(), item.request.audio_format.clone().unwrap_or_else(|| "mp3".to_owned())]);
        }
    }
    if let Some(path) = metadata::ffmpeg_directory(app) { args.extend(["--ffmpeg-location".to_owned(), path.to_string_lossy().to_string()]); }
    if let Some(runtime) = metadata::deno_binary(app) { args.extend(["--js-runtimes".to_owned(), format!("deno:{}", runtime.to_string_lossy())]); }
    args.extend(["--".to_owned(), item.request.url.clone()]);

    let spawned = metadata::yt_dlp_command(app).args(&args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            finish_task(app, inner, &item.id, TaskStatus::Failed, None, Some(format!("无法启动 yt-dlp：{error}")));
            cleanup(inner, &item.id);
            return;
        }
    };
    let (line_tx, line_rx) = mpsc::channel::<String>();
    let mut out_thread = child.stdout.take().map(|stdout| spawn_reader(stdout, line_tx.clone()));
    let mut err_thread = child.stderr.take().map(|stderr| spawn_reader(stderr, line_tx));
    let cancel = inner.controls.lock().ok().and_then(|items| items.get(&item.id).cloned()).unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let mut recent = VecDeque::<String>::with_capacity(24);
    let mut output_path = None;

    let exit_status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            if let Some(handle) = out_thread.take() { let _ = handle.join(); }
            if let Some(handle) = err_thread.take() { let _ = handle.join(); }
            finish_task(app, inner, &item.id, TaskStatus::Cancelled, None, None);
            cleanup(inner, &item.id);
            return;
        }
        match line_rx.recv_timeout(Duration::from_millis(120)) {
            Ok(line) => process_line(app, inner, &item.id, &line, &mut output_path, &mut recent),
            Err(mpsc::RecvTimeoutError::Disconnected) => {},
            Err(mpsc::RecvTimeoutError::Timeout) => {},
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {},
            Err(error) => {
                finish_task(app, inner, &item.id, TaskStatus::Failed, None, Some(format!("下载进程异常：{error}")));
                cleanup(inner, &item.id);
                return;
            }
        }
    };
    if let Some(handle) = out_thread.take() { let _ = handle.join(); }
    if let Some(handle) = err_thread.take() { let _ = handle.join(); }
    while let Ok(line) = line_rx.try_recv() { process_line(app, inner, &item.id, &line, &mut output_path, &mut recent); }
    if exit_status.success() {
        finish_task(app, inner, &item.id, TaskStatus::Completed, output_path, None);
    } else {
        let message = recent.iter().rev().find(|line| line.contains("ERROR:")).or_else(|| recent.back()).cloned().unwrap_or_else(|| "下载失败，请检查网络后重试".to_owned());
        finish_task(app, inner, &item.id, TaskStatus::Failed, None, Some(message.trim_start_matches("ERROR:").trim().to_owned()));
    }
    cleanup(inner, &item.id);
}

fn process_line(app: &AppHandle, inner: &Arc<Inner>, id: &str, line: &str, output_path: &mut Option<String>, recent: &mut VecDeque<String>) {
    if let Some(parsed) = progress::parse_progress(line) {
        update_task(app, inner, id, |task| { task.status = TaskStatus::Downloading; task.percent = parsed.percent.or(task.percent); task.speed = parsed.speed; task.eta = parsed.eta; });
    } else if let Some(path) = line.strip_prefix("ytharbor-file:") {
        *output_path = Some(path.trim().to_owned());
    } else if ["[Merger]", "[ExtractAudio]", "[VideoConvertor]", "[Fixup"].iter().any(|marker| line.contains(marker)) {
        update_task(app, inner, id, |task| { task.status = TaskStatus::Processing; task.percent = Some(100.0); task.speed = None; task.eta = None; });
    }
    if !line.trim().is_empty() {
        if recent.len() == 24 { recent.pop_front(); }
        recent.push_back(line.trim().to_owned());
    }
}

fn spawn_reader<R: std::io::Read + Send + 'static>(reader: R, sender: mpsc::Sender<String>) -> thread::JoinHandle<()> {
    thread::spawn(move || for line in BufReader::new(reader).lines().map_while(Result::ok) { let _ = sender.send(line); })
}

fn update_task<F: FnOnce(&mut DownloadTask)>(app: &AppHandle, inner: &Arc<Inner>, id: &str, change: F) {
    let task = inner.tasks.lock().ok().and_then(|mut tasks| {
        let task = tasks.get_mut(id)?;
        change(task); task.updated_at = now_ms(); Some(task.clone())
    });
    if let Some(task) = task { let _ = app.emit("download://updated", task); }
}

fn finish_task(app: &AppHandle, inner: &Arc<Inner>, id: &str, status: TaskStatus, output: Option<String>, error: Option<String>) {
    let task = inner.tasks.lock().ok().and_then(|mut tasks| {
        let task = tasks.get_mut(id)?;
        task.status = status; task.output_path = output; task.error = error; task.updated_at = now_ms();
        if task.status == TaskStatus::Completed { task.percent = Some(100.0); }
        task.speed = None; task.eta = None; Some(task.clone())
    });
    if let Some(task) = task { let _ = storage::append_history(app, &task); let _ = app.emit("download://updated", task); }
}

fn cleanup(inner: &Arc<Inner>, id: &str) { if let Ok(mut controls) = inner.controls.lock() { controls.remove(id); } }

fn validate_request(request: &DownloadRequest) -> Result<(), String> {
    if request.output_directory.trim().is_empty() { return Err("请先选择下载目录".to_owned()); }
    if request.format_id.len() > 100 || !request.format_id.chars().all(|c| c.is_ascii_alphanumeric() || ".+_-".contains(c)) { return Err("下载格式无效".to_owned()); }
    if request.kind == crate::models::MediaKind::Audio && !matches!(request.audio_format.as_deref(), Some("mp3" | "m4a")) { return Err("音频格式无效".to_owned()); }
    Ok(())
}

fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64 }
fn lock_error<T>(_: std::sync::PoisonError<T>) -> String { "任务状态暂时不可用".to_owned() }
