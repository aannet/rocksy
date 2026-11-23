# Contrat d'interface — spike A (base de la 0.1)

Source de vérité entre le cœur Rust (`crates/rocksy-core`), la colle Tauri (`src-tauri`) et le frontend (`ui`).
Toute modification passe par la session principale. Identifiants en anglais, JSON en `camelCase`.

Révision du 23/11/2026 : intègre `out/audits/2026-11-23_contrat-delta.md` (correctifs des trois audits), puis l'intégration finale (0.2.1) : `ApiError.code`, règle d'`add_vault`, ordre libre d'`open_vault` et `build_tags`, `index-ready` après un enregistrement qui change les liens, `IndexReady.pending`, règles des fins de ligne mixtes et de `tags_list`, politique de sécurité du contenu de la page.

## Arborescence

```
src/
├── Cargo.toml            workspace (crates/rocksy-core ; src-tauri en est exclu)
├── crates/rocksy-core/   cœur sans dépendance d'interface (pas de tauri, pas de tokio obligatoire)
├── src-tauri/            commandes Tauri 2 = enveloppes fines autour de rocksy-core
└── ui/                   Vite + TypeScript + CodeMirror 6, sortie dans ui/dist
```

## Règles dures

- Jamais d'écriture dans `.obsidian/`, dans un `.canvas`, ni à côté d'un fichier hors vault. La seule écriture dans un vault est `save_file` sur un `.md` ou `.markdown` (atomique : fichier temporaire dans le même dossier + rename ; le temporaire commence par `.`, finit par `.rocksy-tmp` et est supprimé en cas d'échec).
- Seule exception : à l'ouverture d'un vault (`open_vault`), Rocksy peut supprimer ses propres fichiers temporaires orphelins d'écriture atomique (nom commençant par `.` et finissant par `.rocksy-tmp`), âgés de plus d'une heure, dans les dossiers visibles du vault. Rien d'autre n'est supprimé, rien n'est créé.
- Données propres à Rocksy : jamais dans le vault, jamais calculées depuis le vault. Deux dossiers (voir « Données de l'application ») : la liste des vaults et la session dans le dossier de données itinérant ; les données dérivées et volumineuses (instantanés d'arbre, index) dans le dossier de données local.
- La page web n'est pas digne de confiance : tout chemin qu'elle envoie est contrôlé par le backend (voir « Accès aux fichiers »), et aucune commande ne lui permet d'élargir ses propres droits (voir « Ce que la page ne peut pas faire »).
- La page est servie avec une politique de sécurité du contenu stricte (voir « Politique de sécurité du contenu ») : aucun script en ligne, aucun `eval`, aucune connexion réseau.
- Dossiers et fichiers commençant par `.` ignorés dans l'arbre.
- Chemins : absolus côté OS dans les commandes ; les entrées d'arbre sont relatives à la racine du vault avec `/` comme séparateur.
- UTF-8 seulement ; un fichier non UTF-8 renvoie une erreur lisible (pas de conversion silencieuse). Les fins de ligne et le BOM d'origine sont préservés à l'enregistrement (voir « Fins de ligne »).

## Commandes (`invoke`)

| Commande | Arguments | Retour |
|---|---|---|
| `startup_info` | — | `StartupInfo` |
| `read_file` | `path` | `FileContent` ; `invalid` si la page n'a pas le droit de lire ce chemin, ou au-delà de 50 Mio |
| `save_file` | `path`, `text`, `expectedMtimeMs?` | `{ mtimeMs, size }` ; `conflict` si le fichier a changé sur disque depuis `expectedMtimeMs` ; `invalid` si l'écriture n'est pas permise ou si l'extension n'est ni `.md` ni `.markdown` ; `io` si le fichier reste verrouillé (le remplacement est retenté environ 2 s, le contrôle de conflit est refait avant chaque tentative) |
| `list_vaults` | — | `VaultRef[]` |
| `add_vault` | `path` | `VaultRef[]` ; `invalid` (code `not_allowed`) si `path` n'est ni un dossier renvoyé par `pick_folder` pendant cette session, ni un vault déjà enregistré (voir « Accès aux fichiers ») |
| `remove_vault` | `path` | `VaultRef[]` ; ferme le vault s'il est ouvert et supprime ses données dérivées (instantané d'arbre, index des tags et des liens, index de recherche) ; ne touche à aucun fichier du vault |
| `open_vault` | `path` | `Tree` : depuis l'instantané s'il existe (`fromSnapshot: true`), sinon par listing. Lance ensuite la vérification en arrière-plan → événement `tree-updated` si l'arbre a changé. Ferme le vault précédemment ouvert (comme `close_vault`). `invalid` si `path` n'est pas un vault enregistré |
| `close_vault` | `path` | `void` : arrête le suivi du dossier, persiste puis libère l'index de ce vault. Sans effet si ce vault n'est pas celui qui est ouvert |
| `build_tags` | `vaultPath` | `void` ; `invalid` si ce n'est pas un vault enregistré. Peut être appelée avant, pendant ou après l'`open_vault` du même vault (voir « Index du vault »). Rend l'index du vault disponible en arrière-plan : (1) si un index persisté existe, il est chargé sans lire aucune note → `tags-ready` avec `fromCache: true` ; (2) réconciliation par taille + mtime qui ne relit que les notes nouvelles ou modifiées → `tags-progress` (`total` = notes à relire) puis `tags-ready` avec `fromCache: false`. Persistance : voir « Index du vault » |
| `tags_list` | `query` (peut être vide) | `TagCount[]` filtrés et triés (voir « Tri et filtre de `tags_list` ») ; un parent `a` agrège `a/b` |
| `tag_fragments` | `tag`, `limit?` (défaut 500, plafond 1000) | `Fragment[]` (le tag et ses descendants) |
| `file_tags` | `path`, `text` | `Fragment[]` pour un texte en mémoire (fichier hors vault ou non enregistré) |
| `search` | `query`, `limit?` (défaut 50 fichiers, plafond 1000) | `SearchResult` : recherche plein texte dans le vault indexé (voir « Recherche ») |
| `backlinks` | `path` | `LinkRef[]` : liens et embeds des autres notes du vault qui pointent vers ce fichier (note ou pièce jointe), triés par chemin puis ligne |
| `outgoing_links` | `path` | `OutLink[]` : liens et embeds contenus dans cette note, dans l'ordre du texte, résolus ou non |
| `unresolved_links` | `limit?` (défaut 500, plafond 1000) | `UnresolvedLink[]` : cibles introuvables dans le vault, avec leur nombre d'occurrences |
| `set_session` | `session: Session` | `void` (persisté dans le dossier de données) ; ignore silencieusement les onglets que la page n'a pas le droit de lire et un `activeVault` non enregistré |
| `pick_folder` / `pick_file` | — | `string \| null` ; le fichier choisi par `pick_file` devient un fichier ouvert explicitement ; le dossier choisi par `pick_folder` devient enregistrable par `add_vault` jusqu'à la fin de la session |
| `log_timing` | `name`, `ms` | `void` (jalons de démarrage mesurés par l'UI) |
| `open_external` | `url` | `void` : ouvre un lien dans le navigateur ou le client mail du système ; `http`, `https` et `mailto` seulement (un `mailto:` portant un paramètre `attach` est refusé), sinon erreur `invalid` |
| `close_ack` | — | `void` : accusé de réception de `close-requested` (voir « Fermeture ») |
| `quit` | — | `void` : ferme l'application proprement (voir « Fermeture ») |

Une commande ne laisse jamais une promesse sans réponse : une défaillance interne du backend donne une erreur `io`.

## Événements (cœur → UI)

| Événement | Charge utile |
|---|---|
| `open-files` | `{ paths: string[] }` : second lancement (instance unique), association de fichiers ou glisser-déposer sur la fenêtre. Ces chemins sont déjà des fichiers ouverts explicitement quand l'événement arrive |
| `tree-updated` | `Tree` |
| `tags-progress` | `{ vault, done, total }` |
| `tags-ready` | `{ vault, files, tags, occurrences, ms, fromCache, reread?, removed?, persisted? }` ; peut être émis deux fois par `build_tags` (index persisté, puis réconcilié) |
| `index-ready` | `IndexReady` : état de l'index du vault (tags, liens, recherche). Émis par `build_tags` quand l'index persisté est chargé (`fromCache: true`), après la réconciliation, quand la recherche plein texte devient disponible (`searchReady: true`), après chaque mise à jour due à un changement externe, et après un `save_file` qui a changé les liens du vault (`linksGeneration` différent ; ni `reread`, ni `removed`, ni `pending` dans ce cas ; aucun événement si les liens n'ont pas bougé) |
| `file-changed` | `{ path, mtimeMs, deleted?: boolean }` : changement externe d'un fichier ouvert ; `deleted: true` (et `mtimeMs: 0`) quand le fichier a disparu (supprimé ou déplacé) |
| `close-requested` | — (sans charge utile) : l'utilisateur demande la fermeture de la fenêtre (voir « Fermeture ») |

`tags-progress`, `tags-ready` et `index-ready` portent `vault` : la racine du vault concerné, sous la même forme que `Tree.root`. L'interface ignore un événement dont le vault n'est pas le vault actif.

## Accès aux fichiers

Le backend tient deux listes, que la page ne peut pas modifier directement : les vaults enregistrés, et les **fichiers ouverts explicitement** par l'utilisateur (argument de ligne de commande, second lancement, dialogue `pick_file`, glisser-déposer sur la fenêtre, onglet restauré qui avait été ouvert ainsi). La liste des fichiers ouverts explicitement est tenue et persistée par le backend (bornée ; au redémarrage, seuls ceux qui figurent encore dans les onglets de la session le restent) ; elle n'est jamais fournie par la page.

- **Lecture permise** (`read_file`) : sous un vault enregistré ; un fichier ouvert explicitement ; et, autour d'un tel fichier hors vault, les fichiers texte et images de son dossier et de ses sous-dossiers : `.md`, `.markdown`, `.txt`, `.canvas`, `.png`, `.jpg`, `.jpeg`, `.gif`, `.svg`, `.webp`, `.bmp`, `.avif` (sans tenir compte de la casse). Un fichier posé à la racine d'un volume n'ouvre pas le volume.
- **Écriture permise** (`save_file`) : sous un vault enregistré, ou dans un fichier ouvert explicitement. Un fichier atteint par un lien depuis une note hors vault est en lecture seule, y compris après redémarrage.
- **Chemins réseau** (`\\serveur\partage`, `//serveur/partage`, `\\?\UNC\…`) venus de la page : refusés, sauf s'il s'agit exactement d'un fichier ouvert explicitement ou d'un chemin sous un vault enregistré sur un partage. Les images et liens d'une note posée sur un partage réseau ne sont pas chargés. Les chemins de périphérique (`\\.\…`, `\\?\Volume{…}`, `\??\…`) sont toujours refusés.
- **Enregistrer un vault** (`add_vault`) donne à la page la lecture et l'écriture de tout le dossier : la page ne choisit donc pas ce dossier. `add_vault` n'accepte qu'un dossier renvoyé par le dialogue natif `pick_folder` pendant la session en cours (liste tenue par le backend, bornée à 32 dossiers, non persistée), ou un vault déjà enregistré (aucun droit nouveau). Seule exception, pour les essais automatiques : dans une instance isolée (`ROCKSY_DATA_DIR` défini, donc décidé par celui qui lance le processus et non par la page), `add_vault` accepte tout dossier.
- Les refus sont l'erreur `invalid` : `read_file`, `save_file` pour un chemin non permis ; `open_vault` et `build_tags` pour un dossier qui n'est pas un vault enregistré ; `add_vault` pour un dossier non choisi dans le dialogue. Ceux que l'interface doit reconnaître portent un `code` (voir « Erreurs »). `set_session` ne renvoie pas d'erreur : il retire ce qui n'est pas permis.
- `save_file` n'accepte que `.md` et `.markdown` ; `.obsidian/` et `.canvas` restent interdits, y compris par un lien ou un nom court Windows.
- `FileContent.readOnly` est vrai quand `save_file` serait refusé pour ce chemin (extension non Markdown, ou écriture non permise). L'interface ouvre alors le document en lecture seule : pas d'édition, pas d'enregistrement automatique.
- Les fichiers ouverts ne sont surveillés (`file-changed`) que s'ils sont lisibles selon ces règles : le backend ne touche jamais un chemin non permis pour le compte de la page.
- Protocole `asset:` (images) : limité aux vaults enregistrés et aux dossiers des fichiers ouverts explicitement ; `.obsidian/` exclu ; aucun chemin réseau (les images d'un vault posé sur un partage réseau ne sont pas affichées).

### Ce que la page ne peut pas faire

Revue de toutes les commandes (0.2.1) : aucune ne permet à la page d'obtenir un accès qu'elle n'a pas déjà.

- Obtenir un droit sur un fichier ou un dossier passe toujours par un geste de l'utilisateur hors de la page : dialogue natif (`pick_file`, `pick_folder`), ligne de commande, second lancement, glisser-déposer. `add_vault`, `open_vault`, `build_tags`, `read_file`, `save_file` et `set_session` ne font que vérifier ces droits.
- `remove_vault` et `close_vault` retirent des droits ou des données dérivées, jamais un fichier de l'utilisateur. `quit` et `close_ack` ferment l'application.
- `tags_list`, `tag_fragments`, `search`, `backlinks`, `outgoing_links`, `unresolved_links` ne lisent que l'index du vault ouvert ; `file_tags` ne lit aucun fichier (texte fourni).
- `open_external` n'ouvre que `http`, `https` et `mailto` ; `log_timing` ajoute une ligne bornée à `timings.jsonl` ; `startup_info` et `list_vaults` ne font que lire.
- La page ne peut qu'écouter les événements (`listen`/`unlisten`) ; elle ne peut pas en émettre. Les commandes ne sont appelables que depuis la fenêtre principale (capacité `default`).

## Politique de sécurité du contenu

`default-src 'none'` ; `script-src 'self'` (ni `'unsafe-inline'`, ni `'unsafe-eval'`) ; `style-src 'self' 'unsafe-inline'` (styles en attribut de l'arbre virtualisé, de CodeMirror, des diagrammes et des tableaux) ; `img-src 'self' asset: http://asset.localhost data: blob:` ; `font-src 'self' data:` ; `connect-src ipc: http://ipc.localhost` (l'IPC seulement) ; `object-src`, `base-uri`, `form-action`, `frame-src`, `media-src`, `worker-src` : `'none'`. Une image distante (`http`, `https`) n'est donc jamais chargée. L'interface consigne toute violation dans `window.rocksyCsp` (diagnostic et essais) : aucune n'est attendue en usage normal.

## Erreurs

`ApiError.kind` dit la nature de l'erreur ; `message` est un texte en anglais pour l'humain, qui peut être reformulé à tout moment. Quand l'interface doit réagir à une cause précise, l'erreur porte `code`, stable :

| `code` | `kind` | Cause |
|---|---|---|
| `too_large` | `invalid` | `read_file` : fichier de plus de 50 Mio |
| `network_path` | `invalid` | `read_file`, `save_file` : chemin réseau non permis |
| `not_allowed` | `invalid` | `read_file`, `save_file` : chemin hors des vaults enregistrés et des fichiers ouverts ; `open_vault`, `build_tags` : dossier qui n'est pas un vault enregistré ; `add_vault` : dossier non choisi dans le dialogue |
| `read_only` | `invalid` | `save_file` : extension autre que `.md`/`.markdown`, `.canvas`, `.obsidian/` |
| `gone` | `conflict` | `save_file` avec `expectedMtimeMs` : le fichier n'existe plus sur le disque |

Les autres erreurs n'ont pas de `code` (chemin mal formé, dossier à la place d'un fichier, conflit de date, erreur d'entrée-sortie…). L'interface ne reconnaît jamais une erreur au texte de `message`.

## Fins de ligne

- Le backend ne transforme jamais les fins de ligne : `read_file` renvoie le texte tel quel, `save_file` écrit le texte reçu tel quel. La règle ci-dessous est appliquée par l'interface au moment d'enregistrer.
- Fichier homogène (une seule sorte de fin de ligne : LF, CRLF ou CR) : toutes les lignes sont écrites avec cette fin. Sans aucun saut de ligne : LF.
- Fichier mixte : la fin de chaque ligne est retenue à la lecture. À l'enregistrement, les lignes inchangées en tête et en queue du document gardent leur fin d'origine ; les lignes modifiées ou ajoutées entre les deux prennent la fin majoritaire du fichier (à égalité : LF, puis CRLF). Un fichier mixte ouvert puis enregistré sans modification est donc identique octet pour octet. Après un enregistrement réussi, les fins écrites deviennent la référence.
- Le BOM d'origine est réécrit en tête ; il n'est jamais ajouté ni retiré.

## Tri et filtre de `tags_list`

- Requête : espaces de début et de fin retirés, tous les `#` de tête retirés, minuscules. Requête vide : tous les tags.
- Filtre : le tag (comparé en minuscules, sans repli des accents : `ecole` ne trouve pas `école`) contient la requête.
- Tri, en trois groupes : (0) le tag commence par la requête ; (1) un niveau du tag commence par elle (une occurrence suit un `/`, les occurrences étant cherchées sans chevauchement de gauche à droite) ; (2) les autres. Dans chaque groupe : nombre d'occurrences décroissant (descendants compris), puis clé en minuscules dans l'ordre des points de code.
- Un parent qui n'existe que par ses enfants est listé. Le nom affiché est celui de la première occurrence dans l'ordre de l'arbre du vault.
- L'interface garde la liste complète (`tags_list("")`) en mémoire et applique elle-même ce filtre à chaque frappe. Les deux implémentations sont tenues identiques par un jeu de cas commun, `testdata/tags_list_cases.json`, joué par `crates/rocksy-core/tests/tags_list_cases.rs` et par `ui/tests/tagcases.test.ts`.

## Démarrage

`StartupInfo.firstFile` est la note que l'interface ouvrira en premier : le premier fichier passé en argument, sinon l'onglet actif de la session. Elle est lue côté backend pendant le démarrage de la WebView, avec les mêmes contrôles d'accès et les mêmes effets que `read_file` (surveillance comprise). `null` s'il n'y en a pas, si la lecture a échoué ou si elle n'est pas terminée quand `startup_info` répond : l'interface retombe alors sur `read_file`, qui donne l'erreur lisible le cas échéant. La valeur n'est fournie qu'une fois (au premier `startup_info`).

## Index du vault : une seule passe de lecture

`build_tags` (nom conservé) construit et entretient **tout** l'index d'un vault : tags, liens, recherche plein texte. Lire une note est l'opération coûteuse (Google Drive) : chaque note nouvelle ou modifiée est lue **une seule fois** et alimente les trois. Tags et liens sont disponibles dès le chargement de l'index persisté ; la recherche plein texte peut l'être un peu plus tard (`searchReady`). `tags-ready` reste émis comme avant.

- `open_vault` et `build_tags` d'un même vault peuvent arriver dans n'importe quel ordre (l'interface les envoie ensemble) : l'index arrive toujours, et le listing fait pour l'arbre sert aussi à l'index (un seul listing du vault). Seul cas à deux listings : `build_tags` appelée plus de 300 ms avant `open_vault`, ou pour un vault qui n'est pas ouvert. Le `close_vault` de l'ancien vault, lors d'un changement de vault, peut lui aussi arriver avant ou après : il n'annule ni l'ouverture ni l'indexation du nouveau.
- Après `save_file` : tags et liens à jour au retour de la commande ; recherche plein texte à jour sous 2 s. Le disque n'est pas relu. Si l'enregistrement a changé les liens du vault, un `index-ready` suit (nouveau `linksGeneration`).
- Notes non lues ou non indexées : une passe peut ne pas réussir à lire une note (fichier tenu par un autre programme, disque synchronisé indisponible) ou perdre des écritures de l'index de recherche (création de fichier refusée dans le dossier de l'index). Ces notes sont comptées dans `IndexReady.pending` ; le backend relance alors lui-même une passe 2 s, puis 5 s, 15 s, 30 s et 60 s plus tard, sans attendre un changement dans le vault, et chaque passe ne relit que ce qui manque. Tant que `pending` n'est pas nul, l'interface n'affiche pas l'index comme à jour. Chaque occurrence laisse une ligne `index-pending` dans `timings.jsonl` (nombre de notes, lectures échouées, erreur du moteur de recherche).
- Persistance de l'index : après une réconciliation qui a relu ou retiré des notes, à la fermeture du vault ou de l'application, sinon au plus toutes les 60 s. Si l'application est tuée entre-temps, les notes concernées sont simplement relues au lancement suivant.
- Index de recherche endommagé : jamais d'erreur ni de plantage ; `searchReady: false` pendant la reconstruction automatique.
- `IndexReady.linksGeneration` change quand un lien du vault change (cible, position) ou quand l'ensemble des fichiers change. L'interface peut éviter de redemander ou de redessiner les liens quand la valeur n'a pas bougé.

## Recherche

- Insensible à la casse et aux accents (`resume` trouve `Résumé`). Les mots sont combinés par ET. Le dernier mot saisi est un préfixe (recherche à la frappe : `archi` trouve `architecture`).
- `"expression exacte"` ; `-mot` exclut ; `path:texte` restreint aux chemins contenant ce texte ; `tag:a/b` (ou `tag:#a/b`) restreint aux notes portant ce tag ou un descendant. Une requête faite seulement de filtres liste les notes correspondantes.
- Le nom du fichier compte plus que le corps. Résultats triés par pertinence.
- Les extraits viennent de l'index, sans relire les fichiers.
- Tant que la recherche n'est pas disponible, `search` ne renvoie pas d'erreur : `hits` vide, `total` 0, `searchReady: false`. Une requête vide renvoie un résultat vide.
- `line`, `col` et `len` suivent le texte exact renvoyé par `read_file` (BOM compris), comme les positions des tags.
- `limit` est plafonné à 1000 (`search`, `tag_fragments`, `unresolved_links`).

## Liens

- Reconnus : `[[cible]]`, `[[cible#titre]]`, `[[cible#^bloc]]`, `[[cible|alias]]`, embeds `![[...]]`, liens Markdown `[texte](chemin.md)` et images `![alt](chemin.png)` vers un fichier local (les URL `http:`, `https:`, `mailto:` etc. sont ignorées). Pas dans le code inline ni les blocs de code. `[[#titre]]` (lien interne à la note) est ignoré.
- Résolution d'une cible wikilink, comme Obsidian : chemin exact depuis la racine du vault, sinon suffixe de chemin, sinon nom de fichier (extension `.md` facultative), sans tenir compte de la casse ; en cas d'ambiguïté, le chemin le plus court, puis l'ordre alphabétique. Un lien Markdown est résolu relativement au dossier de la note, puis depuis la racine. La résolution porte sur tous les fichiers visibles du vault (notes et pièces jointes) et est recalculée quand des fichiers apparaissent ou disparaissent, sans relire les notes.

## Suivi des changements externes

Après `open_vault`, le backend écoute le dossier du vault (dossiers à point ignorés, rafales regroupées : 400 ms de silence, 3 s au plus). Le watcher n'est qu'un déclencheur : à chaque lot, le backend refait **un seul** listing du vault (au plus une passe par seconde), émet `tree-updated` si l'arbre a changé, puis réconcilie l'index avec ce même listing et émet `tags-ready` (`fromCache: false`, `reread`, `removed`) et `index-ready` si des notes ont été relues ou retirées.

- Un enregistrement fait par Rocksy ne déclenche ni listing ni `tree-updated`, sauf s'il crée un fichier.
- Un lot qui ne touche aucun `.md` mais contient un dossier ou un chemin sans extension de fichier connue (dossier renommé ou déplacé) relance aussi la réconciliation de l'index.
- Pendant une indexation en cours, un changement externe rafraîchit l'arbre tout de suite ; les tags, les liens et la recherche suivent à la fin de l'indexation.
- Filet de sécurité : la même passe est relancée quand la fenêtre revient au premier plan, si la dernière date de plus de 30 s.
- Les fichiers ouverts restent suivis par `file-changed` (sondage toutes les 2 s) ; un fichier ouvert qui disparaît est signalé par `deleted: true` après deux sondages consécutifs sans le trouver.
- `close_vault`, l'ouverture d'un autre vault et `remove_vault` arrêtent ce suivi.

## Fermeture

1. L'utilisateur demande la fermeture de la fenêtre (croix, Alt+F4). La fenêtre ne se ferme pas d'elle-même : le backend émet `close-requested`.
2. L'interface appelle `close_ack` dès réception, pour signaler qu'elle est vivante. Sans `close_ack` dans les 5 s, le backend ferme l'application (interface gelée ou page non chargée). Des demandes de fermeture répétées pendant l'attente ne changent rien.
3. L'interface enregistre tous les documents modifiés en attendant la fin de chaque enregistrement. Si un enregistrement échoue, si un document est en conflit, ou si l'enregistrement automatique est désactivé et qu'il reste des modifications, elle demande confirmation (enregistrer / quitter sans enregistrer / annuler) dans un dialogue de l'application (pas `window.confirm`).
4. Quand tout est enregistré ou que l'utilisateur a confirmé, l'interface appelle `quit`. Le backend arrête le watcher, annule l'indexation en cours (2 s d'attente au plus), persiste l'index, puis sort. Si l'utilisateur annule, l'interface n'appelle rien et l'application reste ouverte ; une nouvelle demande de fermeture relance la séquence.

## Données de l'application

- Dossier itinérant (`%APPDATA%\dev.rocksy.app`, `StartupInfo.dataDir`) : `vaults.json`, `session.json`, la liste des fichiers ouverts explicitement (`opened.json`), `timings.jsonl` (jalons de démarrage, une ligne `vault-listing` par listing d'un vault, une ligne `index-pending` par passe incomplète ; pas de contenu de note ; remis à zéro au-delà de 1 Mo).
- Dossier local (`%LOCALAPPDATA%\dev.rocksy.app`) : `trees/` (instantanés d'arbre), `tags/` (tags et liens), `search/` (texte intégral). Ces données contiennent les noms de fichiers et le texte des notes : elles ne partent pas avec un profil itinérant. Celles qui existaient dans le dossier itinérant sont déplacées au premier lancement, sans reconstruction.
- `ROCKSY_DATA_DIR` (tests, mesures) fixe les deux dossiers au même endroit.
- `remove_vault` supprime les données dérivées du vault retiré.

## Types

```ts
type StartupInfo = {
  argFiles: string[];          // fichiers passés en ligne de commande
  vaults: VaultRef[];
  session: Session | null;     // dernière session (vault actif, onglets)
  firstFile: FileContent | null; // note à ouvrir en premier, déjà lue (voir « Démarrage »)
  dataDir: string;
  version: string;
  processStartMs: number;      // epoch ms au début de main()
};
type VaultRef = { path: string; name: string };
type Session = { activeVault: string | null; tabs: string[]; activeTab: string | null };
type FileContent = { path: string; text: string; mtimeMs: number; size: number; readOnly: boolean };
type Tree = { root: string; fromSnapshot: boolean; entries: TreeEntry[] };   // triées : dossiers d'abord, ordre alphabétique insensible à la casse
type TreeEntry = { path: string; isDir: boolean };
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
type TagsProgress = { vault: string; done: number; total: number };
type TagsReady = {
  vault: string;
  files: number; tags: number; occurrences: number; ms: number;
  fromCache: boolean;
  reread?: number; removed?: number; persisted?: boolean;
};
type IndexReady = {
  vault: string;               // racine du vault concerné, même forme que Tree.root
  fromCache: boolean;
  files: number;               // notes indexées
  tags: number; occurrences: number;
  links: number; unresolvedLinks: number;
  linksGeneration: number;     // change quand un lien ou l'ensemble des fichiers change
  searchReady: boolean; searchDocs: number;
  reread?: number; removed?: number;
  pending?: number;            // notes que la dernière passe n'a pas pu mettre à jour (relues automatiquement) ; absent hors passe
  ms: number;
};
type FileChanged = { path: string; mtimeMs: number; deleted?: boolean };
type ApiError = {
  kind: "not_found" | "not_utf8" | "conflict" | "io" | "invalid";
  message: string;             // anglais, pour l'humain ; jamais analysé par l'interface
  code?: "too_large" | "network_path" | "not_allowed" | "read_only" | "gone";  // voir « Erreurs »
};
```

## Règles des tags

- Syntaxe Obsidian : `#` précédé d'un début de ligne ou d'un espace, suivi de lettres, chiffres, `_`, `-`, `/` ; au moins un caractère non numérique ; pas dans le code inline, les blocs de code, les URL, les wikilinks ni les titres de liens.
- Frontmatter : clés `tags` / `tag`, liste YAML ou chaîne séparée par des virgules ; `#` optionnel. Fragment = fichier entier (`startLine: 0`, `endLine` dernière ligne).
- Fragment d'un tag en ligne = bloc englobant : paragraphe ; élément de liste **avec ses sous-éléments** ; titre seul ; ligne de tableau ; citation/callout entier.
