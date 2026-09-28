#!/usr/bin/env python3
"""A scripted player for games/mercy_dungeon, speaking the JSON line protocol.

    python3 agents/mercy_dungeon.py pacifist
    python3 agents/mercy_dungeon.py violent [--seed N]
    python3 agents/mercy_dungeon.py regret      # kill one ghost, then try mercy

It walks (BFS) to the nearest ghost, then spares it or fights it.
It knows nothing about the rules except what `info` says it can do.
"""
import json
import subprocess
import sys
from collections import Counter, deque
from pathlib import Path

policy = sys.argv[1] if len(sys.argv) > 1 else "pacifist"
seed = sys.argv[sys.argv.index("--seed") + 1] if "--seed" in sys.argv else None
root = Path(__file__).resolve().parent.parent

panel = None
if seed:  # the operator's panel with another seed
    import os
    import tempfile
    src = open(f"{root}/games/mercy_dungeon/engine.toml").read()
    fd, panel = tempfile.mkstemp(suffix=".toml")
    os.write(fd, src.replace("seed = 7", f"seed = {seed}").encode())
    os.close(fd)

cmd = [f"{root}/target/release/simcraft-agent", f"{root}/games/mercy_dungeon"] + (["--config", panel] if panel else [])
sim = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)


def call(req):
    sim.stdin.write(json.dumps(req) + "\n")
    sim.stdin.flush()
    return json.loads(sim.stdout.readline())


json.loads(sim.stdout.readline())  # ready
hero_id = call({"cmd": "info"})["you"][0]
events = Counter()
result = None

while True:
    obs = call({"cmd": "observe"})
    ents = obs["entities"]
    hero = next((e for e in ents if e["id"] == hero_id), None)
    if hero is None:
        break
    ghosts = [e for e in ents if e["kind"] == "ghost"]
    grid = obs["map"]
    blocked = {(x, y) for y, row in enumerate(grid) for x, c in enumerate(row) if c != "."}

    dist = lambda g: max(abs(g["x"] - hero["x"]), abs(g["y"] - hero["y"]))
    near = min(ghosts, key=dist) if ghosts else None
    action = None
    if near and dist(near) <= 1:
        killed = events["dust"]
        if policy == "violent" or (policy == "regret" and killed == 0):
            action = {"entity": hero_id, "do": "fight"}
        elif near["state"] == "Calm":
            action = {"entity": hero_id, "do": "spare"}
    if action is None and ghosts:
        # BFS to any free cell next to a ghost
        goals = {(g["x"] + dx, g["y"] + dy) for g in ghosts for dx in (-1, 0, 1) for dy in (-1, 0, 1)}
        start = (hero["x"], hero["y"])
        prev, q = {start: None}, deque([start])
        found = None
        while q:
            c = q.popleft()
            if c in goals and c != start:
                found = c
                break
            for dx in (-1, 0, 1):
                for dy in (-1, 0, 1):
                    n = (c[0] + dx, c[1] + dy)
                    if n not in prev and n not in blocked:
                        prev[n] = c
                        q.append(n)
        if found:
            while prev[found] != start:
                found = prev[found]
            action = {"entity": hero_id, "do": "move", "args": {"dx": found[0] - start[0], "dy": found[1] - start[1]}}
    if action:
        r = call({"cmd": "act", "actions": [action]})["results"][0]
        if not r["ok"]:
            events["refused"] += 1
    step = call({"cmd": "step", "n": 1})
    events.update(e["name"] for e in step["events"])
    if step["done"]:
        result = step["result"] or "timeout"
        break

final = call({"cmd": "observe"})
hero = next((e for e in final["entities"] if e["id"] == hero_id), None)
stats = f"hp={hero['props']['hp']} lv={hero['props']['lv']}" if hero else "hero gone"
print(f"{policy:9} seed={seed or 7:>3}  result={result:7} tick={final['tick']:>3}  {stats:12}  {dict(events)}")
sim.stdin.write('{"cmd":"quit"}\n')
sim.stdin.close()
sim.wait()
