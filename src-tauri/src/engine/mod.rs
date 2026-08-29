//! IDIN download engine.
//!
//! Pure Rust — no Tauri imports here. Everything in this module must stay
//! testable with plain `cargo test`.

pub mod events;
pub mod jalali;
pub mod persist;
pub mod probe;
pub mod segment;
pub mod task;

use crate::config::{self, SharedConfig};
use events::ProgressEvent;
use probe::{probe, RequestOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use task::{DownloadTask, TaskState};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Shared HTTP client for all downloads.
pub type SharedClient = Arc<reqwest::Client>;

/// Build the engine's shared HTTP client (rustls, streaming, redirects).
pub fn build_client() -> SharedClient {
    build_client_with_proxy("").expect("failed to build HTTP client")
}

/// Build the engine's HTTP client, optionally routed through a proxy.
/// Supports `http://`, `https://` and `socks5://` proxy URLs (reqwest `socks`).
pub fn build_client_with_proxy(proxy_url: &str) -> Result<SharedClient, String> {
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!("IDIN/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::limited(10));
    let proxy = proxy_url.trim();
    if !proxy.is_empty() {
        let p = reqwest::Proxy::all(proxy).map_err(|e| format!("invalid proxy URL: {e}"))?;
        builder = builder.proxy(p);
    }
    builder
        .build()
        .map(Arc::new)
        .map_err(|e| format!("failed to build HTTP client: {e}"))
}

/// Events the engine emits toward the app/UI.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    Progress(ProgressEvent),
    StateChanged {
        task_id: u64,
        state: TaskState,
    },
    /// All downloads finished (Done or Failed). Backend can trigger post-action.
    AllFinished,
}

/// One segment's resume state.
#[derive(Debug, Clone)]
struct SegmentState {
    start: u64,
    end: u64,
    written: u64,
}

/// Per-segment state shared between the engine and its workers.
type SegmentList = Vec<Arc<Mutex<SegmentState>>>;

/// A task's live workers.
struct Running {
    /// Abort handles for the active segment workers.
    handles: Vec<JoinHandle<()>>,
    segments: SegmentList,
}

/// The download orchestrator: owns tasks, spawns segment workers,
/// aggregates progress, supports pause/resume/remove.
pub struct Engine {
    /// Shared HTTP client for all downloads. Wrapped in a RwLock so the
    /// global proxy can be swapped at runtime (reqwest clients are immutable).
    client: std::sync::RwLock<SharedClient>,
    next_id: AtomicU64,
    tasks: Arc<Mutex<HashMap<u64, DownloadTask>>>,
    running: Arc<Mutex<HashMap<u64, Running>>>,
    /// Segment offsets saved on pause, keyed by task id (used on resume).
    paused: Arc<Mutex<HashMap<u64, SegmentList>>>,
    /// Global speed limit in bytes/sec (0 = unlimited).
    global_limit: Arc<AtomicU64>,
    /// Task ordering: stores IDs in priority order.
    order: Arc<Mutex<Vec<u64>>>,
    event_tx: mpsc::UnboundedSender<EngineEvent>,
    /// Optional persistence dir (app config dir). When set, task state and
    /// history are written to disk.
    persist_dir: Arc<std::sync::RwLock<Option<PathBuf>>>,
    /// Max simultaneous downloads (0 = unlimited).
    max_concurrent: Arc<AtomicU64>,
}

const MIN_SERVER_SIZE: u64 = 4 * 1024 * 1024; // below this, single connection
/// Default automatic retry attempts per task.
pub const DEFAULT_RETRIES: u32 = 3;

impl Engine {
    pub fn new(event_tx: mpsc::UnboundedSender<EngineEvent>) -> Arc<Self> {
        Arc::new(Self {
            client: std::sync::RwLock::new(build_client()),
            next_id: AtomicU64::new(1),
            tasks: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(Mutex::new(HashMap::new())),
            paused: Arc::new(Mutex::new(HashMap::new())),
            global_limit: Arc::new(AtomicU64::new(0)),
            order: Arc::new(Mutex::new(Vec::new())),
            event_tx,
            persist_dir: Arc::new(std::sync::RwLock::new(None)),
            max_concurrent: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Enable persistence: `dir` is the app config dir.
    pub fn set_persist_dir(&self, dir: PathBuf) {
        *self.persist_dir.write().unwrap() = Some(dir);
    }

    /// Set the max number of simultaneous downloads (0 = unlimited). When a
    /// slot frees up, queued tasks are started automatically.
    pub fn set_max_concurrent(self: &Arc<Self>, n: u64) {
        self.max_concurrent.store(n, Ordering::Relaxed);
        self.pump_queue();
    }

    /// Swap the shared HTTP client to route through a global proxy.
    /// An empty string removes the proxy. On an invalid URL the old client
    /// is kept and an error is returned.
    pub fn set_global_proxy(&self, proxy_url: &str) -> Result<(), String> {
        let new_client = build_client_with_proxy(proxy_url)?;
        *self.client.write().unwrap() = new_client;
        Ok(())
    }

    fn persist_state(&self) {
        let dir = self.persist_dir.read().unwrap().clone();
        if let Some(dir) = dir {
            let tasks = self.list();
            let _ = persist::save_state(&dir, &tasks);
        }
    }

    /// Record a finished task into the history log (if persistence enabled).
    fn record_history(&self, task: &DownloadTask, outcome: &str) {
        let dir = self.persist_dir.read().unwrap().clone();
        if let Some(dir) = dir {
            persist::record_task_outcome(&dir, task, outcome);
        }
    }

    fn active_count(&self) -> usize {
        self.tasks
            .lock()
            .unwrap()
            .values()
            .filter(|t| matches!(t.state, TaskState::Downloading | TaskState::Probing))
            .count()
    }

    /// Start queued tasks while concurrency slots are free (priority order).
    fn pump_queue(self: &Arc<Self>) {
        let max = self.max_concurrent.load(Ordering::Relaxed);
        if max == 0 {
            return;
        }
        loop {
            let active = self.active_count() as u64;
            if active >= max {
                return;
            }
            let next = {
                let tasks = self.tasks.lock().unwrap();
                let order = self.order.lock().unwrap();
                order
                    .iter()
                    .find(|id| {
                        tasks
                            .get(id)
                            .is_some_and(|t| t.state == TaskState::Queued && !t.scheduled)
                    })
                    .copied()
            };
            let Some(id) = next else { return };
            let engine = self.clone();
            tokio::spawn(async move {
                let _ = engine.resume(id);
            });
        }
    }

    fn emit(&self, ev: EngineEvent) {
        let _ = self.event_tx.send(ev);
    }

    fn set_state(&self, id: u64, state: TaskState) {
        if let Ok(mut tasks) = self.tasks.lock() {
            if let Some(t) = tasks.get_mut(&id) {
                t.state = state;
                self.emit(EngineEvent::StateChanged { task_id: id, state });
            }
        }
    }

    /// Add a task; probes the URL and spawns segment workers.
    /// If `config` is provided, uses it for auto-categorization and download dir.
    /// `options` carries per-download headers/cookies/basic auth; `proxy`
    /// overrides the global proxy for this task alone.
    pub async fn add(
        self: &Arc<Self>,
        url: String,
        mut destination: PathBuf,
        segments_requested: u32,
        config: Option<SharedConfig>,
        options: Option<RequestOptions>,
        proxy: Option<String>,
        duplicate_action: Option<String>,
    ) -> Result<u64, String> {
        // Only HTTP(S) is supported — reqwest does not speak FTP or other
        // schemes. Fail fast with a clear, user-facing message.
        let lower = url.trim().to_ascii_lowercase();
        if !(lower.starts_with("http://") || lower.starts_with("https://")) {
            return Err(if lower.starts_with("ftp://") {
                "FTP is not supported — IDIN downloads over HTTP/HTTPS only. \
                 Please use an https:// link."
                    .to_string()
            } else {
                format!("Unsupported URL scheme (HTTP/HTTPS only): {url}")
            });
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // Merge cookies into the header map (the task model stores one map).
        let mut opts = options.unwrap_or_default();
        if let Some(c) = opts.cookies.take() {
            if !c.is_empty() {
                opts.headers.insert("Cookie".into(), c);
            }
        }

        let client = self.client.read().unwrap().clone();
        let p = probe(&client, &url, &opts).await?;

        let name = p
            .filename
            .clone()
            .unwrap_or_else(|| filename_from_url(&p.final_url));

        // Auto-categorize: if destination is a directory (or empty), resolve via config.
        let (category, scheduled) = if let Some(cfg) = config.as_ref() {
            if let Ok(c) = cfg.read() {
                // Use config's download_dir as the base if destination is empty/default.
                if destination.as_os_str().is_empty() || destination.as_os_str() == "." {
                    destination = c.download_dir.clone();
                }
                // Honor a pending schedule: while a scheduled start is in the
                // future, the task waits in the real Queued state and the
                // scheduler starts it later.
                let sched = c.scheduled_start.is_some_and(|ts| {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    now < ts
                });
                (config::classify_file(&c, &name), sched)
            } else {
                (None, false)
            }
        } else {
            (None, false)
        };

        // Append category sub-folder if matched.
        let mut destination = if destination.is_dir() || destination.extension().is_none() {
            let mut base = destination;
            if let Some(ref cat) = category {
                base = base.join(cat);
            }
            base.join(&name)
        } else {
            destination
        };

        // Ensure parent directory exists.
        if let Some(parent) = destination.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // ── Duplicate handling: same URL tracked, or file already on disk ──
        let action = DuplicateAction::parse(duplicate_action.as_deref());
        {
            let matches: Vec<(u64, TaskState)> = {
                let tasks = self.tasks.lock().unwrap();
                tasks
                    .values()
                    .filter(|t| t.url == url || t.url == p.final_url)
                    .map(|t| (t.id, t.state))
                    .collect()
            };
            if !matches.is_empty() {
                match action {
                    DuplicateAction::Overwrite => {
                        // Drop every old task with this URL; the fresh
                        // download takes over the path.
                        for (rid, _) in matches {
                            self.remove(rid);
                        }
                    }
                    DuplicateAction::Resume => {
                        // Continue the first paused/queued task, if any.
                        let resumable = matches
                            .iter()
                            .find(|(_, st)| matches!(st, TaskState::Paused | TaskState::Queued));
                        match resumable {
                            Some(&(rid, _)) => {
                                self.resume(rid)?;
                                return Ok(rid);
                            }
                            // Already active/done: just surface it again.
                            None => return Ok(matches[0].0),
                        }
                    }
                    DuplicateAction::Rename | DuplicateAction::Auto => {
                        destination = alternate_destination(&destination);
                    }
                }
            } else if destination.exists() {
                match action {
                    DuplicateAction::Overwrite => {
                        std::fs::remove_file(&destination)
                            .map_err(|e| format!("cannot overwrite file: {e}"))?;
                    }
                    DuplicateAction::Rename | DuplicateAction::Auto => {
                        destination = alternate_destination(&destination);
                    }
                    // Nothing tracked to resume; download fresh over the file.
                    DuplicateAction::Resume => {}
                }
            }
            // Ensure the (possibly renamed) parent still exists.
            if let Some(parent) = destination.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        let total = p.total_bytes;
        let n_segments = if p.accepts_ranges && total.is_some_and(|t| t > MIN_SERVER_SIZE) {
            segments_requested.max(1) as u64
        } else {
            1
        };

        let state = if scheduled {
            TaskState::Queued
        } else {
            TaskState::Downloading
        };

        {
            let mut tasks = self.tasks.lock().unwrap();
            let mut task = DownloadTask::new(id, p.final_url.clone(), destination.clone());
            task.total_bytes = total;
            task.state = state;
            task.segments = n_segments as u32;
            task.category = category;
            task.headers = opts.headers.clone();
            task.basic_auth = opts.basic_auth.clone();
            task.proxy = proxy.filter(|s| !s.trim().is_empty());
            tasks.insert(id, task);
        }
        // Add to ordering list.
        self.order.lock().unwrap().push(id);
        self.emit(EngineEvent::StateChanged { task_id: id, state });

        // Only start transferring immediately when nothing is scheduled.
        if state == TaskState::Downloading {
            if let Err(e) = self.begin_download(id) {
                self.fail(id, e.clone());
                return Err(e);
            }
        }

        Ok(id)
    }

    /// Start (or restart) the actual transfer for a task.
    /// Called by `add` (immediate start), the scheduler (Queued → start) and
    /// `resume` (Paused → continue from saved offsets).
    fn begin_download(self: &Arc<Self>, id: u64) -> Result<(), String> {
        let (url, dest, total, n_segments) = {
            let tasks = self.tasks.lock().unwrap();
            let Some(t) = tasks.get(&id) else {
                return Err("no such task".into());
            };
            (
                t.url.clone(),
                t.destination.clone(),
                t.total_bytes,
                t.segments as u64,
            )
        };

        // Effective HTTP client: a per-download proxy overrides the global one.
        let client = {
            let task_proxy = self
                .tasks
                .lock()
                .unwrap()
                .get(&id)
                .and_then(|t| t.proxy.clone())
                .filter(|p| !p.trim().is_empty());
            match task_proxy {
                Some(p) => build_client_with_proxy(&p)?,
                None => self.client.read().unwrap().clone(),
            }
        };
        // Per-download headers / basic auth (stored on the task at add time).
        let opts = self
            .tasks
            .lock()
            .unwrap()
            .get(&id)
            .map(|t| RequestOptions {
                headers: t.headers.clone(),
                cookies: None,
                basic_auth: t.basic_auth.clone(),
            })
            .unwrap_or_default();

        // Preallocate the file so segment workers can write at offsets.
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&dest)
            .map_err(|e| format!("cannot create file: {e}"))?;
        if let Some(t) = total {
            file.set_len(t).map_err(|e| e.to_string())?;
        }
        drop(file);

        // Resume from saved offsets when this task was paused before;
        // otherwise split the byte range fresh.
        let saved = self.paused.lock().unwrap().remove(&id);
        let seg_states: Vec<Arc<Mutex<SegmentState>>> = match saved {
            Some(segs) => segs,
            None if total.is_some() => split_ranges(total.unwrap_or(0), n_segments)
                .iter()
                .map(|(s, e)| {
                    Arc::new(Mutex::new(SegmentState {
                        start: *s,
                        end: *e,
                        written: 0,
                    }))
                })
                .collect(),
            None => {
                // Unknown size: single plain (non-Range) connection.
                // `end = u64::MAX` keeps the progress loop from declaring
                // completion early — the worker itself calls finish() when
                // the response stream ends.
                vec![Arc::new(Mutex::new(SegmentState {
                    start: 0,
                    end: u64::MAX,
                    written: 0,
                }))]
            }
        };

        self.spawn_segments(id, &url, dest, seg_states, client, opts);
        Ok(())
    }

    fn spawn_segments(
        self: &Arc<Self>,
        id: u64,
        url: &str,
        destination: PathBuf,
        seg_states: Vec<Arc<Mutex<SegmentState>>>,
        client: SharedClient,
        opts: RequestOptions,
    ) {
        let mut handles = Vec::new();
        let engine = Arc::new(self.clone());

        for seg in seg_states.clone() {
            let client = client.clone();
            let opts = opts.clone();
            let url = url.to_string();
            let dest = destination.clone();
            let (s, e) = {
                let g = seg.lock().unwrap();
                (g.start + g.written, g.end)
            };
            if s > e {
                continue; // segment already complete
            }
            let engine_ev = engine.clone();
            let seg = seg.clone();

            // Effective speed limit: a per-task limit overrides the global one.
            let task_limit = self
                .tasks
                .lock()
                .unwrap()
                .get(&id)
                .map(|t| t.speed_limit)
                .unwrap_or(0);
            let task_limit = if task_limit > 0 {
                task_limit
            } else {
                self.global_limit.load(Ordering::Relaxed)
            };
            let unknown_size = {
                let g = seg.lock().unwrap();
                g.end == u64::MAX
            };

            handles.push(tokio::spawn(async move {
                // Each segment opens its own file handle (ranged writes).
                let mut file = match std::fs::OpenOptions::new().write(true).open(&dest) {
                    Ok(f) => f,
                    Err(e) => {
                        engine_ev.fail(id, e.to_string());
                        return;
                    }
                };

                let client2 = client.clone();
                let url2 = url.clone();
                let mut on_chunk_total: u64 = 0;
                let seg2 = seg.clone();
                let mut on_chunk = |n: u64| {
                    seg2.lock().unwrap().written += n;
                    on_chunk_total += n;
                };

                // Unknown-size tasks stream the whole body over a single
                // plain (non-Range) connection and finish the task themselves.
                let result = if unknown_size {
                    segment::download_plain(&client2, &url2, &mut file, &mut on_chunk, &opts).await
                } else if task_limit > 0 {
                    // If there's a speed limit, wrap the download with throttling.
                    segment::download_segment_throttled(
                        &client2,
                        &url2,
                        s,
                        e,
                        &mut file,
                        &mut on_chunk,
                        task_limit,
                        &opts,
                    )
                    .await
                } else {
                    segment::download_segment(
                        &client2,
                        &url2,
                        s,
                        e,
                        &mut file,
                        &mut on_chunk,
                        &opts,
                    )
                    .await
                };

                match result {
                    Ok(n) => {
                        drop(file);
                        let _ = n;
                        if unknown_size {
                            engine_ev.finish(id);
                        }
                    }
                    Err(err) => {
                        drop(file);
                        engine_ev.fail(id, err);
                    }
                }
            }));
        }

        self.running.lock().unwrap().insert(
            id,
            Running {
                handles,
                segments: seg_states.clone(),
            },
        );
        // Spawn a progress reporter.
        self.spawn_progress_loop(id);
    }

    fn spawn_progress_loop(self: &Arc<Self>, id: u64) {
        let engine = self.clone();
        tokio::spawn(async move {
            let mut last_bytes = 0u64;
            let mut idle = 0u32;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;

                let (downloaded, running_flag) = {
                    let tasks = engine.tasks.lock().unwrap();
                    let Some(t) = tasks.get(&id) else { return };
                    (
                        t.downloaded_bytes,
                        engine.running.lock().unwrap().contains_key(&id),
                    )
                };
                if !running_flag {
                    return;
                }

                // Sum segment progress.
                let sum = {
                    let run = engine.running.lock().unwrap();
                    let Some(r) = run.get(&id) else { return };
                    r.segments
                        .iter()
                        .map(|s| s.lock().unwrap().written)
                        .sum::<u64>()
                };

                let speed = sum.saturating_sub(last_bytes) * 4; // per second
                last_bytes = sum;

                {
                    let mut tasks = engine.tasks.lock().unwrap();
                    if let Some(t) = tasks.get_mut(&id) {
                        t.downloaded_bytes = sum;
                        t.last_speed = speed;
                    }
                }

                let total = engine
                    .tasks
                    .lock()
                    .unwrap()
                    .get(&id)
                    .and_then(|t| t.total_bytes);
                engine.emit(EngineEvent::Progress(ProgressEvent {
                    task_id: id,
                    downloaded_bytes: sum,
                    total_bytes: total,
                    speed_bps: speed,
                }));

                // Done when all segments filled their ranges.
                let complete = {
                    let run = engine.running.lock().unwrap();
                    match run.get(&id) {
                        Some(r) => r.segments.iter().all(|s| {
                            let g = s.lock().unwrap();
                            g.start + g.written > g.end
                        }),
                        None => false,
                    }
                };
                if complete {
                    engine.finish(id);
                    return;
                }

                // No progress for a while and no live segments → fail-safe.
                if sum == downloaded && sum == last_bytes {
                    idle += 1;
                    if idle > 240 {
                        engine.fail(id, "stalled".into());
                        return;
                    }
                } else {
                    idle = 0;
                }
            }
        });
    }

    fn finish(self: &Arc<Self>, id: u64) {
        if let Ok(mut run) = self.running.lock() {
            if let Some(r) = run.remove(&id) {
                for h in r.handles {
                    h.abort();
                }
            }
        }
        let outcome_record: Option<DownloadTask> = {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(t) = tasks.get_mut(&id) {
                // Verify integrity when the caller supplied a SHA-256.
                if let Some(expected) = t.expected_sha256.clone() {
                    match persist::file_sha256(&t.destination) {
                        Ok(actual) if actual.eq_ignore_ascii_case(&expected) => {}
                        Ok(actual) => {
                            t.state = TaskState::Failed;
                            t.last_error = Some(format!(
                                "checksum mismatch: expected {expected}, got {actual}"
                            ));
                            let snapshot = t.clone();
                            drop(tasks);
                            self.record_history(&snapshot, "failed");
                            self.persist_state();
                            self.emit(EngineEvent::StateChanged {
                                task_id: id,
                                state: TaskState::Failed,
                            });
                            self.check_all_finished();
                            self.pump_queue();
                            return;
                        }
                        Err(e) => {
                            log::warn!("sha256 check failed for task {id}: {e}");
                        }
                    }
                }
                t.state = TaskState::Done;
                t.downloaded_bytes = t.total_bytes.unwrap_or(t.downloaded_bytes);
                Some(t.clone())
            } else {
                None
            }
        };
        if let Some(snapshot) = outcome_record {
            self.record_history(&snapshot, "done");
        }
        self.persist_state();
        self.emit(EngineEvent::StateChanged {
            task_id: id,
            state: TaskState::Done,
        });

        // Check if ALL tasks are finished (Done or Failed).
        self.check_all_finished();
        self.pump_queue();
    }

    /// If every task is Done or Failed, emit AllFinished so the backend can
    /// trigger post-download actions (shutdown, sleep, etc.).
    fn check_all_finished(&self) {
        let tasks = self.tasks.lock().unwrap();
        if tasks.is_empty() {
            return;
        }
        let all_done = tasks
            .values()
            .all(|t| matches!(t.state, TaskState::Done | TaskState::Failed));
        if all_done {
            self.emit(EngineEvent::AllFinished);
        }
    }

    fn fail(self: &Arc<Self>, id: u64, err: String) {
        // Automatic retry with exponential backoff when attempts remain.
        let (retries_used, max_retries) = {
            let tasks = self.tasks.lock().unwrap();
            match tasks.get(&id) {
                Some(t) => (t.retries_used, DEFAULT_RETRIES),
                None => (DEFAULT_RETRIES, DEFAULT_RETRIES), // no task → plain fail
            }
        };
        if retries_used < max_retries {
            if let Ok(tasks) = self.tasks.try_lock() {
                if let Some(t) = tasks.get(&id) {
                    // Only network-ish failures are retried; checksum
                    // mismatches are permanent.
                    let _ = t;
                    if !err.contains("checksum mismatch") {
                        let attempt = retries_used + 1;
                        let backoff = std::time::Duration::from_secs(2u64.pow(attempt.min(4)));
                        log::info!(
                            "task {id} failed ({err}); retry {attempt}/{max_retries} in {backoff:?}"
                        );
                        let engine = self.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(backoff).await;
                            {
                                let mut tasks = engine.tasks.lock().unwrap();
                                if let Some(t) = tasks.get_mut(&id) {
                                    t.retries_used = attempt;
                                    t.state = TaskState::Queued;
                                }
                            }
                            engine.emit(EngineEvent::StateChanged {
                                task_id: id,
                                state: TaskState::Queued,
                            });
                            // Retry restarts from the saved segment offsets.
                            let _ = engine.resume(id);
                        });
                        return;
                    }
                }
            }
        }
        let snapshot: Option<DownloadTask> = {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(t) = tasks.get_mut(&id) {
                t.state = TaskState::Failed;
                t.last_error = Some(err.clone());
                Some(t.clone())
            } else {
                None
            }
        };
        if let Ok(mut run) = self.running.lock() {
            if let Some(r) = run.remove(&id) {
                for h in r.handles {
                    h.abort();
                }
            }
        }
        if let Some(snapshot) = snapshot {
            self.record_history(&snapshot, "failed");
        }
        self.persist_state();
        self.emit(EngineEvent::StateChanged {
            task_id: id,
            state: TaskState::Failed,
        });
        self.check_all_finished();
        self.pump_queue();
    }

    /// Pause: abort segment workers, keep per-segment offsets for resume.
    pub fn pause(&self, id: u64) {
        let segs = {
            let mut run = self.running.lock().unwrap();
            run.remove(&id).map(|r| {
                for h in r.handles {
                    h.abort();
                }
                r.segments
            })
        };
        if let Some(segs) = segs {
            // Keep the exact per-segment offsets so `resume` continues
            // from `start + written` instead of re-downloading everything.
            self.paused.lock().unwrap().insert(id, segs);
            self.set_state(id, TaskState::Paused);
            self.persist_state();
        }
    }

    /// Resume a Paused task from its saved offsets, or start a Queued task
    /// (used by the scheduler and the concurrency queue).
    pub fn resume(self: &Arc<Self>, id: u64) -> Result<(), String> {
        let resumable = {
            let tasks = self.tasks.lock().unwrap();
            match tasks.get(&id) {
                Some(t) => matches!(t.state, TaskState::Paused | TaskState::Queued),
                None => return Err("no such task".into()),
            }
        };
        if !resumable {
            return Err("task is not paused or queued".into());
        }
        // Concurrency gate: queued (not scheduled) tasks wait for a slot.
        let max = self.max_concurrent.load(Ordering::Relaxed);
        if max > 0 && self.active_count() >= max as usize {
            let tasks = self.tasks.lock().unwrap();
            if let Some(t) = tasks.get(&id) {
                if t.state == TaskState::Queued {
                    return Ok(()); // stays queued; pump_queue will start it
                }
            }
        }
        self.set_state(id, TaskState::Downloading);
        if let Err(e) = self.begin_download(id) {
            self.fail(id, e.clone());
            return Err(e);
        }
        self.persist_state();
        Ok(())
    }

    /// Remove task (and abort any running work). File is kept on disk.
    /// The removal is recorded in the history log.
    pub fn remove(self: &Arc<Self>, id: u64) {
        let snapshot = {
            let tasks = self.tasks.lock().unwrap();
            tasks.get(&id).cloned()
        };
        self.pause(id);
        self.paused.lock().unwrap().remove(&id);
        self.tasks.lock().unwrap().remove(&id);
        self.order.lock().unwrap().retain(|&x| x != id);
        if let Some(t) = snapshot {
            let was_active = !matches!(t.state, TaskState::Done | TaskState::Failed);
            if was_active {
                self.record_history(&t, "canceled");
            }
        }
        self.persist_state();
        self.pump_queue();
    }

    /// Load persisted tasks from a previous run into the engine
    /// (call once at startup, before any new tasks are added).
    pub fn restore_persisted(self: &Arc<Self>, config_dir: &Path) {
        let tasks = persist::load_state(config_dir);
        if tasks.is_empty() {
            return;
        }
        let max_id = tasks.iter().map(|t| t.id).max().unwrap_or(0);
        {
            let mut map = self.tasks.lock().unwrap();
            for t in tasks {
                map.insert(t.id, t);
            }
        }
        {
            let mut order = self.order.lock().unwrap();
            for id in self.tasks.lock().unwrap().keys().copied() {
                if !order.contains(&id) {
                    order.push(id);
                }
            }
        }
        self.next_id.store(max_id + 1, Ordering::Relaxed);
        self.persist_state();
        self.pump_queue();
    }

    pub fn list(&self) -> Vec<DownloadTask> {
        let tasks = self.tasks.lock().unwrap();
        let order = self.order.lock().unwrap();
        // Return tasks in priority order.
        let mut result: Vec<DownloadTask> = order
            .iter()
            .filter_map(|id| tasks.get(id).cloned())
            .collect();
        // Add any tasks not in the order list (shouldn't happen, but safety).
        for t in tasks.values() {
            if !order.contains(&t.id) {
                result.push(t.clone());
            }
        }
        result
    }

    pub fn set_global_limit(&self, bytes_per_sec: u64) {
        self.global_limit.store(bytes_per_sec, Ordering::Relaxed);
    }

    /// Set per-task speed limit (bytes/sec, 0 = use global).
    pub fn set_task_limit(&self, id: u64, bytes_per_sec: u64) {
        if let Some(t) = self.tasks.lock().unwrap().get_mut(&id) {
            t.speed_limit = bytes_per_sec;
        }
    }

    /// Move a task up in the queue (swap with the one before it).
    pub fn move_up(&self, id: u64) {
        let mut order = self.order.lock().unwrap();
        if let Some(pos) = order.iter().position(|&x| x == id) {
            if pos > 0 {
                order.swap(pos, pos - 1);
            }
        }
    }

    /// Move a task down in the queue (swap with the one after it).
    pub fn move_down(&self, id: u64) {
        let mut order = self.order.lock().unwrap();
        if let Some(pos) = order.iter().position(|&x| x == id) {
            if pos + 1 < order.len() {
                order.swap(pos, pos + 1);
            }
        }
    }
}

/// What to do when a new download collides with an existing task or file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateAction {
    /// Sensible defaults: resume a paused same-URL task, otherwise rename.
    Auto,
    /// Continue an existing paused/queued task with the same URL.
    Resume,
    /// Discard the old task/file and download from scratch.
    Overwrite,
    /// Save under a new name (`file (1).zip`).
    Rename,
}

impl DuplicateAction {
    /// Parse the user-facing action string (empty/None → Auto).
    pub fn parse(s: Option<&str>) -> Self {
        match s.map(str::trim).filter(|s| !s.is_empty()) {
            Some("resume") => Self::Resume,
            Some("overwrite") => Self::Overwrite,
            Some("rename") => Self::Rename,
            _ => Self::Auto,
        }
    }
}

/// Find a free path next to `dest` by inserting " (n)" before the extension.
pub fn alternate_destination(dest: &Path) -> PathBuf {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".into());
    let ext = dest
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 1..1000u32 {
        let candidate = parent.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    // Extremely long queue of same-named files: fall back to a timestamp.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    parent.join(format!("{stem} ({ts}){ext}"))
}

/// Split `0..=total-1` into `n` near-equal inclusive ranges.
pub fn split_ranges(total: u64, n: u64) -> Vec<(u64, u64)> {
    if total == 0 {
        return vec![(0, 0)];
    }
    let n = n.clamp(1, 32);
    let chunk = total.div_ceil(n);
    let mut out = Vec::new();
    let mut s = 0;
    while s < total {
        let e = (s + chunk).saturating_sub(1).min(total - 1);
        out.push((s, e));
        s = e + 1;
    }
    out
}

/// Best-effort filename from a URL path (query string stripped).
pub fn filename_from_url(url: &str) -> String {
    url.split('?')
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .unwrap_or("download.bin")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_ranges_even() {
        assert_eq!(
            split_ranges(100, 4),
            vec![(0, 24), (25, 49), (50, 74), (75, 99)]
        );
    }

    #[tokio::test]
    async fn add_rejects_ftp_with_clear_error() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let engine = Engine::new(tx);
        let err = engine
            .add(
                "ftp://example.com/file.zip".into(),
                PathBuf::from("f"),
                1,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(err.contains("FTP"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn add_rejects_non_http_schemes() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let engine = Engine::new(tx);
        let err = engine
            .add(
                "file:///C:/Windows/calc.exe".into(),
                PathBuf::from("f"),
                1,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(err.contains("Unsupported URL scheme"), "got: {err}");
    }

    #[test]
    fn client_with_proxy_builds_or_rejects() {
        assert!(build_client_with_proxy("").is_ok());
        assert!(build_client_with_proxy("http://127.0.0.1:8080").is_ok());
        assert!(build_client_with_proxy("socks5://127.0.0.1:1080").is_ok());
        assert!(build_client_with_proxy(":::not-a-url:::").is_err());
    }

    #[test]
    fn global_proxy_swap_validates_url() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let engine = Engine::new(tx);
        assert!(engine.set_global_proxy("http://127.0.0.1:8080").is_ok());
        assert!(engine.set_global_proxy(":::bad:::").is_err());
        // Clearing the proxy works.
        assert!(engine.set_global_proxy("").is_ok());
    }

    #[test]
    fn duplicate_action_parses_strings() {
        assert_eq!(DuplicateAction::parse(None), DuplicateAction::Auto);
        assert_eq!(DuplicateAction::parse(Some("")), DuplicateAction::Auto);
        assert_eq!(
            DuplicateAction::parse(Some("resume")),
            DuplicateAction::Resume
        );
        assert_eq!(
            DuplicateAction::parse(Some(" overwrite ")),
            DuplicateAction::Overwrite
        );
        assert_eq!(
            DuplicateAction::parse(Some("rename")),
            DuplicateAction::Rename
        );
        assert_eq!(DuplicateAction::parse(Some("junk")), DuplicateAction::Auto);
    }

    #[test]
    fn alternate_destination_picks_free_name() {
        let dir = std::env::temp_dir().join(format!("idin_dup_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let dest = dir.join("file.zip");
        assert_eq!(alternate_destination(&dest), dir.join("file (1).zip"));

        std::fs::write(&dest, b"x").unwrap();
        let a1 = alternate_destination(&dest);
        assert_eq!(a1, dir.join("file (1).zip"));
        std::fs::write(&a1, b"x").unwrap();
        assert_eq!(alternate_destination(&dest), dir.join("file (2).zip"));

        // Extension-less files also work.
        let bare = dir.join("README");
        std::fs::write(&bare, b"x").unwrap();
        assert_eq!(alternate_destination(&bare), dir.join("README (1)"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn add_auto_renames_when_file_exists() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let payload: Vec<u8> = (0..=255u8).cycle().take(16 * 1024).collect();

        let server_payload = payload.clone();
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                let p = server_payload.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut sock = sock;
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let resp = if req.starts_with("HEAD") {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                            p.len()
                        )
                    } else if let Some(r) = extract_range(&req) {
                        let body = &p[r.0 as usize..=r.1 as usize];
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                            body.len(), r.0, r.1, p.len()
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            p.len()
                        )
                    };
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let body: &[u8] = if req.starts_with("HEAD") {
                        &[]
                    } else if let Some(r) = extract_range(&req) {
                        &p[r.0 as usize..=r.1 as usize]
                    } else {
                        &p
                    };
                    let _ = sock.write_all(body).await;
                });
            }
        });

        let (tx, _rx) = mpsc::unbounded_channel();
        let engine = Engine::new(tx);
        let dir = std::env::temp_dir().join(format!("idin_dup_add_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        let url = format!("http://{addr}/payload.bin");
        let id1 = engine
            .add(url.clone(), dir.clone(), 1, None, None, None, None)
            .await
            .unwrap();
        let dest1 = engine
            .tasks
            .lock()
            .unwrap()
            .get(&id1)
            .unwrap()
            .destination
            .clone();

        // Second add with the same URL and no explicit action → auto-rename.
        let id2 = engine
            .add(url, dir.clone(), 1, None, None, None, None)
            .await
            .unwrap();
        let dest2 = engine
            .tasks
            .lock()
            .unwrap()
            .get(&id2)
            .unwrap()
            .destination
            .clone();
        assert_ne!(id1, id2);
        assert_ne!(dest1, dest2);
        assert!(dest2.to_string_lossy().contains("payload (1).bin"));

        // Overwrite collapses onto the same path.
        let id3 = engine
            .add(
                format!("http://{addr}/payload.bin"),
                dir.clone(),
                1,
                None,
                None,
                None,
                Some("overwrite".into()),
            )
            .await
            .unwrap();
        let dest3 = engine
            .tasks
            .lock()
            .unwrap()
            .get(&id3)
            .unwrap()
            .destination
            .clone();
        assert_eq!(dest3, dest1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_ranges_uneven() {
        let r = split_ranges(10, 4);
        assert_eq!(r.len(), 4);
        assert_eq!(r[0].0, 0);
        assert_eq!(r.last().unwrap().1, 9);
        for w in r.windows(2) {
            assert_eq!(w[0].1 + 1, w[1].0);
        }
    }

    #[test]
    fn split_ranges_more_chunks_than_bytes() {
        assert_eq!(split_ranges(2, 8).len(), 2);
    }

    /// Extract a `bytes=a-b` Range header value from a raw HTTP request.
    fn extract_range(req: &str) -> Option<(u64, u64)> {
        let line = req
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("range:"))?;
        let spec = line.split_once(':')?.1.trim().strip_prefix("bytes=")?;
        let (a, b) = spec.split_once('-')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    }

    #[test]
    fn filename_from_url_strips_query() {
        assert_eq!(
            filename_from_url("https://x.y/a/b/file.zip?tok=1"),
            "file.zip"
        );
    }
}
