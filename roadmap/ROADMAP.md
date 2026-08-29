# IDIN v1 — Roadmap

Status legend: ⏳ not started · 🔄 in progress · ✅ done

## Phase 0 — Setup
- ✅ Duplicate project into `idin-pro` working copy (original untouched)
- ✅ Fresh Git repo with clean history (baseline commit)

## Phase 1 — Cleanup
- ✅ Remove `src-host/target/` build artifacts from the copy (regenerable; already gitignored)
- ✅ Verify no other temp/test files in tree
- Removed files: `src-host/target/**` (Rust build cache for native-messaging host)

## Phase 2 — Feature audit results (existing in v0.3)
- ✅ Multi-threaded segmented downloading (`engine/segment.rs`, Range/206)
- ✅ Pause/Resume (in-memory per-segment offsets)
- ✅ Global + per-download speed limiter (token bucket)
- ✅ Redirect following, filename probing, unknown-size fallback (plain stream)
- ✅ Queue reorder (move up/down), priority field
- ✅ Scheduler (unix timestamp start) + post-download shutdown/sleep/hibernate
- ✅ Category auto-sorting by extension (6 categories, configurable)
- ✅ Clipboard monitoring + auto URL detection + batch URL import
- ✅ System tray + close-to-tray
- ✅ Dark/light theme, bilingual (fa/en) RTL UI
- ✅ Browser extension + native messaging host + one-click setup

## Phase 3 — IDM feature upgrades (this release)
- ✅ 3.1 Persist download state to disk (`downloads.json`) — resume after restart/crash
- ✅ 3.2 Download history log (`history.json`) with timestamps (done/failed/canceled)
- ✅ 3.3 Automatic retry on failure with configurable count + backoff
- ✅ 3.4 Concurrent download limit (configurable max simultaneous)
- ✅ 3.5 Custom HTTP headers, cookies, basic auth per download
- ✅ 3.6 Proxy support (HTTP/SOCKS, global setting)
- ✅ 3.7 File integrity verification (SHA-256 when provided)
- ✅ 3.8 Duplicate download handling (same URL/file → resume/overwrite/rename)
- ✅ 3.9 Dynamic segment re-allocation (fast segments steal remaining bytes)
- ✅ 3.10 Desktop notifications (complete/failed)
- ✅ 3.11 UI: search/filter list, per-download details panel, speed graph, history view
- ✅ 3.12 FTP: out of scope for reqwest (HTTP-only) — clear user-facing error for ftp:// URLs
- ✅ 3.13 Jalali (Persian) calendar support in scheduler display & input
- ⏳ 3.14 Documentation: `PROJECT_DOCUMENTATION.html` (Farsi, beginner-friendly)
- ⏳ 3.15 Final verification: `cargo test` + `npm run build`

## Notes / decisions
- Jalali conversion implemented as a pure-Rust module (`engine/jalali.rs`) with the
  well-known jalaali algorithms + unit tests — avoids adding an unvetted external
  crate and keeps the engine crate-dependency-free (engine must stay Tauri-free).
- FTP: `reqwest` does not speak FTP; adding an FTP client crate for one protocol is
  disproportionate. Non-HTTP URLs now fail fast with a clear message.
