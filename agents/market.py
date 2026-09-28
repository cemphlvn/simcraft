#!/usr/bin/env python3
"""Two scripted players for games/market, one per seat, same JSON line protocol.

    python3 agents/market.py builder dumper      # alice's strategy, bob's strategy

alice's village grows wood, bob's grows stone; a house needs both.
  builder     keep what a house needs, sell the surplus, buy the missing good, build
  dumper      sell everything it grows, every tick; never build
  speculator  sell only when its good is dear (price >= 12), buy only when cheap (<= 6), build
"""
import json
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
strategies = {"alice": sys.argv[1] if len(sys.argv) > 1 else "builder",
              "bob": sys.argv[2] if len(sys.argv) > 2 else "builder"}
grows = {"alice": "wood", "bob": "stone"}

sim = subprocess.Popen([f"{root}/target/release/simcraft-agent", f"{root}/games/market"],
                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)


def call(req):
    sim.stdin.write(json.dumps(req) + "\n")
    sim.stdin.flush()
    return json.loads(sim.stdout.readline())


json.loads(sim.stdout.readline())
info = call({"cmd": "info"})
p = info["params"]
need = {"wood": p["house_wood"], "stone": p["house_stone"]}
mine = {seat: call({"cmd": "info", "as": seat})["you"][0] for seat in strategies}
shorts = 0


def decide(strategy, v, m, own):
    other = "stone" if own == "wood" else "wood"
    price = lambda g: m[f"{g}_price"]
    if strategy == "dumper":
        return ("sell_" + own, v[own]) if v[own] > 0 else None
    if v[own] >= need[own] and v[other] >= need[other]:
        return ("build", None)
    if strategy == "speculator":
        if v[other] < need[other] and price(other) <= 6 and v["gold"] >= price(other):
            return ("buy_" + other, 1)
        if price(own) >= 12 and v[own] > need[own]:
            return ("sell_" + own, v[own] - need[own])
        return None
    # builder
    if v[other] < need[other] and m[f"{other}_stock"] > 0 and v["gold"] >= price(other):
        n = min(need[other] - v[other], m[f"{other}_stock"], v["gold"] // price(other))
        return ("buy_" + other, n)
    if v[own] > need[own]:
        return ("sell_" + own, v[own] - need[own])
    return None


while True:
    obs = call({"cmd": "observe"})
    ents = {e["id"]: e for e in obs["entities"]}
    market = next(e["props"] for e in obs["entities"] if e["kind"] == "market")
    for seat, strategy in strategies.items():
        choice = decide(strategy, ents[mine[seat]]["props"], market, grows[seat])
        if choice:
            name, n = choice
            act = {"entity": mine[seat], "do": name}
            if n is not None:
                act["args"] = {"n": n}
            call({"cmd": "act", "as": seat, "actions": [act]})
    step = call({"cmd": "step", "n": 1})
    shorts += sum(e["name"] == "short" for e in step["events"])
    if step["done"]:
        break

final = call({"cmd": "observe"})
ents = {e["id"]: e for e in final["entities"]}
market = next(e["props"] for e in final["entities"] if e["kind"] == "market")
cols = []
for seat, strategy in strategies.items():
    v = ents[mine[seat]]["props"]
    cols.append(f"{seat}({strategy:10}) score={final['scores'][seat]:>4} houses={v['houses']:>2} gold={v['gold']:>4}")
print("  |  ".join(cols) + f"  | wood ${market['wood_price']:<2} stone ${market['stone_price']:<2} short={shorts}")
sim.stdin.write('{"cmd":"quit"}\n')
sim.stdin.close()
sim.wait()
