# Intégration des correctifs d'audit dans l'app réelle — Rocksy 0.2.1 — 23/11/2026

Exe essayé : `C:\Users\aleca\rocksy-test\wip\rocksy.exe` (0.2.1, 14 054 400 octets, sha256 `300e8849…2347e`), copié ensuite dans `release-0.2.1\`. Comparaison : `release-0.2.0\rocksy.exe` (11 584 512 octets), lancé lui aussi en instance isolée.

Conditions : Windows 11, instances isolées (`ROCKSY_DATA_DIR` neuf par scénario sous `rocksy-test\it5\`, supprimé à la fin), pilotage par le protocole DevTools (`ui/tools/drive.py`, vraies frappes, captures de page seulement). Vaults : **copies** des vaults synthétiques x1 (488 notes, 4 658 fichiers) et x20 (9 760 notes, 22 765 fichiers) faites par `robocopy` sur `C:` (disque local, **cache chaud** : ni le vrai vault, ni Google Drive, ni démarrage à froid). Les vaults de référence n'ont pas été ouverts. Une instance 0.2.0 d'Alec tournait pendant la seconde moitié de la séance (non touchée).

Valeurs brutes : `resultats/<scénario>.json` (app réelle, exe final), `mesures/` (démarrage et comparaison 0.2.0 / 0.2.1), `banc-navigateur/` (interface seule, backend simulé), `captures/`.

## 1. Ordre `open_vault` / `build_tags` (`a5-order`)

| Cas | Événements d'index reçus | Listings du vault |
|---|---|---|
| Ajout d'un vault par l'interface (première indexation, les deux commandes envoyées ensemble) | `tags-ready`, `index-ready` (488 notes, 488 documents de recherche) | 1 (`open`) |
| Changement de vault par l'interface (`close_vault` de l'ancien, `open_vault` et `build_tags` du nouveau envoyés ensemble) | oui | 1 |
| Retour au premier vault (instantané et index persistés) | cache, cache + recherche, réconcilié | 1 (`refresh`) |
| `build_tags` 40 ms avant `open_vault` | oui | 1 |
| `build_tags` 900 ms avant `open_vault` | oui | 2 (cas documenté au contrat) |
| `open_vault` puis `build_tags` | oui | 1 |
| `build_tags`, `open_vault` du nouveau et `close_vault` de l'ancien dans le même tour | oui | 1 |
| Relance avec session | cache à 247 ms, réconcilié à 248 ms après navigation | 1 |

Avant correction, le changement de vault par l'interface ne recevait **aucun** événement d'index (attente de 60 s expirée) : le `close_vault` de l'ancien vault invalidait l'`open_vault` du nouveau quand il arrivait après lui, et le backend restait sans vault courant (ni index, ni suivi du dossier). Les listings sont comptés par les lignes `vault-listing` de `timings.jsonl`.

`index-ready` après enregistrement (`a5-links`) : enregistrement sans changement de lien → 0 événement, panneau non redemandé ; enregistrement qui ajoute un `[[lien]]` → 1 événement (`linksGeneration` 497 → 498), panneau à jour (3 → 4 liens sortants).

## 2. Sécurité (`a5-security`, `a5-csp`)

- Canvas piégé de l'audit (coordonnée `x` contenant `"/><img src=x onerror=…>`) ouvert dans l'app : rien exécuté (titre inchangé, aucun attribut posé, aucune image ni gestionnaire dans la vue), aucune violation CSP (le contenu n'atteint pas le DOM).
- Cellule de tableau de 8 000 caractères (`[a](` × 2 000) : ouverture et affichage 130 ms, frappe p95 3,6 ms (36 frappes), entrée du curseur dans le tableau 92 ms. Pas de gel.
- Images en chemin réseau (`//serveur/…`, `![[\\serveur\…]]`) et image distante : non chargées, message à la place, 0 élément `<img>` vers ces adresses, 0 requête.
- `read_file` refusé : `C:\Windows\win.ini` et fichier hors périmètre (`not_allowed`), chemin réseau (`network_path`), périphérique et chemin relatif (`invalid`). `save_file` refusé : hors vault (`not_allowed`, fichiers non créés, contenu du fichier existant intact), `.obsidian`, `.cmd`, `.canvas`, `.txt` (`read_only`), flux NTFS (`invalid`). `open_vault`/`build_tags` d'un dossier non enregistré : `not_allowed`. `set_session` hostile : rien de persisté (`session.json` vide).
- `.txt` : lecture seule (cadenas), frappe sans effet, Ctrl+S → « Lecture seule : Rocksy n'enregistre que les fichiers .md et .markdown », octets inchangés.
- `asset:` : image du vault chargée ; image de `.obsidian`, image hors vault, fichier de Windows, chemin réseau : refusés (403). Réserve : le refus du chemin réseau a pris 1,3 s contre 2 ms pour les autres, ce qui indique que le protocole touche le chemin avant de refuser (voir « Défauts restants »).
- CSP : **aucune violation** en parcourant éditeur, Live Preview, images du vault, tableaux (alignement par attribut de style), les 9 types Mermaid du vault synthétique (flowchart, sequenceDiagram, classDiagram, stateDiagram-v2, erDiagram, gantt, pie, mindmap, wardley-beta : 1 SVG chacun, 0 erreur), canvas dense (150 nœuds, 17 images), panneaux des tags, des liens et de recherche, sélecteur rapide, dialogue de fermeture. Aucune violation non plus dans les 30 autres scénarios joués. Les événements Tauri arrivent (arbre mis à jour 425 ms après un changement externe).
- Contenu injecté par DevTools : script en ligne, `onerror`, `onclick`, URL `javascript:`, `eval`, `new Function`, `setTimeout("…")`, script externe : **non exécutés**. `fetch` vers l'extérieur et vers localhost, XHR, WebSocket, `sendBeacon`, image distante, iframe, envoi de formulaire : **bloqués** (16 violations consignées, motif `csp` côté réseau). L'IPC et les événements fonctionnent toujours ensuite.
- Non vérifiable en instance isolée : le refus d'`add_vault` pour un dossier non choisi dans le dialogue (l'ajout y est libre par construction). Couvert par les tests du cœur et du backend simulé.

## 3. Pas de perte de texte (`a5-loss`)

- Frappe puis demande de fermeture immédiate (`CloseMainWindow()` sur le PID lancé, minuterie d'enregistrement automatique non échue) : processus sorti en 125 ms, texte sur le disque.
- Enregistrement automatique désactivé : dialogue « Un document n'est pas enregistré » (Annuler / Quitter sans enregistrer / Enregistrer et quitter). Annuler : app ouverte, texte gardé, disque inchangé. Enregistrer et quitter : texte sur le disque. Quitter sans enregistrer : texte absent du disque.
- Écriture externe pendant la frappe : conflit en 1,6 s, bandeau, le disque garde le contenu externe 3 s plus tard, l'éditeur garde la frappe.
- Fichier supprimé pendant qu'il est ouvert : signalé en 2,7 s, texte gardé, fichier non recréé tout seul ; « Enregistrer quand même à cet emplacement » le recrée (`it-f8`).
- Fins de ligne (comparaison d'octets) : LF, CRLF, BOM + LF, BOM + CRLF → seule la ligne modifiée change, nombres de LF/CRLF et BOM identiques. Fichier mixte (5 CRLF, 2 LF) : la ligne modifiée prend la fin majoritaire (CRLF), l'autre ligne LF garde son LF, les lignes CRLF aussi ; une ligne ajoutée prend CRLF.

## 4. Robustesse (`a5-robust`, `a5-robust-x20`, `a5-robust-x20-quit`)

- Index de recherche endommagé, app arrêtée : octets écrasés dans les `.term` et `.store` → l'app démarre, reconstruit (488 notes relues), la recherche répond (188 fichiers pour « plan », comme avant) 0,73 s après navigation. `meta.json` écrasé → idem (0,71 s ; lors d'un passage précédent la reconstruction s'est faite en deux temps, à la reprise automatique 2 s plus tard). Fichier des tags écrasé → reconstruit (614 tags).
  Réserve : avec des segments endommagés, l'index est annoncé prêt pendant environ 0,25 s (avant la vérification des sommes de contrôle), puis reconstruit.
- Fermeture pendant la première indexation de x20, vraie demande de fermeture, copie neuve (873 notes lues sur 9 760 au moment de la demande) : processus sorti en 365 ms. Relance : 2 043 notes déjà indexées rechargées, 7 717 relues, index complet. Variante `quit` envoyée par la page à 1 000 notes lues : 1 901 notes rechargées, 7 859 relues.
- Relance avec index persisté : tags disponibles dès que la page répond (614 tags à 356 ms sur x1, 4 199 tags à 200 ms sur x20) ; événements d'index à 0,2-0,35 s après navigation.

## 5. Intégrité du vault (`a5-integrity`, `it-f7`, `is-f7`)

Empreinte SHA-1 de tous les fichiers, dossiers à point compris (4 658 fichiers). Après une session en lecture (notes, images, deux canvas, onglet image, panneaux des tags, des liens, de recherche, liens non résolus, note Mermaid) et fermeture normale : 0 ajouté, 0 supprimé, 0 modifié. Après un enregistrement : 1 fichier modifié (la note), 0 ajouté (aucun temporaire).

## 6. Observation « 435 documents pour 488 notes » : cause établie

Reproduit : 9 fois sur 55 premières indexations de copies neuves de x1, et 3 fois sur 4 sur x20 (jusqu'à 7 866 notes manquantes sur 9 760).

Établi :
- Aucune lecture de note n'a échoué (`unread: 0` à chaque fois). Ce n'est ni une course avec le listing partagé, ni la capture de panique (aucune panique relevée), ni un commit différé.
- Un fil d'écriture de tantivy échoue en créant un fichier de segment dans le dossier `search` : `Failed to open file for write … Os { code: 5, PermissionDenied, "Accès refusé." }` (`.fieldnorm`, `.idx`, `.term` selon les fois). tantivy tue alors son « writer » ; tout ce qui n'était pas encore committé est perdu pour l'index de recherche (les tags et les liens, eux, sont complets).
- La 0.2.0 a le même défaut, en plus grave et sans signe : mêmes conditions, `searchDocs: 0` avec `searchReady: true` (recherche vide toute la session), ou `searchReady: false` qui n'arrive jamais (5 291 documents sur 9 760, attente de 240 s expirée).
- Le phénomène suit la copie : sur une copie laissée au repos, 0 échec sur 9 premières indexations (x1 et x20) et l'indexation de x20 prend 1,6 à 2,5 s ; juste après la copie de 22 765 fichiers, elle prend 10 à 17 s et l'échec est fréquent.

Hypothèse (non observée directement) : l'analyse antivirus à l'accès des fichiers fraîchement copiés ralentit les lectures et fait refuser momentanément des créations de fichiers dans le dossier de l'index.

Correction : la passe compte les notes restées en retard (`IndexReady.pending`), la barre d'état affiche « Index : N notes à relire… » au lieu de « Index ✓ », et le backend relance seul une passe 2 s plus tard (puis 5, 15, 30, 60 s) qui ne relit que ce qui manque. Observé : index complet 2,1 à 3,5 s après la première passe. Chaque occurrence laisse une ligne `index-pending` dans `timings.jsonl` avec l'erreur du moteur.

## 7. Mesures 0.2.0 / 0.2.1 (même séance, mêmes scénarios, instances isolées, cache chaud)

| Mesure | 0.2.0 | 0.2.1 |
|---|---|---|
| Démarrage x1, éditeur prêt (note de 318 Ko), médiane de 5 lancements, 2 séries | 931 / 907 ms | 1 082 / 924 ms |
| Démarrage x1, arbre prêt | 1 198 / 1 148 ms | 1 416 / 1 107 ms |
| Démarrage x20, éditeur prêt | 905 / 889 ms | 906 / 955 ms |
| Démarrage x20, arbre prêt | 1 156 / 1 182 ms | 1 251 / 1 228 ms |
| Frappe sur la note de 318 Ko, p95 (216 frappes, milieu / début de note), x1 | 4,2 / 5,5 ms | 4,6 / 5,1 puis 5,3 / 6,9 ms (deux passages) |
| idem, x20 | 4,7 / 6,8 ms | 4,6 / 5,4 puis 4,6 / 4,5 ms |
| `save_file` de la note de 318 Ko vu de la page, médiane de 7 | 105 ms (x1), 111 ms (x20) | 27 et 31 ms (x1), 31 et 21 ms (x20) |
| Frappe → résultats de recherche affichés (86 recherches tapées), médiane / p95, x20 | 105 / 129 ms | 104 / 120 puis 101 / 118 ms |
| Changement externe → arbre à jour / recherche à jour, x1 | 442 / 521 ms | 442 / 481 puis 438 / 516 ms |
| idem, x20 | 679 / 686 ms | 670 / 678 puis 567 / 577 ms |
| Première indexation x1, copie au repos (3 passages alternés) | 378, 390, 356 ms | 299, 338, 324 ms |
| Première indexation x20, copie au repos (3 passages alternés) | 2 450, 2 519, 2 437 ms | 2 285, 2 505, 2 424 ms |
| Relance x20 : index du cache / recherche prête / réconcilié, après navigation | 647 / 696 / 751 ms | 548 / 625 / 722 puis 336 / 388 / 445 ms |
| Mémoire du processus 3 s après le démarrage, x20 (jeu de travail) | 58-65 Mo | 121-124 Mo |
| Mémoire privée du processus, x20 | 38 Mo | 39-40 Mo |
| Taille de l'exe | 11 584 512 octets | 14 054 400 octets (+21 %) |

Lecture : les écarts de démarrage (−41 à +151 ms selon la série) sont du même ordre que la dispersion entre lancements (jusqu'à 700 ms dans une série) ; pas de régression nette, pas de gain non plus. Les temps absolus sont plus élevés que dans le run de 15 h 16 (547 ms) : machine plus chargée pendant cette séance. `save_file` est 3 à 4 fois plus rapide. Le jeu de travail double sur x20 alors que la mémoire privée ne bouge pas : la 0.2.1 vérifie les sommes de contrôle de l'index de recherche à chaque ouverture, ce qui lit tout l'index (pages de fichier, pas de mémoire propre) ; non corrigé, signalé.

Les mesures de frappe, d'enregistrement, de recherche et de changement externe de la colonne 0.2.1 viennent de deux passages : l'un sur la construction précédente (mêmes sources), l'autre sur l'exe final. Les mesures de démarrage et de première indexation « au repos » ont été faites sur la construction précédente.

## 8. Scénarios hérités (`it-*`, `is-*`), adaptés aux nouveaux contrôles

17 scénarios rejoués sur l'exe final : 0 erreur, 0 violation CSP. Adaptations : `it-f8`, `it-final`, `it-iso-b` passent en arguments les fichiers hors vault qu'ils modifient (un voisin ouvert par son chemin est en lecture seule : vérifié, `save_file` refusé `not_allowed`) ; `it-f8` lit `opened.json` dans le dossier de données, n'essaie plus le second lancement (impossible en instance isolée) et ne lance plus le navigateur du système ; `it-preview` et le scénario navigateur `f2` ne dépendent plus de la préférence de pliage du frontmatter (c'était la cause du sélecteur « périmé » : la préférence « replié » était restée dans le profil du navigateur d'essai).

## 9. Suites

- `cargo test -p rocksy-core` : 244 réussis, 0 échec. `cargo clippy -p rocksy-core --all-targets` : 0 avertissement. `cargo clippy` de la coquille pour `x86_64-pc-windows-gnullvm` : 0 avertissement.
- `npx vitest run` (vitest 5.1.1) : 124 réussis. `npm run build` : réussi. `npm audit` : 0 vulnérabilité.
- Banc navigateur (16 scénarios) : 0 erreur, 0 violation CSP.

## Défauts restants et remarques

- `asset:` et chemin réseau : une requête `http://asset.localhost/\\serveur\…` est refusée, mais après 1,3 s, signe que Tauri résout le chemin avant de tester son périmètre. L'interface ne fait jamais cette requête ; un contenu injecté pourrait la provoquer (la CSP autorise `asset:` pour les images). Non corrigé.
- Message de console « Viewport failed to stabilize » (CodeMirror), une fois dans `a5-integrity` et une fois dans `a5-perf`, pendant des changements d'onglet enchaînés par le script. Sans effet observé.
- Frappe en fin d'une ligne unique de 216 000 caractères (`it-f8`) : médiane 76 ms. Cas extrême déjà connu, hors du périmètre de ce lot.
- Index endommagé annoncé prêt pendant 0,25 s avant sa vérification (voir 4).
- Jeu de travail doublé au démarrage sur x20 (voir 7).

## Non vérifié

Vrai vault, Google Drive, démarrage à froid ; dialogues natifs (`pick_folder`, `pick_file`) et donc l'ajout d'un vault en profil normal ; glisser-déposer ; second lancement en profil normal (instance unique) ; déplacement des index de `%APPDATA%` vers `%LOCALAPPDATA%` sur le profil réel ; `install.ps1` (jamais exécuté) ; ouverture réelle d'un lien externe dans le navigateur (l'appel est vérifié, pas le lancement).
