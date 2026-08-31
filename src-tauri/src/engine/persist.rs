//! Persistent state for the engine: download-state snapshots and history log.
//!
//! Both files live in the app config dir:
//! - `downloads.json` — in-flight/paused/queued tasks, so pause/resume and the
//!   queue survive app restarts and crashes.
//! - `history.json` — append-style log of finished (done/failed/canceled) tasks.

use crate::engine::task::{DownloadTask, TaskState};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic per-process sequence for history entries. `id` alone is the task
/// id, which can repeat across runs (or when a file is re-downloaded) — the
/// frontend needs a stable unique key and a stable sort order.
static HISTORY_SEQ: AtomicU64 = AtomicU64::new(1);

/// One entry in the persistent history log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u64,
    /// Unique, monotonically increasing record id (survives task-id reuse).
    #[serde(default)]
    pub seq: u64,
    pub url: String,
    pub filename: String,
    /// "done" | "failed" | "canceled"
    pub outcome: String,
    pub total_bytes: Option<u64>,
    /// Bytes actually downloaded before the task ended.
    #[serde(default)]
    pub downloaded_bytes: u64,
    /// How long the download ran, in seconds (0 when unknown).
    #[serde(default)]
    pub duration_secs: u64,
    /// Unix timestamp (seconds) when the entry was recorded.
    pub finished_at: u64,
    pub last_error: Option<String>,
}

fn history_path(config_dir: &Path) -> PathBuf {
    config_dir.join("history.json")
}

fn state_path(config_dir: &Path) -> PathBuf {
    config_dir.join("downloads.json")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ───────────────────────── Download state snapshots ─────────────────────────

/// Save the current task list (any state except Done/Failed — those go to
/// history). Atomic-ish: writes to a temp file then renames.
pub fn save_state(config_dir: &Path, tasks: &[DownloadTask]) -> Result<(), String> {
    let live: Vec<DownloadTask> = tasks
        .iter()
        .filter(|t| !matches!(t.state, TaskState::Done | TaskState::Failed))
        .cloned()
        .collect();
    let path = state_path(config_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&live).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Load tasks saved by a previous run. States are normalized: things that were
/// mid-download become Paused so the user can resume them.
pub fn load_state(config_dir: &Path) -> Vec<DownloadTask> {
    let Ok(s) = std::fs::read_to_string(state_path(config_dir)) else {
        return Vec::new();
    };
    let Ok(mut tasks) = serde_json::from_str::<Vec<DownloadTask>>(&s) else {
        return Vec::new();
    };
    for t in &mut tasks {
        if matches!(t.state, TaskState::Downloading | TaskState::Probing) {
            t.state = TaskState::Paused;
        }
        t.last_speed = 0;
    }
    tasks
}

// ───────────────────────────── History log ─────────────────────────────

/// Serializes history read-modify-write cycles. `record_history` runs on
/// throwaway threads (and `clear_history` on the IPC thread), so two entries
/// finishing at once would otherwise lose one of the records.
static HISTORY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Append an entry to the history log (cap: last 1000 entries).
/// New entries get a fresh monotonic `seq`; legacy entries without one keep
/// `seq = 0` and stay in file order (serde `default`), so old files are never
/// re-shuffled.
pub fn append_history(config_dir: &Path, entry: HistoryEntry) -> Result<(), String> {
    let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut log = load_history(config_dir);
    let mut entry = entry;
    entry.seq = HISTORY_SEQ.fetch_add(1, Ordering::Relaxed);
    log.push(entry);
    if log.len() > 1000 {
        log = log.split_off(log.len() - 1000);
    }
    save_history(config_dir, &log)
}

/// Load the full history log (empty when no file exists yet).
pub fn load_history(config_dir: &Path) -> Vec<HistoryEntry> {
    std::fs::read_to_string(history_path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Overwrite the history log (used by clear-history).
pub fn save_history(config_dir: &Path, entries: &[HistoryEntry]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    std::fs::write(history_path(config_dir), json).map_err(|e| e.to_string())
}

/// Convenience: record a finished task into history.
pub fn record_task_outcome(config_dir: &Path, task: &DownloadTask, outcome: &str) {
    let filename = task
        .destination
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let _ = append_history(
        config_dir,
        HistoryEntry {
            id: task.id,
            seq: 0, // assigned by append_history
            url: task.url.clone(),
            filename,
            outcome: outcome.to_string(),
            total_bytes: task.total_bytes,
            downloaded_bytes: task.downloaded_bytes,
            duration_secs: task.elapsed_secs(),
            finished_at: now(),
            last_error: task.last_error.clone(),
        },
    );
}

// ────────────────────────────── Integrity ──────────────────────────────

/// SHA-256 of a file, hex-encoded lowercase. Uses the `sha2` crate.
pub fn file_sha256(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip_and_normalize() {
        let dir = std::env::temp_dir().join(format!("idin-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut t = DownloadTask::new(1, "http://x/f.bin", dir.join("f.bin"));
        t.state = TaskState::Downloading;
        save_state(&dir, &[t.clone()]).unwrap();

        let loaded = load_state(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].state, TaskState::Paused); // normalized
        assert_eq!(loaded[0].url, "http://x/f.bin");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_roundtrip_and_cap() {
        let dir = std::env::temp_dir().join(format!("idin-hist-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        for i in 0..5 {
            append_history(
                &dir,
                HistoryEntry {
                    id: i,
                    seq: 0, // assigned by append_history
                    url: "http://x".into(),
                    filename: format!("f{i}.bin"),
                    outcome: "done".into(),
                    total_bytes: Some(1),
                    downloaded_bytes: 1,
                    duration_secs: 2,
                    finished_at: now(),
                    last_error: None,
                },
            )
            .unwrap();
        }
        let log = load_history(&dir);
        assert_eq!(log.len(), 5);
        assert_eq!(log[4].filename, "f4.bin");
        // seq is assigned per append and strictly increasing in this process
        assert!(log.windows(2).all(|w| w[0].seq < w[1].seq));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_history_appends_do_not_lose_entries() {
        let dir = std::env::temp_dir().join(format!("idin-hist-race-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut handles = Vec::new();
        for i in 0..8 {
            let d = dir.clone();
            handles.push(std::thread::spawn(move || {
                append_history(
                    &d,
                    HistoryEntry {
                        id: i,
                        seq: 0,
                        url: "http://x".into(),
                        filename: format!("r{i}.bin"),
                        outcome: "done".into(),
                        total_bytes: None,
                        downloaded_bytes: 0,
                        duration_secs: 0,
                        finished_at: now(),
                        last_error: None,
                    },
                )
                .unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(load_history(&dir).len(), 8, "no record may be lost");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_history_without_new_fields_still_loads() {
        let dir = std::env::temp_dir().join(format!("idin-hist-legacy-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // A v1.0.x-era record: no seq / downloaded_bytes / duration_secs.
        std::fs::write(
            history_path(&dir),
            r#"[{
                "id": 7,
                "url": "http://x/old.bin",
                "filename": "old.bin",
                "outcome": "done",
                "total_bytes": 10,
                "finished_at": 1700000000,
                "last_error": null
            }]"#,
        )
        .unwrap();
        let log = load_history(&dir);
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].seq, 0);
        assert_eq!(log[0].downloaded_bytes, 0);
        assert_eq!(log[0].duration_secs, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha256_known_vector() {
        let dir = std::env::temp_dir().join(format!("idin-sha-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("abc.txt");
        std::fs::write(&p, b"abc").unwrap();
        let h = file_sha256(&p).unwrap();
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
