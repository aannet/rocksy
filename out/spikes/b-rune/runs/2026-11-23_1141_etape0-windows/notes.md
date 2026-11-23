# Étape 0 du spike B — essai sous Windows

Essai de faisabilité, pas un run T1/T2 (pas de `coldstart.csv`). Aucun accès au vault ni à `G:\` : la sonde fabrique son document de 319 Ko en mémoire (`PROBE_KB=319`), elle ne lit aucun fichier. Écritures Windows limitées à `C:\Users\aleca\rocksy-test\rune\`.

## Compilation (croisée, depuis WSL2)

```sh
export PATH=$HOME/.cargo/bin:$HOME/rocksy-toolchain/llvm-mingw/bin:$PATH
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER=x86_64-w64-mingw32-clang
export CC_x86_64_pc_windows_gnullvm=x86_64-w64-mingw32-clang CXX_x86_64_pc_windows_gnullvm=x86_64-w64-mingw32-clang++
export AR_x86_64_pc_windows_gnullvm=llvm-ar
export CARGO_TARGET_DIR=$HOME/.cache/rocksy-target/rune-win RUSTUP_TOOLCHAIN=stable
cargo build --release --target x86_64-pc-windows-gnullvm -p hello --features rune/tooling
cargo build --release --target x86_64-pc-windows-gnullvm -p rune-kit --example rocksy_probe --features rune/tooling
```

- Les deux compilent du premier coup, sans contournement ni modification de Rune (hors l'ajout de la sonde). `hello` : 4 min 13 s à froid ; la sonde ensuite : 32 s. Seulement des avertissements (variables inutilisées, motifs inatteignables).
- `blake3` et `psm`, qui bloquaient `-msvc` et `-gnu`, passent avec le clang de llvm-mingw.
- `RUSTUP_TOOLCHAIN=stable` (1.100.0) est nécessaire : la 1.99.1 épinglée par le dépôt n'a pas la cible `gnullvm` installée. Effet de bord : un `rustup show` lancé dans le clone a installé la toolchain 1.99.1 (Linux) sur cette machine.
- `hello.exe` 29,4 Mo, `rocksy_probe.exe` 30,9 Mo (avec `tooling`, non strippés). `objdump -p` : uniquement des DLL système (kernel32, user32, dxgi, dwrite, imm32, uiautomationcore, api-ms-win-crt-*…). Aucune DLL à copier.
- La cible `x86_64-pc-windows-msvc` n'a pas été essayée.

## Lancement en fenêtre réelle (3 lancements chacun)

Méthode : `measure.ps1` (dans ce dossier), lancé par PowerShell 5.1 depuis WSL. `Start-Process -PassThru`, boucle de 50 ms jusqu'à `MainWindowHandle -ne 0`, attente jusqu'à 5 s après le lancement, lecture mémoire, `Stop-Process`. Exe sur le SSD local. Lancements à chaud (le premier de chaque série est le premier lancement de l'exe sur cette session Windows, pas un démarrage après redémarrage).

| Exe | Vivant à 5 s | `Responding` | Fenêtre (ms), 3 lancements | Médiane | Working set à 5 s | Mémoire privée à 5 s |
|---|---|---|---|---:|---:|---:|
| `hello.exe` | 3 sur 3 | oui | 441 / 189 / 157 | 189 ms | 69 Mo | 51 Mo |
| `rocksy_probe.exe` (319 Ko, 12 926 spans) | 3 sur 3 | oui | 252 / 204 / 233 | 233 ms | 125 Mo | 108 Mo |

Titres de fenêtre lus : « Hello » et « Rocksy probe ». 13 threads. CPU cumulé à 5 s : 0,14 à 0,34 s (hello), 0,52 à 0,61 s (sonde), donc pas de boucle active au repos.

Premier commit de la sonde en fenêtre réelle (ligne `rune: commit took` sur stderr) : 371,6 / 356,8 / 364,5 ms, dont 93 % en mise en forme du texte. GPU annoncé : `Intel(R) UHD Graphics`, Dx12.

**Ce que le chiffre « fenêtre » ne dit pas :** `MainWindowHandle` devient non nul quand la fenêtre de premier niveau existe, pas quand le texte est peint. Pour la sonde, le contenu ne peut pas être affiché avant la fin du premier commit (~360 ms) ; l'ordre réel entre création de la fenêtre et premier commit n'a pas été mesuré, donc le délai jusqu'au premier pixel n'est pas connu. Résolution de la boucle : 50 ms.

## Mode headless sous Windows (`--snap`, `--script`)

Fonctionne, code de sortie 0, et passe par le GPU (Dx12), contrairement à l'essai Linux.

| Mesure (319 Ko, release) | Windows (cet essai) | Linux/WSL2 (run de 11:29) |
|---|---:|---:|
| Premier commit avec spans | 310 à 313 ms en headless, 357 à 372 ms en fenêtre | 290 à 410 ms |
| Construction de la vue + spans, 55 passes | 2,4 à 5,1 ms, médiane 3,0 ms | 2,3 à 5,4 ms |
| Commits par frappe (`type60.json`, 54 frappes) au-dessus de 16 / 8 / 4 ms | 0 / 0 / 0 | 0 / 13 / 45 |
| `gpu init` annoncé | 80 à 2 377 ms selon le lancement (2 377 au tout premier, 1 048, puis 80 à 110, un à 608) | sans objet |

Les commits par frappe sont comptés par `RUNE_COMMIT_BUDGET_MS` (4, 8 puis 16) : seul le premier commit dépasse. Ce sont des frappes scriptées en headless, pas une latence touche-écran mesurée en fenêtre.

Le `gpu init` de 2,4 s et 1,0 s des deux premiers lancements headless ne se retrouve pas dans les lancements en fenêtre (fenêtre présente en 157 à 441 ms). Non expliqué ; à remesurer après un redémarrage de Windows.

## Captures (rendu offscreen de Rune, pas de capture du bureau)

- `captures/hello-win.png` : `hello.exe --snap --scale 1`. Libellé « Lights off » et interrupteur.
- `captures/probe-319-win.png` : sonde après `type.json` (`abc`, deux fois Bas, `dé`). Rendu identique à la capture Linux : titres en 22 pt gras, gras en ligne, marqueurs masqués sauf sur la ligne du curseur (en bleu), `é` affiché.
- `captures/probe-319-win-type60.png` : après 54 frappes `x`, toutes insérées.

`logs/` contient les sorties stderr des lancements.

## Non testé

- L'aspect de la fenêtre réelle : aucune capture du bureau, donc aucune preuve visuelle que la fenêtre affiche bien ce que montre `--snap`. Les indices sont indirects (processus vivant, `Responding`, titre, premier commit journalisé).
- Frappe au clavier, souris, IME, presse-papiers, redimensionnement, mise à l'échelle DPI, accessibilité en fenêtre réelle.
- Délai jusqu'au premier pixel, démarrage à froid après redémarrage, vault réel, cible MSVC, binaire sans `tooling` et strippé.
