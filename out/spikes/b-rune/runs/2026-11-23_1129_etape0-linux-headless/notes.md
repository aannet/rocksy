# Étape 0 du spike B — essai headless sous Linux (WSL2)

Pas de mesure de démarrage à froid ici (pas de `coldstart.csv`) : c'est un essai de faisabilité, pas un run T1/T2. Aucun accès au vault, document synthétique.

## Commandes

```sh
# sonde copiée dans crates/rune-kit/examples/ d'un clone de anomalyco/rune (rev 653ef67)
cargo build --release -p rune-kit --example rocksy_probe --features rune/tooling
PROBE_KB=319 RUNE_COMMIT_BUDGET_MS=8 rocksy_probe --script type60.json --snap probe-319.png --at 2.0 --scale 1
```

Édition de liens : `libxkbcommon.so` (lien de développement) absent, contourné sans sudo par un lien symbolique vers `libxkbcommon.so.0` et `LIBRARY_PATH`.

## Résultats (CPU, release, headless)

| Mesure | 10 Ko | 319 Ko |
|---|---:|---:|
| Premier commit (mise en forme de tout le document avec ses spans) | 11 ms | 290 à 410 ms |
| Premier commit sans spans (texte brut) | | 18 ms |
| Construction de la vue + 12 926 spans (passe naïve sur tout le document) | 0,15 ms | 2,3 à 5,4 ms |
| Commit par frappe (54 frappes) | | 0 sur 54 au-dessus de 16 ms, 13 au-dessus de 8 ms, 45 au-dessus de 4 ms ; échantillons affichés : 6,9 / 8,3 / 8,6 ms |

Le « frame cpu » headless (p50 ≈ 9 ms) est identique au repos et en frappe : il mesure le rendu logiciel de la capture, pas l'éditeur.

## Captures

- `captures/hello.png` : `examples/hello --snap` (build debug).
- `captures/probe-319.png` : éditeur `TextArea` de 319 Ko après frappe scriptée (`abc`, deux fois Bas, `dé`). Titres en 22 pt gras, `**gras**` en gras, marqueurs réduits à une taille quasi nulle sauf sur la ligne du curseur (ligne 3, marqueurs visibles en bleu).

## Limites de l'essai

- Rien n'a été exécuté sous Windows. `cargo check --target x86_64-pc-windows-msvc` et `-gnu` échouent avant le code Rust, sur les scripts de build de `blake3` et `psm` qui demandent `ml64.exe` ou un GCC MinGW, absents de cette machine.
- Le masquage des marqueurs par `Span::size(0.1)` est un détournement : le curseur traverse encore les caractères masqués (non testé au clavier).
- L'IME, le presse-papiers système, la souris et l'accessibilité n'ont pas été exercés.
