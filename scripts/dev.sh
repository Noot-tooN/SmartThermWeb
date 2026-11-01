#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
if [ ! -d web/node_modules ]; then (cd web && npm ci); fi
cargo build --locked
./target/debug/smartthermweb &
backend=$!
(cd web && exec node node_modules/vite/bin/vite.js --host 127.0.0.1) &
frontend=$!
cleanup() {
    trap - EXIT INT TERM
    kill "$backend" "$frontend" 2>/dev/null || true
    wait "$backend" "$frontend" 2>/dev/null || true
}
trap cleanup EXIT INT TERM
while kill -0 "$backend" 2>/dev/null && kill -0 "$frontend" 2>/dev/null; do sleep 1; done
