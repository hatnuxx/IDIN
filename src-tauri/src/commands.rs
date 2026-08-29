//! Tauri IPC commands — thin layer over the engine + config.

use crate::config::{AppConfig, Category, ConfigState};
use crate::engine::probe::RequestOptions;
use crate::engine::{task::DownloadTask, Engine, EngineEvent};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::async_runtime;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;

pub struct EngineState(pub Arc<Engine>);

// ───────────────────────────── Tray commands ─────────────────────────────

/// Toggle close-to-tray behavior.
#[tauri::command]
pub fn set_close_to_tray(config: State<'_, ConfigState>, enabled: bool) -> Result<(), String> {
    let config_dir = dirs_config_dir();
    let new_cfg = {
        let mut cfg = config.0.write().map_err(|e| e.to_string())?;
        cfg.close_to_tray = enabled;
        cfg.clone()
    };
    crate::config::save(&config_dir, &new_cfg)
}

/// Add a tray toggle + window controls.
#[tauri::command]
pub fn show_window_cmd(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[tauri::command]
pub fn quit_app(_app: AppHandle) {
    std::process::exit(0);
}

/// Aggregate download stats for the tray tooltip / UI footer.
#[derive(serde::Serialize)]
pub struct DownloadStats {
    pub active: usize,
    pub active_speed: u64,
    pub total: usize,
    pub completed: usize,
}

#[tauri::command]
pub fn get_downloads_stats(state: State<'_, EngineState>) -> DownloadStats {
    let tasks = state.0.list();
    DownloadStats {
        active: tasks
            .iter()
            .filter(|t| t.state == crate::engine::task::TaskState::Downloading)
            .count(),
        active_speed: tasks
            .iter()
            .filter(|t| t.state == crate::engine::task::TaskState::Downloading)
            .map(|t| t.last_speed)
            .sum(),
        total: tasks.len(),
        completed: tasks
            .iter()
            .filter(|t| t.state == crate::engine::task::TaskState::Done)
            .count(),
    }
}

// ───────────────────────────── Download commands ─────────────────────────────

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn add_download(
    state: State<'_, EngineState>,
    config: State<'_, ConfigState>,
    url: String,
    destination: String,
    segments: Option<u32>,
    headers: Option<HashMap<String, String>>,
    cookies: Option<String>,
    username: Option<String>,
    password: Option<String>,
    proxy: Option<String>,
) -> Result<u64, String> {
    let dest = if destination.is_empty() {
        config_download_dir(&config)
    } else {
        PathBuf::from(destination)
    };
    let opts = build_request_options(headers, cookies, username, password);
    state
        .0
        .add(
            url,
            dest,
            segments.unwrap_or(8),
            Some(config.0.clone()),
            Some(opts),
            proxy,
        )
        .await
}

/// Assemble per-download HTTP options from optional IPC parameters.
fn build_request_options(
    headers: Option<HashMap<String, String>>,
    cookies: Option<String>,
    username: Option<String>,
    password: Option<String>,
) -> RequestOptions {
    let mut opts = RequestOptions::default();
    if let Some(h) = headers {
        opts.headers = h
            .into_iter()
            .filter(|(k, _)| !k.trim().is_empty())
            .collect();
    }
    if let Some(c) = cookies {
        if !c.trim().is_empty() {
            opts.cookies = Some(c);
        }
    }
    if let Some(u) = username {
        if !u.trim().is_empty() {
            opts.basic_auth = Some((u, password.unwrap_or_default()));
        }
    }
    opts
}

#[tauri::command]
pub async fn add_downloads(
    state: State<'_, EngineState>,
    config: State<'_, ConfigState>,
    urls: Vec<String>,
    destination: String,
    segments: Option<u32>,
) -> Result<Vec<u64>, String> {
    let dest = if destination.is_empty() {
        config_download_dir(&config)
    } else {
        PathBuf::from(destination)
    };
    let mut ids = Vec::new();
    for url in urls {
        let url = url.trim().to_string();
        if url.is_empty() || !url.starts_with("http") {
            continue;
        }
        if let Ok(id) = state
            .0
            .add(
                url,
                dest.clone(),
                segments.unwrap_or(8),
                Some(config.0.clone()),
                None,
                None,
            )
            .await
        {
            ids.push(id);
        } // skip failed URLs silently in batch
    }
    Ok(ids)
}

#[tauri::command]
pub fn pause_download(state: State<'_, EngineState>, id: u64) {
    state.0.pause(id);
}

#[tauri::command]
pub async fn resume_download(state: State<'_, EngineState>, id: u64) -> Result<(), String> {
    state.0.resume(id)
}

#[tauri::command]
pub fn remove_download(state: State<'_, EngineState>, id: u64) {
    state.0.remove(id);
}

#[tauri::command]
pub fn list_downloads(state: State<'_, EngineState>) -> Vec<DownloadTask> {
    state.0.list()
}

#[tauri::command]
pub fn set_speed_limit(
    state: State<'_, EngineState>,
    config: State<'_, ConfigState>,
    bytes_per_sec: u64,
) {
    state.0.set_global_limit(bytes_per_sec);
    // Persist so the limit survives restarts.
    let config_dir = dirs_config_dir();
    if let Ok(mut cfg) = config.0.write() {
        cfg.global_speed_limit = bytes_per_sec;
        let snapshot = cfg.clone();
        let _ = crate::config::save(&config_dir, &snapshot);
    }
}

#[tauri::command]
pub fn set_task_speed_limit(state: State<'_, EngineState>, id: u64, bytes_per_sec: u64) {
    state.0.set_task_limit(id, bytes_per_sec);
}

#[tauri::command]
pub fn move_task_up(state: State<'_, EngineState>, id: u64) {
    state.0.move_up(id);
}

#[tauri::command]
pub fn move_task_down(state: State<'_, EngineState>, id: u64) {
    state.0.move_down(id);
}

// ───────────────────────────── Config commands ─────────────────────────────

#[tauri::command]
pub fn get_config(config: State<'_, ConfigState>) -> Result<AppConfig, String> {
    config
        .0
        .read()
        .map(|c| c.clone())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_config(
    config: State<'_, ConfigState>,
    engine: State<'_, EngineState>,
    new_cfg: AppConfig,
) -> Result<(), String> {
    // Keep the engine's live global limit in sync with the saved config.
    engine.0.set_global_limit(new_cfg.global_speed_limit);
    // Keep the engine's live proxy in sync as well (validates the URL).
    engine.0.set_global_proxy(&new_cfg.proxy_url)?;
    // Save to config dir.
    let config_dir = dirs_config_dir();
    {
        let mut cfg = config.0.write().map_err(|e| e.to_string())?;
        *cfg = new_cfg.clone();
    }
    crate::config::save(&config_dir, &new_cfg)
}

#[tauri::command]
pub fn update_categories(
    config: State<'_, ConfigState>,
    categories: Vec<Category>,
) -> Result<(), String> {
    let config_dir = dirs_config_dir();
    let new_cfg = {
        let mut cfg = config.0.write().map_err(|e| e.to_string())?;
        cfg.categories = categories;
        cfg.clone()
    };
    crate::config::save(&config_dir, &new_cfg)
}

#[tauri::command]
pub fn set_download_dir(config: State<'_, ConfigState>, path: String) -> Result<(), String> {
    let config_dir = dirs_config_dir();
    let new_cfg = {
        let mut cfg = config.0.write().map_err(|e| e.to_string())?;
        cfg.download_dir = PathBuf::from(&path);
        std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        cfg.clone()
    };
    crate::config::save(&config_dir, &new_cfg)
}

#[tauri::command]
pub fn set_schedule(config: State<'_, ConfigState>, timestamp: Option<u64>) -> Result<(), String> {
    let config_dir = dirs_config_dir();
    let new_cfg = {
        let mut cfg = config.0.write().map_err(|e| e.to_string())?;
        cfg.scheduled_start = timestamp;
        cfg.clone()
    };
    crate::config::save(&config_dir, &new_cfg)
}

#[tauri::command]
pub fn set_post_action(
    config: State<'_, ConfigState>,
    action: Option<String>,
) -> Result<(), String> {
    let config_dir = dirs_config_dir();
    let new_cfg = {
        let mut cfg = config.0.write().map_err(|e| e.to_string())?;
        cfg.post_download_action = action;
        cfg.clone()
    };
    crate::config::save(&config_dir, &new_cfg)
}

#[tauri::command]
pub fn set_max_concurrent(
    engine: State<'_, EngineState>,
    config: State<'_, ConfigState>,
    max: u64,
) -> Result<(), String> {
    engine.0.set_max_concurrent(max);
    let config_dir = dirs_config_dir();
    let new_cfg = {
        let mut cfg = config.0.write().map_err(|e| e.to_string())?;
        cfg.max_concurrent = max;
        cfg.clone()
    };
    crate::config::save(&config_dir, &new_cfg)
}

/// Set the global proxy (HTTP/SOCKS) and persist it. Empty string = no proxy.
#[tauri::command]
pub fn set_proxy(
    engine: State<'_, EngineState>,
    config: State<'_, ConfigState>,
    proxy_url: String,
) -> Result<(), String> {
    // Validate by rebuilding the client first; only then persist.
    engine.0.set_global_proxy(&proxy_url)?;
    let config_dir = dirs_config_dir();
    let new_cfg = {
        let mut cfg = config.0.write().map_err(|e| e.to_string())?;
        cfg.proxy_url = proxy_url;
        cfg.clone()
    };
    crate::config::save(&config_dir, &new_cfg)
}

// ───────────────────────────── History commands ─────────────────────────────

#[tauri::command]
pub fn get_history() -> Vec<crate::engine::persist::HistoryEntry> {
    crate::engine::persist::load_history(&dirs_config_dir())
}

#[tauri::command]
pub fn clear_history() -> Result<(), String> {
    crate::engine::persist::save_history(&dirs_config_dir(), &[])
}

// ───────────────────────────── Jalali calendar commands ─────────────────────────────

/// Jalali date → Gregorian.
#[tauri::command]
pub fn jalali_to_gregorian_cmd(jy: i32, jm: u32, jd: u32) -> crate::engine::jalali::GregorianDate {
    crate::engine::jalali::jalali_to_gregorian(jy, jm, jd)
}

/// Gregorian date → Jalali.
#[tauri::command]
pub fn gregorian_to_jalali_cmd(gy: i32, gm: u32, gd: u32) -> crate::engine::jalali::JalaliDate {
    crate::engine::jalali::gregorian_to_jalali(gy, gm, gd)
}

// ───────────────────────────── Helpers ─────────────────────────────

fn config_download_dir(config: &State<'_, ConfigState>) -> PathBuf {
    config
        .0
        .read()
        .ok()
        .map(|c| c.download_dir.clone())
        .unwrap_or_else(dirs_download_dir)
}

fn dirs_download_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|h| PathBuf::from(h).join("Downloads"))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn dirs_config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|h| PathBuf::from(h).join("IDIN"))
        .unwrap_or_else(|| PathBuf::from("."))
}

// ───────────────────────────── Engine event forwarder ─────────────────────────────

/// Forward engine events to the webview.
pub fn spawn_event_forwarder(app: AppHandle, mut rx: mpsc::UnboundedReceiver<EngineEvent>) {
    async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            match ev {
                EngineEvent::Progress(p) => {
                    let _ = app.emit("download-progress", &p);
                }
                EngineEvent::StateChanged { task_id, state } => {
                    let _ = app.emit(
                        "download-state",
                        serde_json::json!({ "taskId": task_id, "state": state }),
                    );
                }
                EngineEvent::AllFinished => {
                    let _ = app.emit("all-downloads-finished", ());
                }
            }
        }
    });
}

/// Initialize tokio runtime engine as Tauri state.
pub fn setup_engine(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::unbounded_channel();
    let engine = Engine::new(tx);
    app.manage(EngineState(engine));
    spawn_event_forwarder(app.handle().clone(), rx);
    Ok(())
}
