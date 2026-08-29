//! One-click browser-integration setup.
//!
//! Performs everything the user shouldn't have to do manually:
//! 1. Copies the native-messaging host binary to %LOCALAPPDATA%\IDIN
//! 2. Writes the host manifest with the correct extension origin
//! 3. Registers the host for Chrome, Edge, and Firefox (HKCU registry)
//! 4. Opens the browser's extension page so the user only clicks "Add"

use std::process::Command;

pub const HOST_NAME: &str = "com.hatnux.idin";

#[derive(serde::Serialize)]
pub struct SetupResult {
    pub host_installed: bool,
    pub manifest_path: String,
    pub registered: Vec<&'static str>,
    pub host_exe_found: bool,
}

/// Robust copy: retries when the destination is locked by another process
/// (common with idin-host.exe still running as a native messaging host).
fn copy_with_retry(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    // First attempt: direct copy.
    match std::fs::copy(src, dst) {
        Ok(_) => return Ok(()),
        Err(e) if e.raw_os_error() == Some(32) => {} // file in use — try rename strategy
        Err(e) => return Err(format!("copy {}: {}", src.display(), e)),
    }
    // File is locked — write to a temp sibling and atomically replace via rename.
    let tmp = dst.with_extension("tmp");
    std::fs::copy(src, &tmp).map_err(|e| format!("copy to tmp: {e}"))?;
    // std::fs::rename works on Windows when src and dst are on the same volume,
    // even if dst is locked — it replaces the file object in-place.
    match std::fs::rename(&tmp, dst) {
        Ok(_) => Ok(()),
        Err(_) => {
            // rename failed (different volumes or still locked) — fall back to
            // RemoveFile + Rename. Best-effort; will succeed once the host exits.
            let _ = std::fs::remove_file(dst);
            std::fs::rename(&tmp, dst).map_err(|e| format!("replace {}: {e}", dst.display()))?;
            Ok(())
        }
    }
}

/// Run the full setup. `extension_id` is the browser-assigned extension ID
/// (empty = allow any origin via store update_url later).
#[tauri::command]
pub fn setup_browser_integration(extension_id: String) -> Result<SetupResult, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .ok_or("LOCALAPPDATA not set")?
        .join("IDIN");
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;

    // 1. Locate host exe: prefer one next to the running app, else src-host build.
    let host_exe = find_host_exe()
        .ok_or("idin-host.exe not found — build it with: cargo build --release (in src-host/)")?;
    let host_dst = base.join("idin-host.exe");
    copy_with_retry(&host_exe, &host_dst).map_err(|e| format!("copy host: {e}"))?;

    let origin = if extension_id.is_empty() {
        "chrome-extension://EXTENSION_ID/".to_string()
    } else {
        format!("chrome-extension://{extension_id}/")
    };

    // 2. Write host manifest. The `path` MUST point directly at the .exe, not a
    //    .bat wrapper — browsers spawn the native host as a GUI-less child and
    //    hand it a pipe for stdin/stdout. A .bat goes through cmd.exe which can
    //    break stdio piping (the classic "native host not sendable"/error 32).
    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "IDIN Download Manager native messaging host",
        "path": host_dst.display().to_string(),
        "type": "stdio",
        "allowed_origins": [origin]
    });
    let manifest_path = base.join(format!("{HOST_NAME}.json"));
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .map_err(|e| e.to_string())?;

    // 3. Register in HKCU for the three browsers.
    let reg_keys = [
        (r"Software\Google\Chrome\NativeMessagingHosts", "Chrome"),
        (r"Software\Microsoft\Edge\NativeMessagingHosts", "Edge"),
        (r"Software\Mozilla\NativeMessagingHosts", "Firefox"),
    ];
    let mut registered = Vec::new();
    for (key, name) in reg_keys {
        let full = format!("HKCU\\{key}\\{HOST_NAME}");
        let out = Command::new("reg")
            .args(["add", &full, "/ve", "/t", "REG_SZ", "/d"])
            .arg(manifest_path.display().to_string())
            .args(["/f"])
            .output();
        if out.map(|o| o.status.success()).unwrap_or(false) {
            registered.push(name);
        }
    }

    Ok(SetupResult {
        host_installed: true,
        manifest_path: manifest_path.display().to_string(),
        registered,
        host_exe_found: true,
    })
}

fn find_host_exe() -> Option<std::path::PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let candidates = [
        exe_dir.join("idin-host.exe"),
        exe_dir.join("../../../../../src-host/target/release/idin-host.exe"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// Copy the extension folder next to the installed app so users can easily
/// point "Load unpacked" at a stable system path (e.g. %LOCALAPPDATA%\IDIN\extension).
#[tauri::command]
pub fn stage_extension_folder() -> Result<String, String> {
    let exe_dir = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("no parent")?
        .to_path_buf();
    // Dev: src-tauri/target/{debug,release}/... → repo root 4 levels up + extension
    // Installed: install dir has extension copied next to exe by the installer.
    let src_candidates = [
        exe_dir.join("extension"),
        exe_dir.join("../../../../../extension"),
    ];
    let src = src_candidates
        .into_iter()
        .find(|p| p.is_dir())
        .ok_or("extension folder not found next to app")?;

    let dst_base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .ok_or("LOCALAPPDATA not set")?
        .join("IDIN");
    let dst = dst_base.join("extension");
    copy_dir_recursive(&src, &dst)?;
    Ok(dst.display().to_string())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Detect installed browsers (for auto-offer on first run).
#[tauri::command]
pub fn detect_browsers() -> Vec<String> {
    let mut found = Vec::new();
    let checks = [
        (
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe",
            "chrome",
        ),
        (
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe",
            "edge",
        ),
        (r"SOFTWARE\Mozilla\Mozilla Firefox\Main", "firefox"),
    ];
    for (key, name) in checks {
        let status = Command::new("reg")
            .args(["query", &format!("HKLM\\{key}"), "/ve"])
            .output();
        if status.map(|o| o.status.success()).unwrap_or(false) {
            found.push(name.to_string());
        }
    }
    found
}

/// Open the given URL with the OS default browser.
/// Only http(s) links are allowed — never hand arbitrary strings to the shell.
#[tauri::command]
pub fn open_url(url: String) {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        log::warn!("open_url rejected non-http(s) URL");
        return;
    }
    let _ = Command::new("cmd").args(["/c", "start", "", &url]).spawn();
}
