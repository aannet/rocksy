# Intention de Rocksy (v0.1, brouillon)

Statut : **brouillon à valider par Alec**. Il rassemble ce qui a été dit dans le brief, l'audit et les décisions des 22 au 24/11/2026. Ce qui est déduit et non dit est marqué *(déduit)*.

## 1. Pourquoi ce projet existe

Obsidian est devenu trop lent à s'ouvrir (5,98 s de démarrage mesuré, dont 67 % dus aux 19 plugins communautaires) et trop rigide : il ne sert pas de simple lecteur Markdown, tout passe par un vault. Alec veut un outil qui s'ouvre tout de suite, sur ses notes actuelles, et qu'il peut faire évoluer lui-même.

## 2. Objectif général

> Une app de notes Markdown **instantanée**, qui lit le vault Obsidian existant sans conversion, ouvre n'importe quel fichier `.md` avec ou sans vault, et s'étend par ses propres mods.

Trois promesses, dans l'ordre de priorité :

1. **Vitesse.** Note éditable en ≤ 2 s, arbre navigable en ≤ 3 s, à froid, sur Google Drive, et sans dégradation avec 20 fois plus de fichiers.
2. **Liberté d'usage.** Lecteur/éditeur de n'importe quel Markdown (ligne de commande, double-clic, instance unique), sans vault obligatoire.
3. **Continuité.** Le vault reste ouvrable dans Obsidian à tout moment, sans diff. Rien de propriétaire dans `.obsidian/`.

## 3. Pour qui, et dans quel contexte

- **Utilisateur** : Alec, seul. Usage personnel. Distribution éventuelle : non tranchée.
- **Contexte d'usage** : Windows 11, vault ~950 fichiers visibles sur Google Drive, plusieurs machines synchronisées, un nettoyeur IA qui réécrit des fichiers en masse. Obsidian mobile continue d'être utilisé en parallèle.
- **Usages quotidiens connus** : écrire dans un gros journal (319 Ko), retrouver par tags (y compris sur des fragments), liens et backlinks, Mermaid, tableaux, callouts, lecture des canvas.

## 4. Ce que Rocksy n'est pas

- Pas un clone complet d'Obsidian : Bases, note du jour dédiée, LaTeX, édition de canvas, mobile et compatibilité des plugins/thèmes Obsidian sont hors v1.
- Pas un remplaçant de Sync et Publish.
- Pas un outil qui prend le contrôle du vault : il le lit, l'écrit proprement, ne le convertit pas.

## 5. Comment on saura que c'est réussi

| Critère | Seuil | Source |
|---|---|---|
| T1 : frappe acceptée dans la dernière note | ≤ 2 s (≤ 1 s au spike M2) | `measure-coldstart.ps1`, médiane |
| T2 : arbre navigable | ≤ 3 s | idem |
| Tenue à l'échelle | T1 et T2 stables à 20x | vault synthétique 20x |
| Live Preview | présent, frappe p95 ≤ 16 ms sur 319 Ko | spike M2 |
| Hors vault | `.md` isolé ouvert via CLI/double-clic, rien créé à côté | cas F8 |
| Continuité | vault rouvert dans Obsidian sans diff | contrôle par hash |
| Remplacement réel *(déduit)* | une semaine de travail normale sans rouvrir Obsidian | ressenti d'Alec |

## 6. Principes qui guident les arbitrages

1. **Rien avant le premier rendu.** Plugins, index, thèmes chargent après.
2. **Le démarrage ne dépend pas de la taille du vault.** Snapshot de l'arbre, index mmap, réconciliation en tâche de fond.
3. **Le vault est sacré.** Lecture seule par défaut, écritures atomiques et explicites, jamais de données propriétaires dans `.obsidian/`.
4. **Tout ce qui n'est pas le cœur est un mod**, avec un manifeste, des permissions déclarées et un mode d'activation.
5. **On mesure avant de choisir.** La pile d'interface (Tauri + CodeMirror 6 ou natif) se décide sur spike, avec Live Preview comme critère éliminatoire.
6. **Le cœur Rust est indépendant de l'interface**, pour garder le mobile possible plus tard.

## 7. Jalons

M0 baseline · M1 audit plugins · **M2 spike comparatif d'interface** · M3 index Rust persisté · M4 mods natifs avec rechargement à chaud · M5 store. Détail : `../PLAN.md`.

## 8. Réponses d'Alec (24/11/2026)

- **Ordre des trois promesses** (vitesse, liberté d'usage, continuité) : confirmé.
- **Raison de fond** : il est devenu très simple de construire ses propres logiciels. Alec est ingénieur, donc **plaisir de construire**. La vitesse est le motif pratique, le plaisir de construire le moteur.
- **Distribution** : usage strictement personnel pour l'instant, **publication probable sur GitHub ensuite**. Conséquences : licence à choisir avant la publication, aucune donnée personnelle dans le dépôt (le dossier `in/` et `out/data/` contiennent des chemins et tailles du vrai vault : à retirer ou à anonymiser avant toute publication), documentation d'installation, nom du projet à vérifier.
- **Échéance** : **la première release doit être prête aujourd'hui (24/11/2026)**, sans y passer les soirées. Les itérations suivantes sont de la recette, de l'amélioration et de la guidance.
- **Critère final** : confirmé, **une semaine de travail sans rouvrir Obsidian**.

## 9. Conséquence : « release 0.1 » du jour

La première release n'est pas la v1 du périmètre fonctionnel. Elle est un **plus petit sous-ensemble utilisable**, livré aujourd'hui. Le périmètre complet (`perimetre-fonctionnel.md`) devient la feuille de route des itérations. Décision d'Alec (24/11) : le spike comparatif est conservé et passe en premier, puis la 0.1 est construite sur la pile retenue, tags inclus. Voir `../PLAN.md` §6 (étapes 0 à 5).

Le critère « une semaine sans Obsidian » ne s'applique pas à la 0.1 : il se mesure à la fin des itérations de recette.
