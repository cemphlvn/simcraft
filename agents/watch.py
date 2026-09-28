#!/usr/bin/env python3
"""Watch any simcraft game run, live in the terminal, with a legend, trends and controls.

    python3 agents/watch.py games/colony --seed 5
    python3 agents/watch.py games/colony --show nest.food nest.temp nest.insulation

Keys: space pause/resume · + / - speed · s one step while paused · q quit.
Everything shown comes from the game itself: glyphs and states from game.ron, numbers from the world.
"""
import argparse
import collections
import json
import os
import re
import select
import subprocess
import sys
import termios
import time
import tty
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AGENT = ROOT / "target" / "release" / "simcraft-agent"
SPARK = "▁▂▃▄▅▆▇█"
WINDOW = 60  # ticks the sparkline and the trend arrow both cover
COLOURS = [31, 32, 33, 34, 35, 36, 91, 92, 93, 94, 95, 96]


def colour(text, code):
    return f"\033[{code}m{text}\033[0m" if code else text


def spark(values, width=30):
    vals = list(values)[-WINDOW:][:: max(1, WINDOW // width)]
    if not vals:
        return ""
    lo, hi = min(vals), max(vals)
    span = (hi - lo) or 1
    return "".join(SPARK[round((v - lo) / span * (len(SPARK) - 1))] for v in vals)


def bar(value, lo=0, hi=100, width=20):
    filled = max(0, min(width, round((value - lo) / ((hi - lo) or 1) * width)))
    return "█" * filled + "·" * (width - filled)


def leaf(state):
    """`Active.Forager.Search|...#...` -> `Search` (what a person calls it)."""
    return state.split("#")[0].split("^")[0].split("|")[0].split(".")[-1]


class Keys:
    """Non-blocking single-key input (only when attached to a terminal)."""

    def __init__(self):
        self.tty = sys.stdin.isatty()
        if self.tty:
            self.old = termios.tcgetattr(sys.stdin)
            tty.setcbreak(sys.stdin.fileno())

    def get(self, timeout):
        if not self.tty:
            time.sleep(timeout)
            return None
        ready, _, _ = select.select([sys.stdin], [], [], timeout)
        return sys.stdin.read(1) if ready else None

    def restore(self):
        if self.tty:
            termios.tcsetattr(sys.stdin, termios.TCSADRAIN, self.old)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("game", type=Path)
    ap.add_argument("--seed", type=int, help="override [run] seed")
    ap.add_argument("--speed", type=float, default=5, help="ticks per second (+ / - while running)")
    ap.add_argument("--show", nargs="*", default=None, metavar="KIND.PROP", help="numbers to track (summed over the kind)")
    ap.add_argument("--frames", type=int, default=0, help="stop after this many frames (0 = until the end)")
    a = ap.parse_args()
    game = (ROOT / a.game).resolve() if not a.game.is_absolute() else a.game
    if not AGENT.exists():
        subprocess.run(["cargo", "build", "-q", "--release", "-p", "sim-agent"], cwd=ROOT, check=True)

    args = [str(AGENT), str(game)]
    if a.seed is not None:
        panel = re.sub(r"(?m)^seed\s*=\s*\d+", f"seed = {a.seed}", (game / "engine.toml").read_text())
        tmp = ROOT / "target" / "watch_panel.toml"
        tmp.write_text(panel)
        args += ["--config", str(tmp)]
    sim = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

    def call(req):
        sim.stdin.write(json.dumps(req) + "\n")
        sim.stdin.flush()
        return json.loads(sim.stdout.readline())

    hello = json.loads(sim.stdout.readline())
    if not hello.get("ok"):
        sys.exit(json.dumps(hello, indent=1))
    info = call({"cmd": "info"})
    kinds = info["kinds"]
    hidden = {k for k, v in kinds.items() if v.get("hidden")}
    kcol = {k: COLOURS[i % len(COLOURS)] for i, k in enumerate(sorted(kinds))}

    # Legend from the designer's own glyphs (the engine reports them): glyph -> "kind state".
    legend = []
    for k, v in sorted(kinds.items()):
        if k in hidden:
            continue
        by_state = v.get("glyphs") or {}
        if v["glyph"].strip() and v["glyph"] not in by_state.values():
            legend.append(f"{colour(v['glyph'], kcol[k])} {k}")
        legend += [f"{colour(g, kcol[k])} {k} {s.lower()}" for s, g in by_state.items() if g.strip()]

    show = a.show
    if show is None:  # every prop of kinds that exist once (the nest, a market, ...)
        obs = call({"cmd": "observe"})
        once = [k for k, n in collections.Counter(e["kind"] for e in obs["entities"]).items() if n == 1]
        show = [f"{e['kind']}.{p}" for e in obs["entities"] if e["kind"] in once and e["kind"] not in hidden for p in e["props"]]
    history = collections.defaultdict(lambda: collections.deque(maxlen=120))
    story = collections.deque(maxlen=7)
    totals = collections.Counter()
    speed, paused, frames = a.speed, False, 0
    keys = Keys()
    try:
        while True:
            key = keys.get(0 if not paused else 0.1)
            if key == "q":
                break
            if key == " ":
                paused = not paused
            if key in ("+", "="):
                speed = min(200, speed * 2)
            if key in ("-", "_"):
                speed = max(0.5, speed / 2)
            if paused and key != "s":
                continue
            st = call({"cmd": "step", "n": 1})
            obs = call({"cmd": "observe"})
            frames += 1
            for e in st["events"]:
                totals[e["name"]] += 1
            if st["events"]:
                c = collections.Counter(e["name"] for e in st["events"])
                story.append(f"tick {st['tick']:>5}  " + ", ".join(f"{k} ×{n}" if n > 1 else k for k, n in c.items()))
            sums = collections.Counter()
            for e in obs["entities"]:
                for p, v in e["props"].items():
                    sums[f"{e['kind']}.{p}"] += v
            for name in show:
                history[name].append(sums[name])

            out = ["\033[H\033[J"]
            status = "PAUSED" if paused else f"{speed:g} ticks/s"
            end = f"   {colour('END: ' + str(st['result']), 91)}" if st["done"] else ""
            out.append(f"{colour(game.name, 1)}   tick {st['tick']}   [{status}]{end}")
            for e in obs["entities"]:
                if e["kind"] in hidden:
                    parts = [f"{colour(leaf(e['state']), 1)}"]
                    parts += [f"{p} {bar(v, width=12)} {v:>3}" for p, v in e["props"].items() if 0 <= v <= 100]
                    out.append(f"  {e['kind']}: " + "   ".join(parts))
            # Map, coloured by kind.
            where = {}
            for e in obs["entities"]:
                if e["kind"] not in hidden:
                    where[(e["x"], e["y"])] = e["kind"]
            for y, row in enumerate(obs["map"]):
                out.append("  " + "".join(colour(ch, kcol.get(where.get((x, y)))) if ch.strip() else ch for x, ch in enumerate(row)))
            out.append("  " + "   ".join(legend))
            out.append("")
            for kind, states in sorted(st["states"].items()):
                if kind in hidden or len(states) == 1 and "-" in states:
                    continue
                named = collections.Counter()
                for s, n in states.items():
                    named[leaf(s)] += n
                total = sum(named.values())
                if total > 60:  # dense kinds (ground cells): just their states
                    out.append(f"  {kind:<8} " + "  ".join(f"{s} {n}" for s, n in named.most_common()))
                else:
                    out.append(f"  {colour(kind, kcol.get(kind)):<17} {total:>3}  " + "  ".join(f"{s} {n}" for s, n in named.most_common()))
            for name in show:
                h = history[name]
                trend = h[-1] - h[max(0, len(h) - WINDOW)] if h else 0
                arrow = colour("▲", 92) if trend > 0 else colour("▼", 91) if trend < 0 else " "
                out.append(f"  {name:<18} {sums[name]:>6} {arrow} {spark(h)}  (last {WINDOW} ticks)")
            out.append("")
            out.append("  so far: " + "  ".join(f"{k} {v}" for k, v in totals.most_common(8)))
            out += [f"    {line}" for line in story]
            out.append(colour("  space pause · + / - speed · s step · q quit", 2))
            print("\n".join(out), flush=True)
            if st["done"] or (a.frames and frames >= a.frames):
                break
            time.sleep(1 / speed)
    except KeyboardInterrupt:
        pass
    finally:
        keys.restore()
        sim.stdin.close()
        sim.wait()


if __name__ == "__main__":
    main()
