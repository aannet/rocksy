# Runbook: build

Status: living
Last verified at: release 0.2.1 (branch kb-restructure; commands re-run during the restructuring: core tests, clippy, UI build, vitest, cross-compile build)

Everything is built in WSL and cross-compiled to Windows. There is no Rust, Node or MSVC toolchain on Windows and no sudo in WSL.

## Toolchain (user space)

- rustup in `~/.cargo` (not on PATH by default): `export PATH=$HOME/.cargo/bin:$PATH`. Windows target `x86_64-pc-windows-gnullvm`.
- llvm-mingw in `~/rocksy-toolchain/llvm-mingw` (see `docs/decisions/0017-cross-compilation-from-wsl.md`).
- Node and npm in WSL for the UI.
- Consequence of the gnullvm target: `WebView2Loader.dll` must ship next to `rocksy.exe`. `tools/build-win.sh` copies it.

## Core only (Linux)

```bash
export PATH=$HOME/.cargo/bin:$PATH
cd app
CARGO_TARGET_DIR=~/.cache/rocksy-target/core cargo test -p rocksy-core
CARGO_TARGET_DIR=~/.cache/rocksy-target/core cargo clippy -p rocksy-core --all-targets
```

`app/src-tauri` is deliberately outside the workspace (own `Cargo.lock`), so the core builds and tests on Linux without the Tauri dependency tree. The shell has no unit tests on Linux.

## UI

```bash
cd app/ui
npm run build      # TypeScript check, then bundle into app/ui/dist
npx vitest run
```

## Windows exe

```bash
export PATH=$HOME/.cargo/bin:$PATH
tools/build-win.sh [--skip-ui] [--dest <folder under /mnt/c/...>]
```

- Without `--skip-ui` the script builds the UI first (`npm ci` if `node_modules` is missing).
- The script ends with `cp rocksy.exe WebView2Loader.dll <dest>`. The default `--dest` is hard-coded in the script as the `bin` folder of the owner's test area on Windows. Never build into that default folder, into a `release-*` folder or into any folder the owner runs from. Pass a work folder: for example `--dest /mnt/c/Users/<you>/rocksy-test/wip`.
- Cargo output goes to `~/.cache/rocksy-target/tauri` (override with `CARGO_TARGET_DIR`).
- Last full run during the restructuring: `--skip-ui` into the work folder succeeded in about 50 s, exe about 14 MB.

## Notes

- The release profile uses `panic = "unwind"` (see `docs/decisions/0014-panic-unwind-and-index-rebuild.md`). The root `app/Cargo.toml` and `app/src-tauri/Cargo.toml` must stay aligned.
- No installer is produced (`bundle.active` is `false` in `app/src-tauri/tauri.conf.json`); the delivery is a zip (see `docs/runbooks/release.md`).
