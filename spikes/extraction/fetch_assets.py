import hashlib
import json
from pathlib import Path
import urllib.request

root = Path(__file__).resolve().parent
assets = root / "assets"
assets.mkdir(exist_ok=True)
for asset in json.loads((root / "audit/assets.json").read_text())["assets"]:
    target = assets / asset["name"]
    if not target.exists():
        urllib.request.urlretrieve(asset["url"], target)
    if hashlib.sha256(target.read_bytes()).hexdigest() != asset["sha256"]:
        raise SystemExit(f"Checksum mismatch: {target}")
    print(f"Verified {asset['name']}")
