# Spike A — Tauri 2 + CodeMirror 6

Statut (23/11/2026) : l'app se construit, démarre et passe les cas F1 à F9 sur les vaults synthétiques (`grille.md` ; F5 et F6 partiels). Rien n'a encore été mesuré sur le vault réel ni à froid.

Cahier des charges : `../../docs/m2-cahier-des-charges-spikes.md`. Contrat entre le cœur et l'interface : `src/CONTRACT.md`.

## Arborescence

- `src/crates/rocksy-core/` : cœur Rust sans dépendance d'interface.
- `src/src-tauri/` : enveloppe Tauri 2 des commandes.
- `src/ui/` : interface Vite + TypeScript + CodeMirror 6 (voir `src/ui/README.md`).

## Construire

Depuis WSL, compilation croisée pour Windows (UI puis exe, environ une minute) :

```sh
out/tools/build-win.sh            # ou --skip-ui
```

Le script dépose `rocksy.exe` et `WebView2Loader.dll` dans `C:\Users\aleca\rocksy-test\bin\`. Aucun `rocksy.exe` ne doit tourner pendant le build.

Interface seule : `cd src/ui && npm run build` (vérification TypeScript comprise) et `npm test`.

## Lancer

```powershell
C:\Users\aleca\rocksy-test\bin\rocksy.exe                 # dernière session
C:\Users\aleca\rocksy-test\bin\rocksy.exe "C:\chemin\note.md"   # fichier seul, sans charger le vault
```

Pour un essai sans toucher au profil réel, définir `ROCKSY_DATA_DIR` : la liste des vaults, la session, les instantanés d'arbre et le profil WebView2 (`webview\`) vont alors dans ce dossier.

## Tester

- Interface contre le backend simulé, dans Edge headless : `src/ui/tools/run.sh perf scale shell …` (scénarios dans `src/ui/tools/scenarios/`).
- App réelle : `src/ui/tools/run-app.sh it-preview it-perf it-save it-f7 it-f8 it-canvas it-mermaid it-x20 it-final` (scénarios dans `src/ui/tools/app/`). Le pilote lance `rocksy.exe` avec un `ROCKSY_DATA_DIR` neuf sous `C:\Users\aleca\rocksy-test\it\`, s'attache au port de débogage de WebView2 (9223), joue de vraies frappes et de vrais clics, prend des captures de page, compare des octets et des empreintes sur disque, puis tue le processus. Résultats dans `C:\Users\aleca\rocksy-test\ui\shots-app\`.
- Les tests qui écrivent travaillent sur des copies (`it\v1`, `it\v7`), jamais sur `vaults\x1` ni `vaults\x20`.

## Mesures

Les sessions de mesure sont dans `runs/`, un dossier par session (`AAAA-MM-JJ_HHMM_<cas>/`). La campagne d'intégration est dans `runs/2026-11-23_1223_integration/` (`notes.md` pour les chiffres et les conditions). Le tableau des cas F1 à F9 est dans `grille.md`.

## Reste à faire

- Mesures sur le vault réel (Drive, cache froid et chaud) avec `out/tools/measure-coldstart.ps1`.
- Glisser-déposer, dialogues natifs, ouverture effective d'un lien externe : à essayer à la main.
- Restauration de la disposition en volets ; rendu Markdown des cartes de notes dans les canvas.
