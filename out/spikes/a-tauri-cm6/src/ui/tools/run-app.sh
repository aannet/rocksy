#!/usr/bin/env bash
# Plays scenarios against the real app (rocksy.exe, WebView2 + Rust core). Usage: tools/run-app.sh <name>…  (tools/app/*.json)
set -e
UI="$(cd "$(dirname "$0")/.." && pwd)"
WIN=/mnt/c/Users/aleca/rocksy-test/ui
PY=/mnt/c/Users/aleca/AppData/Local/Programs/Python/Python312/python.exe
mkdir -p "$WIN/tools" "$WIN/shots-app" /mnt/c/Users/aleca/rocksy-test/it
cp -r "$UI/tools/." "$WIN/tools/"
cd /mnt/c
for s in "$@"; do
  "$PY" 'C:\Users\aleca\rocksy-test\ui\tools\drive.py' "C:\\Users\\aleca\\rocksy-test\\ui\\tools\\app\\$s.json"
done
