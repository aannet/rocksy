# M2 — Cahier des charges des deux spikes

Version 2, 24/11/2026 (v1 du 23/11). Mise à jour d'après `out/docs/perimetre-fonctionnel.md` : Live Preview indispensable (critère éliminatoire), canvas en lecture seule, mobile hors v1. Les durées sont des estimations à ajuster avec ton temps disponible (question ouverte du brief).

## 1. Objectif

Choisir la pile d'interface de Rocksy avec des mesures, pas des impressions. Deux spikes de même périmètre, mesurés avec les mêmes scripts sur le même vault :

| | Spike A | Spike B |
|---|---|---|
| Pile | Tauri 2 + CodeMirror 6 (WebView WebView2) | Rune (rendu GPU natif, sans WebView) |
| Éditeur | CM6 existant | À construire (buffer, curseur, sélection, IME, rendu Markdown) |
| Mods | JS/DOM possible | Scripting natif (Luau ou autre, à trancher en M4) |
| Mermaid | Pele directement (JS → SVG dans le DOM) | Pele dans un moteur JS embarqué, ou moteur Rust (voir §6) |

Contexte : la compatibilité avec les plugins Obsidian est devenue secondaire (décision du 23/11/2026), ce qui rend le natif viable.

Décisions du 24/11/2026 qui cadrent les spikes (mise à jour : tags et ouverture hors vault, voir F8 et F9) :
- **Live Preview indispensable** dès la v1 : c'est un critère éliminatoire (§7), pas une option mesurée parmi d'autres.
- **Canvas en lecture seule** en v1 : les spikes testent l'affichage, jamais l'écriture d'un `.canvas`.
- **Ouvrir n'importe quel `.md`, même hors vault** : chaque spike doit accepter un chemin en argument et fonctionner sans vault. Instance unique, double-clic et ligne de commande (voir F8).
- **Tags puissants, sur des fragments, recherche instantanée** : voir F9 ; l'index complet reste en M3.
- **Mobile hors v1**, conservé pour plus tard : aucune exigence mobile dans les spikes. Seule contrainte : garder le cœur Rust (accès vault, snapshot, futur index) indépendant de l'interface, donc portable.

## 2. Périmètre commun

**Données :**
- Vault réel (`G:\Mon Drive\MonVault`) en lecture seule, pour la mesure de référence.
- Vaults synthétiques `gen-vault.py` en 1x et 20x (copie locale, hors Drive) pour la tenue à l'échelle.

**Machine :** Windows 11, i7-10750H, NVMe, 31,8 Go (la même que l'audit).

**Chaque spike doit :**
1. Lire le vault sans rien écrire dans `.obsidian/` ni dans les notes.
2. Afficher la note active (`01__WORK/daily-notes.md`, 319 Ko) lue depuis `workspace.json`, sans attendre l'index.
3. Afficher l'arbre des fichiers depuis un snapshot persisté (dossiers à point ignorés), puis réconcilier en arrière-plan.
4. Permettre d'éditer et d'enregistrer cette note (écriture atomique, relecture sans diff dans Obsidian).
5. Offrir un **Live Preview** de cette note (voir « Cas fonctionnels » ci-dessous).
6. Afficher un fichier `.canvas` du vault en **lecture seule** (nœuds texte, notes, images et liens, déplacement et zoom de la vue, aucune écriture).

**Cas fonctionnels communs (issus du périmètre)** : chaque spike est évalué sur la même liste, notée ✅ / ⚠️ partiel / ❌ :

| # | Cas | Où |
|---|---|---|
| F1 | Live Preview : titres, gras/italique, liens `[[ ]]`, tâches `- [ ]`, listes, code inline, citations. Le marqueur Markdown réapparaît quand le curseur entre dans l'élément | Note de 319 Ko |
| F2 | Live Preview : frontmatter (repliable), blocs de code avec coloration, tableau Markdown rendu | Note de test |
| F3 | Embed d'image `![[...]]` rendu dans la note (PNG du dossier `_ASSETS`) | Note de test |
| F4 | Wikilink : autocomplétion `[[` sur la liste des fichiers, ouverture au clic | Vault réel |
| F5 | Onglets, deux splits côte à côte, restauration du workspace depuis `workspace.json` | Vault réel |
| F6 | Canvas en lecture seule : rendu fidèle d'un `.canvas` du vault, zoom et déplacement | 1 canvas simple et 1 canvas dense |
| F7 | Aucune écriture dans un `.canvas` ni dans `.obsidian/` (vérifié par hash avant/après) | Vault copie |
| F8 | **Ouverture d'un `.md` hors vault** : lancement avec le chemin en argument, éditeur complet, aucune création de fichier à côté, image relative rendue. T1 mesuré sur ce chemin (sans snapshot ni index). **Instance unique** : un second lancement avec un autre chemin ouvre un onglet dans la fenêtre existante, délai mesuré. Un fichier situé dans un dossier `.obsidian/` non référencé s'ouvre isolé | Fichier hors de tout vault |
| F9 | Tags : détection `#a/b` dans le texte et le frontmatter (vault synthétique), avec **position du fragment** pour chaque occurrence. Fragment = bloc contenant le tag (paragraphe ou élément de liste avec enfants) ; clic sur un résultat → ouverture à l'endroit exact, fragment surligné. Bibliothèque de tags en mémoire : saisie dans un champ, liste filtrée en ≤ 50 ms et fragments affichés en ≤ 100 ms (20x). Arbre avec compteurs. Pas de `tantivy` ici : une structure en mémoire suffit pour vérifier l'UI et la latence | Vault synthétique 1x et 20x |

**Hors périmètre :** index complet `tantivy`, recherche, graphe, système de plugins, store, thèmes. Seul un index minimal (liste des fichiers + snapshot) est autorisé.

## 3. Mesures et seuils

Mesures avec `measure-coldstart.ps1` adapté aux nouveaux exécutables (paramètre `-ProcessName`) : 3 lancements par cas, médiane, plus un lancement après redémarrage de Windows (Drive à froid).

| KPI | Cible | Notes |
|---|---|---|
| T_window | À relever | Apparition de la fenêtre |
| **T1** | **≤ 1 s** (cible M2 du brief) | Caractère tapé accepté dans la note de 319 Ko |
| **T2** | ≤ 3 s | Arbre navigable, vault réel puis synthétique 20x |
| T1 et T2 en 20x | Pas de dégradation notable vs 1x | Exigence d'échelle du 22/11 |
| Latence de frappe | p95 ≤ 16 ms (une image à 60 Hz) | Proposé. Mesurée pendant et après le chargement |
| Défilement de la note de 319 Ko | Sans à-coups visibles | Capture vidéo ou FPS relevé |
| Live Preview sur la note de 319 Ko | Frappe p95 ≤ 16 ms **avec** le rendu actif | Mesure à part du T1 : le rendu en direct ne doit ni retarder T1, ni dégrader la frappe |
| Ouverture d'un canvas (lecture) | À relever | Temps jusqu'au rendu, canvas dense |
| Mémoire (RSS) après 1 min d'inactivité | À relever | |
| Taille du binaire installé | À relever | |
| Mermaid | Voir §6 | |

Référence Obsidian : 5 984 et 7 664 ms de « Total startup time » (hors lancement Electron). Le chrono T1/T2 d'Obsidian est à relever avec le même script pour comparer à périmètre égal.

## 4. Spike A — Tauri 2 + CodeMirror 6

**Estimation : environ 3 à 4 jours** (avec Live Preview et canvas).

Tâches :
1. Projet Tauri 2 avec commandes Rust : lecture de `workspace.json`, lecture de la note active, listing des fichiers (`walkdir`, dossiers à point ignorés), snapshot de l'arbre persisté dans le dossier de données de l'app.
2. Frontend : CodeMirror 6 avec extensions Markdown, coloration, **Live Preview** couvrant F1 à F3 (décorations CM6, widgets pour tableau et image), arbre à rendu progressif et virtualisé.
2 bis. Visionneuse de canvas en lecture seule (F6) : parsing du JSON Canvas, rendu des nœuds et arêtes dans le DOM ou un `<canvas>`, zoom et déplacement.
3. Enregistrement atomique via commande Rust.
4. Mesure IPC : latence d'un aller-retour pour 1 Ko, 100 Ko et 320 Ko.
5. Mesure de la phase « WebView prête » séparée du reste (`T_window` moins T1).

Questions auxquelles le spike répond :
- Le démarrage de WebView2 tient-il dans la cible T1 ≤ 1 s sur ta machine ?
- L'IPC est-elle un goulot pour la note de 319 Ko ?
- La saisie IME et l'accessibilité fonctionnent-elles (gratuitement avec CM6) ?
- Le Live Preview reste-t-il fluide sur 319 Ko, et le rendu en direct dégrade-t-il T1 ?

## 5. Spike B — Rune (natif)

**Estimation : environ 7 à 10 jours** (avec Live Preview et canvas), le gros du travail étant l'éditeur.

Étape 0 (une demi-journée, avant tout le reste) : vérifier sous Windows que Rune ouvre une fenêtre, lit une police et gère le clavier, et si `parley` fournit déjà un éditeur de texte (sélection, IME). **Vérifier aussi** que le rendu de texte permet un Live Preview : styles mélangés dans une ligne, remplacement d'un segment par un widget (image, tableau), masquage/affichage des marqueurs selon le curseur. Si l'un de ces points manque, le spike B est probablement éliminé d'avance : à signaler avant d'investir les 5 à 7 jours.

Tâches :
1. Mêmes commandes d'accès au vault que le spike A, en Rust direct.
2. Éditeur minimal : buffer de texte (rope), curseur, sélection clavier et souris, annuler/rétablir, copier/coller, repli des lignes, défilement virtualisé.
3. **IME :** saisie d'accents et de caractères composés sous Windows (événements `Preedit`/`Commit` de `winit`).
4. **Live Preview** couvrant F1 à F3 : titres, gras/italique, liens `[[ ]]`, tâches `- [ ]`, listes, citations, blocs de code colorés, frontmatter repliable, tableau rendu, image embarquée. Le marqueur Markdown réapparaît quand le curseur entre dans l'élément. C'est le gros du coût du spike B.
4 bis. Visionneuse de canvas en lecture seule (F6) : nœuds, arêtes, zoom et déplacement.
5. Arbre des fichiers virtualisé depuis le snapshot.
6. Accessibilité : test de base avec un lecteur d'écran (NVDA) pour savoir si le texte de l'éditeur est exposé (`accesskit`).
7. Mode headless : une capture PNG scriptée de l'écran, pour vérifier que les tests d'interface sont automatisables.

Questions auxquelles le spike répond :
- T1 ≤ 1 s est-il atteint nettement mieux qu'avec la WebView, ou la différence est-elle faible ?
- L'éditeur est-il utilisable au quotidien (frappe, sélection, IME) ?
- Quel est le coût de construction du Live Preview complet (pas seulement des cas F1 à F3) ? Le spike mesure les jours réellement passés par cas et extrapole le reste.
- Le Live Preview tient-il sur 319 Ko à 16 ms de latence de frappe ?

## 6. Volet Mermaid (lecture, aperçu, édition)

### État de l'art

Depuis Obsidian 1.15 (ton installation est en 1.15.1), Mermaid.js n'est plus embarqué. Le changelog dit : « Mermaid diagrams and charts are now rendered using our new library Pele ». Pele est open source (MIT, dépôt `obsidianmd/pele`, site pele.run) :

- **Techno :** JavaScript/TypeScript, zéro dépendance, rendu synchrone : entrée en texte Mermaid, sortie en chaîne SVG.
- **Chiffres annoncés par le site (méthode non décrite) :** 230 Ko gzip contre 1,6 Mo pour Mermaid.js, et 0,05 à 0,3 ms contre 6 à 82 ms par rendu.
- **Types supportés :** tous ceux de Mermaid 12.1.0 sauf ZenUML, y compris `wardley-beta`. Pas de labels HTML arbitraires, et sa mise en page est la sienne, donc différente de Mermaid.
- **Erreurs :** `PeleError` avec ligne, colonne et extrait de la source. `mount()` affiche l'erreur dans le conteneur.
- **Thème :** variables CSS `--pele-*` (couleurs, polices, rayon, 8 couleurs de séries). Le passage clair/sombre se fait sans nouveau rendu.
- **Lecture :** `enableZoom()` et ajustement à la largeur.
- **Mesure du texte :** dans un navigateur il mesure avec un canvas, ailleurs avec une table de largeurs intégrée. Un mesureur personnalisé est configurable.
- **Non confirmé :** usage natif ou Rust, WASM, version npm actuelle, conditions du benchmark. Le changelog parle de variables `--mermaid-*` alors que la doc de Pele documente `--pele-*` : la relation entre les deux n'est pas vérifiée.

Alternatives en Rust, sans JavaScript :

| Moteur | Types | Remarques |
|---|---|---|
| `mermaid-rs-renderer` (mmdr) | 23 | MIT, « early development », sortie SVG, et une API de scène vectorielle (feature `scene`) qui passe par `usvg` sans rastérisation |
| `merman` | 37 familles (cible Mermaid 12.3.0) | Parité documentée (3 400+ références SVG), Apache-2.0 ou MIT, mais ELK sous EPL-2.0 dans les fonctionnalités de rendu par défaut : à vérifier côté licence. Artefact WASM de 9,8 Mo |
| `mmdflux` | 4 (flowchart, class, sequence, state) | MIT, sortie texte/SVG/JSON de géométrie |

### Ce que chaque spike teste

| Besoin | Spike A (Tauri) | Spike B (Rune) |
|---|---|---|
| **Aperçu** d'un bloc ```` ```mermaid ```` dans la note | Pele `render()` ou `mount()` dans le DOM | Pele dans un moteur JS embarqué (sortie SVG) rendu en scène vectorielle, **ou** `mmdr`/`merman` directement |
| **Lecture** : zoom, déplacement, thème de l'app | `enableZoom()` et variables `--pele-*` | À implémenter (transformation de la scène, couleurs reprises du thème natif) |
| **Édition** : rendu en direct pendant la frappe | Rendu différé (debounce), erreur affichée avec ligne et colonne | Même principe. Soulignement de la ligne fautive à partir de `PeleError` si Pele est utilisé |

Corpus de test (un fichier chacun, à fabriquer) : flowchart, sequence, class, state, ER, gantt, pie, mindmap, et **`wardley-beta`** (le skill `wardley-map` produit ce format et Pele le supporte).

Mesures Mermaid : temps de rendu par type (médiane de 20 rendus), temps pour un fichier de 20 diagrammes, fidélité visuelle vs Obsidian 1.15.1 (comparaison côte à côte), comportement sur une erreur de syntaxe, thème clair et sombre, 50 diagrammes dans une même note (défilement). Les diagrammes ne doivent jamais bloquer l'ouverture de la note : rendu différé, une fois la note affichée.

Prérequis : savoir combien de blocs Mermaid ton vault contient et de quels types (non mesuré dans `vault-kpis.json`). À ajouter à `vault-kpis.ps1`, puis à faire figurer dans les notes de `gen-vault.py` pour que le vault synthétique en contienne.

## 7. Critères de décision

| Critère | Poids proposé | Remarque |
|---|---:|---|
| T1 et T2 (vault réel et 20x) | 30 % | |
| Qualité d'édition au quotidien (frappe, sélection, IME, undo) | 25 % | Rédhibitoire si inutilisable |
| **Live Preview (F1 à F3) et canvas en lecture (F6)** | **Éliminatoire** | Une pile qui ne couvre pas F1 et F2 sur la note de 319 Ko à 16 ms de latence est écartée, quels que soient ses T1 et T2. F3 à F6 : ⚠️ accepté s'il existe un chemin crédible chiffré |
| Coût estimé pour atteindre l'éditeur et le rendu Markdown complets | 20 % | Estimé à partir du spike |
| Mermaid (aperçu, lecture, édition) | 10 % | |
| Mods : adéquation au modèle Herdr (manifeste, rechargement à chaud, vérifiable par un agent) | 10 % | |
| Accessibilité, mémoire, taille | 5 % | |

**Règle :** une pile éliminée sur Live Preview n'est pas retenue, même si elle gagne sur T1. Ensuite, si le spike B ne dépasse pas A d'au moins 30 % sur T1 médian **et** n'atteint pas une édition utilisable, on retient A. Si B est clairement meilleur sur T1 et que l'éditeur est utilisable, on retient B. Entre les deux, on garde A et on revoit B plus tard. Seuils proposés, à valider.

## 8. Livrables

1. Un dossier par spike avec le code, un README de lancement et un script de mesure.
2. `out/spikes/m2-resultats.md` : tableaux de mesures (3 lancements par cas, médianes), captures d'écran, remarques sur l'édition, Mermaid et mémoire.
3. Une recommandation chiffrée de pile pour M3 à M5, avec la grille F1 à F9 remplie pour chaque spike.

## 9. Prérequis

- [ ] Adapter `measure-coldstart.ps1` pour un nom de processus configurable (fait en parallèle de ce document).
- [ ] Mesurer la baseline Obsidian en 3 lancements par cas (le protocole 7.2 de l'audit n'est pas rempli).
- [ ] Générer les vaults synthétiques 1x et 20x en local.
- [ ] Compter les blocs Mermaid du vault réel et ajouter des blocs au générateur.
- [ ] Fixer le temps disponible par semaine pour caler les estimations.
- [ ] Choisir une note de test pour F2/F3 (frontmatter, tableau, image embarquée) et deux `.canvas` (simple et dense) dans le vault, sans en reproduire le contenu dans le dépôt : travailler sur une copie locale, hors dépôt.

## 10. Risques

- **Spike B :** l'éditeur et l'IME peuvent coûter bien plus que prévu. La timebox de l'étape 0 sert à le savoir tôt.
- **Live Preview sous Rune :** risque principal du spike B. Le rendu de texte natif doit mélanger des styles et des widgets dans le flux du texte, ce qui n'est pas garanti. L'étape 0 existe pour le détecter tôt.
- **Rune est pre-release,** son support Windows n'est pas vérifié, et il n'accepte pas de pull requests (corrections par issues seulement).
- **Estimations :** l'ajout de F1 à F9 augmente le spike A de 1 jour environ (soit 3 à 4 jours) et le spike B de 2 à 3 jours (soit 7 à 10 jours). À recaler quand le temps disponible est fixé.
- **Spike A :** la différence de T1 avec Obsidian peut rester faible si le démarrage de WebView2 domine.
- **Drive à froid** fausse les comparaisons si les deux spikes ne sont pas mesurés dans les mêmes conditions : toujours noter l'état du cache.
