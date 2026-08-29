# IDIN (آیدین) ⬇

<p align="center"><strong>A modern, featherweight Internet Download Manager — rebuilt for 2026.</strong></p>

Rust + Tauri 2 + Svelte 5 · **~3.9 MB** executable · bilingual (فارسی / English) with full RTL · multi-segment engine · browser integration for Chrome, Edge & Firefox.

> ای‌دین یه دانلود منیجر سبک و مدرنه — مثل IDM ولی خوش‌تیپ‌تر، اوپن‌سورس و بدون بloatware.

---

## Why IDIN?

Classic download managers haven't aged well: hundreds of megabytes of installer,
perpetual trial nagging, ad-laden UIs. IDIN takes the _only_ parts that matter —
multi-segment acceleration, browser catch, pause/resume — and ships them in a
binary smaller than most app **icons**.

|                   | IDIN       | Typical download manager |
| ----------------- | ---------- | ------------------------ |
| Installer size    | **1.7 MB** | 50–200 MB                |
| RAM at idle       | ~40 MB     | 300+ MB                  |
| Ads / nag screens | none       | plenty                   |
| Telemetry         | none       | often                    |
| Source code       | open (MIT) | closed                   |

## Features (v0.3)

- ⚡ **Multi-segment downloads** — 8 parallel HTTP `Range` segments, auto-fallback
  to a plain single-connection stream when the server doesn't report a size or
  doesn't support ranges
- ⏸ **Pause / Resume that actually resume** — per-segment byte offsets are kept
  and the transfer continues where it stopped (same segment count)
- ⏰ **Queue scheduler** — pick a start time; queued tasks start automatically
- 📋 **Clipboard monitoring** — copied download links pop up a one-click toast
- 🌐 **Browser extension** (Chrome / Edge / Firefox) — right-click _"Download with
  IDIN"_, plus automatic file-link interception, wired through a native-messaging host
- 🔒 **Hardened local API** — binds 127.0.0.1, requires a per-user shared-secret
  token (`%LOCALAPPDATA%\IDIN\api_token`), and only accepts http(s) URLs
- 🔧 **One-click setup** — the installer registers the native host and extension for
  all three browsers automatically; the app can redo it anytime from Settings
- 🎨 **Dark & Light themes** — instant switching, persisted across restarts
- 🈯 **Persian & English UI** — instant switch, persistent setting, full RTL via CSS
  logical properties, bundled Vazirmatn variable font
- 🚦 **Speed limits** — global _and_ per-task limits (per-task overrides global),
  persisted with the config
- 🪶 **Tiny**: `opt-level="z"` + LTO + `strip` + `panic="abort"` → 3.9 MB exe

## Platform support

| OS                | Status            | Notes                                                                          |
| ----------------- | ----------------- | ------------------------------------------------------------------------------ |
| **Windows 10/11** | ✅ Primary target | WebView2 (preinstalled on Win 11), NSIS + MSI installers                       |
| **Linux**         | ✅ Buildable      | needs `libwebkit2gtk-4.1-dev`, `build-essential`, `libssl-dev`, `libgtk-3-dev` |
| **macOS**         | ✅ Buildable      | Xcode CLT; `.app` + `.dmg`                                                     |

The Rust engine, extension, and UI are platform-independent — only the windowing layer differs.

## Install

Grab the latest **NSIS installer** or **MSI** from
[Releases](https://github.com/hatnuxx/IDIN/releases). The installer:

1. installs the app,
2. copies `idin-host.exe` (the browser bridge) to `%LOCALAPPDATA%\IDIN`,
3. registers the native-messaging host for Chrome, Edge, **and** Firefox (HKCU — no admin prompt),
4. stages the unpacked extension so you only click "Load unpacked / Add".

> Unsigned build — SmartScreen shows _Windows protected your PC_. Click
> **More info → Run anyway**. (See [Signing](#signing-امضا) for the cure.)

## Build & Run

Prerequisites: [Rust](https://rustup.rs) (stable), Node.js ≥ 20, platform deps above.

```bash
npm install            # note: see "Broken npm on this machine" below if install looks thin
npm run tauri dev      # hot-reload dev window
npm run tauri build    # → src-tauri/target/release/bundle/{nsis,msi}
```

Full release pipeline (host binary → app → installers → stage resources):

```cmd
scripts\release.cmd          :: build everything
scripts\release.cmd sign     :: also Authenticode-sign if SIGNTOOL_PFX/PWD are set
```

Tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml    # 26 engine/API tests, runs in ms
npm run build                                      # frontend typecheck+bundle
```

Quality gates (also run in CI via `.github/workflows/ci.yml`):

```bash
npm run lint      # prettier --check + eslint
npm run check     # svelte-check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

> **Broken npm on this machine** (npm 11.17.0 reify bug): plain `npm install`
> installs almost nothing. Workaround that works:
> `rm -rf node_modules package-lock.json && npm install --include=dev --force`
> then verify `ls node_modules | wc -l` ≈ 33 and `node_modules/.bin/tauri` exists.

## Browser extension setup (manual path)

The installer does this automatically; for dev you can do it by hand:

1. `chrome://extensions` → Developer mode → **Load unpacked** → `extension/`
   (Firefox: `about:debugging#/runtime/this-firefox` → `extension/manifest.json`)
2. Copy the assigned extension ID and register the host:
   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\register-host.ps1 -ExtensionId <EXTENSION_ID>
   ```
3. Keep IDIN running. Flow: extension → `com.hatnux.idin` native host →
   `127.0.0.1:45187` (local TCP API) → engine starts downloading.

## Architecture

```
src-tauri/src/
├── main.rs / lib.rs     # Tauri entry, IPC registration, plugin setup
├── commands.rs          # thin IPC layer (add/pause/resume/remove/list/limit)
├── browser_setup.rs     # one-click native-host + extension registration
├── local_api.rs         # TCP API on 127.0.0.1:45187 (extension bridge)
└── engine/              # PURE Rust — zero Tauri imports, fully unit-tested
    ├── mod.rs           # Engine orchestrator: tasks, segments, pause/resume
    ├── task.rs          # DownloadTask state machine (queued→probing→…→done/failed)
    ├── probe.rs         # HEAD probe: size, Accept-Ranges, filename (RFC 6266)
    ├── segment.rs       # HTTP Range segment worker
    └── events.rs        # batched progress events (~4/s per task)
src/                     # Svelte 5 frontend (runes, i18n, Fluent-inspired tokens)
extension/               # MV3 WebExtension (Chrome/Edge/Firefox)
src-host/                # native-messaging host binary (~156 KB, standalone)
scripts/                 # release pipeline, host registration, icon generator
```

**Data flow for a browser download:**

```
browser page → content.js catches link → background.js
  → chrome.runtime.connectNative("com.hatnux.idin")
  → idin-host.exe (stdin JSON, 4-byte length prefix)
  → TCP 127.0.0.1:45187 {"type":"add","url":…,"token":…}
  → Engine::add() → probe (HEAD) → split into 8 ranges
  → 8 segment workers write at file offsets → progress events → UI
```

## Design decisions

- **Engine isolation**: `engine/` never imports Tauri → `cargo test` runs in
  milliseconds and the engine is portable to a CLI/server later.
- **reqwest + rustls**: no OpenSSL headaches on any platform.
- **Event-driven UI**: engine pushes `download-progress` / `download-state` events;
  the frontend only light-polls every 2 s as a safety net.
- **Tauri runtime, not a bare one**: all async work inside Tauri runs on
  `tauri::async_runtime::spawn`. A plain `tokio::spawn` outside a runtime context
  **panics** — with `panic = "abort"` in release that's an instant app crash (this
  was v0.1.1's release-only crash; fixed in v0.1.2).
- **RTL done right**: one stylesheet, `dir` flip + CSS logical properties
  (`margin-inline-start`, `border-inline-end`) — no duplicated fa/en CSS.
- **SQLite (rusqlite)** planned for v0.2 history/queue persistence.

## Signing (امضا) — why and how

Windows shows **"Unknown publisher / Windows protected your PC"** for unsigned
executables. Two different "signing" concepts apply:

1. **Authenticode code-signing** (Windows): an X.509 cert embedded in `idin.exe`,
   `idin-host.exe`, and the installer. Proves the binary came from you and wasn't
   tampered with. This removes the SmartScreen warning.
2. **Extension signing**: Chrome/Edge only auto-install store extensions (verified
   server-side, free to publish). Firefox requires AMO signing for _listed_
   extensions — also free. Unpacked is fine for dev/personal use.

| Certificate option    | Price            | Notes                                                   |
| --------------------- | ---------------- | ------------------------------------------------------- |
| SignPath Foundation   | **free** for OSS | needs project history                                   |
| Azure Trusted Signing | ~$10/mo          | fast SmartScreen trust, easiest modern path             |
| Certum Open Source    | ~€25/yr          | individual devs, OV                                     |
| EV certificate        | $250–400/yr      | instant SmartScreen reputation; usually needs a company |

**Recommendation:** distribute unsigned for personal use (one extra click), then
Certum OSS or Azure Trusted Signing once published. `scripts\release.cmd sign`
signs everything when `SIGNTOOL_PFX` + `SIGNTOOL_PWD` are set.

## How releases are published

1. `scripts\release.cmd` — builds `idin-host.exe`, the app, and both installers,
   then stages host + extension into `src-tauri/resources/` for bundling.
2. `GITHUB_TOKEN` set in the environment (never committed).
3. `python scripts/make_release.py v0.1.2` — creates the GitHub release via the
   REST API and uploads NSIS + MSI as assets.
4. GitHub Actions could do this on tags (future: OIDC instead of tokens).

## Security notes

- Local API binds **127.0.0.1 only** and requires a **shared-secret token**:
  on startup the app generates (once) and stores it in
  `%LOCALAPPDATA%\IDIN\api_token`; the native host attaches it to every request.
  Any other local process poking the port without the token is rejected.
- Only well-formed `http://` / `https://` URLs are accepted by the API and by
  `open_url` — no other schemes ever reach the shell.
- The native host enforces the browser-side origin allowlist.
- Never hardcode tokens: release publishing reads `GITHUB_TOKEN` from the env
  (`.env` is gitignored and now only contains a placeholder).
- If a token ever leaks in chat/CI logs — **revoke it immediately**
  (GitHub → Settings → Developer settings → Tokens).

## Roadmap

Shipped in v0.2–v0.3: clipboard monitoring, batch downloads, system tray +
close-to-tray, queue scheduler with reorder, global & per-download speed limits.

Still planned:

- SQLite history + search; queue persistence across restarts
- Checksum verification (MD5 / SHA-1 / SHA-256)
- Mirror links; proxy (HTTP + SOCKS5); portable mode

---

License: MIT · Author: hatnux <hatnux@gmail.com> · Made with Rust 🦀 + Svelte ⚡
