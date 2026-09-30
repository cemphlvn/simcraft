#!/usr/bin/env python3
"""How the engine scales: run a game at growing entity counts and measure what a tick costs.

    tools/perf.py games/traffic --kind car --counts 100,400,1600 [--ticks 600] [--save "what changed"]

For every count it sets `[spawn] KIND = N` in the game's panel, steps a warm-up, then times `--ticks` ticks through
the agent protocol: milliseconds a tick (wall clock, all cores) and CPU milliseconds a tick (every core added up),
and the world's hash at the end. `--save` records a step in games/<name>/perf/ and compares it with the last one:
the times say whether it got faster, the hashes whether it is still the same world (an optimisation must not change
a single tick). Timing is not deterministic: run on a quiet machine, and read big changes, not small ones.
"""

import argparse
import json
import re
import resource
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AGENT = ROOT / "target" / "release" / "simcraft-agent"


def run(game_dir: Path, kind: str, n: int, ticks: int, warmup: int, tmp: Path) -> dict:
    panel = (game_dir / "engine.toml").read_text()
    panel, found = re.subn(rf"(?m)^{re.escape(kind)}\s*=.*$", f"{kind} = {n}", panel)
    if not found:
        sys.exit(f"engine.toml has no `{kind} = ...` line under [spawn]")
    panel = re.sub(r"(?m)^max_ticks\s*=.*$", f"max_ticks = {warmup + ticks + 10}", panel)
    path = tmp / f"panel_{n}.toml"
    path.write_text(panel)
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    sim = subprocess.Popen([str(AGENT), str(game_dir), "--config", str(path)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

    def call(req):
        sim.stdin.write(json.dumps(req) + "\n")
        sim.stdin.flush()
        return json.loads(sim.stdout.readline())

    if not json.loads(sim.stdout.readline()).get("ok"):
        sys.exit(f"{game_dir} does not load with {kind} = {n}")
    call({"cmd": "step", "n": warmup})
    t0 = time.perf_counter()
    call({"cmd": "step", "n": ticks})
    wall = time.perf_counter() - t0
    hash_ = call({"cmd": "hash"})["hash"]
    counts = call({"cmd": "observe"})["counts"]
    sim.stdin.close()
    sim.wait()
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu = (after.ru_utime - before.ru_utime) + (after.ru_stime - before.ru_stime)
    return {
        "n": n,
        "alive": counts.get(kind, 0),
        "ms_per_tick": round(wall * 1000 / ticks, 3),
        # CPU of the whole process (warm-up and loading included): the work, whatever the core count.
        "cpu_ms_per_tick": round(cpu * 1000 / (warmup + ticks), 3),
        "hash": hash_,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("game")
    ap.add_argument("--kind", required=True)
    ap.add_argument("--counts", default="100,400,1600")
    ap.add_argument("--ticks", type=int, default=600)
    ap.add_argument("--warmup", type=int, default=60)
    ap.add_argument("--save")
    a = ap.parse_args()
    game_dir = (ROOT / a.game).resolve() if not Path(a.game).is_absolute() else Path(a.game)
    with tempfile.TemporaryDirectory() as t:
        rows = [run(game_dir, a.kind, int(n), a.ticks, a.warmup, Path(t)) for n in a.counts.split(",")]
    hist = sorted((game_dir / "perf").glob("*.json")) if (game_dir / "perf").exists() else []
    last = {r["n"]: r for r in json.loads(hist[-1].read_text())["rows"]} if hist else {}
    print(f"{game_dir.name}: {a.ticks} ticks after {a.warmup}" + (f"   (vs {hist[-1].name})" if hist else ""))
    print(f"  {a.kind:>8} {'alive':>6} {'ms/tick':>9} {'cpu ms/tick':>12}   {'x faster':>9}  world")
    for r in rows:
        b = last.get(r["n"])
        faster = f"{b['ms_per_tick'] / max(r['ms_per_tick'], 1e-6):8.1f}x" if b else "        -"
        same = ("same" if b["hash"] == r["hash"] else "DIFFERENT WORLD") if b else r["hash"]
        print(f"  {r['n']:>8} {r['alive']:>6} {r['ms_per_tick']:>9.3f} {r['cpu_ms_per_tick']:>12.3f}   {faster}  {same}")
    if a.save:
        (game_dir / "perf").mkdir(exist_ok=True)
        slug = re.sub(r"[^a-z0-9]+", "-", a.save.lower()).strip("-")[:40]
        out = game_dir / "perf" / f"{len(hist):03d}-{slug}.json"
        out.write_text(json.dumps({"step": a.save, "kind": a.kind, "ticks": a.ticks, "rows": rows}, indent=1) + "\n")
        print(f"saved {out.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
