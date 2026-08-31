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

/// Robust self-replacing copy for `idin-host.exe`, which the browser keeps
/// running (locked) as a native-messaging host.
///
/// On Windows a running exe's *replacement* is allowed but its *deletion* is
/// not, so the reliable sequence is:
///   1. try a plain copy (first install — nothing is running yet)
///   2. if the destination is locked (os error 32): rename the OLD exe to
///      `idin-host.old.exe` (renaming a running exe is legal), then copy the
///      new one into place
///   3. best-effort delete any stale `.old` from a previous replace — it may
///      still be running; it will succeed on some future run
fn copy_locked_host(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    // Fast path: normal copy (host not running).
    if std::fs::copy(src, dst).is_ok() {
        return Ok(());
    }
    // Destination locked — move the running exe aside and copy fresh.
    let old = dst.with_extension("old.exe");
    let _ = std::fs::remove_file(&old); // stale leftover from a previous swap
    std::fs::rename(dst, &old).map_err(|e| format!("move old host aside: {e}"))?;
    std::fs::copy(src, dst).map_err(|e| format!("copy host: {e}"))?;
    Ok(())
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
    copy_locked_host(&host_exe, &host_dst).map_err(|e| format!("copy host: {e}"))?;

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
        // Installed (NSIS): exe sits in install dir, host next to it / in resources.
        exe_dir.join("idin-host.exe"),
        exe_dir.join("resources").join("idin-host.exe"),
        // Dev: src-tauri/target/<profile>/ → repo root is 3 levels up.
        exe_dir.join("../../../src-host/target/release/idin-host.exe"),
        // Dev fallback: repo-local src-host build tree relative to the repo root.
        exe_dir.join("../../../src-host/target/debug/idin-host.exe"),
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
    // Dev: src-tauri/target/{debug,release}/ → repo root is 3 levels up + extension.
    // Installed (NSIS): the installer hooks copy the extension into
    // %LOCALAPPDATA%\IDIN\extension; the bundled resources dir is another fallback.
    let src_candidates = [
        exe_dir.join("extension"),
        exe_dir.join("../../../extension"),
        exe_dir.join("resources").join("extension"),
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

/// One-click per-browser install: stages the extension folder, installs the
/// native-messaging host, registers it for the given browser, and opens the
/// browser's extension page. `extension_id` may be empty — the manifest keeps
/// the placeholder origin and the dialog can be re-run after the user copies
/// the ID shown by the browser.
#[tauri::command]
pub fn install_for_browser(browser: String, extension_id: String) -> Result<SetupResult, String> {
    let result = setup_browser_integration(extension_id)?;
    let page = match browser.as_str() {
        "edge" => "edge://extensions",
        "firefox" => "about:debugging#/runtime/this-firefox",
        _ => "chrome://extensions",
    };
    open_extension_page(page.to_string());
    Ok(result)
}

/// Open the browser's extension/settings page.
///
/// `chrome://`, `edge://` and `about:` pages are NOT http(s) URLs, so
/// `ShellExecute` on the bare URL is rejected ("file not found"). The reliable
/// route is `<browser.exe> <url>`; registry App Paths locates the exe without
/// guessing install directories. Falls back to the browser's homepage when
/// the exe can't be found.
#[tauri::command]
pub fn open_extension_page(page: String) {
    let allowed = [
        "chrome://extensions",
        "edge://extensions",
        "about:debugging#/runtime/this-firefox",
        "about:debugging",
    ];
    if !allowed.contains(&page.as_str()) {
        log::warn!("open_extension_page rejected: {page}");
        return;
    }
    let (app_paths_key, fallback) = match page.as_str() {
        "edge://extensions" => (
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe",
            "https://microsoft.com/edge",
        ),
        "about:debugging#/runtime/this-firefox" | "about:debugging" => (
            r"SOFTWARE\Mozilla\Mozilla Firefox\Main",
            "https://mozilla.org",
        ),
        _ => (
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe",
            "https://google.com",
        ),
    };
    let exe = Command::new("reg")
        .args(["query", &format!("HKLM\\{app_paths_key}"), "/ve"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .and_then(|stdout| {
            // `reg query` prints "    (Default)    REG_SZ    C:\path\to\exe".
            stdout
                .lines()
                .find(|l| l.contains("REG_SZ"))
                .and_then(|l| l.split("REG_SZ").nth(1))
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty());

    let launched = if let Some(exe) = exe {
        Command::new(exe).arg(&page).spawn().is_ok()
    } else {
        false
    };
    if !launched {
        // Fallback: default browser homepage (http(s) — always launchable).
        let _ = Command::new("cmd")
            .args(["/c", "start", "", fallback])
            .spawn();
    }
}

/// Ensure the browser integration is in place WITHOUT any user interaction:
/// run automatically at app startup. Installs the host binary + manifest and
/// registers Chrome/Edge/Firefox when possible; never blocks startup. The
/// extension ID is unknown until the user loads the unpacked extension, so
/// the manifest keeps a placeholder origin that a later "Auto-install"
/// (with the ID pasted) refines.
pub fn ensure_browser_setup() {
    let result = setup_browser_integration(String::new());
    match result {
        Ok(r) => {
            if r.registered.is_empty() {
                log::warn!("browser integration: no browser registered");
            } else {
                log::info!("browser integration ready for: {}", r.registered.join(", "));
            }
        }
        // Expected in dev without a built host binary — not an error users see.
        Err(e) => log::warn!("browser integration auto-setup skipped: {e}"),
    }
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
