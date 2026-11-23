# M1 — Matrice d'audit des plugins (v2, après décisions d'Alec du 23/11/2026)

Sources : `in/audit-vault-obsidian.md`, les deux « Start-up time » du 22/11 (moyenne, ms) et `out/data/plugin-api-usage.json` (scan des `main.js`).
**Décidé** = tranché par Alec. **Proposé** = ma proposition, non validée.

> **Mise à jour du 23/11/2026 :** la compatibilité avec les plugins Obsidian devient secondaire. L'app aura son propre système de plugins à chargement à chaud, et un importeur de plugins Obsidian viendra peut-être plus tard. Dans la colonne « Stratégie », « Shim » signifie donc désormais « à porter ou à importer plus tard » ; les modes d'activation, les verdicts et les jobs restent valables. La surface d'API du shim en fin de document ne sert que si l'importeur est construit.

## Modèle d'activation (tout reste dans l'architecture de plugins/mods)

| Mode | Quand le mod démarre | Sur le chemin critique ? |
|---|---|---|
| `post-paint` | Après T1 (note éditable) et T2 (arbre), par tranches en temps libre | Non |
| `on-file-type` | À l'ouverture d'un fichier d'un type déclaré | Non |
| `on-command` / `on-event` | À la première commande ou au premier événement déclaré | Non |
| `admin` | Lancé à la main depuis le panneau d'admin | Non |
| `job` | Tâche planifiée (cron) exécutée par le core en tâche de fond, sans UI | Non |

Rien n'est chargé avant le premier rendu. Les modes décrivent le même manifeste de plugin.

## Matrice

| Plugin | Classe | ms (moy.) | Décision | Mode | Stratégie | Remarque |
|---|---|---:|---|---|---|---|
| Custom Icons | C | 899 | **Décidé** : actif par défaut | `post-paint` | Shim | Appliquer le CSS généré (`CustomIcon-AutoGen.css`) dès le démarrage pour éviter le saut visuel |
| Templater | B | 638 | **Décidé** : templates simples | `on-command` + événement `create` | Shim | Pas de `child_process`, pas de scripts utilisateur en v1 |
| Excalidraw | C | 368 | **Décidé** : désactivé par défaut | `on-file-type` (`.excalidraw`, `.excalidraw.md`) | Shim, en dernier | 48 symboles `obsidian`, Worker, 9 méthodes `app.*` qu'il est seul à utiliser |
| Wispr Flow Sync | C | 255 | **Décidé** : devient un job | `job` | Natif (Rust) | Remplace `fs`/`path`/`os` de Node. Voir les points d'attention ci-dessous |
| Tag Wrangler | B | 238 | **Décidé** : depuis l'admin seulement | `admin` | Shim | |
| TagFolder | B | 132 | **Décidé** : actif par défaut | `post-paint` | Shim | `registerView`, événement `resolved` |
| Editing Toolbar | C | 196 | **Décidé** : actif par défaut | `post-paint` | Shim | Réserver la hauteur de la barre pour éviter le saut de mise en page. Utilise `app.commands` et `app.setting` |
| Outliner | B | 92 | Must-have (proposé) | `post-paint` | Shim | Extensions CM6 attachées après coup via les « compartments » |
| Style Settings | B | 362 | Proposé | `post-paint` | Natif ou shim | Lit les blocs `@settings` du thème |
| File Color | A | 210 | Proposé | `post-paint` | Natif (treeview) | |
| File Explorer Note Count | B | 65 | Proposé | `post-paint` | Natif (treeview) | |
| Dynamic Wide Content | A | 147 | Proposé | `post-paint` | Natif ou shim | 13 Ko |
| Heading Shifter | A | 123 | Proposé | `on-command` | Shim | |
| Occura | A | 115 | Proposé | `on-command` | Shim | |
| Paste image rename | A | 106 | Proposé | `on-event` (collage) | Shim | |
| Multi-Column Markdown | A | 84 | Proposé | `on-file-type`/post-processeur | Shim | |
| Image Toolkit | A | 80 | Proposé | `on-command` | Shim | |
| Note Refactor | A | 71 | Proposé | `on-command` | Shim | |
| Copy document as HTML | A | 51 | Proposé | `on-command` | Shim | |
| **Scan des médias orphelins** | — | — | **Décidé** (nouveau) | `job` | Natif (Rust) | S'appuie sur le graphe de liens de l'index persisté : aucune lecture de fichier |

Inactifs (Advanced Canvas, File Cleaner Redux, Advanced Tables, Clear Unused Images, Find Orphaned Images, QuadTasks, dossier orphelin) : hors périmètre. Le scan natif d'orphelins remplace Clear Unused Images et Find Orphaned Images.

## Surface d'API du shim (d'après le scan)

| Périmètre | Méthodes `app.*.*` | Symboles `obsidian` | Enregistrements |
|---|---:|---:|---:|
| Templater + Outliner | 34 | 31 | 12 |
| + tous les plugins chargés par le shim, hors Excalidraw | 59 | 48 | 18 |
| Les 19 actifs | 85 | 62 | 20 |

- Un noyau de 18 méthodes sert au moins 5 plugins (`workspace.on`, `onLayoutReady`, `getLeavesOfType`, `getActiveViewOfType`, `vault.read/modify/create/on`, `metadataCache.getFileCache`, `getFirstLinkpathDest`…) : il se fait en premier.
- 31 noms d'événements, dont `modify`, `create`, `delete`, `rename`, `file-open`, `layout-change`, `active-leaf-change`, `file-menu`, `editor-menu`.
- CodeMirror 6 : modules `state`, `view`, `language`, `commands`, `@lezer/*`.
- Internals à simuler : surtout Editing Toolbar et Custom Icons (`app.commands.commands`, `app.setting.open*`), plus de petits accès de configuration pour Outliner.
- Node/Electron : Wispr (`fs`, `path`, `os`), Templater (`child_process`, `window.require`), Editing Toolbar (`http`, `window.require`).

## Points d'attention

1. **Jobs et multi-machines :** un job qui écrit dans le vault (Wispr crée des notes) tournerait sur chaque machine synchronisée par Drive, d'où des doublons et des conflits. Il faut désigner la machine qui exécute chaque job, par réglage local.
2. **Jobs et Drive :** I/O à faible priorité et budgetées, jamais pendant le démarrage. Le scan d'orphelins est en lecture seule : la suppression passe par l'admin.
3. **Mods par défaut qui touchent l'interface** (Custom Icons, Editing Toolbar) : visibles dès le premier rendu sans attendre leur code, sinon la fenêtre « saute » à leur chargement.
4. **Latence de frappe pendant le chargement des mods :** à mesurer, voir `CLAUDE.md`.
