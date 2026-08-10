#!/usr/bin/env python3
"""Fail if any locked crate version is younger than MIN_DAYS.

A compromised release is usually caught within a few days, so waiting before you take a
new version removes most of the risk. cargo has no such setting yet (rust-lang/cargo#15973
asks for one, after pnpm shipped minimumReleaseAge), so this reads Cargo.lock and asks
crates.io when each locked version was published.

    scripts/dep-age.py            # 8 days
    scripts/dep-age.py 30
    scripts/dep-age.py 8 --fix    # print the cargo update commands that pin older versions

crates.io asks for one request per second and a user agent that says who you are.
"""

import json
import sys
import time
import tomllib
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path

FIX = "--fix" in sys.argv
MIN_DAYS = int(sys.argv[1]) if len(sys.argv) > 1 and sys.argv[1].isdigit() else 8
AGENT = "asf-dep-age (https://github.com/wassname/asf)"
CACHE = Path.home() / ".cache/asf-dep-age.json"

lock = tomllib.loads((Path(__file__).parent.parent / "Cargo.lock").read_text())
crates = [(p["name"], p["version"]) for p in lock["package"] if "source" in p]
cache = json.loads(CACHE.read_text()) if CACHE.exists() else {}

cutoff = datetime.now(timezone.utc) - timedelta(days=MIN_DAYS)
young, unknown = [], []
for name, version in crates:
    key = f"{name} {version}"
    if key not in cache:
        url = f"https://crates.io/api/v1/crates/{name}/{version}"
        request = urllib.request.Request(url, headers={"User-Agent": AGENT})
        with urllib.request.urlopen(request, timeout=30) as response:
            cache[key] = json.load(response)["version"]["created_at"]
        CACHE.write_text(json.dumps(cache))
        time.sleep(1)
    published = datetime.fromisoformat(cache[key])
    if published > cutoff:
        young.append((key, published))

for key, published in sorted(young, key=lambda k: k[1], reverse=True):
    print(f"TOO NEW  {key:40} published {published:%Y-%m-%d}")
print(f"\n{len(crates)} locked crates, {len(young)} younger than {MIN_DAYS} days")

if FIX and young:
    print("\n# newest version of each that is old enough, same major:")
    for key, _ in young:
        name, version = key.split()
        url = f"https://crates.io/api/v1/crates/{name}/versions"
        request = urllib.request.Request(url, headers={"User-Agent": AGENT})
        with urllib.request.urlopen(request, timeout=30) as response:
            versions = json.load(response)["versions"]
        time.sleep(1)
        major = version.split(".")[0] if version.split(".")[0] != "0" else ".".join(version.split(".")[:2])
        older = [
            v for v in versions
            if not v["yanked"]
            and v["num"].startswith(major + ".")
            and datetime.fromisoformat(v["created_at"]) < cutoff
        ]
        if not older:
            print(f"# no {name} {major}.x is older than {MIN_DAYS} days")
            continue
        print(f"cargo update -p {name}@{version} --precise {older[0]['num']}")

sys.exit(1 if young else 0)
