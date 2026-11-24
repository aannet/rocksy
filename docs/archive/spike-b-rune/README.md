# Spike B — Rune (natif)

Statut : étape 0 faite le 2026-11-23 (verdict ci-dessous), complétée le même jour par un essai sous Windows. Spike complet pas commencé.

Cahier des charges : `../../docs/m2-cahier-des-charges-spikes.md` (cas F1 à F9, KPI, tâches).

## Étape 0 — verdict

**PASSE SOUS CONDITIONS.** Rune fournit déjà un éditeur de texte riche multi-ligne et un Live Preview « en ligne » (styles mélangés, marqueurs masqués hors curseur) tient sur 319 Ko à moins de 16 ms par frappe, mais les widgets dans le flux du texte (image, tableau) n'existent pas : il faut les construire par blocs. Sous Windows, `hello` et la sonde compilent (cible `gnullvm`), s'ouvrent et restent vivants, et le mode `--snap` rend la même image que sous Linux (voir « Essai sous Windows ») ; la frappe, l'IME et l'aspect de la fenêtre réelle n'y sont pas encore vérifiés. **B n'entre pas dans la release 0.1 d'aujourd'hui**, et n'est pas éliminé pour la suite.

Dépôt examiné : `github.com/anomalyco/rune`, public, branche `dev`, rev `653ef67` du 2026-11-23, cloné hors dépôt dans `~/rocksy-scratch/rune/`. Les chemins ci-dessous sont relatifs à ce clone.

Niveaux de preuve : **exécuté** (compilé et lancé ici, sous Linux/WSL2 sauf mention « sous Windows »), **lu** (code ou documentation du dépôt, non exécuté), **déduit**.

### 1. État du projet

| Point | Constat | Preuve |
|---|---|---|
| Activité | 200 commits sur les seules journées des 22 et 23 novembre 2026, numéros de PR vers 1650, dernier commit le jour même | exécuté (`git log`) |
| Version, maturité | `0.0.1`, aucun tag, « Rune is pre-release. Pin a rev of `dev` » | lu (`README.md`, `crates/rune/Cargo.toml:3`) |
| Licence | MIT, © 2026 Anomaly | lu (`LICENSE`) |
| Plateformes | macOS, Windows, Linux, web annoncés. Code Windows présent : `crates/rune/src/platform/win32/` (18 fichiers), IME dans `platform/ime/windows.rs`, instance unique par tube nommé dans `open_url/instance/windows.rs`, correctifs DX12 dans `vendor/wgpu-hal.patch` | lu pour le code ; **exécuté sous Windows 11** pour `hello` et la sonde : compilation croisée `x86_64-pc-windows-gnullvm`, fenêtre ouverte, rendu Dx12 (voir « Essai sous Windows »). Cible MSVC, macOS et web non essayés |
| Rendu | `wgpu` 30 (DX12 sous Windows), `winit` 0.30.13, `parley` 0.12 pour le texte, `taffy` 0.15 pour la mise en page. Pas de `vello`. Quatre dépendances sont des copies modifiées dans `vendor/` (wgpu-hal, winit, parley_engine, accesskit) et une application qui utilise Rune doit reprendre les mêmes `[patch.crates-io]` | lu (`Cargo.toml`) |
| Contributions | « We don't take pull requests » : uniquement des issues, traitées par leurs agents | lu (`README.md`) |
| Toolchain | épingle Rust 1.99.1 ; compile aussi avec la 1.100 installée | exécuté |

### 2. Texte

| Point | Constat | Preuve |
|---|---|---|
| Éditeur multi-ligne | Oui : `text_input` et `rune_kit::TextArea` sur un modèle `TextEdit` (crate `rune-edit`) : curseur, sélection, déplacement par mot et par ligne, copier/couper/coller, annuler/rétablir | lu (`crates/rune/README.md:455`, `:1295`) ; frappe et touches Bas **exécutées** en headless |
| IME | Préédition par winit, complétée sous Windows par une sous-classe de la fenêtre (clauses, reconversion) | lu (`crates/rune/src/platform/ime/windows.rs:1`). Un `é` tapé par script passe (exécuté), ce qui ne teste pas l'IME |
| Styles mélangés dans une ligne | Oui : `Span` porte graisse, taille, couleur, fond, soulignement, italique, mono, par plage d'octets ; `.decorations(spans)` les applique à un éditeur à chaque frame sans toucher au texte | lu (`crates/rune-text/src/layout/span.rs:12`, `crates/rune/README.md:1468`) ; **exécuté** (capture) |
| Texte riche éditable | Marques (`Bold`, `Italic`, `Strike`, `Code`, `Link`), blocs par paragraphe (puce, numéro, citation, code), export et collage HTML | lu (`crates/rune/README.md:1409`) |
| Masquage des marqueurs | Pas de primitive. Contournement qui fonctionne à l'affichage : `Span::size(0.1)` sur le marqueur, sauf sur la ligne du curseur | **exécuté** (capture). Navigation clavier à travers les caractères masqués non testée |
| Widgets dans le flux du texte | Non. Les seules boîtes en ligne sont les « pastilles » de mention (`Mark::Pill`), un libellé texte avec icône. Pas d'image ni de tableau dans un champ | lu (`crates/rune-text/src/layout/shape.rs:379`, seul appel à `push_inline_box`) |
| Document de 319 Ko | Texte stocké en `String` (pas de rope). Pas de virtualisation du champ, mais seules les lignes visibles sont peintes et une frappe ne remet en forme que le paragraphe modifié | lu (`crates/rune-text/src/paragraphs.rs:1`) ; **exécuté** : voir mesures |
| Annulation sur gros document | Historique limité à 128 étapes et 1 Mio, chaque étape copiant le texte entier : environ une étape à 319 Ko | lu (`crates/rune/README.md:1298`, `research/27-long-note-editing.md`). Non vérifié à l'exécution |
| Conseil du projet lui-même | Au-delà de 10 Ko, un champ par bloc Markdown ; déplacement, sélection et annulation entre blocs à la charge de l'application ; un champ focalisé dans une pile virtualisée posait problème | lu (`research/27-long-note-editing.md`, section « Advice for a notes app »). Les chiffres de frappe de cette note (46 ms à 200 Ko) sont dépassés par nos mesures |

Mesures de la sonde (`src/etape0/rocksy_probe.rs`, document synthétique de 319 Ko, 12 926 spans recalculés sur tout le document à chaque commit, release, headless, i7-10750H sous WSL2) :

| Mesure | Résultat |
|---|---:|
| Premier commit (mise en forme complète avec spans) | 290 à 410 ms |
| Premier commit en texte brut, sans spans | 18 ms |
| Commit par frappe, 54 frappes | aucune au-dessus de 16 ms, 13 au-dessus de 8 ms |

Détail et capture : `runs/2026-11-23_1129_etape0-linux-headless/` (`captures/probe-319.png` montre titres en 22 pt gras, gras en ligne, marqueurs masqués sauf sur la ligne du curseur).

Le premier commit de 290 à 410 ms est à surveiller : il entame le budget T1 de 1 s sur la note de 319 Ko.

### 3. Construire un Live Preview : ce qui manque

Déjà fourni par Rune, donc à ne pas écrire : buffer et curseur, sélection, IME, presse-papiers, annulation, coloration de code (`rune_kit::syntax`, 80 langages), rendu Markdown en lecture avec tableaux GFM, images et Mermaid (`rune_kit::Markdown`), arbre de fichiers virtualisé (`FileTree`, 50 000 fichiers annoncés à 0,4 ms par frame), onglets et volets (`TabStrip`, `SplitView`).

À écrire :

1. Analyse Markdown incrémentale produisant les spans et l'état « marqueur visible ou masqué » selon le curseur. Estimation : 1 à 2 jours pour F1.
2. Vrai masquage des marqueurs (le curseur doit sauter les caractères masqués, la sélection et le clic doivent rester justes). Soit au-dessus de `TextEdit`, soit demandé en amont par une issue. 1 à 2 jours, incertain.
3. Architecture par blocs pour les widgets : le document devient une liste virtualisée de blocs, chacun étant un champ de texte ou un rendu (tableau, image, frontmatter replié, Mermaid) qui redevient texte quand le curseur y entre. Il faut alors écrire le curseur, la sélection et l'annulation qui traversent les blocs. C'est le gros du coût : 3 à 6 jours, avec le risque signalé par le projet sur les champs dans une pile virtualisée.
4. Historique d'annulation par différences pour les gros documents : 0,5 à 1 jour, ou attendre la correction prévue en amont.
5. Visionneuse de canvas (F6) avec `canvas_input` et les formes vectorielles : 1 à 2 jours.

Total pour F1 à F3 et F6 : **6 à 12 jours**, cohérent avec les 7 à 10 jours du cahier des charges. F1 seul est à portée en 1 à 2 jours.

### 4. Mods, headless, accessibilité

| Point | Constat | Preuve |
|---|---|---|
| Scripting | Crate `rune-luau` : une VM Luau isolée par script, `render(view)` construit des éléments natifs, rechargement à chaud avec report d'état, carte d'erreur au lieu d'un plantage, limite de 250 ms CPU et 64 Mo. Proche du modèle Herdr, mais en Luau et non en JS | lu (`crates/rune-luau/src/lib.rs:1`, `examples/luau-panel`). Non compilé (build C++ de Luau) |
| Headless | `--snap`, `--script` (souris, clavier, texte), `--tree` (arbre des éléments en JSON), `--video`. Toujours présent en debug, feature `tooling` en release | **exécuté** : deux captures PNG et frappe scriptée |
| Accessibilité | `accesskit` 0.25 avec adaptateurs Windows, macOS et Unix, copies modifiées pour les titres et les champs de plus de 64 Kio | lu (`crates/rune/Cargo.toml:76`, `crates/rune/src/a11y/text.rs:21`). Non testé avec un lecteur d'écran |

### 5. Essai pratique

- `cargo build -p hello` : compile (environ 5 min à froid). L'édition de liens demande `libxkbcommon.so`, absent ; contourné sans sudo par un lien symbolique vers `libxkbcommon.so.0`.
- `hello --snap` et la sonde `rocksy_probe` : captures produites sans serveur d'affichage.
- `cargo check --target x86_64-pc-windows-msvc` et `-gnu` : **échec avant le code Rust**, sur les scripts de build de `blake3` et `psm` qui demandent `ml64.exe` ou un GCC MinGW. La cible MSVC reste non vérifiée. La cible `x86_64-pc-windows-gnullvm` compile, elle : voir ci-dessous.

### 5 bis. Essai sous Windows

Fait le 2026-11-23 à 11:41, après le reste de l'étape 0. Détail, journaux et captures : `runs/2026-11-23_1141_etape0-windows/`. Tout ce qui suit est **exécuté** sur la machine Windows 11 (i7-10750H, GPU Intel UHD, Dx12), exe sur le SSD local, document synthétique de 319 Ko généré en mémoire par la sonde.

- **Compilation** : croisée depuis WSL2 avec llvm-mingw, cible `x86_64-pc-windows-gnullvm`, rustc stable 1.100. `hello` puis `rocksy_probe` compilent du premier coup, sans contournement ni modification de Rune ; `blake3` et `psm` passent avec ce clang. Exe de 29 à 31 Mo (feature `tooling`, non strippés), qui ne dépendent que de DLL système.
- **Lancement en fenêtre**, 3 lancements chacun, à chaud :

| Exe | Vivant à 5 s | Fenêtre de premier niveau présente (médiane ; 3 valeurs) | Working set à 5 s | Mémoire privée à 5 s |
|---|---|---:|---:|---:|
| `hello.exe` | 3 sur 3 | 189 ms (441 / 189 / 157) | 69 Mo | 51 Mo |
| `rocksy_probe.exe`, 319 Ko | 3 sur 3 | 233 ms (252 / 204 / 233) | 125 Mo | 108 Mo |

- Le délai « fenêtre » est celui où `MainWindowHandle` devient non nul (boucle de 50 ms), pas celui du premier pixel. Le premier commit de la sonde en fenêtre prend 357 à 372 ms (journal de Rune), donc le texte n'est pas affiché avant ; le délai jusqu'au premier pixel n'est pas mesuré.
- **Headless** : `--snap` et `--script` fonctionnent sous Windows et passent par le GPU. `captures/probe-319-win.png` est identique à la capture Linux (titres 22 pt, gras en ligne, marqueurs masqués hors ligne du curseur, `é` affiché). Premier commit 310 à 313 ms. Sur 54 frappes scriptées, aucun commit au-dessus de 4 ms (sous WSL2 : 45 au-dessus de 4 ms, 13 au-dessus de 8 ms).
- Anomalie non expliquée : `gpu init` de 2,4 s puis 1,0 s aux deux premiers lancements headless, 80 à 110 ms ensuite (un à 608 ms). Les lancements en fenêtre n'en montrent pas trace. À remesurer après un redémarrage de Windows.
- **Non vérifié sous Windows** : l'aspect de la fenêtre réelle (aucune capture du bureau), la frappe au clavier, la souris, l'IME, le presse-papiers, le DPI, l'accessibilité, le démarrage à froid, le vault réel, la cible MSVC.

### 6. Mermaid côté natif

Rune embarque `crates/rune-mermaid`, un moteur Mermaid écrit en Rust et dessiné avec ses formes vectorielles, sans JavaScript : organigrammes, séquences, états, classes, ER, gantt, camembert, mindmap, timeline, gitgraph, sankey, C4 et d'autres d'après son en-tête (lu, non exécuté ; fidélité à Mermaid 12.1 non évaluée). Il est branché sur `rune_kit::Markdown` par la feature `mermaid`. Les deux autres voies restent possibles : embarquer un moteur JS (QuickJS, Boa) pour faire tourner Pele et rastériser son SVG avec `resvg`, déjà dépendance de Rune, ce qui donne la parité avec Obsidian 1.15 au prix d'un moteur JS dans le binaire ; ou les crates `mermaid-rs-renderer` / `merman`, sans intérêt évident face à `rune-mermaid`. Les erreurs avec ligne et colonne de Pele n'ont pas d'équivalent vérifié dans `rune-mermaid`.

### Recommandation

**Aujourd'hui : release 0.1 sur la pile A (Tauri 2 + CodeMirror 6).** Deux raisons restent après l'essai sous Windows (la troisième, l'absence de preuve d'exécution sous Windows, est levée pour la compilation, l'ouverture de la fenêtre et le rendu, pas pour la frappe ni l'IME) : F2 et F3 demandent l'architecture par blocs, soit plusieurs jours ; l'API change de plus de 100 commits par jour sans version stable.

**Plus tard : faire le vrai spike B.** L'étape 0 lève le doute principal (l'éditeur existe et tient la charge). Ordre proposé :

1. Sur la machine Windows : compilation et ouverture déjà faites (« Essai sous Windows »). Reste à vérifier à l'œil la fenêtre, la police, les accents et l'IME, et à mesurer T1 jusqu'au premier pixel sur la note de 319 Ko. Une à deux heures.
2. F1 complet dans un seul champ (1 à 2 jours), avec mesure de la latence de frappe en fenêtre réelle.
3. Prototype de l'architecture par blocs sur un tableau et une image (F2, F3). C'est le point qui décide.

Risques à garder en tête : dépendance à un projet pré-release tenu par une seule équipe qui n'accepte pas de PR, quatre dépendances patchées à reprendre, mods en Luau alors que le modèle Herdr visé est en JS.

## Construire et lancer

Sonde de l'étape 0 (Linux, headless) : copier `src/etape0/rocksy_probe.rs` dans `crates/rune-kit/examples/` d'un clone de Rune, puis

```sh
cargo build --release -p rune-kit --example rocksy_probe --features rune/tooling
PROBE_KB=319 RUNE_COMMIT_BUDGET_MS=8 <target>/release/examples/rocksy_probe \
  --script src/etape0/type60.json --snap probe.png --at 2.0 --scale 1
```

Build pour Windows (compilation croisée depuis WSL2, llvm-mingw dans `~/rocksy-toolchain/llvm-mingw/`) :

```sh
export PATH=$HOME/.cargo/bin:$HOME/rocksy-toolchain/llvm-mingw/bin:$PATH
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER=x86_64-w64-mingw32-clang
export CC_x86_64_pc_windows_gnullvm=x86_64-w64-mingw32-clang AR_x86_64_pc_windows_gnullvm=llvm-ar
export RUSTUP_TOOLCHAIN=stable   # la 1.99.1 épinglée n'a pas la cible gnullvm
cargo build --release --target x86_64-pc-windows-gnullvm -p rune-kit --example rocksy_probe --features rune/tooling
```

L'exe ne demande aucune DLL hors système. Mesure de lancement : `runs/2026-11-23_1141_etape0-windows/measure.ps1`.

## Mesures

Les sessions de mesure sont dans `runs/`, un dossier par session (`AAAA-MM-JJ_HHMM_<cas>/`).
Le tableau des cas F1 à F9 est dans `grille.md`.
