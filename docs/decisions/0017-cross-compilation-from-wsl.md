# ADR 0017: Build for Windows by cross-compiling from WSL

Status: accepted
Date: 2026-11-23

## Context

Windows had no Rust, Node or Visual Studio Build Tools, and WSL has no sudo. Waiting for a manual installation of several GB would have blocked the work.

## Decision

Compile from WSL with a user-space toolchain: rustup in `~/.cargo`, target `x86_64-pc-windows-gnullvm`, llvm-mingw in `~/rocksy-toolchain/llvm-mingw`, instead of MSVC. WSL interop runs the resulting `.exe`. Nothing is installed on Windows; a test area is created under the user folder on Windows for exes and synthetic vaults.

## Consequences

- `WebView2Loader.dll` must ship next to `rocksy.exe`.
- A native MSVC build stays possible later if Build Tools are installed (a code-signing and CI question, see `docs/handoffs/code-signing.md`).
- Runbook: `docs/runbooks/build.md`.

## Sources

Journal 2026-11-23 10:50, 10:52, 11:00.
