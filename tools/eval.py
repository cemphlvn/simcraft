#!/usr/bin/env python3
"""Eval-driven game development: measure a game on fixed seeds, compare with the last step.

    tools/eval.py games/colony                       # run, compare with the last saved eval
    tools/eval.py games/colony --save "brood reserve" # ...and record it as the next step
    tools/eval.py games/colony --check                # re-run the last saved step; fail if anything moved

The designer declares the metrics in games/<name>/eval.toml (see docs/evals.md). Every run uses
the same seeds, and simcraft is deterministic, so a saved eval is also a regression test.
Standard library only.
"""
import argparse
import collections
import json
import math
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AGENT = ROOT / "target" / "release" / "simcraft-agent"


def panel_for(panel: str, seed: int, max_ticks: int, overrides=()) -> str:
    """engine.toml with the eval's seed and length, plus `--set` param overrides."""
    for kv in overrides:
        key, value = kv.split("=", 1)
        panel, n = re.subn(rf"(?m)^{re.escape(key.strip())}\s*=.*$", f"{key.strip()} = {value.strip()}", panel)
        if n == 0:
            if not re.search(r"(?m)^\[params\]", panel):
                panel += "\n[params]\n"
            panel = re.sub(r"(?m)^\[params\]$", f"[params]\n{key.strip()} = {value.strip()}", panel, count=1)
    for key, value in (("seed", seed), ("max_ticks", max_ticks)):
        panel, n = re.subn(rf"(?m)^{key}\s*=\s*\d+", f"{key} = {value}", panel)
        if n != 1:
            sys.exit(f"engine.toml needs exactly one `{key} = <number>` line in [run]")
    return panel


class Run:
    """One seed: the numbers a metric expression can read."""

    def __init__(self, game_dir: Path, seed: int, cfg: dict, tmp: Path):
        panel = tmp / f"panel_{seed}.toml"
        panel.write_text(panel_for((game_dir / "engine.toml").read_text(), seed, cfg["max_ticks"], cfg.get("set", ())))
        sim = subprocess.Popen([str(AGENT), str(game_dir), "--config", str(panel)],
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

        def call(req):
            sim.stdin.write(json.dumps(req) + "\n")
            sim.stdin.flush()
            return json.loads(sim.stdout.readline())

        hello = json.loads(sim.stdout.readline())
        if not hello.get("ok"):
            sys.exit(f"{game_dir} does not load: {json.dumps(hello)}")
        self.p = call({"cmd": "info"})["params"]
        self.events = collections.Counter()
        self.first = {}
        self.series = collections.defaultdict(list)  # name -> [(tick, value)]
        self.errors = collections.Counter()
        self.result = None
        every = cfg.get("sample_every", 20)
        self._sample(call, 0, call({"cmd": "step", "n": 0}))
        while True:
            st = call({"cmd": "step", "n": every})
            for e in st["events"]:
                name = e["name"]
                if name.startswith("error"):
                    self.errors[name] += 1
                    continue
                self.events[name] += 1
                self.first.setdefault(name, e["tick"])
            self._sample(call, st["tick"], st)
            if st["done"] or st["tick"] >= cfg["max_ticks"]:
                self.end_tick, self.result = st["tick"], st["result"]
                break
        sim.stdin.close()
        sim.wait()

    def _sample(self, call, tick, st):
        for kind, n in st["counts"].items():
            self.series[f"count.{kind}"].append((tick, n))
        for kind, states in st["states"].items():
            for state, n in states.items():
                self.series[f"state.{kind}.{state}"].append((tick, n))
        sums = collections.Counter()
        for e in call({"cmd": "observe"})["entities"]:
            for prop, v in e["props"].items():
                sums[f"sum.{e['kind']}.{prop}"] += v
        for name, v in sums.items():
            self.series[name].append((tick, v))

    def scope(self):
        s = self.series

        def values(name):
            return [v for _, v in s.get(name, [])] or [0]

        return {
            "end_tick": self.end_tick, "result": self.result, "p": self.p,
            "events": self.events, "first": lambda name, default=None: self.first.get(name, self.end_tick if default is None else default),
            "peak": lambda name: max(values(name)), "low": lambda name: min(values(name)),
            "mean": lambda name: sum(values(name)) / len(values(name)), "last": lambda name: values(name)[-1],
            "at": lambda tick, name: min(s.get(name, [(0, 0)]), key=lambda tv: abs(tv[0] - tick))[1],
            "min": min, "max": max, "abs": abs, "round": round, "sum": sum, "len": len, "range": range,
        }


def evaluate(game_dir: Path, cfg: dict):
    tmp = ROOT / "target" / "eval"
    tmp.mkdir(parents=True, exist_ok=True)
    per_seed, errors = {}, collections.Counter()
    for seed in cfg["seeds"]:
        run = Run(game_dir, seed, cfg, tmp)
        errors.update(run.errors)
        scope = run.scope()
        per_seed[seed] = {}
        for name, m in cfg["metrics"].items():
            try:
                per_seed[seed][name] = eval(m["expr"], {"__builtins__": {}, **scope})  # the designer's own file
            except Exception as e:  # noqa: BLE001 — report which metric is broken
                sys.exit(f"metric '{name}' (`{m['expr']}`) failed: {e}")
    agg = {}
    for name in cfg["metrics"]:
        vals = [per_seed[s][name] for s in cfg["seeds"]]
        agg[name] = {"mean": round(sum(vals) / len(vals), 2), "min": min(vals), "max": max(vals)}
    return per_seed, agg, dict(errors)


def saved(game_dir: Path):
    d = game_dir / "evals"
    return sorted(d.glob("*.json")) if d.exists() else []


def verdict(goal: str, before: float, after: float) -> str:
    if goal not in ("max", "min") or math.isclose(before, after):
        return "="
    better = after > before if goal == "max" else after < before
    return "better" if better else "worse"


def fmt(v):
    return f"{v:g}" if isinstance(v, (int, float)) else str(v)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("game", type=Path)
    ap.add_argument("--save", metavar="CHANGE", help="record this run as the next step, described by CHANGE")
    ap.add_argument("--check", action="store_true", help="re-run the last saved step and fail if any metric moved")
    ap.add_argument("--set", action="append", default=[], metavar="PARAM=VALUE",
                    help="try a param without editing the game (repeatable); not allowed with --save")
    a = ap.parse_args()
    game_dir = (ROOT / a.game).resolve() if not a.game.is_absolute() else a.game
    cfg = tomllib.loads((game_dir / "eval.toml").read_text())
    if a.set and a.save:
        sys.exit("--set is for trying; put the change in the game, then --save")
    cfg["set"] = a.set
    if not AGENT.exists():
        subprocess.run(["cargo", "build", "-q", "--release", "-p", "sim-agent"], cwd=ROOT, check=True)

    per_seed, agg, errors = evaluate(game_dir, cfg)
    history = saved(game_dir)
    last = json.loads(history[-1].read_text()) if history else None

    if a.check:
        if not last:
            sys.exit("nothing saved yet: run with --save first")
        moved = {k: (last["seeds"][str(s)][k], per_seed[s][k]) for s in cfg["seeds"] for k in cfg["metrics"]
                 if last["seeds"][str(s)].get(k) != per_seed[s][k]}
        print(f"check against {history[-1].name}: " + ("OK, identical" if not moved else f"{len(moved)} values moved"))
        for k, (b, n) in list(moved.items())[:10]:
            print(f"  {k}: {b} -> {n}")
        sys.exit(1 if moved else 0)

    print(f"{game_dir.name}: {len(cfg['seeds'])} seeds, up to {cfg['max_ticks']} ticks"
          + (f"   (vs {history[-1].name})" if last else ""))
    print(f"  {'metric':<18} {'mean':>9} {'min':>7} {'max':>7}   {'before':>9}  {'':<6}  goal")
    for name, m in cfg["metrics"].items():
        now = agg[name]
        before = last["metrics"].get(name, {}).get("mean") if last else None
        v = verdict(m.get("goal", ""), before, now["mean"]) if before is not None else ""
        print(f"  {name:<18} {fmt(now['mean']):>9} {fmt(now['min']):>7} {fmt(now['max']):>7}   "
              f"{fmt(before) if before is not None else '-':>9}  {v:<6}  {m.get('goal', 'info')}")
    if errors:
        print(f"  runtime errors: {errors}")

    if a.save:
        n = len(history)
        slug = re.sub(r"[^a-z0-9]+", "-", a.save.lower()).strip("-")[:40]
        out = game_dir / "evals" / f"{n:03d}-{slug}.json"
        out.parent.mkdir(exist_ok=True)
        out.write_text(json.dumps({"step": n, "change": a.save, "seeds": {str(k): v for k, v in per_seed.items()},
                                   "metrics": agg, "errors": errors}, indent=1) + "\n")
        print(f"saved {out.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
