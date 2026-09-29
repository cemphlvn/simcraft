"""Metrics that read a whole field (tens of thousands of voxels per sample, many samples, many seeds).

Written in Cython's pure-Python mode: this is an ordinary Python file (ruff formats and lints it, it runs anywhere),
and `tools/build_evalmetrics.sh` compiles it to C where Cython is available (`uv run --with cython`), which is what
`tools/eval.py` then imports. The compiled and the plain version must give the same numbers:
`python3 tools/evalmetrics.py` checks that.
"""

from array import array

try:
    import cython
except ImportError:  # plain Python: the Cython types below are only hints

    class _Type:
        def __class_getitem__(cls, _):
            return cls

    class cython:  # noqa: N801 (stands in for the `cython` module)
        longlong = Py_ssize_t = _Type
        compiled = False

        @staticmethod
        def locals(**_):
            return lambda f: f

        boundscheck = wraparound = cdivision = staticmethod(lambda _: lambda f: f)


def compiled() -> bool:
    return cython.compiled


@cython.boundscheck(False)
@cython.wraparound(False)
@cython.locals(
    vals=cython.longlong[:],
    cols=cython.longlong[:],
    w=cython.Py_ssize_t,
    h=cython.Py_ssize_t,
    d=cython.Py_ssize_t,
    plane=cython.Py_ssize_t,
    i=cython.Py_ssize_t,
    n=cython.Py_ssize_t,
    z=cython.Py_ssize_t,
    value=cython.longlong,
    pillar=cython.longlong,
    built=cython.longlong,
    roofs=cython.longlong,
    square=cython.longlong,
    tallest=cython.longlong,
    pillars=cython.longlong,
    c=cython.longlong,
)
def _structure(vals, cols, w, h, d, value, pillar):
    plane = w * h
    n = plane * d
    built = roofs = square = tallest = pillars = 0
    for i in range(n):
        if vals[i] != value:
            continue
        built += 1
        cols[i % plane] += 1
        z = i // plane
        if z + 1 < d and vals[i + plane] == 0:
            roofs += 1
    for i in range(plane):
        c = cols[i]
        square += c * c
        if c > tallest:
            tallest = c
        if c >= pillar:
            pillars += 1
    return built, tallest, square, pillars, roofs


def structure(field: dict, value: int = 1, pillar: int = 4) -> dict:
    """What was built, from a `{"cmd":"field"}` reply:
    built    voxels holding `value`
    height   the tallest column of them (voxels)
    stacking for each built voxel, how many built voxels share its column, averaged: ~1 for scattered work,
             high when work lands on work (pillars). Grows with density too: compare with a control.
    pillars  columns with at least `pillar` built voxels
    roofs    built voxels with open air right under them (arches, overhangs)"""
    w, h, d = field["width"], field["height"], field["depth"]
    vals = array("q", field["values"])
    cols = array("q", bytes(8 * w * h))
    built, tallest, square, pillars, roofs = _structure(memoryview(vals), memoryview(cols), w, h, d, value, pillar)
    return {
        "built": built,
        "height": tallest,
        "stacking": round(square / built, 2) if built else 0,
        "pillars": pillars,
        "roofs": roofs,
    }


if __name__ == "__main__":
    import importlib.util
    import random
    import sys
    import time
    from pathlib import Path

    # The same numbers from both versions, and how long each takes on a 40x40x20 world.
    rng = random.Random(1)
    w, h, d = 40, 40, 20
    field = {"width": w, "height": h, "depth": d, "values": [rng.choice([0, 0, 0, 1, 2]) for _ in range(w * h * d)]}
    here = Path(__file__)
    spec = importlib.util.spec_from_file_location("evalmetrics_plain", here)
    plain = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(plain)
    sys.path.insert(0, str(here.parent))
    import evalmetrics as fast

    runs = 20
    for name, mod in (("plain", plain), ("compiled" if fast.compiled() else "plain (not built)", fast)):
        t = time.perf_counter()
        for _ in range(runs):
            out = mod.structure(field, value=2)
        print(f"{name:18} {1000 * (time.perf_counter() - t) / runs:7.2f} ms per sample  {out}")
    if plain.structure(field, value=2) != fast.structure(field, value=2):
        sys.exit("the compiled and the plain version disagree")
