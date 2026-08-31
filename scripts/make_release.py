import json, glob, urllib.request, os, sys

# GitHub release publisher for IDIN.
# Reads token from GITHUB_TOKEN env var (never hardcode it!).
# Usage: python scripts/make_release.py [tag]   (tag defaults to the app version)

TOKEN = os.environ.get("GITHUB_TOKEN")
if not TOKEN:
    print("Set GITHUB_TOKEN env var first:")
    print('  PowerShell: $env:GITHUB_TOKEN = "ghp_..."')
    print('  bash:       export GITHUB_TOKEN="ghp_..."')
    sys.exit(1)

REPO = "hatnuxx/IDIN"

# Version comes from tauri.conf.json so it never drifts from the build.
with open(os.path.join(os.path.dirname(__file__), "..", "src-tauri", "tauri.conf.json"), encoding="utf-8") as f:
    VERSION = json.load(f)["version"]
TAG = sys.argv[1] if len(sys.argv) > 1 else f"v{VERSION}"

RELEASE_NOTES = """## IDIN v1.0.1 — رفع اشکال و بهبود رابط کاربری 🏛

### 🐛 رفع باگ‌ها
- 🔧 **رفع باگ پاک‌شدن تنظیمات** — تنظیمات (پوشه دانلود، دسته‌ها، محدودیت سرعت و…) بعد از هر ری‌استارت گم می‌شدند؛ حالا همه‌چیز در یک مسیر واحد ذخیره می‌شود و داده‌های نسخه‌های قبلی خودکار منتقل می‌شوند.
- 🧩 **دانلودهای افزونه مرورگر** حالا از پوشه دانلود تعیین‌شده و دسته‌بندی خودکار (Docs/Videos/Music/…) پیروی می‌کنند.

### ✨ بهبودهای رابط کاربری
- 📊 **نوار آمار زنده** — تعداد فعال، سرعت کل، در انتظار، تکمیل‌شده
- 🏷 **چیپ‌های فیلتر با شمارنده زنده** به‌جای منوی کشویی وضعیت
- 🔔 اعلان‌های کوچک (toast) برای افزودن/توقف/ادامه/حذف و خطاها
- 🎬 آیکون نوع فایل + نمایش **زمان باقی‌مانده (ETA)** + نوار پیشرفت انیمیشنی
- 🗂 حالت خالی دوستانه با دکمه شروع سریع
- 🖥 **تولتیپ زنده تری‌آیکون** — «۲ فعال · ۱.۴ MB/s» حین دانلود

### 📦 نصب
فایل `*-setup.exe` را دانلود و اجرا کنید.

> **Unsigned build** — در SmartScreen روی **More info → Run anyway** کلیک کنید.

### ✅ کیفیت
۴۴ تست Rust (شامل تست‌های شبکه با سرور محلی واقعی) + svelte-check بدون خطا + اجرای آزمایشی کامل برنامه.
"""

body = {
    "tag_name": TAG,
    "target_commitish": "main",
    "name": f"IDIN {TAG}",
    "body": RELEASE_NOTES,
    "draft": False,
    "prerelease": False,
}
req = urllib.request.Request(
    f"https://api.github.com/repos/{REPO}/releases",
    data=json.dumps(body).encode(),
    headers={"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"},
    method="POST",
)
try:
    rel = json.load(urllib.request.urlopen(req))
except urllib.error.HTTPError as e:
    print("CREATE FAILED", e.code, e.read().decode()[:500])
    raise SystemExit(1)

rid = rel["id"]
print("release id:", rid, rel["html_url"])

# Discover the current build artifacts dynamically (version-aware).
bundle = os.path.join("src-tauri", "target", "release", "bundle")
assets = sorted(glob.glob(os.path.join(bundle, "nsis", "*-setup.exe"))) + sorted(
    glob.glob(os.path.join(bundle, "msi", "*.msi"))
)
if not assets:
    print("No build artifacts found under", bundle, "— run the release build first.")
for path in assets:
    if not os.path.exists(path):
        print("missing:", path)
        continue
    name = os.path.basename(path)
    data = open(path, "rb").read()
    req = urllib.request.Request(
        f"https://uploads.github.com/repos/{REPO}/releases/{rid}/assets?name={name}",
        data=data,
        headers={"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/octet-stream"},
        method="POST",
    )
    try:
        a = json.load(urllib.request.urlopen(req))
        print("uploaded:", a["name"], a["state"], f"{a['size']/1e6:.1f}MB")
    except urllib.error.HTTPError as e:
        print("UPLOAD FAILED", name, e.code, e.read().decode()[:300])
