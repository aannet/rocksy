# Intégration de la recherche et des liens dans l'app réelle — 23/11/2026

Tout a été observé dans `rocksy.exe` 0.2.0 (WebView2 + cœur Rust), piloté par le protocole DevTools avec de vraies frappes. Captures de page dans `captures/`, valeurs brutes dans `resultats/<scénario>.json`.

Conditions : Windows 11, fenêtre de 1280 × 820 px, vaults synthétiques sur C: (disque local, cache chaud), un profil isolé neuf par scénario. Rien sur le vault réel.

## 1. Recherche (x20 : 9 760 notes)

| Mesure | Valeur |
|---|---|
| Frappe → résultats affichés, 86 recherches tapées au clavier (7 requêtes : mot fréquent, mot rare, préfixe, expression, `tag:`, `path:`, sans accents) | médiane 100 ms, p95 116 ms, max 122 ms |
| dont attente voulue avant l'appel | 80 ms |
| dont appel `search` aller-retour | médiane 7,2 ms, p95 16,7 ms, max 20 ms |
| dont rendu de la liste (50 fichiers, jusqu'à 150 extraits) | médiane 12,5 ms, p95 22,8 ms |
| Entrée sur un résultat → note ouverte et ligne surlignée | 85 ms |

- 10 résultats tirés au hasard sur 10 requêtes : le texte de la note à `line`/`col`/`len` est bien le terme (10 sur 10), et les plages `ranges` des extraits aussi.
- Ouverture d'un résultat : la note s'ouvre à la bonne ligne, le terme est sélectionné (`Plan`, `zygomatique`), la ligne est surlignée et les occurrences sont marquées dans la note.
- Fichier CRLF : second résultat ouvert ligne 8, colonne 15, sélection `crlfzygo`. Fichier avec BOM : le cœur donne `col` 1 sur la ligne 0 (BOM compté), l'éditeur sélectionne `bomzygo` à la colonne 0 ; ligne 2 correcte aussi.
- Accents et ligature : `ete resume oeuvre` trouve la note ; dans la note, « Été », « résumé » et « œuvre » sont surlignés.
- Navigation clavier : flèches, Entrée, Échap vus (`is-search-keyboard.png`).
- Première indexation (profil neuf) : le panneau affiche « Index de recherche en préparation… » (`is-search-preparing.png`, 6 693 notes lues sur 9 760 à ce moment), puis la requête tapée se relance seule : premiers résultats 1,7 s après l'ajout du vault.
- Relance sur un vault déjà indexé : `searchReady` arrive 0,38 à 0,43 s après la navigation de la page (5 relances) ; une recherche répond alors en 8 ms aller-retour.

## 2. Liens

- Note de 318 Ko (446 liens sortants, 35 entrants) : rafraîchissement du panneau en 33 à 68 ms (appel `outgoing_links` 13,6 ms), 3 585 nœuds DOM, 15 liens non résolus estompés (`is-links-big-note.png`).
- Liens entrants d'une note : 2 sources ; clic → la source s'ouvre, curseur sur `[[Audit processus 00402]]`.
- Liens entrants d'une image : 1 embed ; clic → curseur sur `![[Pasted image 0000397.png]]` (le `!` compris).
- Après enregistrement d'une note où un `[[lien]]` est ajouté : les sortants passent de 2 à 3, et la cible affiche la note dans ses entrants.
- Watcher : un fichier créé hors de l'app, qui résout un lien non résolu, fait passer ce lien à « résolu » dans le panneau 498 ms après, sans rien faire ; la recherche trouve le nouveau fichier, l'arbre le contient.

## 3. Liens non résolus

La commande liste 398 cibles sur x20 (`is-unresolved.png`), 27 sur la copie de x1. Entrée ouvre la première source, curseur sur le lien (`[[Missing 000-0]]`).

## 4. État de l'index

Séquence relevée à chaque relance sur x20 : `fromCache` sans recherche (≈ 380 ms après la navigation), `fromCache` avec recherche (≈ 16 ms plus tard), réconcilié avec `reread: 0` (≈ 45 ms plus tard). Les trois états durent trop peu pour une capture ; le texte de la barre d'état pour chacun a été lu en rejouant les charges utiles : « Index : tags ✓ liens ✓ · recherche… », « Index : vérification… », « Index ✓ ». Pendant la première indexation : « Index : 6 693 / 9 760 notes ». Info-bulle : « 9 760 notes · 4 199 tags · 19 257 liens (398 non résolus) · recherche : 9 760 notes ».

## 5. Non-régression

| Mesure | Valeur |
|---|---|
| `editor-ready` depuis le début du processus, 5 relances, x20 avec session (note de 318 Ko) | 508, 782, 547, 541, 560 ms (médiane 547) |
| `tree-ready`, mêmes relances | 640, 931, 710, 683, 705 ms (médiane 705) |
| Frappe sur la note de 318 Ko, panneau des liens ouvert (151 frappes) | médiane 2,4 ms, p95 3,4 ms, max 7,8 ms |
| Frappe, panneau de recherche ouvert (156 frappes) | médiane 2,2 ms, p95 2,8 ms, max 3,3 ms |
| Frappe pendant 11 enregistrements en arrière-plan de la note de 318 Ko (474 frappes, 86 tombées pendant un enregistrement) | médiane 2,6 ms, p95 4,1 ms, max 13,9 ms |
| Ctrl+S sur la note de 318 Ko (frappe → « Enregistré ») | 69 à 109 ms |
| `save_file` seul, note de 318 Ko | 61 à 68 ms (73 à 91 ms pendant la frappe) |
| Ctrl+S sur une petite note | 50 ms (31 à 34 ms en 0.1.0) |
| Mémoire du processus de l'app après première indexation de x20 | 152 Mo (95 Mo privés) |
| Mémoire du processus de l'app après relance sur x20 | 57 Mo (37 Mo privés) |
| Mémoire des 6 processus WebView2 | 388 à 411 Mo |

Lors de la campagne 0.1.0, `editor-ready` valait 686 à 850 ms sur la même note ouverte hors vault : pas de régression visible malgré l'exe passé de 6,7 à 11,6 Mo.

## 6. F7

Copie `it\v7` (4 658 fichiers, dossiers à point compris). Session : vault ajouté, indexation, 4 fichiers ouverts, deux recherches avec ouverture d'un résultat, panneau des liens avec clic sur un lien entrant, commande « Liens non résolus » avec ouverture, panneau des tags, puis fermeture normale de la fenêtre. Résultat : 0 fichier ajouté, 0 supprimé, 0 modifié. Le dossier de données contient `search\<id>`, `tags\<id>.tags`, `trees`, `session.json`, `vaults.json`, `timings.jsonl`, `webview`.

## 7. Robustesse

- 16 requêtes inhabituelles (`"`, `-`, `tag:`, `path:`, guillemet non fermé, `-mot`, caractères spéciaux de regex, emoji avant et après un mot, espaces seuls, 500 caractères, `tag:#`, `""`, `*`…) : aucune erreur, aucun rejet, réponse en 3 à 29 ms, « Aucun résultat » ou les résultats du mot valide.
- Fermeture pendant la première indexation de x20 (profil neuf, fermeture de la fenêtre au premier `tags-progress`, 0 note lue sur 9 760) : le processus se termine normalement. À la relance, la session est restaurée, l'index arrive depuis le cache avec 9 760 notes, `reread: 0`, la recherche répond (3 685 fichiers pour « plan »), 4 199 tags. L'app n'a pas planté. Je ne peux pas dire si l'indexation a « repris » : au vu du résultat, elle s'est terminée et a été enregistrée avant la sortie du processus.

## Corrections faites dans l'interface

1. Le terme trouvé est sélectionné à l'ouverture (`SearchMatch.len`).
2. Découpage des mots aligné sur le cœur pour le surlignage dans la note (lettres et chiffres ; `œ`/`oe`, `æ`/`ae`).
3. Un second résultat ouvert moins de 2,6 s après le premier perdait son surlignage de ligne (minuterie du premier) : corrigé.
4. Temps de recherche affiché avec une décimale (« 5,9 ms » au lieu de « 5,884 ms »).

## Remarques sur le backend (non bloquantes)

- Aucun `index-ready` n'est émis après `save_file`. L'interface rafraîchit le panneau des liens elle-même après un enregistrement, donc rien ne manque à l'écran, mais l'info-bulle de l'index (nombre de liens) ne bouge qu'au prochain événement.
- `SearchResult.ms` vaut 0,002 à 0,14 ms pour les requêtes vides de sens : normal, rien n'est cherché.

## Non vérifié

- Vault réel, Drive, cache froid.
- Instance unique (second lancement → onglet) : non refait, les profils isolés sont des instances séparées.
- Les trois états de la barre d'état en capture (trop brefs) : textes lus, pas photographiés.
- Défilement dans la liste des liens sortants (446 lignes) à la molette : seul le temps de rendu est mesuré.
