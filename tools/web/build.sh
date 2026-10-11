#!/usr/bin/env bash
# The browser build of the phone app (docs/architecture.md, Distribution: `web`): MAZE, SMASH and the playground on a
# canvas, WebGPU where the browser has it, WebGL2 otherwise. Output: dist/simcraft-web/{index.html, simcraft_bg.wasm}
# plus page.html, the same page without its document skeleton (for hosts that add their own).
# Needs: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli --version 0.2.129
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/dist/simcraft-web"
cd "$root"
cargo build -p sim-mobile --lib --target wasm32-unknown-unknown --profile web
tmp="$(mktemp -d)"
wasm-bindgen --target web --no-typescript --out-dir "$tmp" --out-name simcraft target/wasm32-unknown-unknown/web/sim_mobile.wasm
mkdir -p "$out"
cp "$tmp/simcraft_bg.wasm" "$out/"
# The glue goes inline: one page and one wasm file, nothing else to allow or host.
python3 - "$tmp/simcraft.js" tools/web/page.html "$out" <<'PY'
import re, sys, pathlib
glue, page, out = (pathlib.Path(p) for p in sys.argv[1:])
js = glue.read_text()
js = re.sub(r"^export \{[^}]*\};?\s*$", "", js, flags=re.M)
js = re.sub(r"^export (function|class|const|let|async function)", r"\1", js, flags=re.M)
body = page.read_text().replace("/*GLUE*/", js)
(out / "page.html").write_text(body)
(out / "index.html").write_text('<!doctype html>\n<html lang="en"><head><meta charset="utf-8">'
    '<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover"></head><body>\n'
    + body + "\n</body></html>\n")
PY
rm -rf "$tmp"
ls -la "$out"
