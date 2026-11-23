#!/usr/bin/env bash
# Plays scenarios against the real app (rocksy.exe, WebView2 + Rust core). Usage: tools/run-app.sh <name>…  (tools/app/*.json)
# The app runs as an isolated instance (its own ROCKSY_DATA_DIR under the test folder), never in the user's profile.
# Optional (Windows paths):
#   ROCKSY_EXE     exe to test (default: C:\Users\aleca\rocksy-test\wip\rocksy.exe, never the exe the user runs)
#   ROCKSY_IT      test folder: data directories and working copies of the vaults (default: ...\rocksy-test\it)
#   ROCKSY_VAULTS  folder T.ref() points to (default: the reference vaults; pass a folder of copies to keep them untouched)
set -e
UI="$(cd "$(dirname "$0")/.." && pwd)"
WIN=/mnt/c/Users/aleca/rocksy-test/ui
PY=/mnt/c/Users/aleca/AppData/Local/Programs/Python/Python312/python.exe
mkdir -p "$WIN/tools" "$WIN/shots-app"
[ -n "${ROCKSY_IT:-}" ] || mkdir -p /mnt/c/Users/aleca/rocksy-test/it
cp -r "$UI/tools/." "$WIN/tools/"
EXTRA=()
[ -n "${ROCKSY_EXE:-}" ] && EXTRA+=(--exe "$ROCKSY_EXE")
[ -n "${ROCKSY_IT:-}" ] && EXTRA+=(--it "$ROCKSY_IT")
[ -n "${ROCKSY_VAULTS:-}" ] && EXTRA+=(--vaults "$ROCKSY_VAULTS")
cd /mnt/c
for s in "$@"; do
  "$PY" 'C:\Users\aleca\rocksy-test\ui\tools\drive.py' "C:\\Users\\aleca\\rocksy-test\\ui\\tools\\app\\$s.json" "${EXTRA[@]}"
done
