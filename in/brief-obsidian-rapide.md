# Brief Projet — Obsidian rapide (réécriture core Rust)

20/11/2026 · Alec

## 1. Contexte & Problématique

Obsidian met trop longtemps à devenir utilisable au lancement. L'objectif est une app qui s'ouvre et soit fonctionnelle en 3 s max, sans perdre le vault, le store de plugins ni les plugins utilisés au quotidien.

Obsidian tourne sur Electron (TypeScript, Chromium + Node.js). Le cold start cumule le boot d'Electron, la reconstruction ou relecture du cache de métadonnées (IndexedDB, hors vault) et le chargement de tous les plugins actifs au démarrage.

Hypothèse à vérifier avant tout : une bonne part du temps vient des plugins (indexation Dataview, Templater, Excalidraw…), pas du moteur. Si c'est le cas, une partie du gain s'obtient sans réécriture.

Partie prenante : Alec, seul (projet perso). ⚠️ À préciser : usage perso uniquement, ou produit distribué à terme ?

## 2. Objectifs & Critères de Succès

Deux seuils en cold start, mesurés depuis le clic sur l'icône : 2 s pour écrire dans la dernière note ouverte, 3 s pour avoir le treeview du filesystem.

| Critère | Cible | Mesure |
| --- | --- | --- |
| Cold start → frappe acceptée dans la dernière note ouverte | ≤ 2 s | Chrono scripté, médiane sur 10 lancements |
| Cold start → treeview du filesystem navigable | ≤ 3 s | Horodatage du premier rendu complet de l'arbre |
| Recherche, backlinks, plugins non critiques | En tâche de fond, sans bloquer l'édition ni l'arbre | Horodatage de fin d'indexation |
| Compatibilité vault | 100 % des `.md` et de `.obsidian/` lus sans conversion | Ouvrir le vault réel, puis le rouvrir dans Obsidian sans diff |
| Plugins | Les plugins « must-have » de la liste fonctionnent | Checklist par plugin |
| Store | Recherche, installation, activation, mise à jour depuis `obsidian-releases` | Test manuel |

⚠️ À préciser :

- La machine de référence et la taille du vault (nombre de notes, Go).
- Le temps actuel dans Obsidian, qui sert de baseline.

## 3. Stack Technique & Contraintes

On part sur un core Rust avec une UI web dans Tauri. Un natif complet casserait les plugins JS.

| Couche | Choix pressenti | Pourquoi |
| --- | --- | --- |
| Shell | Tauri 2 (WebView système) | Binaire ~10-20 Mo, RAM ~50-100 Mo, boot 2x plus rapide qu'Electron |
| Éditeur | CodeMirror 6 | Les plugins qui étendent l'éditeur restent compatibles |
| Index | Rust : parser Markdown, graphe de liens, `tantivy` pour le full-text, persisté sur disque | Au démarrage on relit l'index, on ne le reconstruit pas |
| Watcher | `notify` + réindexation incrémentale par hash ou mtime | Seuls les fichiers modifiés depuis la dernière session sont retraités |
| Compat plugins | Shim JS de l'API `obsidian` (d'après `obsidian.d.ts`) branché sur le core via IPC | Les plugins existants tournent sans modification |
| Store | Client de `obsidianmd/obsidian-releases` + releases GitHub | Même format que `.obsidian/plugins/` et `community-plugins.json` |

Principes de démarrage :

1. Afficher la dernière note avant tout le reste : lecture directe du fichier référencé dans `workspace.json`, sans attendre l'index ni les plugins.
2. Construire le treeview à partir d'un simple listing du filesystem (`walkdir`, rendu progressif dossier par dossier), indépendant de l'index.
3. Charger l'index persisté en mmap, puis réconcilier en tâche de fond.
4. Charger les plugins en lazy : les critiques après le premier rendu, les autres à la première commande.

Contraintes :

- Les plugins qui utilisent des internals non documentés (`app.internalPlugins`, classes privées) demandent une compat au cas par cas, ou sont exclus.
- Sync et Publish sont des services propriétaires, donc hors périmètre (Git ou Syncthing en remplacement).
- Licence : on reproduit l'API publique, mais pas de décompilation de `app.js`. Validation juridique si le projet sort du perso.
- Le vault doit rester ouvrable dans Obsidian à tout moment (aucune écriture propriétaire dans `.obsidian/`).

⚠️ À préciser : OS cibles (macOS, Windows, Linux, mobile ?) et liste des plugins actuels.

## 4. Livrables & Jalons

Le premier jalon décide si la réécriture est nécessaire : on mesure avant de coder.

1. **M0 — Baseline (gate go/no-go).** Mesurer le cold start actuel d'Obsidian sur la machine de référence, avec et sans plugins (mode restreint). Si on passe sous 3 s en mode restreint, on optimise d'abord la config plugins (lazy loading, retrait des plus lourds) avant de réécrire.
2. **M1 — Audit des plugins.** Pour chaque plugin de la liste : coût au démarrage, API publique ou internals, must-have ou nice-to-have.
3. **M2 — Spike shell.** Tauri + CodeMirror 6 qui ouvre le vault réel et rend la dernière note éditable. Cible : ≤ 1 s.
4. **M3 — Index Rust persisté.** Liens, backlinks, tags, frontmatter, full-text, avec réconciliation incrémentale au démarrage.
5. **M4 — Shim API `obsidian`.** Couvrir les plugins must-have issus de M1, un par un.
6. **M5 — Store.** Parcours, installation, activation et mise à jour depuis `obsidian-releases`.

Livrables : le rapport de baseline (M0), la matrice d'audit des plugins (M1), puis l'app avec un script de mesure du cold start rejoué à chaque jalon.

⚠️ À préciser : temps disponible par semaine et échéance visée.

---

*Brief généré le 20/11/2026 — v0.1 (draft, avant interview)*
