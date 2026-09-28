#!/usr/bin/env python3
"""Deletes stale build artifacts: cargo keeps every older build of a crate (a new hash per change of features,
dependencies or flags) next to the current one. For each crate in target/<profile>/deps, keeps only the newest
build and deletes the rest; also their .fingerprint entries. Safe: anything deleted is rebuilt (or fetched from
sccache) on demand.

    tools/trim_target.py            # report and delete
    tools/trim_target.py --dry-run  # report only
Standard library only.
"""

import collections
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "target"
HASHED = re.compile(r"^(?P<name>.+?)-(?P<hash>[0-9a-f]{16})(?P<ext>(\..*)?)$")


def trim(dry: bool) -> int:
    freed = 0
    for profile in ("debug", "release"):
        for sub in ("deps", ".fingerprint"):
            d = ROOT / profile / sub
            if not d.is_dir():
                continue
            groups = collections.defaultdict(lambda: collections.defaultdict(list))
            for f in d.iterdir():
                m = HASHED.match(f.name)
                if m:
                    groups[m["name"]][m["hash"]].append(f)
            for builds in groups.values():
                if len(builds) < 2:
                    continue
                when = {h: max(p.stat().st_mtime for p in files) for h, files in builds.items()}
                latest = max(when.values())
                for h, files in builds.items():
                    if latest - when[h] < 3600:
                        continue
                    for f in files:
                        size = sum(p.stat().st_size for p in f.rglob("*") if p.is_file()) if f.is_dir() else f.stat().st_size
                        freed += size
                        if not dry:
                            shutil.rmtree(f) if f.is_dir() else f.unlink()
    return freed


if __name__ == "__main__":
    dry = "--dry-run" in sys.argv
    n = trim(dry)
    print(f"{'would free' if dry else 'freed'} {n / 2**20:.0f} MB")
