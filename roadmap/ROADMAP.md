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
- ✅ 3.14 Documentation: `PROJECT_DOCUMENTATION.html` (Farsi, beginner-friendly)
- ✅ 3.15 Final verification: `cargo test` + `npm run build`

## Notes / decisions
- Jalali conversion implemented as a pure-Rust module (`engine/jalali.rs`) with the
  well-known jalaali algorithms + unit tests — avoids adding an unvetted external
  crate and keeps the engine crate-dependency-free (engine must stay Tauri-free).
- FTP: `reqwest` does not speak FTP; adding an FTP client crate for one protocol is
  disproportionate. Non-HTTP URLs now fail fast with a clear message.
- 3.5/3.6: per-download HTTP options (headers/cookies/Basic Auth) flow through
  `RequestOptions` into the probe and every segment worker; the global proxy is
  swapped by rebuilding the shared client (RwLock) since reqwest clients are
  immutable; a per-download proxy builds a dedicated client.
- 3.8: duplicate resolution lives inside `engine.add` (auto/resume/overwrite/rename)
  so batch + extension downloads get safe defaults; the UI asks via `check_duplicate`.
- 3.9: work stealing shrinks the victim segment's `end` under the existing mutex
  (live_end hook); no atomics or double bookkeeping, completion checks unchanged.
- 3.10: `tauri-plugin-notification` v2 + `notification:default` capability; the
  event forwarder fires language-neutral toasts (✓/✕ + filename).
- All Phase 3 items complete. Version bumped 0.3.0 → 1.0.0 (Cargo.toml +
  tauri.conf.json in sync). Verified: 44 cargo tests, npm build, svelte-check,
  `tauri dev` smoke run (`Running target\debug\idin.exe`).
- Release (if user asks): `GITHUB_TOKEN=<pat> python scripts/make_release.py v1.0.0`
- ✅ v1.0.0 RELEASED: https://github.com/hatnuxx/IDIN/releases/tag/v1.0.0 —
  assets: `IDIN_1.0.0_x64-setup.exe` (1.9 MB) + `IDIN_1.0.0_x64_en-US.msi` (2.9 MB),
  Farsi release notes, tag on `main`. (Stale 0.3.0 bundle leftover cleaned up;
  use `scripts/fix_release_assets.py` for idempotent asset maintenance.)

## Post-release (v1.0.0+) — UI/UX & bug-fix pass (2026-08-31)
- ✅ UI: live stats bar (active / total speed / waiting / completed)
- ✅ UI: filter chips with live counts replacing the status dropdown
- ✅ UI: global action toasts (add / pause / resume / remove / errors) via a tiny Svelte-5 toast store
- ✅ UI: per-file-type icons, ETA display, animated striped progress bar, friendly empty states
- ✅ fix(config): config dir unified to `%APPDATA%\IDIN` — load previously came from Tauri's
  `app_config_dir()` while saves went to `%APPDATA%\IDIN`, silently resetting settings on every
  restart. One-time migration copies legacy `com.hatnux.idin` files into the canonical dir.
- ✅ fix(extension): extension downloads now honor the configured `download_dir` and
  auto-categorization (they used to bypass config and land in `~/Downloads` uncategorized)
- ✅ feat(tray): live tooltip `IDIN — N active · X MB/s` while downloads run
- Verified: cargo check + 44 tests, svelte-check 0 errors, vite build, working tree clean
- ✅ v1.0.1 RELEASED: https://github.com/hatnuxx/IDIN/releases/tag/v1.0.1 —
  assets: `IDIN_1.0.1_x64-setup.exe` (2.0 MB) + `IDIN_1.0.1_x64_en-US.msi` (2.9 MB),
  Farsi release notes. Versions bumped in sync: package.json / Cargo.toml /
  Cargo.lock / tauri.conf.json → 1.0.1.
- `fix_release_assets.py` is now version-parameterized (`[version]` arg, defaults
  to tauri.conf.json) — no more stale-asset cleanup by hand-editing the script.
