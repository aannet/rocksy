#!/usr/bin/env bash
# Compile Rocksy pour Windows depuis WSL (compilation croisee, sans rien installer sous Windows).
# Prerequis (espace utilisateur) : rustup avec la cible x86_64-pc-windows-gnullvm, llvm-mingw dans ~/rocksy-toolchain/llvm-mingw, Node.
# Usage : out/tools/build-win.sh [--skip-ui] [--dest /mnt/c/Users/<toi>/rocksy-test/bin]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../spikes/a-tauri-cm6/src" && pwd)"
DEST="/mnt/c/Users/aleca/rocksy-test/bin"
SKIP_UI=0
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-ui) SKIP_UI=1 ;;
    --dest) DEST="$2"; shift ;;
  esac
  shift
done
export PATH="$HOME/.cargo/bin:$HOME/rocksy-toolchain/llvm-mingw/bin:$PATH"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/rocksy-target/tauri}"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER=x86_64-w64-mingw32-clang
TARGET=x86_64-pc-windows-gnullvm

if [ "$SKIP_UI" = 0 ]; then
  (cd "$ROOT/ui" && { [ -d node_modules ] || npm ci; } && npm run build)
fi
(cd "$ROOT/src-tauri" && cargo build --release --target "$TARGET")

OUT="$CARGO_TARGET_DIR/$TARGET/release"
mkdir -p "$DEST"
cp "$OUT/rocksy.exe" "$DEST/"
# La cible gnullvm lie WebView2Loader dynamiquement : la DLL doit accompagner l'exe.
DLL="$(find "$OUT/build" -path '*x64/WebView2Loader.dll' | head -1)"
cp "$DLL" "$DEST/"
ls -la "$DEST/rocksy.exe" "$DEST/WebView2Loader.dll"
