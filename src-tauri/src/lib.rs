// Library target for mobile; desktop entry is main.rs.
pub mod browser_setup;
mod clipboard;
pub mod commands;
pub mod config;
pub mod engine;
mod local_api;
mod tray;

use tauri::Manager as _;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            commands::setup_engine(app)?;

            // ── Config dir: ONE canonical location for every read/write ──
            // `%APPDATA%\IDIN` — the same dir commands::dirs_config_dir()
            // writes to. (Old builds loaded from Tauri's app_config_dir(),
            // so saves never came back after restart; fixed here.)
            let config_dir = commands::dirs_config_dir();
            let legacy_dir = app
                .path()
                .app_config_dir()
                .ok()
                .filter(|p| p != &config_dir);
            // Copy config/history/state from the legacy dir (first run after fix).
            commands::migrate_legacy_config(legacy_dir.as_deref(), &config_dir);
            let shared_cfg = config::load_or_create(&config_dir);
            app.manage(config::ConfigState(shared_cfg.clone()));

            // Enable engine persistence + restore tasks from the last run.
            if let Some(state) = app.try_state::<commands::EngineState>() {
                state.0.set_persist_dir(config_dir.clone());
                state.0.restore_persisted(&config_dir);
                if let Ok(c) = shared_cfg.read() {
                    state.0.set_max_concurrent(c.max_concurrent);
                }
            }

            // Sync the global speed limit from config into the engine.
            if let Some(state) = app.try_state::<commands::EngineState>() {
                if let Ok(c) = shared_cfg.read() {
                    state.0.set_global_limit(c.global_speed_limit);
                    // Restore the global proxy (if any) from the last run.
                    let _ = state.0.set_global_proxy(&c.proxy_url);
                }
            }

            // Setup system tray.
            tray::setup_tray(app.handle())?;

            // Close-to-tray behavior: hide on X instead of quit.
            tray::install_close_to_tray(app);

            // Clipboard watcher: surface copied URLs to the UI.
            clipboard::start_clipboard_watcher(app.handle().clone());

            // Local API for the browser extension host.
            let engine = app
                .try_state::<commands::EngineState>()
                .map(|s| s.0.clone());
            let handle = app.handle().clone();
            if let Some(engine) = engine {
                let api_cfg = shared_cfg.clone();
                tauri::async_runtime::spawn(async move {
                    local_api::serve(engine, Some(api_cfg)).await;
                    let _ = handle;
                });
            }

            // ── Scheduler: check every second if scheduled_start has arrived ──
            {
                let cfg = shared_cfg.clone();
                let engine_handle = app
                    .try_state::<commands::EngineState>()
                    .map(|s| s.0.clone());
                if let Some(eng) = engine_handle {
                    tauri::async_runtime::spawn(async move {
                        loop {
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            let should_start = {
                                let c = cfg.read().unwrap();
                                match c.scheduled_start {
                                    Some(ts) => {
                                        let now = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap()
                                            .as_secs();
                                        now >= ts
                                    }
                                    None => false,
                                }
                            };
                            if should_start {
                                // Resume all Queued tasks.
                                let queued: Vec<u64> = eng
                                    .list()
                                    .iter()
                                    .filter(|t| t.state == engine::task::TaskState::Queued)
                                    .map(|t| t.id)
                                    .collect();
                                for id in queued {
                                    let _ = eng.resume(id);
                                }
                                // Clear the schedule so it doesn't fire again.
                                {
                                    let mut c = cfg.write().unwrap();
                                    c.scheduled_start = None;
                                }
                            }
                        }
                    });
                }
            }

            // ── Post-download action: poll every 2s, execute when all done ──
            {
                let cfg = shared_cfg.clone();
                let handle = app.handle().clone();
                // Same canonical config dir as above (persist cleared action).
                let config_dir = config_dir.clone();
                tauri::async_runtime::spawn(async move {
                    let engine = handle
                        .try_state::<commands::EngineState>()
                        .map(|s| s.0.clone());
                    if let Some(ref _eng) = engine {
                        loop {
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            let action = {
                                let c = cfg.read().unwrap();
                                c.post_download_action.clone()
                            };
                            let Some(action) = action else { continue };
                            if action.is_empty() || action == "none" {
                                continue;
                            }
                            let all_done = {
                                if let Some(ref eng) = engine {
                                    let tasks = eng.list();
                                    if tasks.is_empty() {
                                        continue;
                                    }
                                    let any_active = tasks.iter().any(|t| {
                                        matches!(
                                            t.state,
                                            engine::task::TaskState::Downloading
                                                | engine::task::TaskState::Queued
                                                | engine::task::TaskState::Probing
                                        )
                                    });
                                    !any_active
                                } else {
                                    false
                                }
                            };
                            if all_done {
                                execute_post_action(&action);
                                {
                                    let mut c = cfg.write().unwrap();
                                    c.post_download_action = None;
                                }
                                let _ = crate::config::save(&config_dir, &cfg.read().unwrap());
                                break;
                            }
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Download commands
            commands::add_download,
            commands::add_downloads,
            commands::check_duplicate,
            commands::pause_download,
            commands::resume_download,
            commands::remove_download,
            commands::list_downloads,
            commands::set_speed_limit,
            commands::set_task_speed_limit,
            commands::move_task_up,
            commands::move_task_down,
            // Config commands
            commands::get_config,
            commands::set_config,
            commands::update_categories,
            commands::set_download_dir,
            commands::set_close_to_tray,
            commands::get_downloads_stats,
            // Schedule & post-action commands
            commands::set_schedule,
            commands::set_post_action,
            commands::set_max_concurrent,
            commands::set_proxy,
            // History commands
            commands::get_history,
            commands::clear_history,
            // Jalali calendar commands
            commands::jalali_to_gregorian_cmd,
            commands::gregorian_to_jalali_cmd,
            // Tray commands
            commands::show_window_cmd,
            commands::quit_app,
            // Browser commands
            browser_setup::setup_browser_integration,
            browser_setup::stage_extension_folder,
            browser_setup::detect_browsers,
            browser_setup::open_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Execute a post-download action: "shutdown", "sleep", or "hibernate".
/// Uses Windows-specific commands. Cross-platform support planned for v0.3.
fn execute_post_action(action: &str) {
    log::info!("Post-download action triggered: {action}");
    match action {
        "shutdown" => {
            // shutdown /s /t 60 → 60 second warning so user can cancel
            let _ = std::process::Command::new("shutdown")
                .args([
                    "/s",
                    "/t",
                    "60",
                    "/c",
                    "IDIN: All downloads complete. Shutting down...",
                ])
                .spawn();
        }
        "sleep" => {
            // rundll32 powrprof.dll,SetSuspendState 0,1,0 → sleep (not hibernate)
            let _ = std::process::Command::new("rundll32.exe")
                .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                .spawn();
        }
        "hibernate" => {
            // rundll32 powrprof.dll,SetSuspendState 1,1,0 → hibernate
            let _ = std::process::Command::new("rundll32.exe")
                .args(["powrprof.dll,SetSuspendState", "1,1,0"])
                .spawn();
        }
        _ => {
            log::warn!("Unknown post-download action: {action}");
        }
    }
}
