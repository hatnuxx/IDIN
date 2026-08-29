//! Download task model and state machine.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Lifecycle states of a download task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// Waiting in queue.
    Queued,
    /// Asking the server for size / range support / filename.
    Probing,
    /// Actively downloading (one or more segments).
    Downloading,
    /// Paused by the user; segments keep their resume offsets.
    Paused,
    /// Completed and verified.
    Done,
    /// Failed after retries; `last_error` holds the reason.
    Failed,
}

/// A single download task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: u64,
    /// Origin URL (mirrors are additional, later milestone).
    pub url: String,
    /// Final destination path on disk.
    pub destination: PathBuf,
    /// Total size in bytes, if the server reported it.
    pub total_bytes: Option<u64>,
    /// Bytes written so far (sum over segments).
    pub downloaded_bytes: u64,
    pub state: TaskState,
    /// Number of parallel segments actually in use.
    pub segments: u32,
    /// Human-readable failure reason, if any.
    pub last_error: Option<String>,
    /// Queue priority (lower = higher priority, default 0).
    #[serde(default)]
    pub priority: u32,
    /// Per-task speed limit in bytes/sec (0 = use global limit).
    #[serde(default)]
    pub speed_limit: u64,
    /// Last observed speed in bytes/sec (updated by the progress loop).
    #[serde(default)]
    pub last_speed: u64,
    /// The category folder this file was sorted into (e.g. "Videos").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

impl DownloadTask {
    pub fn new(id: u64, url: impl Into<String>, destination: PathBuf) -> Self {
        Self {
            id,
            url: url.into(),
            destination,
            total_bytes: None,
            downloaded_bytes: 0,
            state: TaskState::Queued,
            segments: 1,
            last_error: None,
            priority: 0,
            speed_limit: 0,
            last_speed: 0,
            category: None,
        }
    }

    /// Progress as a fraction in `0.0..=1.0` (None when size unknown).
    pub fn progress(&self) -> Option<f64> {
        self.total_bytes.map(|t| {
            if t == 0 {
                1.0
            } else {
                self.downloaded_bytes as f64 / t as f64
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_full_when_total_zero() {
        let mut t = DownloadTask::new(1, "http://x/f", PathBuf::from("f"));
        t.total_bytes = Some(0);
        assert_eq!(t.progress(), Some(1.0));
    }

    #[test]
    fn progress_fraction() {
        let mut t = DownloadTask::new(1, "http://x/f", PathBuf::from("f"));
        t.total_bytes = Some(200);
        t.downloaded_bytes = 50;
        assert_eq!(t.progress(), Some(0.25));
    }

    #[test]
    fn progress_none_when_unknown() {
        let t = DownloadTask::new(1, "http://x/f", PathBuf::from("f"));
        assert_eq!(t.progress(), None);
    }

    #[test]
    fn serializes_state_snake_case() {
        let mut t = DownloadTask::new(7, "http://x/f", PathBuf::from("f"));
        t.state = TaskState::Downloading;
        let s = serde_json::to_string(&t).unwrap();
        assert!(s.contains("\"downloading\""));
    }

    #[test]
    fn default_priority_is_zero() {
        let t = DownloadTask::new(1, "http://x/f", PathBuf::from("f"));
        assert_eq!(t.priority, 0);
        assert_eq!(t.speed_limit, 0);
        assert_eq!(t.category, None);
    }
}
