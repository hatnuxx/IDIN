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
use probe::probe;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use task::{DownloadTask, TaskState};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Shared HTTP client for all downloads.
pub type SharedClient = Arc<reqwest::Client>;

/// Build the engine's shared HTTP client (rustls, streaming, redirects).
pub fn build_client() -> SharedClient {
    Arc::new(
        reqwest::Client::builder()
            .user_agent(concat!("IDIN/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .expect("failed to build HTTP client"),
    )
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
    client: SharedClient,
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
}

const MIN_SERVER_SIZE: u64 = 4 * 1024 * 1024; // below this, single connection

impl Engine {
    pub fn new(event_tx: mpsc::UnboundedSender<EngineEvent>) -> Arc<Self> {
        Arc::new(Self {
            client: build_client(),
            next_id: AtomicU64::new(1),
            tasks: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(Mutex::new(HashMap::new())),
            paused: Arc::new(Mutex::new(HashMap::new())),
            global_limit: Arc::new(AtomicU64::new(0)),
            order: Arc::new(Mutex::new(Vec::new())),
            event_tx,
        })
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
    pub async fn add(
        self: &Arc<Self>,
        url: String,
        mut destination: PathBuf,
        segments_requested: u32,
        config: Option<SharedConfig>,
    ) -> Result<u64, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let p = probe(&self.client, &url).await?;

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
        let destination = if destination.is_dir() || destination.extension().is_none() {
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

        self.spawn_segments(id, &url, dest, seg_states);
        Ok(())
    }

    fn spawn_segments(
        self: &Arc<Self>,
        id: u64,
        url: &str,
        destination: PathBuf,
        seg_states: Vec<Arc<Mutex<SegmentState>>>,
    ) {
        let mut handles = Vec::new();
        let engine = Arc::new(self.clone());

        for seg in seg_states.clone() {
            let client = self.client.clone();
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
                    segment::download_plain(&client2, &url2, &mut file, &mut on_chunk).await
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
                    )
                    .await
                } else {
                    segment::download_segment(&client2, &url2, s, e, &mut file, &mut on_chunk).await
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

    fn finish(&self, id: u64) {
        if let Ok(mut run) = self.running.lock() {
            if let Some(r) = run.remove(&id) {
                for h in r.handles {
                    h.abort();
                }
            }
        }
        {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(t) = tasks.get_mut(&id) {
                t.state = TaskState::Done;
                t.downloaded_bytes = t.total_bytes.unwrap_or(t.downloaded_bytes);
            }
        }
        self.emit(EngineEvent::StateChanged {
            task_id: id,
            state: TaskState::Done,
        });

        // Check if ALL tasks are finished (Done or Failed).
        self.check_all_finished();
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

    fn fail(&self, id: u64, err: String) {
        {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(t) = tasks.get_mut(&id) {
                t.state = TaskState::Failed;
                t.last_error = Some(err.clone());
            }
        }
        if let Ok(mut run) = self.running.lock() {
            if let Some(r) = run.remove(&id) {
                for h in r.handles {
                    h.abort();
                }
            }
        }
        self.emit(EngineEvent::StateChanged {
            task_id: id,
            state: TaskState::Failed,
        });
        self.check_all_finished();
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
        }
    }

    /// Resume a Paused task from its saved offsets, or start a Queued task
    /// (used by the scheduler for scheduled downloads).
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
        self.set_state(id, TaskState::Downloading);
        if let Err(e) = self.begin_download(id) {
            self.fail(id, e.clone());
            return Err(e);
        }
        Ok(())
    }

    /// Remove task (and abort any running work). File is kept on disk.
    pub fn remove(&self, id: u64) {
        self.pause(id);
        self.paused.lock().unwrap().remove(&id);
        self.tasks.lock().unwrap().remove(&id);
        self.order.lock().unwrap().retain(|&x| x != id);
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

fn filename_from_url(url: &str) -> String {
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

    #[test]
    fn filename_from_url_strips_query() {
        assert_eq!(
            filename_from_url("https://x.y/a/b/file.zip?tok=1"),
            "file.zip"
        );
    }
}
