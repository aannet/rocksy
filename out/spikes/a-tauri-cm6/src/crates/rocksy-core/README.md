# rocksy-core

Cœur de Rocksy, sans aucune dépendance d'interface (ni tauri, ni runtime asynchrone). Dépendances : `serde`, `serde_json`, `memchr`. Les types publics se sérialisent dans le JSON `camelCase` de `../../CONTRACT.md`. Les fonctions sont bloquantes : c'est à l'appelant (commandes Tauri) de lancer `reconcile_tree` et `TagIndex::build` sur un thread.

Règle de base : la crate n'écrit dans un vault que par `save_file`. Tout le reste (registre, session, snapshots d'arbre) va dans `data_dir`, fourni par l'appelant. L'index des tags n'est pas persisté.

## API

```rust
// Fichiers (files.rs)
fn read_file(path: &Path) -> Result<FileContent>;                       // UTF-8 strict, BOM et fins de ligne conservés
fn save_file(path: &Path, text: &str, expected_mtime_ms: Option<u64>) -> Result<SaveResult>;   // { mtimeMs, size }

// Registre de vaults (vaults.rs) : data_dir/vaults.json
fn list_vaults(data_dir: &Path) -> Vec<VaultRef>;
fn add_vault(data_dir: &Path, path: &Path) -> Result<Vec<VaultRef>>;
fn remove_vault(data_dir: &Path, path: &Path) -> Result<Vec<VaultRef>>;
fn vault_for_path(data_dir: &Path, path: &Path) -> Option<VaultRef>;
fn find_vault<'a>(vaults: &'a [VaultRef], path: &Path) -> Option<&'a VaultRef>;   // sans accès disque

// Arbre (tree.rs) : data_dir/trees/<hash de la racine>.tree
fn list_tree(root: &Path) -> Result<Tree>;                               // parcours réel, fromSnapshot: false
fn open_tree(data_dir: &Path, root: &Path) -> Result<Tree>;              // snapshot s'il existe, sinon listing (+ snapshot)
fn reconcile_tree(data_dir: &Path, root: &Path, on_update: impl FnOnce(&Tree)) -> Result<bool>;
fn save_tree_snapshot(data_dir: &Path, tree: &Tree) -> Result<()>;
fn load_tree_snapshot(data_dir: &Path, root: &Path) -> Option<Tree>;
fn tree_snapshot_path(data_dir: &Path, root: &str) -> PathBuf;

// Tags (tags/)
fn file_tags(path: &str, text: &str) -> Vec<Fragment>;                   // texte en mémoire
impl TagIndex {                                                          // à partager dans un Arc
    fn new() -> Self;
    fn build(&self, vault: &Path, options: BuildOptions, cancel: &CancelToken,
             on_progress: impl Fn(TagsProgress) + Sync) -> Result<Option<TagsReady>>;   // None = annulé
    fn tags_list(&self, query: &str) -> Vec<TagCount>;
    fn tag_fragments(&self, tag: &str, limit: Option<usize>) -> Vec<Fragment>;
    fn update_file(&self, path: &Path, text: &str) -> bool;               // après un enregistrement
    fn remove_file(&self, path: &Path);
    fn clear(&self);
    fn stats(&self) -> TagStats;                                          // { files, tags, occurrences, skipped }
}

// Session (session.rs) : data_dir/session.json
fn load_session(data_dir: &Path) -> Option<Session>;
fn save_session(data_dir: &Path, session: &Session) -> Result<()>;

// Erreurs : ApiError { kind: not_found | not_utf8 | conflict | io | invalid, message }
```

## Comportements à connaître

- **`save_file`** : écrit `.<nom>.<pid>-<n>.rocksy-tmp` dans le même dossier, `fsync`, puis `rename` par-dessus la cible (remplacement atomique aussi sous Windows) ; le temporaire est supprimé sur tout échec. `conflict` si le mtime sur disque diffère de `expected_mtime_ms`, ou si le fichier a disparu. `invalid` pour un chemin relatif, un composant `.obsidian` ou une extension `.canvas` (casse ignorée, y compris derrière un lien symbolique). Les dossiers manquants ne sont pas créés (`not_found`). Le `mtimeMs` renvoyé est celui à repasser au prochain enregistrement.
- **Arbre** : entrées en ordre de parcours en profondeur ; dans chaque dossier, sous-dossiers d'abord puis fichiers, tri insensible à la casse. Un dossier est donc suivi immédiatement de son contenu. Noms commençant par `.` ignorés, liens symboliques ni suivis ni listés. `reconcile_tree` ne réécrit le snapshot et n'appelle le callback que si la liste diffère (chemin, type, taille ou mtime d'un fichier ; le mtime des dossiers est ignoré). Si le vault est inaccessible, elle renvoie une erreur et garde le snapshot.
- **Tags** : `tagLine` et `tagCol` suivent le modèle de lignes de CodeMirror (`\n`, `\r\n`, `\r`), colonnes en unités UTF-16, comptées dans le texte tel que renvoyé par `read_file` (un BOM compte pour une unité sur la première ligne). `tagCol` pointe sur le `#` (ou sur le début de la valeur dans le frontmatter). `count` = nombre d'occurrences du tag et de ses descendants = nombre de fragments renvoyés par `tag_fragments` sans limite. Les parents implicites (`a` quand seul `#a/b` existe) sont listés.
- **Tri de `tags_list`** : tags qui commencent par la requête, puis tags dont un niveau commence par elle (`b` trouve `a/b`), puis autres sous-chaînes ; dans chaque groupe par fréquence décroissante puis ordre alphabétique.
- **`TagIndex::build`** : remplace le contenu à la fin (les requêtes voient l'ancien index pendant la construction) ; les `update_file` reçus entre-temps sont rejoués. Fichiers illisibles ou non UTF-8 ignorés et comptés dans `stats().skipped`.

## Tests

```bash
export PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=~/.cache/rocksy-target/core
cd out/spikes/a-tauri-cm6/src
cargo test -p rocksy-core
cargo clippy -p rocksy-core --all-targets
cargo check -p rocksy-core --target x86_64-pc-windows-msvc --all-targets
```

`tests/no_unwanted_writes.rs` est le test F7 : empreinte (contenu, taille, mtime) de tous les fichiers d'un vault de test avant et après ouverture de l'arbre, réconciliation, indexation des tags et lectures.

## Mesures (`rocksy-cli`)

```bash
cargo build -p rocksy-core --release
BIN=~/.cache/rocksy-target/core/release/rocksy-cli
python3 out/tools/gen-vault.py --out ~/rocksy-vaults/x20 --scale 20

$BIN tree ~/rocksy-vaults/x20 [--data-dir D]        # listing, écriture et relecture du snapshot, réconciliation
$BIN tags ~/rocksy-vaults/x20 [--threads N] [--reps N]   # construction de l'index, latence de tags_list et tag_fragments
$BIN fragments note.md                               # fragments d'un fichier
```

Sorties en JSON sur stdout (médiane et maximum en millisecondes). Sans `--data-dir`, `tree` utilise un dossier temporaire supprimé à la fin. Les temps `withJson` incluent la sérialisation `serde_json` du résultat, c'est-à-dire ce que paiera une commande Tauri.
