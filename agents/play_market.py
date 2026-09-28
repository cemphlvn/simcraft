#!/usr/bin/env python3
"""Play games/market yourself: you are alice (wood), a bot plays bob (stone).

    python3 agents/play_market.py [builder|dumper|speculator]   # bob's strategy

Each day, queue orders, then press Enter to end the day. Orders apply at the start of the next day.
  sw N / ss N   sell N wood / stone        bw N / bs N   buy N wood / stone
  b             build a house (3 wood + 3 stone, worth 40 at the end)
  <Enter>       end the day                n K           end K days
  q             quit
"""
import json
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
bot = sys.argv[1] if len(sys.argv) > 1 else "builder"
sim = subprocess.Popen([f"{root}/target/release/simcraft-agent", f"{root}/games/market"],
                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)


def call(req):
    sim.stdin.write(json.dumps(req) + "\n")
    sim.stdin.flush()
    return json.loads(sim.stdout.readline())


json.loads(sim.stdout.readline())
p = call({"cmd": "info"})["params"]
need = {"wood": p["house_wood"], "stone": p["house_stone"]}
me = call({"cmd": "info", "as": "alice"})["you"][0]
them = call({"cmd": "info", "as": "bob"})["you"][0]
CODES = {"sw": "sell_wood", "ss": "sell_stone", "bw": "buy_wood", "bs": "buy_stone", "b": "build"}


def bob_move(v, m):
    """Same strategies as agents/market.py, for bob (grows stone)."""
    own, other = "stone", "wood"
    price = lambda g: m[f"{g}_price"]
    if bot == "dumper":
        return ("sell_" + own, v[own]) if v[own] > 0 else None
    if v[own] >= need[own] and v[other] >= need[other]:
        return ("build", None)
    if bot == "speculator":
        if v[other] < need[other] and price(other) <= 6 and v["gold"] >= price(other):
            return ("buy_" + other, 1)
        if price(own) >= 12 and v[own] > need[own]:
            return ("sell_" + own, v[own] - need[own])
        return None
    if v[other] < need[other] and m[f"{other}_stock"] > 0 and v["gold"] >= price(other):
        return ("buy_" + other, min(need[other] - v[other], m[f"{other}_stock"], v["gold"] // price(other)))
    if v[own] > need[own]:
        return ("sell_" + own, v[own] - need[own])
    return None


def act(seat, ent, name, n):
    a = {"entity": ent, "do": name}
    if n is not None:
        a["args"] = {"n": n}
    return call({"cmd": "act", "as": seat, "actions": [a]})["results"][0]


def show(obs, log):
    ents = {e["id"]: e for e in obs["entities"]}
    v, b = ents[me]["props"], ents[them]["props"]
    m = next(e["props"] for e in obs["entities"] if e["kind"] == "market")
    s = obs["scores"]
    print("\n" + "─" * 64)
    print("\n".join(obs.get("map", [])))
    print(f"Day {obs['tick']}/{p['days']}   score  you {s['alice']}  ·  bob({bot}) {s['bob']}")
    print(f"  YOU (A)  wood {v['wood']:>3}  stone {v['stone']:>3}  gold {v['gold']:>4}  houses {v['houses']}")
    print(f"  bob (B)  wood {b['wood']:>3}  stone {b['stone']:>3}  gold {b['gold']:>4}  houses {b['houses']}")
    print(f"  MARKET   wood ${m['wood_price']:<3}(stock {m['wood_stock']})   stone ${m['stone_price']:<3}(stock {m['stone_stock']})")
    if len(log) > 8:
        print(f"  · … {len(log) - 8} earlier")
    for line in log[-8:]:
        print("  · " + line)
    return v, m


def step(n):
    log = []
    for _ in range(n):
        obs = call({"cmd": "observe"})
        ents = {e["id"]: e for e in obs["entities"]}
        m = next(e["props"] for e in obs["entities"] if e["kind"] == "market")
        choice = bob_move(ents[them]["props"], m)
        if choice:
            r = act("bob", them, *choice)
            if r.get("ok"):
                log.append(f"bob: {choice[0]}" + (f" {choice[1]}" if choice[1] else ""))
        st = call({"cmd": "step", "n": 1})
        for e in st["events"]:
            if e.get("name") in ("house", "short", "conflict"):
                who = "you" if e.get("entity") == me else "bob" if e.get("entity") == them else ""
                log.append(f"{e['name']} {who}".strip())
        if st["done"]:
            return log, True
    return log, False


print(__doc__)
log, done = [], False
obs = call({"cmd": "observe"})
while not done:
    show(obs, log)
    log = []
    while True:
        try:
            line = input("> ").strip().lower()
        except EOFError:
            line = "q"
        if line == "q":
            done = True
            break
        if line == "" or line.startswith("n"):
            k = int(line.split()[1]) if len(line.split()) > 1 else 1
            log, done = step(k)
            break
        parts = line.split()
        if parts[0] not in CODES or (parts[0] != "b" and (len(parts) < 2 or not parts[1].isdigit())):
            print("  ? sw N, ss N, bw N, bs N, b, <Enter>, n K, q")
            continue
        n = None if parts[0] == "b" else int(parts[1])
        r = act("alice", me, CODES[parts[0]], n)
        print("  ✓ queued" if r.get("ok") else f"  ✗ {r.get('reason') or r}")
    obs = call({"cmd": "observe"})

show(obs, log)
s = obs["scores"]
print(f"\nFINAL  you {s['alice']}  vs  bob {s['bob']}  →  " +
      ("you win! 🏆" if s["alice"] > s["bob"] else "bob wins." if s["bob"] > s["alice"] else "draw."))
sim.stdin.write('{"cmd":"quit"}\n')
sim.stdin.close()
sim.wait()
