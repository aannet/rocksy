# Handoff — signing `rocksy.exe`

Status: living
Last verified at: not applicable (handoff written 2026-11-24, nothing started)

Written on 2026-11-24. Scope: sign the Windows executable of Rocksy so that it shows an identified publisher. Nothing has been started: this document says where things stand, what must be decided and in which order to do it.

The prices and access conditions below are given **from memory, not verified** at the time of writing. First task for whoever takes this over: check them on the providers' sites.

## 1. Current state

- `rocksy.exe` is not signed. Windows SmartScreen shows "Unknown publisher" at the first launch of a downloaded exe.
- `WebView2Loader.dll`, shipped next to the exe, is already signed by Microsoft: only `rocksy.exe` needs signing.
- The exe is built **under WSL**, by cross-compilation (`tools/build-win.sh`, target `x86_64-pc-windows-gnullvm`). There is no Rust and no Windows SDK on Windows, hence no `signtool`.
- No installer is produced: `bundle.active` is `false` in `app/src-tauri/tauri.conf.json`. Delivery is a zip (`rocksy.exe`, `WebView2Loader.dll`, `install.ps1`, README), uploaded by hand to a GitHub release.
- The repository `github.com/aannet/rocksy` is public, **without a license file**; the `Cargo.toml` files declare `license = "UNLICENSED"`.
- There is no CI: no `.github/` folder.
- `install.ps1` (PowerShell script) is not signed either, and has never been run on a machine.

## 2. What signing brings, and its limits

- The publisher name is shown instead of "Unknown publisher", and the exe can no longer be modified without the signature becoming invalid.
- The SmartScreen warning **does not disappear immediately**: it depends on a reputation built with downloads, per certificate. An EV certificate no longer removes it automatically.
- Signing changes nothing in the behaviour of the app or in the code.

## 3. Options

| Option | Cost (to check) | Conditions (to check) | Name shown | Fit for Rocksy |
|---|---|---|---|---|
| SignPath Foundation | Free | Open source project: free license in the repository, exe built by a public CI, application to submit and get accepted | SignPath Foundation | Yes, after license and CI |
| Azure Trusted Signing (Microsoft) | About $10 per month | Identity verification; access for individuals outside the United States and Canada was limited | The verified holder | To check for an individual in France |
| OV certificate from an authority (Sectigo, DigiCert, Certum...) | About 200 to 500 EUR per year | Identity verification; private key must be on a secure USB token or HSM | The holder | Yes, but expensive and needs hardware |
| Microsoft Store | Developer account | MSIX packaging; the Store signs | The account holder | Changes the distribution mode |
| Self-signed certificate | Free | None | Not recognised | No: useless outside the machine that created it |

## 4. Recommendation

SignPath Foundation route, because the project is public and personal and the cost is zero. It requires two prerequisites that are worth having anyway: a license and a CI build.

As long as the owner is the only user, staying unsigned only costs a click on the warning. Signing becomes useful when other people download the app.

## 5. Decisions for the owner

1. **Repository license** (MIT, Apache-2.0, GPL...). Without a free license, SignPath is not possible. First check compatibility with the dependencies: Tauri (MIT/Apache-2.0), CodeMirror 6 (MIT), tantivy (MIT), Pele (MIT).
2. **Name shown**: accept "SignPath Foundation" as publisher, or want one's own name (hence Azure Trusted Signing or an OV certificate).
3. **Budget** if the free route does not fit.
4. **Also sign `install.ps1`** or not. A signed script avoids `-ExecutionPolicy Bypass`, with the same certificate.

## 6. Implementation plan (SignPath route)

1. Add `LICENSE` at the root and update `license` in the `Cargo.toml` files and `app/ui/package.json`.
2. Set up a GitHub Actions CI that produces the exe on every `v*` tag. Two ways, to choose from:
   - reuse the current cross-compilation on a Linux runner (rustup, target `x86_64-pc-windows-gnullvm`, llvm-mingw: everything `build-win.sh` does);
   - build natively on a Windows runner (target `x86_64-pc-windows-msvc`). The exe is then no longer the same binary as the one tested so far, and the question of `WebView2Loader.dll` next to the exe must be revisited.
   The first choice keeps the binary already proven; the second is more standard for Tauri.
3. Check that the CI produces a working exe before adding signing: run it as an isolated instance (`ROCKSY_DATA_DIR`) and replay the scenarios in `app/ui/tools/app/`.
4. Submit the application to SignPath Foundation (public repository, license, project description), then configure the SignPath project and the secret in GitHub.
5. Add to the CI the signing step for `rocksy.exe`, then the release creation with the signed zip.
6. Check the result under Windows: `Get-AuthenticodeSignature .\rocksy.exe` must give `Valid`, and the file properties must show the signature and a timestamp.

## 7. Variant without CI (Azure Trusted Signing or OV certificate)

Signing is done at the end of `tools/build-win.sh`, under Linux, after the exe is copied:

- `jsign` (Java) can sign with Azure Trusted Signing and with a secure USB token;
- `osslsigncode` can sign with a secure USB token through PKCS#11.

In both cases: always timestamp the signature (otherwise it expires with the certificate), and never put credentials or a key in the repository. None of these tools is installed today; there is no sudo in WSL, so install in the home folder, as for Rust and `gh`.

## 8. Points of attention

- **Order of operations**: sign the final exe, after any change to the binary, and before building the zip. If an installer is added later, it is signed too, after the exe it contains.
- **What is published must be what was tested**: the release zip must contain the signed exe built from the same commit as the tag.
- **Secrets**: signing credentials live in GitHub secrets or on the owner's machine, never in the repository, the logs or the dashboard.
- **SmartScreen reputation**: it restarts from zero if the certificate changes. Choose one route and stick to it.
- **Out of scope of this document**: automatic app update, an MSI/NSIS installer, signing for other systems.

## 9. Where to look in the repository

| Subject | File |
|---|---|
| Current Windows build | `tools/build-win.sh`, `docs/runbooks/build.md` |
| Tauri configuration (identifier, `bundle`) | `app/src-tauri/tauri.conf.json` |
| Content and use of the delivery | `release/README.md`, `release/install.ps1` |
| Toolchain and real-app testing | `AGENTS.md` |
| Scenarios to replay on an exe produced another way | `app/ui/tools/app/` |
