#!/usr/bin/env bash
# Compiles tools/evalmetrics.py to C (Cython, pure-Python mode) for the python3 that runs tools/eval.py.
# Optional: without it, eval.py uses the same file as plain Python (same numbers, slower).
set -euo pipefail
cd "$(dirname "$0")"
uv run -q --no-project --python "$(command -v python3)" --with cython --with setuptools cythonize -q -i -3 evalmetrics.py
rm -rf evalmetrics.c build
python3 evalmetrics.py
