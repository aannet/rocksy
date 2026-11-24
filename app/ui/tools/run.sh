#!/usr/bin/env bash
# Copies dist + tools to the Windows test folder and plays scenarios in Edge headless.
# Usage: tools/run.sh <scenario-name> [more...]   (names from tools/scenarios/*.json)
set -e
UI="$(cd "$(dirname "$0")/.." && pwd)"
WIN=/mnt/c/Users/aleca/rocksy-test/ui
PY=/mnt/c/Users/aleca/AppData/Local/Programs/Python/Python312/python.exe
mkdir -p "$WIN/dist" "$WIN/tools" "$WIN/shots"
rm -rf "$WIN/dist" && cp -r "$UI/dist" "$WIN/dist"
cp -r "$UI/tools/." "$WIN/tools/"
cd /mnt/c
for s in "$@"; do
  "$PY" 'C:\Users\aleca\rocksy-test\ui\tools\drive.py' "C:\\Users\\aleca\\rocksy-test\\ui\\tools\\scenarios\\$s.json"
done
