#!/usr/bin/env bash
# Make the simcraft skills available to your AI tool.
#   skills/install.sh              → this repo (.claude/skills), for Claude Code opened here
#   skills/install.sh --user       → every project (~/.claude/skills)
#   skills/install.sh --to DIR     → any folder your tool reads skills from
# Skills are symlinked, so `git pull` updates them. Tools that read Agent Skills
# (SKILL.md with name/description) can also be pointed at skills/<name>/ directly.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
dest="$here/../.claude/skills"
case "${1:-}" in
  --user) dest="$HOME/.claude/skills" ;;
  --to) dest="${2:?--to needs a folder}" ;;
  "") ;;
  *) echo "usage: skills/install.sh [--user | --to DIR]" >&2; exit 1 ;;
esac
mkdir -p "$dest"
for skill in "$here"/*/; do
  name="$(basename "$skill")"
  [ "$name" = "_template" ] && continue
  ln -sfn "${skill%/}" "$dest/$name"
  echo "  $name -> $dest/$name"
done
