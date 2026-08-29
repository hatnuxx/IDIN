"""One-off: clean up the v1.0.0 release assets.

- deletes the stray IDIN_0.3.0_x64-setup.exe asset
- uploads any missing bundle artifacts (msi)
Run with GITHUB_TOKEN set. Idempotent: skips assets already present.
"""
import json, glob, os, urllib.request, sys

TOKEN = os.environ.get("GITHUB_TOKEN")
if not TOKEN:
    sys.exit("set GITHUB_TOKEN")

REPO = "hatnuxx/IDIN"
KEEP_VERSION = "1.0.0"


def api(url, method="GET", data=None, ctype="application/json"):
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Authorization": f"Bearer {TOKEN}", "Content-Type": ctype},
        method=method,
    )
    with urllib.request.urlopen(req) as resp:
        body = resp.read()
    return json.loads(body) if body else None


rel = next(r for r in api(f"https://api.github.com/repos/{REPO}/releases") if r["tag_name"] == "v1.0.0")
rid = rel["id"]
print("release:", rid)

# 1) Delete stale assets that don't match the current version.
for a in rel["assets"]:
    if KEEP_VERSION not in a["name"]:
        print("deleting stale asset:", a["name"])
        api(f"https://api.github.com/repos/{REPO}/releases/assets/{a['id']}", method="DELETE")
    else:
        print("keeping:", a["name"], a["state"])

# 2) Upload bundle artifacts that are missing.
have = {a["name"] for a in rel["assets"]}
bundle = os.path.join("src-tauri", "target", "release", "bundle")
paths = sorted(glob.glob(os.path.join(bundle, "nsis", "*-setup.exe"))) + sorted(
    glob.glob(os.path.join(bundle, "msi", "*.msi"))
)
for path in paths:
    name = os.path.basename(path)
    if KEEP_VERSION not in name:
        print("skipping stale local artifact:", name)
        continue
    if name in have:
        print("already uploaded:", name)
        continue
    print("uploading:", name, os.path.getsize(path), "bytes ...")
    data = open(path, "rb").read()
    a = api(
        f"https://api.github.com/repos/{REPO}/releases/{rid}/assets?name={name}",
        method="POST",
        data=data,
        ctype="application/octet-stream",
    )
    print("uploaded:", a["name"], a["state"], f"{a['size']/1e6:.1f}MB")

print("done")
