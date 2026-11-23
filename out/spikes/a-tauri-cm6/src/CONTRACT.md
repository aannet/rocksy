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
| `build_tags` | `vaultPath` | `void` ; rend les tags du vault disponibles en arrière-plan : (1) si un index persisté existe (`data_dir/tags/`), il est chargé sans lire aucune note → `tags-ready` avec `fromCache: true` ; (2) réconciliation par taille + mtime qui ne relit que les notes nouvelles ou modifiées → `tags-progress` (`total` = notes à relire) puis `tags-ready` avec `fromCache: false`. L'index est persisté après la réconciliation, quelques secondes après un enregistrement, et à la fermeture |
| `tags_list` | `query` (peut être vide) | `TagCount[]` triés ; un parent `a` agrège `a/b` |
| `tag_fragments` | `tag`, `limit?` | `Fragment[]` (le tag et ses descendants) |
| `file_tags` | `path`, `text` | `Fragment[]` pour un texte en mémoire (fichier hors vault ou non enregistré) |
| `search` | `query`, `limit?` (défaut 50 fichiers) | `SearchResult` : recherche plein texte dans le vault indexé (voir « Recherche ») |
| `backlinks` | `path` | `LinkRef[]` : liens et embeds des autres notes du vault qui pointent vers ce fichier (note ou pièce jointe), triés par chemin puis ligne |
| `outgoing_links` | `path` | `OutLink[]` : liens et embeds contenus dans cette note, dans l'ordre du texte, résolus ou non |
| `unresolved_links` | `limit?` (défaut 500) | `UnresolvedLink[]` : cibles introuvables dans le vault, avec leur nombre d'occurrences |
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
| `tags-ready` | `{ files, tags, occurrences, ms, fromCache, reread?, removed?, persisted? }` ; peut être émis deux fois par `build_tags` (index persisté, puis réconcilié) |
| `index-ready` | `IndexReady` : état de l'index du vault (tags, liens, recherche). Émis par `build_tags` quand l'index persisté est chargé (`fromCache: true`), après la réconciliation, quand la recherche plein texte devient disponible (`searchReady: true`), puis après chaque mise à jour due à un changement externe |
| `file-changed` | `{ path, mtimeMs }` : changement externe d'un fichier ouvert |

## Index du vault : une seule passe de lecture

`build_tags` (nom conservé) construit et entretient **tout** l'index d'un vault : tags, liens, recherche plein texte. Lire une note est l'opération coûteuse (Google Drive) : chaque note nouvelle ou modifiée est lue **une seule fois** et alimente les trois. Tags et liens sont disponibles dès le chargement de l'index persisté ; la recherche plein texte peut l'être un peu plus tard (`searchReady`). `tags-ready` reste émis comme avant. `save_file` met à jour les trois sans relire le disque.

## Recherche

- Insensible à la casse et aux accents (`resume` trouve `Résumé`). Les mots sont combinés par ET. Le dernier mot saisi est un préfixe (recherche à la frappe : `archi` trouve `architecture`).
- `"expression exacte"` ; `-mot` exclut ; `path:texte` restreint aux chemins contenant ce texte ; `tag:a/b` (ou `tag:#a/b`) restreint aux notes portant ce tag ou un descendant. Une requête faite seulement de filtres liste les notes correspondantes.
- Le nom du fichier compte plus que le corps. Résultats triés par pertinence.
- Les extraits viennent de l'index, sans relire les fichiers.
- Tant que la recherche n'est pas disponible, `search` ne renvoie pas d'erreur : `hits` vide, `total` 0, `searchReady: false`. Une requête vide renvoie un résultat vide.
- `line`, `col` et `len` suivent le texte exact renvoyé par `read_file` (BOM compris), comme les positions des tags.

## Liens

- Reconnus : `[[cible]]`, `[[cible#titre]]`, `[[cible#^bloc]]`, `[[cible|alias]]`, embeds `![[...]]`, liens Markdown `[texte](chemin.md)` et images `![alt](chemin.png)` vers un fichier local (les URL `http:`, `https:`, `mailto:` etc. sont ignorées). Pas dans le code inline ni les blocs de code. `[[#titre]]` (lien interne à la note) est ignoré.
- Résolution d'une cible wikilink, comme Obsidian : chemin exact depuis la racine du vault, sinon suffixe de chemin, sinon nom de fichier (extension `.md` facultative), sans tenir compte de la casse ; en cas d'ambiguïté, le chemin le plus court, puis l'ordre alphabétique. Un lien Markdown est résolu relativement au dossier de la note, puis depuis la racine. La résolution porte sur tous les fichiers visibles du vault (notes et pièces jointes) et est recalculée quand des fichiers apparaissent ou disparaissent, sans relire les notes.

## Suivi des changements externes

Après `open_vault`, le backend écoute le dossier du vault (dossiers à point ignorés, rafales regroupées : 400 ms de silence, 3 s au plus). Le watcher n'est qu'un déclencheur : à chaque lot, le backend relance le listing du vault (au plus une passe par seconde) et émet `tree-updated` si l'arbre a changé, puis `tags-ready` (`fromCache: false`, `reread`, `removed`) si des notes ont été relues ou retirées. Filet de sécurité : la même passe est relancée quand la fenêtre revient au premier plan, si la dernière date de plus de 30 s. Les fichiers ouverts restent suivis par `file-changed`.

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
type SearchResult = { query: string; total: number; hits: SearchHit[]; ms: number; searchReady: boolean };  // total = nombre de fichiers correspondants (peut dépasser hits.length)
type SearchHit = {
  path: string;                // absolu
  score: number;
  nameMatch: boolean;          // la requête correspond au nom du fichier
  matchCount: number;          // occurrences dans le corps
  matches: SearchMatch[];      // 3 au plus par fichier, dans l'ordre du texte
};
type SearchMatch = {
  line: number;                // 0-based, modèle de lignes de CodeMirror
  col: number;                 // unités UTF-16, début de la première occurrence sur la ligne
  len: number;                 // longueur de cette occurrence dans la ligne, en unités UTF-16 (pour la sélectionner)
  excerpt: string;             // ≤ 240 caractères, une ligne
  ranges: [number, number][];  // plages à surligner dans excerpt (unités UTF-16, fin exclue)
};
type LinkKind = "link" | "embed";
type LinkRef = {               // un lien entrant
  path: string;                // note source, absolu
  kind: LinkKind;
  line: number; col: number;   // position du lien dans la source (0-based, UTF-16)
  excerpt: string;             // ≤ 240 caractères, une ligne
  ranges: [number, number][];  // plage du lien dans excerpt
  heading: string | null;      // titre ou bloc visé (`#titre`, `#^bloc`), sans le `#`
  alias: string | null;
};
type OutLink = {
  target: string;              // cible telle qu'écrite, sans titre ni alias
  resolved: string | null;     // chemin absolu du fichier visé, null si introuvable
  kind: LinkKind;
  line: number; col: number;
  heading: string | null;
  alias: string | null;
};
type UnresolvedLink = { target: string; count: number; sources: string[] };  // sources : 5 chemins au plus
type IndexReady = {
  fromCache: boolean;
  files: number;               // notes indexées
  tags: number; occurrences: number;
  links: number; unresolvedLinks: number;
  searchReady: boolean; searchDocs: number;
  reread?: number; removed?: number;
  ms: number;
};
type ApiError = { kind: "not_found" | "not_utf8" | "conflict" | "io" | "invalid"; message: string };
```

## Règles des tags

- Syntaxe Obsidian : `#` précédé d'un début de ligne ou d'un espace, suivi de lettres, chiffres, `_`, `-`, `/` ; au moins un caractère non numérique ; pas dans le code inline, les blocs de code, les URL, les wikilinks ni les titres de liens.
- Frontmatter : clés `tags` / `tag`, liste YAML ou chaîne séparée par des virgules ; `#` optionnel. Fragment = fichier entier (`startLine: 0`, `endLine` dernière ligne).
- Fragment d'un tag en ligne = bloc englobant : paragraphe ; élément de liste **avec ses sous-éléments** ; titre seul ; ligne de tableau ; citation/callout entier.
