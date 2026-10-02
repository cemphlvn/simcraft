#!/usr/bin/env bash
# The whole gate, one command, cheapest first (stops at the first failing step):
#   1. rustfmt --check        house style (rustfmt.toml)
#   2. clippy                 workspace lints, perf set included (Cargo.toml [workspace.lints]); warnings fail
#   3. ruff                   tools/*.py
#   4. cargo test             golden hashes, scenarios, work profiles (test/snapshots/work__*.snap), conformance
#   5. simcraft-check         every game: loads, its views check out, plays with presses; errors and warnings fail
#   6. eval --check           games with evals/: the last saved step re-run, nothing may have moved
#
#   tools/check.sh           all of it
#   tools/check.sh --quick   1-5 (evals take a minute)
set -euo pipefail
cd "$(dirname "$0")/.."
quick=${1:-}
step() { printf '\n== %s\n' "$*"; }

step fmt
cargo fmt --all --check

step clippy
cargo clippy -q --all-targets -- -D warnings

step ruff
if command -v ruff >/dev/null; then
  ruff format -q --check tools
  ruff check -q tools
else
  echo "(ruff not installed, skipped)"
fi

step test
if ! out=$(cargo test -q 2>&1); then
  printf '%s\n' "$out" | tail -40
  exit 1
fi
printf '%s\n' "$out" | grep '^test result' | grep -v ' 0 passed' || true

step simcraft-check
cargo build -q --release -p sim-gpu --bin simcraft-check
bad=0
for g in games/*/; do
  [ -f "$g/engine.toml" ] || continue
  out=$(target/release/simcraft-check "${g%/}" 2>&1) || true
  printf '%s\n' "$out" | grep -v '^note' || true
  printf '%s\n' "$out" | grep -q '^error\|^warn' && bad=1
done
cargo build -q --release -p sim-mobile --bin simcraft-smash
out=$(target/release/simcraft-smash check --dir games/smash 2>&1) || true
printf '%s\n' "$out" | grep -q '^error' && { printf '%s\n' "$out"; bad=1; }
[ "$bad" = 0 ] || { echo "simcraft-check found problems"; exit 1; }

if [ "$quick" != "--quick" ]; then
  step eval --check
  for g in games/*/; do
    [ -d "$g/evals" ] && [ -f "$g/engine.toml" ] || continue
    python3 tools/eval.py "${g%/}" --check | tail -1
  done
  # Native mobile games measure themselves (sim-mobile).
  cargo build -q --release -p sim-mobile --bin simcraft-smash
  target/release/simcraft-smash eval --check --dir games/smash
fi
printf '\nall green\n'
