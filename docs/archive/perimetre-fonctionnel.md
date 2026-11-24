# Périmètre fonctionnel de Rocksy (v0.1, brouillon)

Statut : **brouillon, partiellement validé par Alec** (24/11/2026 : mobile, Live Preview, canvas, tags, ouverture hors vault). Le reste est proposé, sauf mention « Décidé ».
Sources : `in/brief-obsidian-rapide.md`, `in/audit-vault-obsidian.md` (core plugins activés, workspace, config), `out/docs/m1-plugins-matrix.md` et les décisions du `CLAUDE.md`.

## 1. Comment lire ce document

Chaque fonction a un niveau :

| Niveau | Sens |
|---|---|
| **v1** | Indispensable pour que Rocksy remplace Obsidian chez Alec au quotidien. |
| **v1.x** | Utile, arrive juste après la v1 (souvent sous forme de mod). |
| **Plus tard** | Intéressant, pas avant M5. |
| **Hors périmètre** | Volontairement exclu. |

Chaque ligne indique aussi la **preuve d'usage** : ce que l'audit montre que tu utilises réellement. « Activé » veut dire que le core plugin est actif dans ton vault, ce qui ne prouve pas un usage fréquent. C'est la principale chose à vérifier avec toi.

## 2. Critère d'acceptation global

Rocksy remplace Obsidian quand tu peux faire une semaine de travail normale sans rouvrir Obsidian, sur ton vrai vault, avec les perfs du brief (T1 ≤ 2 s, T2 ≤ 3 s). Les contraintes dures, valables pour tout :

- Le vault reste ouvrable dans Obsidian sans diff. Rocksy n'écrit jamais de données propriétaires dans `.obsidian/`. *(Décidé)*
- Aucune conversion de fichiers. *(Décidé)*
- Un document Markdown ouvert hors vault n'est jamais « adopté » : aucune création de vault, d'index ni de `.obsidian/` à côté de lui. *(Décidé)*
- Rien ne charge avant le premier rendu. *(Décidé)*

## 3. Fonctions de base

### 3.1 Édition de notes

| Fonction | Niveau proposé | Preuve d'usage / remarque |
|---|---|---|
| Éditeur Markdown avec frappe immédiate | **v1** | Objectif T1. Pire cas : note de 319 Ko. |
| Rendu en direct (Live Preview : titres, gras, liens rendus pendant la frappe) | **v1, indispensable** *(Décidé)* | Critère éliminatoire au M2 : une pile sans Live Preview crédible sur la note de 319 Ko est écartée. |
| Mode source / mode lecture | **v1** | Standard. |
| Largeur de ligne lisible (`readableLineLength`) | **v1** | Activé dans `app.json`. |
| Listes, indentation, tâches `- [ ]` | **v1** | Outliner est un mod « must-have » proposé. |
| Tableaux Markdown (affichage et édition) | **v1** *(Décidé : utilisés)* | Rendu en Live Preview (F2) et édition confortable. |
| Blocs de code avec coloration | **v1** | À confirmer. |
| Notes de bas de page | **Plus tard** | Core plugin `footnotes` désactivé. |
| Mathématiques (LaTeX) | **Plus tard** *(déduit : non coché le 24/11)* | À reprendre si le besoin apparaît. |
| Mermaid | **v1** *(Décidé : utilisé)* | Pele côté web, ou moteur Rust. Voir `CLAUDE.md` et M2 §6. |
| Callouts (`> [!note]`) | **v1** *(Décidé : utilisés)* | Rendu en Live Preview, avec le thème. |
| Barre d'outils d'édition | **v1** (mod par défaut) | Editing Toolbar. Décidé. |
| Raccourcis personnalisables | **v1** | 5 raccourcis perso : indent/outdent liste, déplacer ligne haut/bas, insérer template. |
| Annuler/refaire, recherche dans la note, remplacer | **v1** | Standard. |
| Coller une image, la renommer, la ranger dans `_ASSETS` | **v1** | `attachmentFolderPath` + Paste image rename. |

### 3.2 Liens et structure

| Fonction | Niveau | Preuve d'usage / remarque |
|---|---|---|
| Wikilinks `[[note]]`, `[[note#titre]]`, alias | **v1** | 362 wikilinks dans le vault, graphe peu dense. |
| Autocomplétion des liens | **v1** | Sinon l'écriture est ralentie. |
| Mise à jour des liens au renommage (`alwaysUpdateLinks`) | **v1** | Activé. Risque de corruption du vault : tests dédiés. |
| Embeds `![[...]]` (notes, images, PDF) | **v1** | 381 embeds. Images : indispensables (420 PNG). |
| Backlinks et liens sortants (panneaux) | **v1** | Core plugins activés, panneau droit configuré. |
| Aperçu au survol (page-preview) | **v1.x** | Activé. |
| Plan de la note (outline) | **v1** | Activé, panneau droit. |
| Graphe de liens | **v1.x** *(utilisé ; priorité à confirmer)* | Vue graphe sur un graphe peu dense (362 liens). Dépend du graphe de liens de l'index (M3). |
| Compteur de mots | **v1.x** | Activé. |

### 3.3 Fichiers et navigation

| Fonction | Niveau | Preuve d'usage / remarque |
|---|---|---|
| Arbre des fichiers (explorateur) | **v1** | Objectif T2. Snapshot persisté. |
| Créer / renommer / déplacer / supprimer (vers `.trash`) | **v1** | `promptDelete: true`. |
| Dossier des nouvelles notes (`00__INBOX`) | **v1** | Dans `app.json`. |
| Onglets, groupes d'onglets, splits | **v1** | Workspace : 7 onglets, 3 groupes, 3 splits. Restaurer le workspace au lancement. |
| Panneaux latéraux gauche/droite repliables | **v1** | Gauche : recherche, fichiers, favoris. |
| Palette de commandes | **v1** | Activée, et base du système de mods. |
| Sélecteur rapide de notes (Ctrl+O) | **v1** *(Décidé : utilisé)* | Recherche floue du nom, sur la liste des fichiers du snapshot, disponible avant l'index complet. |
| Favoris (bookmarks) | **v1.x** | Activé, panneau gauche. |
| Couleur et compteur dans l'arbre | **v1.x** | File Color, File Explorer Note Count : mods natifs. |
| Notes quotidiennes | **Pas de fonction dédiée** *(déduit : non coché le 24/11)* | `daily-notes.md` reste un gros fichier ordinaire. |
| Ouverture de la dernière note au lancement | **v1** | Cœur de T1. |
| Fichiers non Markdown (images, PDF) : visualisation | **v1** | 420 PNG, 10 PDF. |
| **Ouvrir un document Markdown hors vault** | **v1** *(Décidé)* | Voir §3.6. |
| Corbeille et récupération de fichiers | **v1.x** | `.trash` présent, file-recovery activé. |

### 3.4 Recherche et métadonnées

| Fonction | Niveau | Preuve d'usage / remarque |
|---|---|---|
| Recherche plein texte | **v1** | Panneau gauche ; `tantivy`, en tâche de fond. |
| Syntaxe de recherche d'Obsidian (`tag:`, `path:`…) | **v1.x** | À définir : quels opérateurs utilises-tu ? |
| Propriétés / frontmatter (lecture, édition) | **v1** | 468 notes sur 488 ont un frontmatter. Éditeur de propriétés : à décider. |
| **Tags : gestion complète** | **v1, important** *(Décidé : « assez puissante » à reproduire)* | Voir détail ci-dessous. |
| · Détection des tags `#tag` dans le texte et `tags:` dans le frontmatter | **v1** | Les deux sources, sans doublon. Règles de syntaxe d'Obsidian (caractères autorisés, pas de tag purement numérique). |
| · Tags hiérarchiques `#a/b/c` | **v1** *(Décidé : indispensable)* | Un tag parent regroupe ses enfants, en recherche comme dans le panneau. |
| · Panneau des tags : arbre, compteurs, clic pour filtrer | **v1** | Core plugin `tag-pane` activé, panneau droit. Calculé depuis l'index persisté, jamais par relecture des fichiers (Drive). |
| · **Tags sur des fragments** *(Décidé)* : étiqueter un fichier entier ou un passage à l'intérieur d'un fichier | **v1** | Un `#tag` dans le texte s'applique à un fragment, pas seulement au fichier. L'index garde la position (fichier, plage de lignes, extrait) de chaque occurrence. Les résultats d'une recherche de tag montrent le fragment avec son contexte, pas seulement le nom du fichier. Aucune donnée propriétaire dans les notes : ce sont des `#tag` ordinaires, relus tels quels par Obsidian. **Position libre** *(Décidé)* : un tag peut être placé absolument n'importe où (corps, titre, liste, citation, tableau, frontmatter). **Portée du fragment** *(Décidé)* : le bloc qui contient le tag, c'est-à-dire le paragraphe, ou l'élément de liste avec ses sous-éléments. Pour un tag dans un titre, le bloc est le titre seul (à confirmer en pratique). Pour le frontmatter, il s'applique au fichier entier. Pas de syntaxe de plage explicite en v1. |
| · Clic sur un résultat de tag | **v1** *(Décidé)* | Ouvre la note à l'endroit exact, fragment surligné. Un panneau liste tous les fragments d'un tag, chacun avec un aperçu. |
| · **Bibliothèque de tags et recherche instantanée** *(Décidé)* | **v1** | Tous les tags connus sont en mémoire (chargés depuis l'index persisté). Taper un tag filtre la bibliothèque et affiche les fragments correspondants en quelques millisecondes, sans attendre la fin de l'indexation en tâche de fond. Cible proposée : résultats affichés en moins de 50 ms pour la liste de tags et moins de 100 ms pour les fragments, sur le vault synthétique 20x. À valider. |
| · Recherche par tag (`tag:#a/b`) | **v1** | Recherche simple. Les combinaisons ET/OU/NON et `path:` : **réponse ambiguë** (voir §7), v1.x par défaut. |
| · Autocomplétion des tags à la frappe | **v1** | Standard. |
| · TagFolder (notes organisées en arbre par tag) | **v1** (mod par défaut) | Décidé. |
| · Renommer / fusionner un tag dans tout le vault | **v1** *(Décidé : indispensable ; livré comme mod `admin`)* | Tag Wrangler. Écrit dans de nombreux fichiers : aperçu, annulation possible, jamais sans confirmation. |
| · Tags dans les propriétés (liste, édition) | **v1** | Lié à l'éditeur de propriétés. |
| Bases (core plugin) | **Hors v1** *(déduit : non coché le 24/11)* | Core plugin activé mais pas d'usage déclaré. À reprendre si des fichiers `.base` apparaissent. |
| Dataview / requêtes | **Hors périmètre** (à confirmer) | Absent des plugins actifs. |

### 3.5 Contenu spécial

| Fonction | Niveau | Remarque |
|---|---|---|
| Canvas (`.canvas`) | **Lecture seule en v1** *(Décidé)* ; édition : plus tard | 10 fichiers. Affichage fidèle, aucune écriture : un `.canvas` ne doit jamais être modifié par Rocksy. |
| Excalidraw | **v1.x** (mod, `on-file-type`) | 2 fichiers `.excalidraw`. Décidé : désactivé par défaut. |
| PDF : lecture | **v1** | 10 fichiers. |
| `.docx`, `.odt`, `.gdoc` | **Hors périmètre** | Ouverture dans l'app externe. |
| Templates | **v1** (mod, `on-command`) | Templater simple, sans scripts. Décidé. |
| Export PDF / copie en HTML | **v1.x** | `pdfExportSettings` configuré, mod Copy as HTML. |
| Diaporama, enregistreur audio, Web viewer | **Hors périmètre** | Core plugins désactivés. |

### 3.6 Ouvrir n'importe quel Markdown, dans ou hors du vault *(Décidé)*

Rocksy doit pouvoir ouvrir un fichier `.md` quel qu'il soit, même s'il n'est dans aucun vault : un fichier du Bureau, un clone Git, un fichier d'un autre dossier ou d'un autre disque.

| Fonction | Niveau | Remarque |
|---|---|---|
| Ouvrir un `.md` passé en argument de ligne de commande (`rocksy fichier.md`) | **v1** *(Décidé)* | Base de tout le reste. |
| Association de fichiers Windows (double-clic, « Ouvrir avec ») | **v1** *(Décidé)* | Installeur : à définir en M3 ou plus tard, mais l'architecture doit le permettre dès le spike. |
| Glisser-déposer d'un `.md` sur la fenêtre | **v1** | |
| Éditeur complet sur ce fichier (Live Preview, enregistrement atomique, annuler/refaire) | **v1** | Même éditeur, mêmes perfs. Pas de mode dégradé. |
| **Aucun vault requis** | **v1** | Pas d'arbre de vault, pas d'index, pas de création de `.obsidian/` dans le dossier du fichier. Interdit d'écrire quoi que ce soit à côté du fichier, sauf l'enregistrement du fichier lui-même. |
| Démarrage rapide pour ce cas | **v1** | Ce chemin ne dépend pas du vault : il doit être le plus rapide de tous, sans attendre snapshot ni index. À mesurer en T1 (fichier hors vault). |
| Liens relatifs et images relatives (`![](img/a.png)`, `[x](autre.md)`) | **v1** | Résolus par rapport au dossier du fichier. Les wikilinks `[[...]]` sont résolus dans le vault courant si on en a un, sinon affichés comme non résolus. |
| Fichier hors vault ouvert pendant qu'un vault est ouvert | **v1** | S'ouvre dans un onglet de la même fenêtre, marqué « hors vault ». Les backlinks, tags et recherche du vault n'incluent pas ce fichier. |
| **Fichier situé dans un vault** ouvert seul *(Décidé)* | **v1** | Il s'ouvre tout seul, tout de suite, sans charger le vault. Rocksy ne devine pas qu'un dossier est un vault. Voir « Vaults référencés » ci-dessous. |
| **Instance unique** : un second lancement (ligne de commande ou double-clic) transmet le chemin au processus déjà ouvert, qui ouvre un onglet dans sa fenêtre et la met au premier plan | **v1** *(Décidé : instance unique ; processus partagé choisi par Claude)* | Raisons : un seul watcher, un seul index et un seul verrou sur le dossier de données, pas de WebView ni de mémoire dupliquées, et ouverture d'un second fichier quasi instantanée (pas de démarrage à froid). Le premier lancement reste le chemin à optimiser en T1. Détail technique : mutex/pipe nommé sous Windows. Une fenêtre supplémentaire à la demande (« nouvelle fenêtre ») reste possible dans le même processus, plus tard. |
| Ouvrir un dossier quelconque comme espace de travail sans vault | **Plus tard** | Variante possible ensuite. |
| Fichiers `.markdown`, `.txt`, encodages non UTF-8 | **À demander** | UTF-8 seulement en v1 ? |
| Fichier ouvert hors vault qui change sur disque (autre éditeur) | **v1** | Détection et rechargement, avec avertissement si modifications non enregistrées. |

**Vaults référencés** *(Décidé)* : Rocksy ne reconnaît un vault que s'il est **référencé** (ajouté à une liste de vaults connus) et qu'il peut l'ouvrir normalement. Un dossier contenant `.obsidian/` mais non référencé reste un dossier ordinaire : ouvrir un fichier dedans l'ouvre comme fichier isolé. La liste est stockée par Rocksy dans son dossier de données (jamais dans le vault ni dans `.obsidian/`). Si le fichier appartient à un vault référencé déjà ouvert, il s'ouvre dans ce vault comme une note normale ; si ce vault n'est pas ouvert, le fichier s'ouvre seul, sans déclencher le chargement du vault *(confirmé)*.

**Ajouter un vault** *(Décidé)* : une commande « Ajouter un vault » (choix d'un dossier) est prévue. **Au premier lancement**, Rocksy propose d'importer les vaults d'Obsidian : lecture seule de la liste d'Obsidian (`obsidian.json` dans son dossier de configuration), choix vault par vault, rien n'est écrit chez Obsidian ni dans les vaults, et l'import peut être refusé ou refait plus tard. Ajouter un vault ne le modifie pas et ne lance pas d'indexation longue avant que tu l'ouvres.

## 4. Plateforme et comportement système

| Sujet | Niveau | Remarque |
|---|---|---|
| Windows 11 desktop | **v1** | Machine de référence. |
| Linux, macOS | **À demander** | Brief : OS cibles non précisés. WSL ne voit pas le vault. |
| Mobile | **Pas en v1, conservé pour plus tard** *(Décidé)* | Tu utilises Obsidian mobile en parallèle (`workspace-mobile.json`). Il continue donc de lire le vault, et Rocksy ne doit rien casser pour lui. Garder le cœur Rust portable. |
| Thème sombre natif proche de Blue Topaz | **v1** *(Décidé)* | Thème intégré, sobre. Pas de chargement du CSS de Blue Topaz (1,3 Mo) ni de Style Settings en v1 : la compatibilité avec les thèmes d'Obsidian est repoussée à plus tard. |
| Snippets CSS | **v1.x** | Aucun actif pour l'instant, mais Custom Icons en génère un. |
| Vault synchronisé par Drive, plusieurs machines | **v1** | Fichiers `(conflict …)` tolérés. Index local par machine. |
| Détection des changements externes (IA qui réécrit en masse) | **v1** | Watcher avec debounce, voir `CLAUDE.md`. |
| Mode restreint, sécurité des mods | **v1** | Permissions déclarées dans le manifeste. |

## 5. Système de mods (déjà décidé, résumé)

Tout ce qui n'est pas le cœur est un mod avec un manifeste et un mode d'activation (`post-paint`, `on-file-type`, `on-command`/`on-event`, `admin`, `job`). Voir `out/docs/m1-plugins-matrix.md` pour la matrice complète.

- **Par défaut** : Custom Icons, Editing Toolbar, TagFolder, Outliner (proposé).
- **Jobs natifs** : Wispr Flow Sync, scan des médias orphelins.
- **Hors v1** : Compatibilité avec les plugins Obsidian (importeur plus tard). Store : M5.

## 6. Hors périmètre (explicite)

- Obsidian Sync et Publish. Remplacement par Drive ou Git.
- Collaboration temps réel.
- Écrire des données propres à Rocksy dans `.obsidian/`.
- Conversion du vault dans un autre format.
- Plugins Obsidian s'appuyant sur des internals non documentés (cas par cas).

## 7. Questions ouvertes pour Alec

Nouvelles (24/11) :

Réponses du 24/11 (formulaire) : tags hiérarchiques et renommage global indispensables ; graphe et Ctrl+O utilisés ; callouts, Mermaid et tableaux utilisés ; thème natif proche de Blue Topaz. Les options non cochées (Bases, note du jour, LaTeX) sont traitées comme « non utilisées » : à corriger si c'est faux.

Reste ouvert :

- **Requêtes de tags combinées** (ET/OU/NON, avec `path:`) : tu as coché « Aucun en v1 » en même temps que deux autres choix. J'ai gardé : hiérarchie et renommage en v1, requêtes combinées en v1.x. À confirmer.
- **Titre et tableau** : j'ai retenu par défaut le titre seul et la ligne du tableau.
- **Irritants** : voir §7 bis. Deux sur trois sont connus, le troisième reste à donner (facultatif).
- **Thème natif proche** : *(Décidé)* ; seule la compatibilité avec les thèmes Obsidian est reportée à plus tard.

## 7 bis. Irritants d'Obsidian à corriger *(Alec, 24/11)*

| # | Irritant | Réponse dans Rocksy |
|---|---|---|
| 1 | **Lenteur à l'ouverture** | Objectif central : T1 ≤ 2 s, T2 ≤ 3 s, démarrage indépendant de la taille du vault. |
| 2 | **Impossible d'utiliser Obsidian comme simple lecteur Markdown** : tout passe par un vault, même pour ouvrir un fichier isolé | §3.6 : ouverture de n'importe quel `.md` sans vault, par ligne de commande ou double-clic, instance unique, sans rien créer à côté du fichier. |
| 3 | *(non donné)* | À compléter si un autre irritant te vient. |

Conséquence de conception : le mode « lecteur/éditeur de fichier isolé » n'est pas une fonction annexe. C'est un parcours de premier rang, avec sa propre mesure (F8) et le démarrage le plus rapide de l'app.

## 8. Utilisation pour M2

Deux cas s'ajoutent : le **chemin « fichier hors vault »** (T1 mesuré sur un `.md` hors de tout vault, sans snapshot ni index) et les **tags** (le panneau doit se calculer depuis l'index, ce qui concerne surtout M3).

Le spike comparatif doit tester, en plus de la vitesse, un petit sous-ensemble de ce périmètre. Live Preview étant indispensable, il devient un critère éliminatoire. À tester : Live Preview sur la note de 319 Ko, rendu d'un `.canvas` en lecture seule, un tableau, un embed d'image, un wikilink avec autocomplétion, des onglets et splits. La liste exacte dépend des réponses de la section 7.

## 9. Ce qui est deviné

- Les niveaux v1 / v1.x / plus tard sont mes propositions, fondées sur les core plugins activés et le workspace. « Activé » ne prouve pas l'usage.
- Je n'ai pas lu le contenu des notes (règle du projet), donc je ne sais pas si tu utilises des callouts, du LaTeX ou des Bases.
