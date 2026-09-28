#!/usr/bin/env bash
# Scaffold a new skill: skills/new.sh simcraft-unity-view
set -euo pipefail
name="${1:?usage: skills/new.sh <skill-name> (lowercase, hyphens)}"
[[ "$name" =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]] || { echo "skill names are lowercase words joined by hyphens" >&2; exit 1; }
dir="$(cd "$(dirname "$0")" && pwd)/$name"
[ -e "$dir" ] && { echo "$dir already exists" >&2; exit 1; }
mkdir -p "$dir"
sed "s/simcraft-your-skill/$name/" "$(dirname "$0")/_template/SKILL.md" > "$dir/SKILL.md"
echo "created $dir/SKILL.md — fill it in, then: skills/install.sh"
