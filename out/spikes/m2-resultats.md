# M2 — Résultats du spike comparatif (23/11/2026)

Statut : **pile A validée par Alec le 23/11/2026** (« la pile a l'air ok »). Spike A complet, spike B limité à son étape 0 et à un essai sous Windows.

## 1. Décision proposée

**Pile retenue pour la 0.1 et pour M3 : A, Tauri 2 + CodeMirror 6.**

Règle appliquée (cahier des charges §7) : une pile qui ne couvre pas F1 et F2 est écartée, quels que soient ses temps de démarrage. Rune couvre F1 de façon crédible, mais pas F2 ni F3 : il n'a pas de widget dans le flux du texte (tableau, image, frontmatter replié), et l'architecture par blocs qui les permettrait est estimée à 3 à 6 jours, sur 6 à 12 jours au total. Le plafond de temps du plan (« si B n'est pas clairement meilleur en fin d'étape 3, on retient A ») s'applique donc.

Rune reste intéressant pour plus tard : il démarre environ trois fois plus vite (voir §3). Un vrai spike B se justifie si le temps de démarrage de WebView2 devient le facteur limitant.

## 2. Conditions de mesure

Toutes les mesures de ce document sont **automatiques** (relevées par script ou par l'app elle-même), sur la machine de référence (Windows 11, i7-10750H, NVMe), avec des **vaults synthétiques sur `C:`** et un **cache chaud**. Elles ne couvrent pas :

- le vrai vault sur Google Drive ;
- le démarrage à froid après redémarrage de Windows ;
- le chrono manuel T1/T2 (`measure-coldstart.ps1`, touches F8/F9) ;
- la baseline Obsidian à périmètre égal (non relevée).

Ces quatre points restent à faire par Alec. Les seuils (16 ms, 50 ms, 100 ms, 30 %) sont des propositions.

Vaults : `x1` (488 notes, 955 fichiers visibles) et `x20` (9 760 notes, 19 062 fichiers visibles, 64 Mo de texte), générateur v2. Note pire cas : 318 Ko. Le vault synthétique est plus lourd que le réel (+31 % de texte par bloc).

## 3. Démarrage

### Spike A (app assemblée : interface + cœur Rust)

Jalons écrits par l'app et horodatés côté Rust depuis le début du processus. 5 lancements par cas.

| Cas | Éditeur prêt, médiane (min à max) | Arbre prêt, médiane (min à max) |
|---|---|---|
| Fichier hors vault, note de 318 Ko | 708 ms (632 à 1 027) | sans objet |
| Vault 1x, note de 318 Ko active | 755 ms (609 à 1 036) | 944 ms (771 à 1 196) |
| Vault 20x, note de 318 Ko active | 680 ms (617 à 975) | 939 ms (871 à 1 222) |

- Cibles du spike : T1 ≤ 1 s, T2 ≤ 3 s. Tenues en médiane ; un lancement sur cinq dépasse légèrement 1 s pour l'éditeur.
- Pas de dégradation de 1x à 20x : l'arbre vient du snapshot (4,6 ms pour 20 747 entrées côté Rust, 67 ms aller-retour jusqu'à la page).
- Le processus crée sa fenêtre en ~30 ms ; le reste est le démarrage de WebView2 puis de l'interface.
- Mémoire : 26 à 45 Mo pour le processus de l'app, environ 370 à 400 Mo pour les processus WebView2 (relevé par filtre sur la ligne de commande, à confirmer).
- Taille : `rocksy.exe` 6,5 Mo + `WebView2Loader.dll` 0,16 Mo. JS initial 135 Ko gzip.

Runs : `a-tauri-cm6/runs/2026-11-23_1159_auto-*`.

### Spike B (Rune, sonde avec note de 319 Ko générée en mémoire)

| Mesure | Valeur |
|---|---|
| Fenêtre créée, médiane de 3 lancements | 233 ms |
| Premier affichage du document (journal de Rune) | 357 à 372 ms |
| Mémoire | 125 Mo |
| Taille de l'exe (non strippé, avec outillage) | 31 Mo |

La sonde n'est pas une app : pas d'arbre, pas de lecture de fichier, pas de Live Preview complet. La comparaison donne un ordre de grandeur (environ 0,36 s contre 0,7 s), pas un écart à périmètre égal.

Runs : `b-rune/runs/2026-11-23_1141_etape0-windows`.

## 4. Cœur Rust (commun aux deux piles)

Mesuré sous Windows avec `rocksy-cli`, vault 20x.

| Mesure | Valeur |
|---|---|
| Ouverture de l'arbre depuis le snapshot | 4,6 ms |
| Listing complet (réconciliation) | 61 à 72 ms |
| Index des tags, cache chaud | 0,32 à 0,34 s |
| Index des tags, première lecture des fichiers | 13 s |
| Liste des tags (4 199 tags) | 1,1 ms |
| 200 fragments du tag le plus fréquent | 0,08 ms |

Dans l'app (aller-retour complet jusqu'à la page) : liste des tags 5 à 14 ms, 500 fragments 11 ms, note de 318 Ko lue en 11 ms. L'IPC de Tauri n'est pas un goulot.

Point d'attention : la première indexation des tags lit tous les fichiers. Sur Google Drive à froid (~155 ms par fichier d'après l'audit), elle prendra de l'ordre de la minute sur le vrai vault. Elle est en arrière-plan et ne bloque ni l'éditeur ni l'arbre, mais **la persistance de l'index est la première chose à faire après la 0.1**.

## 5. Édition et Live Preview

### Spike A, dans l'app réelle (WebView2 + cœur Rust)

Grille F1 à F9 : `a-tauri-cm6/grille.md`. Sept cas passent (F1, F2, F3, F4, F7, F8, F9), deux sont partiels :
- **F5** : onglets, deux volets et restauration des onglets à la relance fonctionnent ; la disposition en volets n'est pas restaurée, et la session vient du dossier de données de Rocksy, pas de `workspace.json` d'Obsidian.
- **F6** : les deux canvas s'ouvrent (83 à 106 ms), zoom et déplacement fonctionnent, le fichier reste identique ; les cartes de notes montrent un extrait brut et deux pas de zoom sur huit dépassent 200 ms sur le canvas dense.

Frappe sur la note de 318 Ko, Live Preview actif, environ 216 frappes par endroit (cible proposée : p95 ≤ 16 ms) :

| Endroit | Médiane | p95 | Max |
|---|---|---|---|
| Début | 2,4 ms | 3,9 ms | 9,5 ms |
| Milieu | 2,9 ms | 5,0 ms | 7,0 ms |
| Fin | 2,5 ms | 4,2 ms | 6,6 ms |
| Tâche imbriquée avec balisage | 2,2 ms | 3,8 ms | 17 ms |

La latence est mesurée de l'événement clavier (envoyé par le protocole de débogage) à la fin de la mise à jour de l'éditeur : ce n'est pas une latence touche-écran.

- Défilement : intervalle médian entre images 16,7 ms ; une image à 33,8 ms sur 448.
- Tags sur x20 (4 199 tags) : filtre par frappe 15 à 43 ms, fragments 56 à 91 ms, clic sur un fragment → note surlignée en 38 ms. Neuf fragments comparés au texte sont exacts.
- Enregistrement : sur des fichiers LF, CRLF et avec BOM, seule la ligne modifiée change. Vault de 4 658 fichiers identique (dossiers à point compris) après une session en lecture ; un seul fichier diffère après un enregistrement.
- Instance unique : second lancement → onglet actif en 42 ms.
- Mermaid (Pele) : les 9 types du vault synthétique se rendent, premier rendu d'un type en 4,9 à 31,9 ms. Fidélité par rapport à Obsidian non comparée.

Run : `a-tauri-cm6/runs/2026-11-23_1223_integration`.

Non vérifié : glisser-déposer, dialogues natifs de choix de dossier et de fichier, ouverture effective d'un lien externe, IME, lecteur d'écran.

### Spike B

Frappe scriptée sans fenêtre, aucun commit au-dessus de 4 ms sur 54 frappes sous Windows. Clavier réel, IME et souris non exercés.

## 6. Grille comparée

| Critère (poids proposé) | A : Tauri 2 + CM6 | B : Rune |
|---|---|---|
| Live Preview F1 et F2 (éliminatoire) | Couvert, frappe p95 ≤ 5 ms sur 318 Ko | F1 à portée (1 à 2 jours), F2 et F3 non couverts |
| T1 et T2 (30 %) | ~0,7 s et ~0,94 s, stables à 20x | ~0,36 s sur une sonde, pas d'arbre |
| Qualité d'édition (25 %) | CodeMirror 6 : IME, undo, sélection fournis | Éditeur riche présent, non exercé au clavier ; undo limité à ~1 étape sur 319 Ko |
| Coût pour l'éditeur complet (20 %) | Fait dans la journée | 6 à 12 jours estimés |
| Mermaid (10 %) | Pele, chargé à la demande | Moteur Rust intégré à Rune, fidélité non évaluée |
| Mods, modèle Herdr (10 %) | JS/DOM possible, à concevoir en M4 | Luau, VM isolée, rechargement à chaud : proche du modèle, mais pas en JS |
| Accessibilité, mémoire, taille (5 %) | ~400 Mo avec WebView2, exe 6,5 Mo | 125 Mo, exe 31 Mo |
| Risque de dépendance | Tauri 2 stable | Version 0.0.1, 200 commits en 2 jours, pas de PR acceptées, 4 dépendances patchées |

## 7. Ce qui reste à mesurer

1. Baseline Obsidian avec `measure-coldstart.ps1` (3 lancements, médiane).
2. Rocksy sur le vrai vault, chrono manuel, à chaud puis après redémarrage (Drive à froid).
3. Mémoire de WebView2 à confirmer, et mémoire après 1 min d'inactivité.
4. Fidélité de Mermaid (Pele) par rapport à Obsidian 1.15.1, côte à côte.
