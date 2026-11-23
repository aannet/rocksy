# rocksy-core

Cœur de Rocksy, sans aucune dépendance d'interface (ni tauri, ni runtime asynchrone). Dépendances : `serde`, `serde_json`, `memchr`. Les types publics se sérialisent dans le JSON `camelCase` de `../../CONTRACT.md`. Les fonctions sont bloquantes : c'est à l'appelant (commandes Tauri) de lancer `reconcile_tree` et `TagIndex::build` sur un thread.

Règle de base : la crate n'écrit dans un vault que par `save_file`. Tout le reste (registre, session, snapshots d'arbre, index des tags) va dans `data_dir`, fourni par l'appelant.

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
fn tags_index_path(data_dir: &Path, root: &str) -> PathBuf;
impl TagIndex {                                                          // à partager dans un Arc
    fn new() -> Self;
    fn build(&self, vault: &Path, options: BuildOptions, cancel: &CancelToken,
             on_progress: impl Fn(TagsProgress) + Sync) -> Result<Option<TagsReady>>;   // relit tout, ne persiste rien
    // Persistance : data_dir/tags/<hash de la racine>.tags
    fn load_persisted(&self, data_dir: &Path, vault: &Path) -> Option<TagsReady>;      // aucune note lue
    fn reconcile(&self, data_dir: &Path, vault: &Path, options: BuildOptions, cancel: &CancelToken,
                 on_progress: impl Fn(TagsProgress) + Sync) -> Result<Option<ReconcileReport>>;   // None = annulé
    fn save(&self, data_dir: &Path) -> Result<bool>;                      // true si un fichier a été écrit
    fn is_dirty(&self) -> bool;
    fn root(&self) -> Option<String>;                                     // racine du vault chargé
    fn update_file_with_meta(&self, path: &Path, text: &str, size: u64, mtime_ms: u64) -> bool;
    fn tags_list(&self, query: &str) -> Vec<TagCount>;
    fn tag_fragments(&self, tag: &str, limit: Option<usize>) -> Vec<Fragment>;
    fn update_file(&self, path: &Path, text: &str) -> bool;               // compatibilité : fait un stat du fichier
    fn remove_file(&self, path: &Path);
    fn clear(&self);                                                      // oublie aussi la racine
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

## Persistance de l'index des tags

Séquence prévue dans l'app : `load_persisted` à l'ouverture du vault (tags interrogeables sans lire aucune note), puis `reconcile` sur un thread, `update_file_with_meta` après chaque enregistrement avec le `{ size, mtimeMs }` renvoyé par `save_file`, et `save` en différé puis à la fermeture.

- **`load_persisted`** : renvoie `None` et ne touche pas à l'index si le fichier est absent, tronqué, corrompu, d'une autre version ou d'une autre racine. Jamais d'erreur ni de panique.
- **`reconcile`** : liste les `.md` (mêmes règles que `build`), compare taille et mtime (en millisecondes) avec l'index en mémoire, ne relit que les fichiers nouveaux ou changés, retire les disparus, puis réécrit le fichier si quelque chose a changé, si l'index a des modifications non enregistrées ou si le fichier n'existe pas. Index vide ou d'un autre vault : construction complète. `total` de la progression = nombre de fichiers à relire. Un échec d'écriture du fichier n'est pas une erreur (`persisted: false`, `save` donne la raison). Vault inaccessible : erreur, index et fichier conservés.
- **Annulation de `reconcile`** : renvoie `Ok(None)` et n'écrit rien sur disque, mais garde dans l'index les fichiers déjà relus (index cohérent, marqué non enregistré). La réconciliation suivante ne relit que le reste, et un `save` à la fermeture conserve le travail fait : utile quand la première lecture prend des minutes sur un lecteur synchronisé.
- **Limite** : pas de hash. Un fichier dont le contenu change sans changer de taille ni de mtime n'est pas vu ; `build` relit tout.
- **`update_file`** (ancienne signature) : fait un `stat` et ne retient taille et mtime que si la taille sur disque est celle du texte ; sinon le fichier sera relu à la prochaine réconciliation.
- **Fichiers illisibles** : un fichier non UTF-8 est mémorisé avec sa taille et son mtime et n'est pas relu ; une erreur de lecture est retentée à chaque réconciliation.

### Format du fichier (version 1)

Binaire, little-endian, un fichier par vault. Il contient des extraits de notes et ne quitte pas `data_dir`.

```text
en-tête : "RKTAGIDX" | u32 version | u64 longueur de la charge | u64 somme de contrôle de la charge
charge  : racine du vault
          table des tags     : nombre, puis chaque tag (graphie affichée)
          table des fichiers : nombre, puis pour chaque .md du vault, avec ou sans tag
              chemin relatif (séparateur '/')
              drapeaux (bit 0 : taille et mtime connus ; bit 1 : illisible ou non UTF-8)
              taille, mtime en ms              (si bit 0)
              extraits    : nombre, puis chaque chaîne
              occurrences : nombre, puis pour chacune
                  tag (indice dans la table) | source (0 inline, 1 frontmatter)
                  startLine | endLine - startLine | tagLine - startLine | tagCol | extrait (indice)
```

Entiers en varint LEB128 sauf largeur indiquée ; chaînes = longueur en varint puis UTF-8. Écriture atomique (temporaire + `fsync` + `rename`).

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
$BIN tags-persist ~/rocksy-vaults/x20 [--data-dir D] [--work-dir W] [--modify N]   # persistance de l'index
$BIN fragments note.md                               # fragments d'un fichier
```

`tags-persist` mesure la construction complète par `reconcile`, la taille du fichier, `save`, `load_persisted` (20 répétitions), `reconcile` sans changement, puis `reconcile` après modification de N fichiers (50 par défaut). Ce dernier cas travaille sur une copie temporaire des `.md` du vault dans `--work-dir`, supprimée à la fin ; le vault de référence n'est que lu.

Sorties en JSON sur stdout (médiane et maximum en millisecondes). Sans `--data-dir`, `tree` utilise un dossier temporaire supprimé à la fin. Les temps `withJson` incluent la sérialisation `serde_json` du résultat, c'est-à-dire ce que paiera une commande Tauri.

### Persistance : mesures du 23/11/2026

Médianes en millisecondes, build release, cache disque chaud. Linux = WSL2 sur ext4 ; Windows = exécutable `x86_64-pc-windows-gnullvm` sur NTFS local. Rien n'est mesuré sur Google Drive.

| Mesure | Linux 1x | Linux 20x | Windows 1x | Windows 20x |
|---|---|---|---|---|
| Taille du fichier | 311 Ko | 4,38 Mo | 311 Ko | 4,38 Mo |
| `save` | 4,8 | 12 à 22 | 4 à 7 | 15 à 19 |
| `load_persisted` (médiane / max) | 0,9 à 1,1 / 1,4 | 12,5 à 16 / 22 | 1,8 à 2,1 / 10,5 | 25 à 38 / 39 à 52 |
| `reconcile` sans changement | 1,6 à 2,0 | 27 | 3,5 à 3,8 | 42 à 59 |
| `reconcile` après 50 fichiers modifiés (copie) | 7 à 10 | 44 à 71 | 21 à 33 | 107 à 204 |
| `reconcile` sans changement sur la copie | 1,5 | 15 | 4,5 à 5,8 | 47 à 50 |

Les fourchettes sont les médianes de deux ou trois lancements successifs. 20x = 9 760 notes, 46 208 occurrences, 4 199 tags.
