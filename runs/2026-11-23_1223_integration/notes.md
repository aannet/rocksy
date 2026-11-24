# Intégration de l'interface dans l'app réelle — 23/11/2026

Tout ce qui suit a été observé dans `rocksy.exe` (WebView2 + cœur Rust réel), piloté par le protocole DevTools. Aucune capture du bureau : uniquement des captures de page (`captures/`). Les valeurs brutes de chaque scénario sont dans `resultats/<scénario>.json`.

## Conditions

- Windows 11, WebView2 (Edg/157), fenêtre de 1280 × 820 px CSS, facteur d'échelle 2.
- Vaults synthétiques sur C: (disque local), cache chaud. Pas de Drive, pas de vault réel.
- Un `ROCKSY_DATA_DIR` neuf par scénario. Depuis le correctif du backend, le profil WebView2 est sous `<ROCKSY_DATA_DIR>\webview` (isolation vérifiée : un réglage changé dans un dossier n'apparaît pas dans un dossier neuf).
- Les frappes sont de vrais événements clavier (`Input.dispatchKeyEvent`). La latence va du `keydown` à la fin de la mise à jour CodeMirror ; « avec mise en page » ajoute une mise en page forcée.

## Démarrage (note de 318 Ko passée en argument, hors vault)

| Jalon | Valeur |
|---|---|
| `editor-ready` depuis le début du processus | 686 à 850 ms sur 5 lancements (794 et 821 ms après le correctif du profil WebView) |
| `editor-ready` depuis la navigation | 235 à 291 ms |
| Éditeur peint depuis la navigation | 289 à 377 ms |

## Latence de frappe, note de 318 Ko (309 938 caractères, 4 372 lignes), Live Preview actif

| Endroit | Frappes | Médiane | p95 | Max |
|---|---|---|---|---|
| Début (ligne 40, liste de liens) | 216 | 2,4 ms | 3,9 ms | 9,5 ms |
| Milieu (ligne 2 185, paragraphe) | 216 | 2,9 ms | 5,0 ms | 7,0 ms |
| Fin (ligne 4 371) | 216 | 2,5 ms | 4,2 ms | 6,6 ms |
| Dans une tâche imbriquée, avec gras, tag, wikilink, code | 215 | 2,2 ms | 3,8 ms | 17 ms |

Avec mise en page forcée : p95 de 4,0 à 5,3 ms. Premier passage (build précédent) : p95 de 3,5 à 4,3 ms.

## Défilement de la même note

Défilement programmé, une étape par image : 120 px par image sur 300 images, puis 600 px par image sur 150 images. Intervalle médian 16,7 ms, p95 16,8 ms ; une seule image à 33,8 ms sur 448 (aucune au premier passage).

## Enregistrement (copie `it\v1`)

Une ligne modifiée puis Ctrl+S, comparaison des octets avant/après :

| Fichier | Résultat sur disque |
|---|---|
| LF (10 666 octets, 121 LF) | 1 ligne changée, 121 LF, fin identique |
| CRLF (121 CRLF) | 1 ligne changée, 121 CRLF, 0 LF isolé |
| LF + BOM | 1 ligne changée, BOM conservé |
| CRLF + BOM, sans saut de ligne final | 1 ligne changée, BOM conservé, toujours sans saut final |

- Ctrl+S : fichier enregistré en 31 à 34 ms (frappe → état « Enregistré »).
- Enregistrement automatique : 2 018 ms après la dernière frappe.
- Annulation : Ctrl+Z annule la saisie par groupes, Ctrl+Y la rétablit ; huit Ctrl+Z ramènent le texte d'origine (10 367 caractères), y compris par-dessus des enregistrements.
- Modification externe d'un onglet propre : rechargé 1 966 ms après l'écriture (scrutation du backend toutes les 2 s).
- Modification externe d'un onglet modifié : bandeau après 1 978 ms ; « Recharger » reprend la version du disque.
- Ctrl+S alors que le disque a changé : refus `conflict` en 16 ms, bandeau ; « Écraser » enregistre.
- Fichier supprimé pendant qu'il est ouvert : rien n'est signalé tant qu'on n'enregistre pas ; Ctrl+S donne le bandeau de conflit ; « Écraser » recrée le fichier.

## F7 (copie `it\v7`, 4 658 fichiers, dossiers à point compris)

- Session en lecture (vault ajouté, arbre, tags, 5 notes, 2 canvas, panneau des tags, Ctrl+O) : 0 fichier ajouté, 0 supprimé, 0 modifié.
- Puis une note modifiée et enregistrée automatiquement : 1 seul fichier diffère (`00_INBOX\Format bilan 00103.md`), aucun fichier ajouté.
- Dossier de données : `session.json`, `timings.jsonl`, `trees`, `vaults.json` (puis `webview` depuis le correctif).

## F8

- Fichier hors vault en argument : pas d'arbre, pas de vault, image relative affichée (32 × 32 réels), dossier du fichier identique avant et après.
- Second lancement avec un autre fichier : onglet actif 42 ms après le lancement du second processus, qui se termine. Troisième lancement : 37 ms.
- Fichier dans un dossier contenant `.obsidian\` non référencé : ouvert isolé, `.obsidian\` inchangé.

## Vault x20 (20 747 entrées, 9 760 notes)

| Mesure | Valeur |
|---|---|
| `add_vault` + `open_vault` + arbre affiché | 234 ms (premier passage, sans instantané) |
| Tags prêts (4 199 tags, 46 208 occurrences) | 347 ms d'indexation |
| Arbre entièrement déplié : 20 747 lignes | 69 lignes dans le DOM, rendu médian 5,1 ms, p95 7,7 ms |
| Ctrl+O sur 18 544 fichiers | filtrage 1 à 5,5 ms, rendu de la liste 12 ms |
| Autocomplétion `[[` | 3,3 ms pour 30 propositions |
| Filtre des tags, par frappe (appel + arbre + rendu) | 15 à 43 ms après correction (avant : jusqu'à 112 ms) ; l'appel seul prend 5 à 11 ms |
| Fragments d'un tag (500 affichés) | 56 à 91 ms |
| Clic sur un fragment → note ouverte et surlignée | 38 ms |

Fragments vérifiés un par un contre le texte du fichier (9 au total) : élément de liste avec ses deux sous-éléments (lignes 96 à 98), sous-élément seul, deux paragraphes, deux titres, deux callouts entiers (3 et 4 lignes), une ligne de tableau. `tagCol` tombe sur le `#` dans les 8 cas relevés. Dans les fichiers CRLF, BOM et BOM+CRLF, `file_tags` donne les mêmes lignes que dans le fichier LF, et le fragment ouvert dans le fichier CRLF surligne bien l'élément et son sous-élément.

## Canvas (copie `it\v1`)

| Canvas | Ouverture → nœuds affichés | Contenu |
|---|---|---|
| Simple | 106 ms | 6 nœuds |
| Dense | 83 ms | 150 cartes + 6 groupes, 200 liens, 17 images sur 17 chargées, 40 extraits de notes |

Zoom et déplacement fonctionnent. Un pas de zoom sur le canvas dense prenait 110 à 155 ms ; après correction (calque composité pendant le geste), 25 à 33 ms, avec deux pas à 209 et 267 ms sur huit. Ctrl+S ne fait rien, le fichier `.canvas` est identique octet pour octet.

## Mermaid (21 blocs dans x1, rendus par Pele)

Les 9 types se rendent, 21 blocs sur 21, aucune erreur : flowchart, sequenceDiagram, classDiagram, stateDiagram-v2, erDiagram, gantt, pie, mindmap, wardley-beta. Premier rendu d'un type (chargement de son module compris) : 4,9 à 31,9 ms. Rendus suivants : 0,1 à 1,7 ms. Un bloc de chaque type a été vu dans l'app (captures `it-mermaid-*.png`).

## Robustesse

- Fichier Latin-1 : message lisible, en anglais, venu du backend (« is not valid UTF-8 (first invalid byte at offset 5) »).
- Fichier vide : s'ouvre, s'enregistre (3 octets écrits, sans saut de ligne ajouté).
- Chemin avec accents, espaces, apostrophe et `&` : s'ouvre.
- Ligne unique de 20 000 caractères avec 2 800 éléments balisés : 310 ms par frappe avant correction, 20 ms (p95 27 ms) après ; la ligne reste alors en style source.
- Ligne unique de 216 000 caractères : 391 ms par frappe avant, 65 ms (p95 81 ms) après.

## Liens externes

`open_external` refuse `file:///C:/x` et `javascript:` avec `invalid`. Un clic sur un lien appelle la commande avec `https://example.org` (commande remplacée par une sonde pendant le test). L'ouverture réelle d'un site n'a pas été faite.

## Corrections faites dans l'interface pendant cette campagne

1. Liens externes : `open_external` à la place de `window.open` sous Tauri.
2. Arbre des tags : construction 2 à 3 fois plus rapide (collation partagée, pas de tri de chaînes).
3. Canvas : calque composité pendant le zoom et le déplacement.
4. Lignes de plus de 6 000 caractères : laissées en style source.
5. Écran d'accueil : bouton « Ouvrir un fichier… » à côté de « Ajouter un vault ».

## Non vérifié

- Glisser-déposer d'un fichier.
- Dialogues natifs `pick_folder` et `pick_file` (le vault a été ajouté par `add_vault` puis ouvert ; les boutons existent mais n'ont pas été cliqués).
- Ouverture effective d'un lien dans le navigateur.
- Vault réel sur Google Drive, cache froid, IME, lecteur d'écran.
- Restauration de la disposition en deux volets (la session est une liste plate d'onglets).
