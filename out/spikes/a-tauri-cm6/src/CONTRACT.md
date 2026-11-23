# Contrat d'interface — spike A (base de la 0.1)

Source de vérité entre le cœur Rust (`crates/rocksy-core`), la colle Tauri (`src-tauri`) et le frontend (`ui`).
Toute modification passe par la session principale. Identifiants en anglais, JSON en `camelCase`.

## Arborescence

```
src/
├── Cargo.toml            workspace (crates/rocksy-core, src-tauri)
├── crates/rocksy-core/   cœur sans dépendance d'interface (pas de tauri, pas de tokio obligatoire)
├── src-tauri/            commandes Tauri 2 = enveloppes fines autour de rocksy-core
└── ui/                   Vite + TypeScript + CodeMirror 6, sortie dans ui/dist
```

## Règles dures

- Jamais d'écriture dans `.obsidian/`, dans un `.canvas`, ni à côté d'un fichier hors vault. La seule écriture dans un vault est `save_file` sur un `.md` (atomique : fichier temporaire dans le même dossier + rename ; le temporaire commence par `.` et est supprimé en cas d'échec).
- Données propres à Rocksy (liste des vaults, snapshots d'arbre, état de session) : dossier de données de l'app (`data_dir`, passé au cœur par l'appelant ; jamais calculé depuis le vault).
- Dossiers et fichiers commençant par `.` ignorés dans l'arbre.
- Chemins : absolus côté OS dans les commandes ; les entrées d'arbre sont relatives à la racine du vault avec `/` comme séparateur.
- UTF-8 seulement ; un fichier non UTF-8 renvoie une erreur lisible (pas de conversion silencieuse). Les fins de ligne et le BOM d'origine sont préservés à l'enregistrement.

## Commandes (`invoke`)

| Commande | Arguments | Retour |
|---|---|---|
| `startup_info` | — | `StartupInfo` |
| `read_file` | `path` | `FileContent` |
| `save_file` | `path`, `text`, `expectedMtimeMs?` | `{ mtimeMs, size }` ; erreur `conflict` si le fichier a changé sur disque depuis `expectedMtimeMs` |
| `list_vaults` | — | `VaultRef[]` |
| `add_vault` | `path` | `VaultRef[]` |
| `remove_vault` | `path` | `VaultRef[]` (ne touche à aucun fichier du vault) |
| `open_vault` | `path` | `Tree` : depuis le snapshot s'il existe (`fromSnapshot: true`), sinon par listing. Lance ensuite la réconciliation en arrière-plan → événement `tree-updated` |
| `build_tags` | `vaultPath` | `void` ; indexation en arrière-plan → événements `tags-progress`, `tags-ready` |
| `tags_list` | `query` (peut être vide) | `TagCount[]` triés ; un parent `a` agrège `a/b` |
| `tag_fragments` | `tag`, `limit?` | `Fragment[]` (le tag et ses descendants) |
| `file_tags` | `path`, `text` | `Fragment[]` pour un texte en mémoire (fichier hors vault ou non enregistré) |
| `set_session` | `session: Session` | `void` (persisté dans `data_dir`) |
| `pick_folder` / `pick_file` | — | `string \| null` |
| `log_timing` | `name`, `ms` | `void` (jalons de démarrage mesurés par l'UI) |
| `open_external` | `url` | `void` : ouvre un lien dans le navigateur ou le client mail du système ; `http`, `https` et `mailto` seulement, sinon erreur `invalid` |

## Événements (cœur → UI)

| Événement | Charge utile |
|---|---|
| `open-files` | `{ paths: string[] }` : second lancement (instance unique) ou association de fichiers |
| `tree-updated` | `Tree` |
| `tags-progress` | `{ done, total }` |
| `tags-ready` | `{ files, tags, occurrences, ms }` |
| `file-changed` | `{ path, mtimeMs }` : changement externe d'un fichier ouvert |

## Types

```ts
type StartupInfo = {
  argFiles: string[];          // .md passés en ligne de commande
  vaults: VaultRef[];
  session: Session | null;     // dernière session (vault actif, onglets)
  dataDir: string;
  version: string;
  processStartMs: number;      // epoch ms au début de main()
};
type VaultRef = { path: string; name: string };
type Session = { activeVault: string | null; tabs: string[]; activeTab: string | null };
type FileContent = { path: string; text: string; mtimeMs: number; size: number };
type Tree = { root: string; fromSnapshot: boolean; entries: TreeEntry[] };   // triées : dossiers d'abord, ordre alphabétique insensible à la casse
type TreeEntry = { path: string; isDir: boolean; size: number; mtimeMs: number };
type TagCount = { tag: string; count: number };      // tag sans '#', casse d'origine de la 1re occurrence ; comparaison insensible à la casse
type Fragment = {
  path: string;                // absolu
  tag: string;
  source: "inline" | "frontmatter";
  startLine: number;           // 0-based, inclus : début du bloc
  endLine: number;             // 0-based, inclus : fin du bloc
  tagLine: number;             // ligne du tag
  tagCol: number;              // colonne en unités UTF-16 (comme CodeMirror)
  excerpt: string;             // ≤ 240 caractères, une ligne
};
type ApiError = { kind: "not_found" | "not_utf8" | "conflict" | "io" | "invalid"; message: string };
```

## Règles des tags

- Syntaxe Obsidian : `#` précédé d'un début de ligne ou d'un espace, suivi de lettres, chiffres, `_`, `-`, `/` ; au moins un caractère non numérique ; pas dans le code inline, les blocs de code, les URL, les wikilinks ni les titres de liens.
- Frontmatter : clés `tags` / `tag`, liste YAML ou chaîne séparée par des virgules ; `#` optionnel. Fragment = fichier entier (`startLine: 0`, `endLine` dernière ligne).
- Fragment d'un tag en ligne = bloc englobant : paragraphe ; élément de liste **avec ses sous-éléments** ; titre seul ; ligne de tableau ; citation/callout entier.
