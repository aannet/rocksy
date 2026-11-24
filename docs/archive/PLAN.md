# Plan de reprise — Rocksy (v3, 24/11/2026)

Document de passation pour une nouvelle session. Il est conçu pour être lu seul, avec `CLAUDE.md`. Il remplace tout plan antérieur.

## 1. Mission en une phrase

Remplacer Obsidian par **Rocksy**, une app rapide (démarrage ≤ 2 s pour écrire, ≤ 3 s pour l'arbre) qui lit le vault existant sans conversion, ouvre n'importe quel `.md` même hors vault, et se prolonge par un système de mods propre. Projet personnel d'Alec, seul, publication probable sur GitHub ensuite. Aucun code pour l'instant. Dépôt git initialisé.

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
- **Chronos manuels** : `measure-coldstart.ps1` demande à Alec d'appuyer sur F8/F9. Ces runs ne peuvent pas être faits par l'assistant. Les builds Windows non plus, sauf si l'assistant a un accès à une machine Windows.
- **Ne jamais présenter une estimation comme une mesure.** Les durées et les seuils (50 ms, 100 ms, 16 ms, 30 %) sont des propositions.
- **Parallélisme** : utiliser des sous-agents (avec worktrees) pour les chantiers indépendants (générateur, spike A, spike B), et garder les décisions et validations dans la session principale.
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

## 6. Plan d'exécution : la journée du 24/11

**Objectif du jour (Alec) : une release 0.1 prête aujourd'hui, sans y passer les soirées.** Le spike comparatif est **conservé et passe en premier** : il choisit la pile, puis la 0.1 se construit dessus. Les itérations suivantes (soirées) sont de la recette et de l'amélioration.

### Comment lire les durées

Les durées de l'ancien plan (jours) étaient des durées de travail humain. Ici les chantiers sont réalisés par l'IA, avec des sous-agents en parallèle : les durées sont des **ordres de grandeur en heures, estimations non mesurées, à recaler après la première heure**. Le vrai goulot n'est pas l'écriture du code : ce sont les **actions d'Alec** (build sous Windows, chronos F8/F9, validations) et le **Drive à froid**. Le plan est donc organisé pour les paralléliser et limiter les aller-retours.

### Étape 0 — Cadrage et environnement (≈ 30 min)

1. **Vérifier la chaîne Windows** avec Alec : Rust (`rustup`, cible MSVC) et Visual Studio Build Tools, Node.js, WebView2 (inclus dans Windows 11). Sans ça, rien ne se compile.
2. **Décider où vit le code pour les builds** : le dépôt est sous WSL. Option recommandée : cloner le dépôt côté Windows (`git clone \\wsl$\...`) et compiler là, avec l'IA qui édite côté WSL puis Alec (ou un script) qui synchronise et compile. À confirmer avec Alec.
3. **Premier contact avec Rune** (étape 0 du spike B) : ouvre-t-il une fenêtre sous Windows, rend-il du texte, permet-il un Live Preview ? **Porte d'entrée** : si non, B est abandonné pour aujourd'hui et on passe directement à A.
4. Consigner les réponses restantes (OS cibles, premier commit de la suite).

**Terminé** quand : la chaîne Windows compile un « hello world » Tauri, et le verdict de l'étape 0 de B est écrit.

### Étape 1 — Prérequis en parallèle (≈ 1 h)

Deux sous-agents indépendants, pendant que le spike démarre :
- **Générateur** : enrichir `out/tools/gen-vault.py` : tags hiérarchiques et placés en paragraphes, éléments de liste avec enfants, titres, lignes de tableau, frontmatter ; blocs Mermaid (flowchart, sequence, class, state, ER, gantt, pie, mindmap, `wardley-beta`) ; callouts ; tableaux ; embeds d'images relatifs ; wikilinks avec alias et ancres. Rétro-compatible (mêmes fichiers pour les blocs existants, sinon documenter et régénérer). Générer 1x et 20x en local, hors Drive. **Terminé** : `--stats-only` rapporte ces éléments, déterminisme vérifié.
- **KPI du vrai vault** : étendre `out/tools/vault-kpis.ps1` (Mermaid par type, callouts, tableaux, tags plats/hiérarchiques, embeds). **Alec le lance** sous Windows ; sortie dans `out/data/`, sans contenu de notes.
- Choix des fichiers de test dans le vrai vault, par chemin et taille, sans lire leur contenu (une note avec frontmatter, tableau et image ; un canvas simple et un dense ; un `.md` hors vault), travaillés sur une copie locale hors dépôt.

### Étape 2 — Les deux spikes en parallèle (≈ 3 à 5 h, estimation)

Deux sous-agents en isolation (worktrees), un par pile, sur le même cahier des charges (`out/docs/m2-cahier-des-charges-spikes.md`, cas F1 à F9). Le code va dans `out/spikes/a-tauri-cm6/src/` et `out/spikes/b-rune/src/`.
- **Spike A (Tauri 2 + CM6)** : écrit comme base réutilisable de la 0.1 : une crate Rust de cœur séparée de l'interface (accès vault, parseur, tags, ouverture de fichier, instance unique) et un frontend CM6. Ne pas en faire un jouet à jeter.
- **Spike B (Rune)** : seulement si l'étape 0 a passé. Même cœur Rust (la crate de A est partagée si possible), éditeur et Live Preview natifs.
- Chaque spike remplit sa `grille.md` (F1 à F9) et ses `runs/` datés.

### Étape 3 — Mesures (≈ 30 à 45 min, actions d'Alec)

Alec lance `measure-coldstart.ps1` (3 lancements par cas, médiane) sur Obsidian (baseline), spike A, spike B, sur le vault réel puis le 20x, plus un lancement après redémarrage (Drive à froid). L'assistant prépare les commandes exactes, avec `-RunsRoot` pour chaque spike. Les sorties vont dans des dossiers datés.

### Étape 4 — Décision (≈ 30 min)

Appliquer la règle de `out/docs/m2-cahier-des-charges-spikes.md` §7 : une pile éliminée sur Live Preview est écartée, ensuite T1/T2 et qualité d'édition. Remplir `out/spikes/m2-resultats.md`. **Plafond de temps : si B n'est pas clairement meilleur en fin d'étape 3, on retient A et on continue.** Alec valide.

### Étape 5 — Release 0.1 (reste de la journée)

Construite sur la pile retenue, à partir du code du spike gagnant.

**Dans la 0.1 :**
1. Ouvrir n'importe quel `.md` : argument de ligne de commande, association de fichiers (double-clic), **instance unique**.
2. Éditeur avec Live Preview (titres, gras/italique, liens `[[ ]]` et `[](...)`, tâches, listes, citations, code) ; tableaux, images, callouts si le spike les a déjà couverts.
3. Enregistrement atomique, vault toujours ouvrable dans Obsidian sans diff.
4. « Ajouter un vault », arbre des fichiers (dossiers à point ignorés), snapshot de l'arbre si le temps le permet.
5. Ctrl+O, onglets, thème sombre.
6. **Tags** : `#tag` dans le texte et le frontmatter, hiérarchie `a/b`, fragment = bloc contenant le tag, index en mémoire reconstruit à l'ouverture, panneau de recherche par fragments, clic → ouverture à l'endroit exact. Sans persistance en 0.1 : à surveiller sur le vrai vault sur Drive.
7. Build Windows, version `0.1.0`, étiquette git, README d'installation.

**Hors 0.1 (itérations de recette, dans cet ordre proposé) :** persistance de l'index et des tags, renommage global de tags, backlinks et recherche plein texte (`tantivy`), canvas en lecture seule, Mermaid, splits, restauration du workspace, import des vaults d'Obsidian, mods, store.

**Ce que « prête aujourd'hui » veut dire :** un exécutable qui se lance, ouvre et enregistre des fichiers, sans corrompre le vault. Ce n'est pas encore le remplaçant d'Obsidian : le critère « une semaine sans Obsidian » se juge après plusieurs itérations.

**Avant publication sur GitHub :** choisir une licence, retirer ou anonymiser `in/` et `out/data/` (chemins et tailles du vrai vault), vérifier qu'aucune donnée personnelle n'est dans l'historique git, vérifier le nom du projet.

### Au-delà de la 0.1 (cadrage)

- **M3** : cœur Rust persisté : parseur, graphe de liens, tags avec fragments, `tantivy` en mmap, `notify` avec debounce, snapshot de l'arbre, registre de vaults.
- **M4** : mods natifs avec rechargement à chaud (manifeste, modes d'activation, permissions déclarées, `check`/`status`/`shot` pour les agents).
- **M5** : store.

## 7. Actions qui ne dépendent que d'Alec

- Valider la matrice M1 (`out/docs/m1-plugins-matrix.md`).
- Lancer les scripts PowerShell (baseline, KPI, scan d'API).
- Répondre aux questions ci-dessous.

## 8. Questions ouvertes

| Question | Impact |
|---|---|
| ~~Échéance~~ : réglée, 0.1 aujourd'hui (24/11), soirées réservées à la recette | |
| OS cibles au-delà de Windows (Linux, macOS) | Influence la pile et l'installeur. |
| Requêtes de tags combinées (ET/OU/NON, `path:`) : v1 ou v1.x ? Réponse ambiguë : v1.x par défaut | Périmètre de la recherche en M3. |
| Graphe de liens : v1 ou v1.x ? Mis en v1.x par défaut | Dépend de l'index. |
| Tag dans un titre ou un tableau : titre seul / ligne ? Par défaut oui | Modèle de fragments. |
| Bases, note du jour, LaTeX : non cochés, traités comme « non utilisés » | À corriger si faux. |
| Troisième irritant d'Obsidian | Facultatif. |
| ~~Distribution~~ : usage perso, publication GitHub probable ensuite | Licence, anonymisation de `in/` et `out/data/`, nom du projet : à traiter avant publication. |
| Où compiler : clone Windows du dépôt, ou autre ? | Premier point de l'étape 0. |
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

## 10. Prompt de lancement (à coller dans la nouvelle session)

```
Lis CLAUDE.md puis out/PLAN.md, out/docs/intent.md, out/docs/perimetre-fonctionnel.md
et out/docs/m2-cahier-des-charges-spikes.md. Nous sommes le 24/11/2026.
Objectif du jour : release 0.1 de Rocksy prête ce soir, après le spike comparatif
Tauri 2 + CodeMirror 6 contre Rune (spike conservé, il passe en premier).
Exécute le plan de la section 6 de out/PLAN.md, étape par étape.
Règles : les durées sont en temps IA (estimations à recaler après une heure) ;
parallélise avec des sous-agents en worktrees pour le générateur et les deux spikes ;
demande-moi seulement ce que je suis seul à pouvoir faire (build Windows, chronos F8/F9,
validations) ; ne commite que si je le demande ; vault réel en lecture seule,
jamais de contenu de notes ni de valeurs de data.json dans le dépôt.
Commence par l'étape 0 : vérifie la chaîne Windows, dis-moi où compiler, et donne-moi
le verdict de l'étape 0 du spike Rune.
```
