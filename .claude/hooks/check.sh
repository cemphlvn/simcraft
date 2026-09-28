#!/usr/bin/env bash
# After every edit Claude makes: format the file, and make bugs show themselves.
#   *.rs              rustfmt (house style: rustfmt.toml), milliseconds
#   *.py              ruff format + ruff check
#   games/<g>/*       simcraft-check games/<g>: loads the game and its views, plays 300 ticks pressing its buttons,
#                     reports errors (broken) and warnings (runs, but probably not as meant)
#   assets/*.ron      simcraft-check on every game (a pack is shared)
# Findings go back to Claude (exit 2 = shown to it), so a broken edit is fixed in the same loop.
set -u
input=$(cat)
file=$(printf '%s' "$input" | python3 -c 'import json,sys; d=json.load(sys.stdin); print((d.get("tool_input") or {}).get("file_path",""))' 2>/dev/null)
[ -z "$file" ] && exit 0
root="${CLAUDE_PROJECT_DIR:-$(pwd)}"
cd "$root" || exit 0
rel="${file#"$root"/}"
check="$root/target/release/simcraft-check"

report() { # $1 = findings; exit 2 if any error line
  [ -z "$1" ] && return 0
  if printf '%s\n' "$1" | grep -q '^error\|^warn'; then
    printf '%s\n' "$1" | grep '^error\|^warn' >&2
    exit 2
  fi
}

case "$rel" in
  *.rs)
    command -v rustfmt >/dev/null && rustfmt --edition 2024 --config-path "$root/rustfmt.toml" "$file" 2>&1 | head -20 >&2
    ;;
  *.py)
    if command -v ruff >/dev/null; then
      ruff format -q "$file"
      out=$(ruff check -q "$file" 2>&1) || { printf '%s\n' "$out" >&2; exit 2; }
    fi
    ;;
  games/*/*)
    game=$(printf '%s' "$rel" | cut -d/ -f1-2)
    [ -x "$check" ] && report "$("$check" "$game" 2>&1)"
    ;;
  assets/*.ron)
    if [ -x "$check" ]; then
      for g in games/*/; do report "$("$check" "${g%/}" 2>&1)"; done
    fi
    ;;
esac
exit 0
