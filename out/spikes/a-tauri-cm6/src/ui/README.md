# Rocksy — interface (spike A, base de la 0.1)

Frontend Vite + TypeScript + CodeMirror 6, sans framework (DOM direct). Il parle au cœur Rust uniquement par `src/backend.ts`, qui suit `../CONTRACT.md`.

## Commandes

```sh
npm install
npm run dev      # navigateur ordinaire, backend simulé
npm run build    # vérification TypeScript puis build dans dist/ (chemins relatifs)
npm test         # tests unitaires (vitest)
tools/run.sh perf scale shell   # scénarios dans Edge headless (voir plus bas)
```

Sous Tauri, l'interface utilise l'API globale (`withGlobalTauri`) : `window.__TAURI__.core.invoke`, `event.listen`, `core.convertFileSrc`. Rien de `@tauri-apps/api` n'est embarqué. Le backend simulé est choisi quand `window.__TAURI_INTERNALS__` n'existe pas ; il vit dans un chunk séparé, jamais chargé dans l'app.

## Architecture

| Fichier | Rôle |
|---|---|
| `src/main.ts` | Ordre de démarrage : `startup_info`, ouverture du fichier (premier `argFiles`, sinon `session.activeTab`), `log_timing("editor-ready")`, puis seulement `open_vault`, `build_tags`, panneaux et extensions |
| `src/backend.ts` | Interface `Backend`, implémentation Tauri, sélection du backend |
| `src/mock/` | Backend simulé : vault en mémoire, note d'environ 320 Ko, arbre de taille réglable, tags, délais, images fabriquées |
| `src/app.ts` | Coque : volets, onglets, enregistrement, conflits, session, vaults, commandes, barre d'état |
| `src/editor/mdlang.ts` | Grammaire : `@lezer/markdown` + GFM + frontmatter, wikilinks, embeds, `==surligné==`, tags |
| `src/editor/livepreview.ts` | Live Preview : décorations en ligne limitées au viewport (plugin de vue) et blocs (tableaux, frontmatter et callouts repliés, Mermaid) dans un champ d'état |
| `src/editor/syntax.ts` | Logique pure du Live Preview (callouts, tableaux, Markdown en ligne des cellules, listes) |
| `src/editor/editor.ts` | Fabrique d'état CM6, raccourcis, coloration |
| `src/editor/extras.ts` | Chargé après le premier rendu : recherche/remplacement, autocomplétion `[[` et `#` |
| `src/editor/langs.ts` | Langages des blocs de code, chacun derrière un import dynamique |
| `src/sidebar.ts` | Arbre virtualisé et panneau des tags (chargé après l'éditeur) |
| `src/searchpanel.ts`, `src/linkspanel.ts` | Recherche plein texte (Ctrl+Maj+F) et liens de la note active (entrants, sortants), dans le même chunk paresseux que le panneau latéral |
| `src/excerpt.ts` | Logique pure : segments surlignés depuis des plages UTF-16, repli casse/accents, analyse de requête, groupement |
| `src/editor/termhl.ts` | Surlignage des termes de la dernière recherche dans la note ouverte |
| `src/picker.ts` | Sélecteur flou : Ctrl+O, Ctrl+P, choix du vault |
| `src/canvas.ts` | Visionneuse `.canvas` en lecture seule |
| `src/resolve.ts`, `fuzzy.ts`, `tagtree.ts`, `paths.ts` | Logique pure testée : liens et images, recherche floue, arbre des tags, chemins |

Choix à connaître :

- La grammaire est construite sur `@lezer/markdown` directement, pas sur `@codemirror/lang-markdown`, pour ne pas embarquer les grammaires HTML/CSS/JS au démarrage. La continuation des listes (Entrée) est donc réécrite ici.
- Un fichier passé en ligne de commande s'ouvre seul : le vault de la session n'est pas chargé (il reste noté dans la session et se charge si l'on active un de ses onglets).
- Les images distantes (`http…`) ne sont pas chargées : aucun appel réseau.
- Le correcteur orthographique du WebView est désactivé dans l'éditeur.
- Préférences d'interface (largeur du panneau, repli des dossiers, enregistrement automatique) dans `localStorage`, jamais dans le vault.

## Raccourcis

Ctrl+S enregistrer · Ctrl+Maj+F rechercher dans le vault · Ctrl+O ouvrir une note · Ctrl+P palette · Ctrl+W fermer l'onglet · Ctrl+Tab onglet suivant · Ctrl+E Live Preview / source · Ctrl+\ deux volets · Ctrl+F rechercher · Ctrl+B / Ctrl+I gras / italique · Ctrl+clic sur un tag : panneau des tags.

## Tests dans Edge headless

Il n'y a pas de navigateur sous WSL. `tools/run.sh <scénario>…` copie `dist/` et `tools/` dans `C:\Users\aleca\rocksy-test\ui\`, puis `tools/drive.py` (Python de Windows, bibliothèque standard) pilote Edge headless par le protocole DevTools : vraies frappes clavier, clics, captures. Les résultats sont dans `C:\Users\aleca\rocksy-test\ui\shots\` (`<scénario>.json` et PNG).

| Scénario | Ce qu'il vérifie |
|---|---|
| `f1`, `f1b` | Live Preview en ligne, curseur dans un élément, sélection, case à cocher, recherche, callouts |
| `f2` | Frontmatter, tableau, code coloré, images, clic dans un tableau |
| `mermaid` | Rendu Mermaid (Pele), erreur de syntaxe, retour au source |
| `edit` | Flèches dans un tableau, continuation de liste, gras, Ctrl+S, lien vers un titre, enregistrement automatique désactivé |
| `shell` | Ctrl+O, Ctrl+P, autocomplétion, conflit, rechargement, `open-files`, clic milieu, deux volets, ajout de vault |
| `scale` | Arbre de ~19 000 entrées, Ctrl+O, panneau des tags, fragment surligné |
| `misc` | Fichier hors vault, canvas, premier lancement, mode source |
| `search` | Recherche : état « en préparation », résultats, navigation clavier, ouverture à la ligne, syntaxe, latence |
| `links` | Liens entrants et sortants, mise à jour après enregistrement, image, fichier hors vault, liens non résolus |
| `perf` | Note de ~320 Ko : démarrage et latence de frappe à quatre endroits |

App réelle (WebView2 + cœur Rust) : `tools/run-app.sh <scénario>…` joue les scénarios de `tools/app/` (`it-*` pour la 0.1, `is-*` pour la recherche et les liens) avec un profil `ROCKSY_DATA_DIR` neuf sous `C:\Users\aleca\rocksy-test\it\` ; résultats dans `...\ui\shots-app\`.

Paramètres d'URL du backend simulé : `tree=<entrées>`, `start=session|arg|fresh|novault`, `open=<chemin relatif au vault>`, `delay=<facteur>`, `bigkb=<Ko>`, `searchdelay=<ms>` (délai avant `searchReady`). Dans la console : `rocksy` (l'app), `rocksyMock` (journal des appels, `touch(path, texte)` pour simuler une modification externe, `emit(événement, charge)`).

## Écarts et points ouverts par rapport au contrat

- Liens externes : `open_external(url)` (http, https, mailto) sous Tauri ; `window.open` en mode navigateur seulement.
- Recherche : l'appel `search` part 80 ms après la dernière frappe ; `unresolved_links` est appelé avec `limit: 5000` ; les positions (`line`, `col`, `len`) comptent le BOM, que l'éditeur retire (compensé sur la ligne 0).
- Après `save_file`, l'interface rafraîchit elle-même le panneau des liens : le cœur n'émet pas `index-ready` à ce moment.
- Fins de ligne et BOM : l'interface renvoie à `save_file` le texte avec les fins de ligne d'origine (`\r\n` si le fichier en contenait) et le BOM s'il y en avait un. Vérifié dans l'app réelle sur des fichiers LF, CRLF, avec BOM, et sans saut de ligne final : seule la ligne modifiée change sur disque.
- `log_timing("editor-ready", ms)` : `ms` est compté depuis `processStartMs`. Deux jalons en plus : `editor-ready-since-navigation`, `editor-painted-since-navigation`, puis `tree-ready`.
