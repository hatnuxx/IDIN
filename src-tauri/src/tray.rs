//! System tray integration: tray icon, context menu, close-to-tray behavior.
//!
//! Architecture:
//! - `setup_tray()` builds the icon + context menu (Show / Quit).
//! - `install_close_to_tray()` intercepts `WindowEvent::CloseRequested` and hides the window instead of closing.
//! - Quit-from-tray calls `std::process::exit(0)` which skips all window event handlers — no need for a static flag.

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

/// Setup the tray icon with its context menu.
pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    // ── Context menu ──
    let show = MenuItem::with_id(app, "show", "Show IDIN", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit IDIN", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    // ── Tray icon ──
    let _tray = TrayIconBuilder::with_id("idin-tray")
        .tooltip("IDIN — Download Manager")
        .menu(&menu)
        // Use default window icon; unwrap is safe because tauri.conf.json sets one.
        .icon(
            app.default_window_icon()
                .cloned()
                .expect("default icon must be set"),
        )
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => {
                // process::exit bypasses window event handlers — no "is_shutting_down" flag needed.
                std::process::exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// Bring the main window to the front (and un-minimize) when activated from tray.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Intercept window close: if `close_to_tray` is ON, hide the window instead of quitting.
///
/// This is called once during app setup. We clone per-iteration to satisfy borrow rules.
pub fn install_close_to_tray(app: &tauri::App) {
    use tauri::WindowEvent;

    let enabled = app
        .state::<crate::config::ConfigState>()
        .0
        .read()
        .map(|c| c.close_to_tray)
        .unwrap_or(true);

    if !enabled {
        return; // close_to_tray OFF → let window close normally (quits app).
    }

    // We need to intercept CloseRequested on ALL webview windows.
    // Clone handle + window per-iteration to satisfy Rust's move-in-closure rules.
    for window in app.webview_windows().values() {
        let handle = app.handle().clone();
        let win = window.clone();
        let win2 = window.clone();
        win.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = win2.hide();
                // Update tray tooltip to reflect active downloads (optional enhancement).
                if let Some(tray) = handle.tray_by_id("idin-tray") {
                    let _ = tray.set_tooltip(Some("IDIN — running in background"));
                }
            }
        });
    }
}
