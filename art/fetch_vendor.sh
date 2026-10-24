#!/bin/zsh
# Fetches the third-party packs listed in art/vendor.json into art/vendor/, checking each archive's
# SHA-256. Archives are cached in ~/Library/Caches/aldermoor-vendor. Needs Google Chrome and Node.
set -euo pipefail
ROOT=${0:A:h:h}
CACHE=~/Library/Caches/aldermoor-vendor
mkdir -p $CACHE "$ROOT/art/vendor/quaternius"
(cd "$ROOT/tools/fetch" && [ -d node_modules ] || npm install --silent)
python3 -I - "$ROOT" "$CACHE" <<'PY'
import json, hashlib, subprocess, sys, zipfile, shutil, pathlib, time
root, cache = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
for pack in json.load(open(root / "art/vendor.json"))["packs"]:
    zpath = cache / pack["file"]
    if pack.get("url") and (not zpath.exists() or hashlib.sha256(zpath.read_bytes()).hexdigest() != pack["sha256"]):
        subprocess.run(["curl", "-sL", "-o", str(zpath), pack["url"]], check=True)
    if not zpath.exists() or hashlib.sha256(zpath.read_bytes()).hexdigest() != pack["sha256"]:
        slug = pack["page"].rstrip("/").rsplit("/", 1)[1]
        out = cache / f"dl-{slug}"
        shutil.rmtree(out, ignore_errors=True)
        subprocess.run(["node", str(root / "tools/fetch/itch.mjs"), slug, str(out)], check=True)
        shutil.move(str(out / pack["file"]), zpath)
        shutil.rmtree(out, ignore_errors=True)
        time.sleep(15)  # itch.io rate-limits quick repeats
    digest = hashlib.sha256(zpath.read_bytes()).hexdigest()
    if digest != pack["sha256"]:
        sys.exit(f"{pack['file']}: SHA-256 {digest} does not match art/vendor.json (a new upstream version?)")
    dest = root / "art/vendor" / pack.get("dest", "quaternius")
    with zipfile.ZipFile(zpath) as z:
        for src, dst in pack["extract"].items():
            for name in z.namelist():
                if name == src or (src.endswith("/") and name.startswith(src) and not name.endswith("/")):
                    target = dest / (dst + name[len(src):] if src.endswith("/") else dst)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(z.read(name))
    print("ok", pack["id"])
PY
