"""The two-way highway at the start of a run: traffic near the start, ramps on the median, bridges, coins. Writes
the rows of game.ron between `// rows:begin` and `// rows:end`. Deterministic (same seed, same road).

    python3 games/highway_surfers/gen_layout.py [--seed N] [--rows N]

Five columns: 0 and 1 oncoming, 2 the median, 3 and 4 with you. Row 0 holds the directors ahead, row 1 those
behind (they join the surfer on the first tick); y grows ahead.
"""

import argparse
import random
import re
from pathlib import Path

GAME = Path(__file__).with_name("game.ron")

LANES = [0, 1, 3, 4]
MEDIAN = 2
# What drives in each lane at the start: (car, van, truck) weights (the directors then use game.ron's params).
MIX = {0: (70, 20, 10), 1: (70, 20, 10), 3: (55, 30, 15), 4: (25, 30, 45)}
GAP = (2, 6)  # cells between two vehicles in a lane at the start
START = 700  # the surfer's row: room behind it, since the oncoming side carries you back (about a minute of it)
LANE = 3  # and lane: the fast lane of your side, next to the median
TRAFFIC = 60  # rows around the start laid out with traffic; beyond, the directors send it
CALM = 30  # rows ahead of the start with no bridge
BRIDGE_EVERY = (70, 110)  # rows between two bridges
RAMP_EVERY = (130, 200)  # rows between two ramps on the median (15-25 s at your side's speed)
RAMP_CLEAR = 25  # rows past a ramp kept free of bridges (a BIG JUMP's flight, both sides)
COIN_RUNS = (12, 22)  # rows between two runs of coins


def build(seed: int, rows: int) -> list[str]:
    rng = random.Random(seed)
    grid = [["." for _ in range(5)] for _ in range(rows)]
    for x in LANES:
        grid[0][x], grid[1][x] = "A", "B"
    grid[START][LANE] = "S"
    # Traffic around the start.
    for lane in LANES:
        y = START - TRAFFIC // 2 + rng.randint(0, GAP[1])
        while y < START + TRAFFIC:
            if grid[y][lane] == ".":
                grid[y][lane] = rng.choices("cvT", weights=MIX[lane])[0]
            y += rng.randint(*GAP)
    # Ramps on the median: the first one soon, so the BIG JUMP is met early.
    ramps = []
    y = START + 25
    while y < rows - 20:
        grid[y][MEDIAN] = "R"
        ramps.append(y)
        y += rng.randint(*RAMP_EVERY)
    # Bridges over the whole road, in empty cells, never where a BIG JUMP flies (from a ramp to 25 rows past it).
    y = START + CALM
    while y < rows - 10:
        if not any(r - 5 <= y <= r + RAMP_CLEAR for r in ramps):
            grid[y] = ["=" if c == "." else c for c in grid[y]]
        y += rng.randint(*BRIDGE_EVERY)
    # Coins: short runs in one lane, low (a roof rider catches them) or high (a jump or a truck roof).
    y = START + 8
    while y < rows - 10:
        lane, high = rng.choice(LANES), rng.random() < 0.35
        for k in range(rng.randint(3, 6)):
            if grid[y + k][lane] == ".":
                grid[y + k][lane] = "O" if high else "o"
        y += rng.randint(*COIN_RUNS)
    return ["".join(r) for r in grid]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--rows", type=int, default=2400)
    a = ap.parse_args()
    rows = build(a.seed, a.rows)
    body = "".join(f'            "{r}",\n' for r in rows)
    text = GAME.read_text()
    new, n = re.subn(r"(// rows:begin\n).*?(\s*// rows:end)", lambda m: m.group(1) + body.rstrip("\n") + m.group(2), text, flags=re.S)
    assert n == 1, "rows markers not found in game.ron"
    GAME.write_text(new)
    print(f"{len(rows)} rows, {sum(r.count(c) for r in rows for c in 'cvT')} vehicles, {sum(r.count('R') for r in rows)} ramps")


if __name__ == "__main__":
    main()
