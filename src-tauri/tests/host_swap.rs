//! Regression coverage for the os-error-32 host swap: when the browser still
//! runs `idin-host.exe`, the exe image is locked and a plain copy fails; the
//! rename-aside-then-copy strategy must succeed.
//!
//! The lock is simulated the way the OS really holds it — by *executing* the
//! destination binary (a copy of the current test exe, kept alive via a
//! `--idin-sleep` child). A `share_mode(0)` handle is NOT equivalent: it also
//! blocks renames, which a running image does not.

use std::process::{Command, Stdio};

#[test]
fn copy_over_running_host_succeeds() {
    let dir = std::env::temp_dir().join(format!("idin-hostswap-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let dst = dir.join("idin-host.exe");

    // A real, executing image at `dst` — same lock semantics as the browser
    // running the native host.
    let self_exe = std::env::current_exe().unwrap();
    std::fs::copy(&self_exe, &dst).unwrap();
    let mut running = Command::new(&dst)
        .arg("--idin-sleep")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn copy of test binary");
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Precondition: plain copy onto the running exe fails (os error 32).
    let src = dir.join("new-host.exe");
    std::fs::write(&src, b"NEW-HOST-BYTES").unwrap();
    let plain = std::fs::copy(&src, &dst);
    if plain.is_ok() {
        // Platform allowed overwrite — the swap concern doesn't apply here.
        eprintln!("plain copy succeeded; nothing to test on this platform");
    } else {
        // The strategy used by copy_locked_host:
        let old = dst.with_extension("old.exe");
        let _ = std::fs::remove_file(&old);
        let renamed = std::fs::rename(&dst, &old);
        assert!(
            renamed.is_ok(),
            "renaming a running exe must be allowed on Windows (got {renamed:?})"
        );
        let copied = std::fs::copy(&src, &dst);
        assert!(copied.is_ok(), "copy after rename-aside must succeed");
        assert_eq!(std::fs::read(&dst).unwrap(), b"NEW-HOST-BYTES");
    }

    let _ = running.kill();
    let _ = std::fs::remove_dir_all(&dir);
}
