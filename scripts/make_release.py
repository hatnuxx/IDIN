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

RELEASE_NOTES = """## IDIN v1.0.2 — نصب یک‌کلیکی افزونهٔ مرورگر 🧩

### ✨ قابلیت جدید: نصب خودکار افزونه
- 🖱 **نصب یک‌کلیکی افزونه کروم/اج/فایرفاکس** — از بخش «تنظیمات ← اتصال مرورگر» مرورگر خود را انتخاب کنید؛ IDIN همه‌چیز (native host + فایل‌های افزونه + ثبت رجیستری) را خودکار نصب می‌کند و صفحهٔ افزونه‌های مرورگر را برای شما باز می‌کند. فقط یک بار «Load unpacked / Load Temporary Add-on» را تأیید کنید.
- 🚀 **نصب در همان اجرای اول** — هنگام اولین اجرای برنامه، integration مرورگرها بی‌صدا آماده می‌شود؛ دیالوگ قبلی فقط برای انتخاب مرورگر و وارد کردن Extension ID لازم است.
- 🎯 **ثبت انتخابی** — فقط مرورگرهایی که واقعاً نصب‌اند ثبت می‌شوند؛ مرورگرهای شناسایی‌شده بالای لیست می‌آیند.

### 🐛 رفع باگ‌ها
- 🔗 **دیالوگ اتصال مرورگر** که در نسخهٔ قبل از UI قابل دسترس نبود، حالا از تنظیمات باز می‌شود.
- 🦇 نصاب (NSIS) به‌جای مسیر مستقیم exe به فایل bat اشاره می‌کرد که لولهٔ ارتباطی افزونه با برنامه را می‌شکست.
- 🔁 **انتخاب «ادامه/رونویسی/نام جدید» برای دانلود تکراری** نادیده گرفته می‌شد — حالا واقعاً اعمال می‌شود.
- 🧭 مسیرهای پیدا کردن افزونه و host در حالت توسعه/نصب‌شده اصلاح شد.
- 🌐 ده‌ها رشتهٔ فارسی هاردکد‌شده (تاریخچه، تنظیمات، فیلترها) به سیستم ترجمه منتقل شدند — نسخهٔ انگلیسی دیگر فارسی نمی‌بیند؛ وضعیت «در حال آماده‌سازی»، شمارندهٔ «در انتظار» و مقدار اولیهٔ تم هم درست شد.

### 📦 نصب
فایل `*-setup.exe` را دانلود و اجرا کنید.

> **Unsigned build** — در SmartScreen روی **More info → Run anyway** کلیک کنید.

### ✅ کیفیت
۴۴ تست Rust + svelte-check بدون خطا + تست زندهٔ کامل جریان نصب افزونه (تشخیص مرورگر، نصب، به‌روزرسانی Extension ID، باز شدن صفحهٔ افزونه‌ها).
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
