//! Progress events emitted by the engine, batched for the frontend.

use serde::Serialize;

/// One batched progress tick (frontend receives ~10/s per task).
#[derive(Debug, Clone, Serialize)]
pub struct ProgressEvent {
    pub task_id: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    /// Bytes per second over the recent window.
    pub speed_bps: u64,
}
