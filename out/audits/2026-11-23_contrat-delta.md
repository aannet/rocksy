# Changements de contrat décidés pour les correctifs des audits (23/11/2026)

Document de travail de la session principale. Il fixe, pour les lots 3 (coquille Tauri) et 4 (interface), les noms et formes exacts à utiliser. `CONTRACT.md` est mis à jour à partir de ce document par le lot 3 ; en cas de doute, ce document prime jusqu'à ce que `CONTRACT.md` soit à jour.

## 1. Accès aux fichiers (SEC-3, SEC-4)

- `read_file`, `save_file`, `set_session` renvoient l'erreur `invalid` pour un chemin que la page n'a pas le droit d'atteindre.
- Lecture permise : sous un vault enregistré ; un fichier ouvert explicitement par l'utilisateur (argument de ligne de commande, second lancement, dialogue `pick_file`, glisser-déposer, onglet restauré qui avait été ouvert ainsi) ; et, autour d'un tel fichier hors vault, les images et fichiers texte (`.md`, `.markdown`, `.txt`, `.canvas`, images) de son dossier et de ses sous-dossiers.
- Écriture permise : sous un vault enregistré, ou dans un fichier ouvert explicitement. Un fichier atteint par un lien depuis une note hors vault est en lecture seule, y compris après redémarrage : la liste des fichiers ouverts explicitement est tenue et persistée par le backend, jamais fournie par la page.
- Chemins réseau (`\\serveur\partage`, `//serveur/partage`, `\\?\UNC\…`) venus de la page : refusés, sauf s'il s'agit exactement d'un fichier ouvert explicitement ou d'un vault enregistré sur un partage. Les images et liens d'une note posée sur un partage réseau ne sont pas chargés. Chemins de périphérique toujours refusés.
- `save_file` n'accepte que `.md` et `.markdown` ; toute autre extension → `invalid`. Le remplacement du fichier est retenté environ 2 s si le fichier est verrouillé (erreur `io` ensuite) ; le contrôle de conflit est refait avant chaque tentative.
- `read_file` renvoie `invalid` au-delà de 50 Mio.
- `open_vault` et `build_tags` renvoient `invalid` pour un dossier qui n'est pas un vault enregistré.
- Nouveau champ : `FileContent.readOnly: boolean`, vrai quand `save_file` serait refusé pour ce chemin (extension non Markdown, ou écriture non permise). L'interface ouvre alors le document en lecture seule (pas d'édition, pas d'enregistrement automatique).
- `set_session` ignore silencieusement les onglets que la page n'a pas le droit de lire et un `activeVault` non enregistré.
- Protocole `asset:` : limité aux vaults enregistrés et aux dossiers des fichiers ouverts explicitement ; `.obsidian/` exclu.

## 2. Démarrage (PERF-5)

- `StartupInfo.firstFile: FileContent | null` : la note que l'interface ouvrira en premier (premier fichier passé en argument, sinon l'onglet actif de la session), lue côté backend pendant le démarrage de la WebView, avec les mêmes contrôles d'accès que `read_file`. `null` si aucune, si la lecture échoue ou si elle n'est pas terminée quand `startup_info` répond (l'interface retombe alors sur `read_file`).

## 3. Index, recherche, liens (PERF-3, PERF-4, PERF-7, SEC-15, BUG-3)

- Après `save_file` : tags et liens à jour au retour ; recherche plein texte à jour sous 2 s.
- Persistance de l'index : après une réconciliation qui a relu ou retiré des notes, à la fermeture, sinon au plus toutes les 60 s.
- `search`, `tag_fragments`, `unresolved_links` : `limit` plafonné à 1000.
- Index de recherche endommagé : jamais d'erreur ni de plantage ; `searchReady: false` pendant la reconstruction automatique.
- `IndexReady.linksGeneration: number` : change quand un lien du vault change (cible, position) ou quand l'ensemble des fichiers change. L'interface peut éviter de redemander ou de redessiner les liens quand la valeur n'a pas bougé.

## 4. Événements (BUG-15, BUG-14, PERF-6, PERF-1)

- `tags-progress`, `tags-ready` et `index-ready` portent un champ `vault: string` (racine du vault concerné, même forme que `Tree.root`). L'interface ignore un événement dont le vault n'est pas le vault actif.
- `TreeEntry` devient `{ path: string; isDir: boolean }` (plus de `size` ni de `mtimeMs`, que l'interface n'utilisait pas).
- `file-changed` : `{ path, mtimeMs, deleted?: boolean }` ; `deleted: true` quand un fichier ouvert a disparu (supprimé ou déplacé).
- Un enregistrement fait par Rocksy ne déclenche plus de listing ni de `tree-updated`, sauf s'il crée un fichier.
- Pendant une indexation en cours, un changement externe rafraîchit l'arbre tout de suite ; les tags, liens et la recherche suivent à la fin de l'indexation.

## 5. Fermeture (BUG-1, BUG-6, BUG-18)

- Nouvel événement `close-requested` (sans charge utile) : émis quand l'utilisateur demande la fermeture de la fenêtre. La fenêtre ne se ferme pas d'elle-même.
- Nouvelle commande `close_ack` (sans argument, `void`) : l'interface l'appelle dès réception de `close-requested`, pour signaler qu'elle est vivante. Sans `close_ack` dans les 5 s, le backend ferme l'application (interface gelée).
- L'interface enregistre alors tous les documents modifiés en attendant la fin de chaque enregistrement. Si un enregistrement échoue, si un document est en conflit, ou si l'enregistrement automatique est désactivé et qu'il reste des modifications, elle demande confirmation (enregistrer / quitter sans enregistrer / annuler) dans un dialogue de l'application (pas `window.confirm`).
- Nouvelle commande `quit` (sans argument, `void`) : l'interface l'appelle quand tout est enregistré ou que l'utilisateur a confirmé. Le backend arrête le watcher, annule l'indexation en cours (2 s d'attente au plus), persiste l'index, puis sort. Si l'utilisateur annule, l'interface n'appelle rien et l'application reste ouverte.

## 6. Vaults et données (BUG-16, BUG-9, SEC-7, BUG-17)

- Nouvelle commande `close_vault` (`path`, `void`) : arrête le suivi du dossier, persiste puis libère l'index de ce vault. `open_vault` d'un autre vault fait de même pour le précédent. `remove_vault` ferme le vault s'il est ouvert et supprime ses données dérivées (instantané d'arbre, index des tags et des liens, index de recherche) ; il ne touche toujours à aucun fichier du vault.
- Les données dérivées et volumineuses (`trees/`, `tags/`, `search/`) vivent dans le dossier de données local de l'application (`%LOCALAPPDATA%\dev.rocksy.app`), plus dans le profil itinérant ; les données existantes sont déplacées au premier lancement, sans reconstruction. La liste des vaults et la session restent dans `%APPDATA%\dev.rocksy.app`. `ROCKSY_DATA_DIR` fixe les deux dossiers au même endroit.
- Exception à la règle « la seule écriture dans un vault est `save_file` » : à l'ouverture d'un vault, Rocksy peut supprimer ses propres fichiers temporaires orphelins d'écriture atomique (nom commençant par `.` et finissant par son suffixe propre), âgés de plus d'une heure. Rien d'autre.
