# Plan de reprise — Rocksy (v2, 24/11/2026)

Document de passation pour une nouvelle session. Il est conçu pour être lu seul, avec `CLAUDE.md`. Il remplace tout plan antérieur.

## 1. Mission en une phrase

Remplacer Obsidian par **Rocksy**, une app rapide (démarrage ≤ 2 s pour écrire, ≤ 3 s pour l'arbre) qui lit le vault existant sans conversion, ouvre n'importe quel `.md` même hors vault, et se prolonge par un système de mods propre. Projet personnel d'Alec, seul. Aucun code pour l'instant, pas de dépôt git.

## 2. À lire d'abord, dans cet ordre

1. `CLAUDE.md` : état, architecture, décisions, contraintes de l'audit, conventions.
2. `out/docs/intent.md` : pourquoi le projet existe, objectif général, critères de réussite, principes (brouillon).
3. `out/docs/perimetre-fonctionnel.md` : ce que Rocksy doit faire (v1 / v1.x / plus tard / hors périmètre), questions ouvertes, irritants.
4. `out/docs/m2-cahier-des-charges-spikes.md` : spikes A (Tauri 2 + CodeMirror 6) et B (Rune), cas F1–F9, KPI, règle de décision.
5. `out/docs/m1-plugins-matrix.md` : mods, modes d'activation, jobs.
6. `in/brief-obsidian-rapide.md` et `in/audit-vault-obsidian.md` (le `.json` brut : grep seulement).

## 3. Décisions figées (ne pas les rouvrir)

| Sujet | Décision |
|---|---|
| Perf | T1 ≤ 2 s (note éditable), T2 ≤ 3 s (arbre). Le démarrage ne dépend pas de la taille du vault (tenir à 20x : ~10k notes). |
| M0 | Plus un go/no-go. La baseline Obsidian sert de comparaison. |
| Plugins Obsidian | Secondaires. Système de mods propre, modèle Herdr. Importeur plus tard, éventuellement. |
| Live Preview | **Indispensable en v1. Critère éliminatoire** pour la pile d'interface. |
| Canvas | Lecture seule en v1. Jamais d'écriture dans un `.canvas`. |
| Mobile | Hors v1, conservé pour plus tard. Garder le cœur Rust indépendant de l'interface. |
| Tags | Puissants : n'importe où dans le texte, fragment = bloc contenant le tag (paragraphe ou élément de liste avec enfants), clic → ouverture exacte avec surlignage, panneau des fragments, hiérarchie `a/b`, renommage global (mod `admin`), TagFolder par défaut. Bibliothèque en mémoire, recherche instantanée. |
| Hors vault | Ouvrir n'importe quel `.md` : ligne de commande + double-clic, **instance unique** (processus partagé). Aucun vault, index ni `.obsidian/` créé à côté du fichier. |
| Vaults | Reconnus seulement s'ils sont **référencés** dans la liste propre à Rocksy (dossier de données de l'app). Commande « Ajouter un vault » + proposition d'import (lecture seule) des vaults d'Obsidian au premier lancement. Fichier d'un vault référencé non ouvert : s'ouvre seul, sans charger le vault. |
| Thème | Natif, sombre, proche de Blue Topaz. Pas de CSS Blue Topaz ni de Style Settings en v1. |
| Mods par défaut | Custom Icons, Editing Toolbar, TagFolder, Outliner (proposé). Excalidraw à l'ouverture d'un `.excalidraw`. Tag Wrangler en `admin`. Templater simple. Wispr Flow Sync et scan d'orphelins en `job` Rust. |
| Contrainte dure | Vault toujours ouvrable dans Obsidian sans diff. Rien de propriétaire dans `.obsidian/`. |
| Index | Local par machine (dossier de données de l'app), jamais dans le vault. |

## 4. Règles de travail

- **Langue** : documents et réponses en français. Code et identifiants en anglais.
- **Vault réel = données personnelles**. Lecture seule. Ne jamais reproduire le contenu de notes ni les valeurs de `data.json` (clés d'API d'Excalidraw) dans le dépôt, les logs ou les fixtures. Choix de fichiers de test : par chemin et taille, sans lire le contenu.
- **Environnement** : développement sous WSL2 (Linux), mais l'app, les mesures et le vault réel sont sous **Windows 11** (`G:\Mon Drive\MonVault`, Google Drive). Le vault n'est **pas accessible depuis WSL**. Les builds Tauri/Rune et les chronos se font sous Windows (PowerShell). Vérifier dès le début comment partager le code entre les deux (par exemple `\\wsl$\...` côté Windows ou dossier Windows).
- **Chronos manuels** : `measure-coldstart.ps1` demande à Alec d'appuyer sur F8/F9. Ces runs ne peuvent pas être faits par l'assistant.
- **Ne jamais présenter une estimation comme une mesure.** Les durées de spike et les seuils (50 ms, 100 ms, 16 ms, 30 %) sont des propositions.
- Le dépôt git est initialisé (branche `main`, premier commit du 24/11). Ne pas commiter sans demande d'Alec.
- Les commandes passent par le hook RTK (`rtk`). Sans incidence sur le travail.

## 5. Ce qui existe

| Fichier | Rôle |
|---|---|
| `out/tools/gen-vault.py` | Générateur de vault synthétique déterministe (`--out`, `--scale` 1/2/5/10/20, seed `rocksy`, marqueur `.rocksy-synth/`). Contient déjà frontmatter, tags en ligne plats, liens, pièces jointes. **Ne contient pas** : tags hiérarchiques ni tags de fragments ciblés, blocs Mermaid, callouts, tableaux, embeds d'images rendables. À enrichir. |
| `out/tools/vault-kpis.ps1` / `.json` | Mesure du vault réel (à étendre : Mermaid, callouts, tableaux, tags hiérarchiques, tags en ligne). |
| `out/tools/measure-coldstart.ps1` | Chrono de démarrage (T_window, T1, T2) avec `-ProcessName` configurable. |
| `out/tools/scan-plugin-api.ps1` / `plugin-api-usage.json` | Taille de l'API `obsidian` si un shim est un jour construit. |
| `out/docs/m1-plugins-matrix.md` | Verdicts plugins, **en attente de validation d'Alec**. |

## 6. Plan d'exécution

Chaque étape donne un livrable et un critère « terminé ».

### Étape 0 — Mise en ordre (0,5 j)

- Fait le 24/11 : `git init`, rangement de `out/` (voir `out/README.md`), `CLAUDE.md` à jour.
- Demander à Alec : temps disponible par semaine ? OS cibles au-delà de Windows ? Premier commit ?
- **Terminé** quand : ces réponses sont consignées.

### Étape 1 — Prérequis des spikes (1 à 2 j)

1. **Enrichir `gen-vault.py`** (rétro-compatibilité : mêmes fichiers pour les blocs existants, sinon documenter le changement de contenu et régénérer) :
   - tags hiérarchiques `#a/b/c`, tags placés dans des paragraphes, éléments de liste avec enfants, titres, lignes de tableau, frontmatter ;
   - blocs Mermaid (flowchart, sequence, class, state, ER, gantt, pie, mindmap, `wardley-beta`) ;
   - callouts, tableaux Markdown, embeds d'images relatifs vers `_ASSETS`, wikilinks avec alias/ancres ;
   - **Terminé** quand : `--stats-only` rapporte ces éléments, génération 1x et 20x en moins de quelques minutes, déterminisme vérifié (deux générations, mêmes hash).
2. **Étendre `vault-kpis.ps1`** pour compter Mermaid par type, callouts, tableaux, tags (plats/hiérarchiques), embeds. Alec le lance sous Windows et dépose le JSON (sans contenu de notes).
3. **Choisir les fichiers de test** dans le vault réel, par chemin et taille : une note avec frontmatter + tableau + image, un canvas simple, un canvas dense, un `.md` hors vault. Travail sur une copie locale hors dépôt.
4. **Générer** les vaults synthétiques 1x et 20x en local (hors Drive).

### Étape 2 — Baseline Obsidian (0,5 j, Alec)

- Alec lance `measure-coldstart.ps1 -Case Normal -Runs 3`, plus un run après redémarrage (Drive à froid). Restreint en option.
- Ajouter la baseline « fichier hors vault » si pertinent.
- **Terminé** quand : un CSV avec médianes existe et est cité dans `out/spikes/m2-resultats.md`.

### Étape 3 — Spike B, étape 0 d'abord (0,5 j)

Avant d'investir 7 à 10 jours, vérifier sous Windows que Rune : ouvre une fenêtre, charge une police, gère clavier et IME, **et** permet un Live Preview (styles mélangés dans une ligne, remplacement d'un segment par un widget, masquage des marqueurs selon le curseur). Si l'un manque : documenter, signaler à Alec, et proposer d'abandonner B sans le construire.

- **Terminé** quand : un verdict écrit « go / no-go sur B » avec preuves (captures, extraits de code).

### Étape 4 — Spike A : Tauri 2 + CodeMirror 6 (3 à 4 j)

Voir `out/docs/m2-cahier-des-charges-spikes.md` §4 et les cas F1–F9. Points à ne pas oublier :
- instance unique et ouverture d'un chemin passé en argument (F8), même sans vault ;
- Live Preview avec décorations CM6 et widgets (F1–F3) ;
- canvas en lecture seule (F6), aucune écriture (F7) ;
- tags avec position des fragments, bibliothèque en mémoire, latence ≤ 50 / 100 ms (F9) ;
- écriture atomique de la note active, relecture sans diff dans Obsidian ;
- mesure IPC pour 1 Ko, 100 Ko, 320 Ko.
- **Terminé** quand : la grille F1–F9 est remplie et les KPI du cahier des charges sont mesurés (3 lancements, médiane) sur vault réel et 20x.

### Étape 5 — Spike B complet (7 à 10 j), seulement si l'étape 3 est « go »

Éditeur (rope, curseur, sélection, IME, undo), Live Preview F1–F3, canvas F6, arbre virtualisé, accessibilité (NVDA), mode headless pour capture PNG, mêmes cas F1–F9.

### Étape 6 — Décision (1 j)

- Remplir `out/spikes/m2-resultats.md` : tableaux, captures, grille F1–F9 pour chaque pile.
- Appliquer la règle du cahier des charges §7 : une pile éliminée sur Live Preview n'est pas retenue ; ensuite le seuil de 30 % sur T1 et l'édition utilisable départagent. Les seuils sont des propositions que Alec valide.
- **Terminé** quand : une recommandation chiffrée de pile pour M3 à M5, validée par Alec, est inscrite dans `CLAUDE.md`.

### Après M2 (cadrage seulement)

- **M3** : cœur Rust persisté : parser Markdown, graphe de liens, tags avec fragments, `tantivy` en mmap, `notify` avec debounce, snapshot de l'arbre, registre de vaults, instance unique. Cible : tenir les KPI à 20x.
- **M4** : système de mods natif avec rechargement à chaud (manifeste, modes d'activation, permissions déclarées, `check`/`status`/`shot` pour les agents).
- **M5** : store.

## 7. Actions qui ne dépendent que d'Alec

- Valider la matrice M1 (`out/docs/m1-plugins-matrix.md`).
- Lancer les scripts PowerShell (baseline, KPI, scan d'API).
- Répondre aux questions ci-dessous.

## 8. Questions ouvertes

| Question | Impact |
|---|---|
| Temps disponible par semaine et échéance | Cale les estimations des spikes. |
| OS cibles au-delà de Windows (Linux, macOS) | Influence la pile et l'installeur. |
| Requêtes de tags combinées (ET/OU/NON, `path:`) : v1 ou v1.x ? Réponse ambiguë : v1.x par défaut | Périmètre de la recherche en M3. |
| Graphe de liens : v1 ou v1.x ? Mis en v1.x par défaut | Dépend de l'index. |
| Tag dans un titre ou un tableau : titre seul / ligne ? Par défaut oui | Modèle de fragments. |
| Bases, note du jour, LaTeX : non cochés, traités comme « non utilisés » | À corriger si faux. |
| Troisième irritant d'Obsidian | Facultatif. |
| Usage personnel seulement, ou distribution un jour ? | Licences, installeur, signature. |
| Encodages et extensions hors UTF-8 `.md` | Périmètre « lecteur Markdown ». |

## 9. Pièges connus

- **Drive à froid** : 57 s pour le premier listing contre 1,3 s à chaud. Toujours noter l'état du cache. Arbre depuis snapshot persisté.
- **Dossiers à point** : 79 % des fichiers (`.opencode/node_modules`). À ignorer dans le parcours et le watcher.
- **Changements en masse** : un nettoyeur IA sur une machine réécrit beaucoup de fichiers. mtime change en bloc : taille + mtime, hash seulement si doute. Debounce, commits d'index groupés, détection des renommages.
- **Fichiers `(conflict …)`** dans `.obsidian/` (conflits Drive) : à tolérer. `core-plugins.json` fait foi, `core-plugins-migration.json` est périmé.
- **Note pire cas** : `01__WORK/daily-notes.md`, 319 Ko.
- **Rune** : pre-release, Windows non vérifié, n'accepte pas de pull requests.
- **Jobs qui écrivent dans le vault** : une seule machine désignée (Drive dupliquerait la sortie).
- **Mods qui dessinent l'interface** : réserver leur place avant leur chargement, pour éviter un saut de mise en page.
- **Ne rien mettre d'« adopté » à côté d'un `.md` hors vault** : aucun dossier, index ni `.obsidian/`.
