"""A scripted surfer for highway_surfers: plays a run through the agent protocol and saves its presses as a replay
(`simcraft-play games/highway_surfers --replay FILE` watches it; `--shot out.png --record N` films it).

    python3 games/highway_surfers/bot.py [--seed N] [--ticks N] [--out runs/bot.jsonl] [--skill 0..100]

It predicts with the game's physics (gravity, speeds, footprints), not by asking the engine: a move is simulated
before it is made. `--skill` is how often (%) it notices a danger in time; lower skill
makes a worse player, for difficulty evals.
"""

import argparse
import json
import random
import re
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
AGENT = ROOT / "target/release/simcraft-agent"
GAME = Path(__file__).resolve().parent
NEAR = 45  # cells around the surfer it looks at (the world is long; it needs its surroundings)
SIZE = {"car": (620, 900), "van": (660, 950), "truck": (720, 980)}
# The surfer's gravity, read from its `motion` in game.ron (one source of truth).
GRAVITY = int(re.search(r'"surfer":.*?gravity: (\d+)', (GAME / "game.ron").read_text(), re.S).group(1))


class Player:
    """The policy: an observation in, actions out (eval.toml [player] uses it; `main` runs it standalone).

    It predicts with the game's physics: every move is simulated tick by tick against the traffic (each vehicle at
    its own constant speed) before it is made."""

    def __init__(self, info: dict, seed: int, skill: int = 100):
        self.p = info["params"]
        self.rng = random.Random(seed)
        self.skill = skill
        # Its rhythm is in seconds of game time, so it plays the same at any tick rate.
        self.rate = tomllib.loads((GAME / "engine.toml").read_text())["run"]["tick_rate"]
        self.every = lambda secs: max(1, round(secs * self.rate))

    def fly(self, me, cars, bridges, vh, dx):
        """Simulates a hop (dx = +-1) or a jump (dx = 0) from now: the roof top it lands on, or None (road, a side,
        a bridge)."""
        p = self.p
        mount = next((c for c in cars if c["id"] == me["props"]["mount"]), None)
        m = me["props"]
        px, py, ph, vy = m["px"], m["py"], m["ph"], mount["props"]["vy"] if mount else m["vy"]
        target = (me["x"] + dx) * 1000 + 500
        vx = m["vx"]
        ys = [c["props"]["py"] for c in cars]
        clamp = lambda v, lo, hi: max(lo, min(hi, v))  # noqa: E731
        for _ in range(60):
            d = target - px
            if d * d < 36 and vx * vx <= p["steer_acc"] ** 2:
                px, vx = target, 0
            elif d or vx:
                aim = clamp(int(d * p["steer_k"] / 100), -p["steer_max"], p["steer_max"])
                vx += clamp(aim - vx, -p["steer_acc"], p["steer_acc"])
            px, py = px + vx, py + vy
            vh -= GRAVITY
            ph += vh
            if ph <= 0 and vh <= 0:
                return None
            ys = [y + c["props"]["vy"] for y, c in zip(ys, cars, strict=True)]
            head = ph + p["body"]
            for b in bridges:
                if b["x"] == px // 1000 and b["y"] == py // 1000 and head >= b["props"]["clearance"]:
                    return None
            for y, c in zip(ys, cars, strict=True):
                w, ln = SIZE[c["kind"]]
                cx = c["props"]["px"]
                if cx - w // 2 <= px < cx + w // 2 and y - ln // 2 <= py < y + ln // 2:
                    top = c["props"]["top"]
                    if vh < 0 and top - p["land_band"] <= ph <= top:
                        return top
                    if ph < top - p["land_band"]:
                        return None
        return None

    def decide(self, o) -> list:
        me = next((e for e in o["entities"] if e["kind"] == "surfer"), None)
        if me is None:
            return []
        t = o["tick"]
        cars = [e for e in o["entities"] if e["kind"] in SIZE and abs(e["y"] - me["y"]) < 40]
        bridges = [e for e in o["entities"] if e["kind"] == "bridge" and 0 <= e["y"] - me["y"] < 40]
        state = me["state"]
        noticed = self.rng.randrange(100) < self.skill
        p = self.p
        if "Air" in state:
            return []
        if "Ducking" in state:
            under = any(b["x"] == me["x"] and 0 <= b["y"] - me["y"] <= 2 for b in bridges)
            return [] if under else [{"do": "stand", "args": {}}]
        if "Riding" in state:
            head = me["props"]["ph"] + p["body"]
            mount = next((c for c in cars if c["id"] == me["props"]["mount"]), None)
            vy = mount["props"]["vy"] if mount else 0
            for b in bridges:
                if b["x"] == me["x"] and head >= b["props"]["clearance"]:
                    ticks = (b["y"] * 1000 - me["props"]["py"]) / max(vy, 1)
                    if 0 <= ticks <= self.every(0.27) and noticed:
                        return [{"do": "crouch", "args": {}}]
            # The BIG JUMP: beside a ramp, over the median, if it lands on a roof.
            ramp = any(e["kind"] == "ramp" and abs(e["y"] - me["y"]) <= 2 for e in o["entities"])
            to_median = p["median"] - me["x"]
            if ramp and to_median * to_median == 1 and self.fly(me, cars, bridges, p["big_v"], 2 * to_median):
                return [{"do": "big_jump", "args": {"dir": to_median}}]
            if t % self.every(0.4) == 0 and self.rng.random() < 0.6:
                for d in (-1, 1) if self.rng.random() < 0.75 else (1, -1):
                    lane = me["x"] + d
                    if 0 <= lane < p["lanes"] and lane != p["median"] and self.fly(me, cars, bridges, p["hop_v"], d):
                        return [{"do": "leap", "args": {"dx": d, "dy": 0}}]
            if t % self.every(1.33) == self.every(0.67) and self.fly(me, cars, bridges, p["jump_v"], 0):
                return [{"do": "jump", "args": {}}]
            return []
        if "Ground" in state:
            for d in (-1, 1):
                lane = me["x"] + d
                if 0 <= lane < p["lanes"] and lane != p["median"] and self.fly(me, cars, bridges, p["hop_v"], d):
                    return [{"do": "leap", "args": {"dx": d, "dy": 0}}]
            if noticed and self.fly(me, cars, bridges, p["jump_v"], 0):
                return [{"do": "jump", "args": {}}]
        return []


def run(seed: int, ticks: int, skill: int):
    """Plays one run through the agent protocol; returns (last step report, presses, events)."""
    proc = subprocess.Popen([str(AGENT), str(GAME)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    proc.stdout.readline()

    def call(req):
        proc.stdin.write(json.dumps(req) + "\n")
        proc.stdin.flush()
        return json.loads(proc.stdout.readline())

    player = Player(call({"cmd": "info"}), seed, skill)
    log, events, end = [], [], {}
    for _ in range(ticks):
        o = call({"cmd": "observe", "near": NEAR})
        if not o.get("you"):
            break
        for a in player.decide(o):
            r = call({"cmd": "act", "actions": [dict(a, entity=o["you"][0])]})
            if r.get("ok"):
                log.append({"tick": o["tick"], "action": a["do"], "args": a["args"]})
        end = call({"cmd": "step", "n": 1})
        events += [e["name"] for e in end.get("events", []) if e.get("entity") == o["you"][0]]
        if end.get("done"):
            break
    proc.stdin.close()
    return end, log, events


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--ticks", type=int, default=1800)
    ap.add_argument("--skill", type=int, default=100)
    ap.add_argument("--out", default=str(ROOT / "runs/highway_surfers-bot.jsonl"))
    a = ap.parse_args()
    end, log, events = run(a.seed, a.ticks, a.skill)
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text("".join(json.dumps(p) + "\n" for p in log))
    count = {}
    for n in events:
        count[n] = count.get(n, 0) + 1
    print(json.dumps({"tick": end.get("tick"), "result": end.get("result"), "scores": end.get("scores"), "events": count}))
    print(f"{len(log)} presses -> {a.out}")


if __name__ == "__main__":
    main()
