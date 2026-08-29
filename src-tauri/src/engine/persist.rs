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

/// One entry in the persistent history log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u64,
    pub url: String,
    pub filename: String,
    /// "done" | "failed" | "canceled"
    pub outcome: String,
    pub total_bytes: Option<u64>,
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

/// Append an entry to the history log (cap: last 1000 entries).
pub fn append_history(config_dir: &Path, entry: HistoryEntry) -> Result<(), String> {
    let mut log = load_history(config_dir);
    log.push(entry);
    if log.len() > 1000 {
        log = log.split_off(log.len() - 1000);
    }
    let json = serde_json::to_string_pretty(&log).map_err(|e| e.to_string())?;
    std::fs::write(history_path(config_dir), json).map_err(|e| e.to_string())
}

/// Load the full history log (empty when no file exists yet).
pub fn load_history(config_dir: &Path) -> Vec<HistoryEntry> {
    std::fs::read_to_string(history_path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
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
            url: task.url.clone(),
            filename,
            outcome: outcome.to_string(),
            total_bytes: task.total_bytes,
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
                    url: "http://x".into(),
                    filename: format!("f{i}.bin"),
                    outcome: "done".into(),
                    total_bytes: Some(1),
                    finished_at: now(),
                    last_error: None,
                },
            )
            .unwrap();
        }
        let log = load_history(&dir);
        assert_eq!(log.len(), 5);
        assert_eq!(log[4].filename, "f4.bin");
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
