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
| `src/main.ts` | Ordre de démarrage : `startup_info`, ouverture du fichier (premier `argFiles`, sinon `session.activeTab` ; `StartupInfo.firstFile` évite le `read_file` quand il correspond), `log_timing("editor-ready")`, puis seulement `open_vault` et `build_tags` (lancés ensemble), panneaux et extensions |
| `src/backend.ts` | Interface `Backend`, implémentation Tauri, sélection du backend |
| `src/mock/` | Backend simulé : vault en mémoire, note d'environ 320 Ko, arbre de taille réglable, tags, délais, images fabriquées |
| `src/app.ts` | Coque : volets, onglets, session, vaults, commandes, barre d'état, bandeaux |
| `src/docsync.ts` | Sans DOM, testé : enregistrement (attente de l'enregistrement en cours, nouvelles tentatives après une erreur `io` : 2 s, 5 s, 15 s puis 30 s), rechargement, conflits (`save`, `external`, `deleted`), lecture seule, séquence de fermeture (`close-requested` → `close_ack` → enregistrements attendus → dialogue si besoin → `quit`) |
| `src/eol.ts` | Fins de ligne et BOM : fichier homogène réécrit tel quel ; fichier mixte conservé ligne par ligne (lignes inchangées en tête et en queue : fin d'origine ; lignes modifiées ou ajoutées : fin majoritaire) |
| `src/dialog.ts` | Dialogue de l'application (fermeture avec modifications non enregistrées) ; jamais `window.confirm` |
| `src/vaultevents.ts` | Filtre des événements d'index par vault |
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
| `close` | Fermeture : `close_ack` immédiat, enregistrements attendus avant `quit`, dialogue si échec / conflit / enregistrement automatique désactivé, demandes répétées, fermeture d'onglet pendant un enregistrement, nouvelle tentative automatique |
| `readonly` | Lecture seule (`FileContent.readOnly`), chemin refusé, fichier trop gros, liens et images en chemin réseau |
| `reload` | Frappe pendant un rechargement, fichier supprimé, `tree-updated` sans changement de chemins, événements d'un autre vault, panneau des liens et `linksGeneration`, filtre des tags en cache, écouteurs du canvas, `close_vault`, `firstFile` |
| `security` | Canvas hostile, cellule de tableau pathologique |

App réelle (WebView2 + cœur Rust) : `tools/run-app.sh <scénario>…` joue les scénarios de `tools/app/` (`it-*` pour la 0.1, `is-*` pour la recherche et les liens, `a5-*` pour les correctifs d'audit de la 0.2.1) avec un profil `ROCKSY_DATA_DIR` neuf sous le dossier d'essai ; résultats dans `...\ui\shots-app\`. L'exe joué est par défaut celui de `rocksy-test\wip` (jamais celui de `bin`, qu'Alec utilise). Variables (chemins Windows) : `ROCKSY_EXE` (exe à essayer), `ROCKSY_IT` (dossier d'essai : profils et copies de travail, par défaut `rocksy-test\it`), `ROCKSY_VAULTS` (dossier visé par `T.ref()` : y mettre des copies pour ne jamais ouvrir les vaults de référence). À la fin de chaque scénario, le pilote relève les violations de la politique de sécurité du contenu (`window.rocksyCsp`, aucune attendue ; `"allowCspLog": true` dans un scénario qui injecte volontairement du contenu hostile), les erreurs de console et les requêtes sorties de l'application.

Ce que ces scénarios ne couvrent pas, parce qu'un profil isolé est sa propre instance et que l'ajout de vault y est libre : le second lancement absorbé par l'instance en cours, le refus d'`add_vault` pour un dossier non choisi dans le dialogue (couvert par les tests du cœur et du backend simulé), les dialogues natifs, le glisser-déposer. Les fichiers hors vault qu'un scénario doit modifier sont passés en arguments de ligne de commande (`app.args`) : un fichier voisin ouvert par son chemin est en lecture seule.

Paramètres d'URL du backend simulé : `tree=<entrées>`, `start=session|arg|fresh|novault`, `open=<chemin relatif au vault>`, `delay=<facteur>`, `bigkb=<Ko>`, `searchdelay=<ms>` (délai avant `searchReady`), `firstfile=0` (pas de `StartupInfo.firstFile`). Dans la console : `rocksy` (l'app), `rocksyMock` (journal des appels, `touch(path, texte)` pour simuler une modification externe, `remove(path)` une suppression, `setFile(path, texte, taille?)`, `textOf(path)`, `allowOpened(path)`, `failSaves` (erreurs injectées), `saveDelayMs`, `readDelayMs`, `emit(événement, charge)`, par exemple `emit('close-requested', null)`).

Le backend simulé applique le contrat, refus compris : chemins hors des vaults enregistrés et des fichiers ouverts explicitement (`invalid`), `readOnly`, enregistrement des seuls `.md` / `.markdown`, 50 Mio, `limit` plafonné à 1000, `vault` dans les événements d'index, ordre de `tags_list` identique à celui du cœur.

## Écarts et points ouverts par rapport au contrat

- Liens externes : `open_external(url)` (http, https, mailto) sous Tauri ; `window.open` en mode navigateur seulement.
- Recherche : l'appel `search` part 80 ms après la dernière frappe ; `unresolved_links` est appelé avec `limit: 5000` ; les positions (`line`, `col`, `len`) comptent le BOM, que l'éditeur retire (compensé sur la ligne 0).
- Après `save_file`, l'interface redemande elle-même les liens de la note active si le panneau est affiché (le cœur n'émet pas `index-ready` à ce moment, donc `linksGeneration` n'est pas connu) ; le DOM n'est remplacé que si le résultat diffère. Dans les autres cas (`index-ready`, changement d'onglet), rien n'est redemandé tant que le vault, la note et `linksGeneration` sont ceux déjà affichés.
- Filtre des tags : la liste complète (`tags_list("")`) est mise en cache et filtrée en mémoire avec la règle du cœur (`filterTagLibrary`, test de parité) ; elle est invalidée par `tags-ready`, `index-ready`, un enregistrement et un changement de vault.
- `unresolved_links` est appelé avec `limit: 1000` (plafond du contrat) : dans un vault qui a plus de 1000 cibles non résolues, seules les plus citées sont listées.
- Fins de ligne et BOM : l'interface renvoie à `save_file` le texte avec les fins de ligne d'origine et le BOM s'il y en avait un. Vérifié dans l'app réelle (version 0.2.0) sur des fichiers LF, CRLF, avec BOM, et sans saut de ligne final : seule la ligne modifiée change sur disque. Fichier aux fins de ligne mixtes : conservé tel quel, les lignes modifiées ou ajoutées prennent la fin majoritaire (`src/eol.ts`, tests unitaires ; pas encore vérifié dans l'app réelle).
- `log_timing("editor-ready", ms)` : `ms` est compté depuis `processStartMs`. Deux jalons en plus : `editor-ready-since-navigation`, `editor-painted-since-navigation`, puis `tree-ready`. Jalons fins, en ms depuis la navigation, envoyés en un lot après la première image peinte : `ui-script-start`, `ui-backend-ready`, `ui-startup-info`, `ui-file-read`, `ui-state-created`, `ui-events-wired`, `ui-vault-requested`, `ui-first-frame`, `ui-nav-response-end`, `ui-nav-dom-content-loaded`, `ui-tree-received`, `ui-tree-applied` ; `ui-first-file-prefetched` vaut 1 quand `firstFile` a servi, 0 sinon (ce n'est pas une durée).
