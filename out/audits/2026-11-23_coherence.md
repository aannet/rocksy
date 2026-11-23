# Cohérence des trois audits — Rocksy 0.2.0 (commit 8db67f6, branche `release-0.1`)

Date : 2026-11-23. Objet : si l'on applique les correctifs de `2026-11-23_securite.md` (SEC-1 à SEC-18), `2026-11-23_bugs-robustesse.md` (BUG-1 à BUG-23) et `2026-11-23_performance.md` (PERF-1 à PERF-15), se marchent-ils dessus, et que risque-t-on de casser ?

Aucun fichier de code modifié, rien commité. Seule écriture hors de ce rapport : deux compilations de `rocksy-cli` dans le dossier temporaire de session (mesure de taille `abort`/`unwind`).

Légende : **[V]** vérifié dans le code ou les sources des dépendances (`~/.cargo/registry`), **[M]** mesuré ici, **[D]** déduit des rapports ou d'un raisonnement, non observé. Les chemins sont relatifs à `out/spikes/a-tauri-cm6/src/` sauf mention contraire.

## Synthèse

1. Les trois audits sont globalement compatibles ; il y a 5 doublons nets (BUG-5 = PERF-1, SEC-8 = BUG-11, SEC-12 = BUG-3, SEC-18b = BUG-17, PERF-8 absorbé par PERF-5) et **quatre conflits réels** : la CSP de SEC-2 telle qu'écrite casse l'interface (Tauri injecte un hash/nonce dans `style-src`, ce qui neutralise `'unsafe-inline'`) ; l'allowlist de SEC-3 casse la navigation entre fichiers hors vault et la restauration de session si elle n'est pas conçue avec elles ; PERF-3, PERF-11 et BUG-22 se marchent sur le « writer » tantivy (le manifeste en mémoire est mis à jour avant le commit) ; PERF-5 fait doublon avec `build_tags` et perd les événements émis avant `wireEvents`.
2. Le point le plus sérieux côté régression est BUG-3 (`panic = "unwind"`) : `tree.rs:214` avale une panique de thread (`unwrap_or_default`) et renverrait un listing partiel traité comme la vérité (fichiers « supprimés » de l'index et de l'instantané). Parade dans le correctif.
3. `main.rs` concentre 14 constats (watcher, worker, ouverture de vault, sortie, autorisation des chemins) et n'est ni compilé ni testé sous Linux **[V]** : toute la logique à corriger doit d'abord descendre dans `rocksy-core` pour être testable, sinon aucun test existant ne protège ces correctifs.
4. Le contrat doit être réécrit **une seule fois**, avant le lot qui touche `main.rs` : 5 types ou événements changent, 2 commandes sont ajoutées, 9 règles de comportement changent, et BUG-17/SEC-18b exigent d'assouplir la règle dure « aucune écriture dans le vault hors `save_file` » (suppression de temporaires orphelins).
5. Ordre recommandé : lot 0 (tests de preuve et décisions de conception), lot 1 (correctifs indépendants en parallèle), lot 2 (moteur : BUG-3, PERF-3, PERF-4), lot 3 (contrat puis coque `main.rs` en séquence), lot 4 (interface, parallélisable par fichier), lot 5 (CSP, capacités, mesures Windows). Ne pas faire : PERF-12, PERF-15, PERF-8 ; reporter PERF-11.

Mesures acquises à ne pas dégrader (`out/spikes/m2-resultats.md`, `out/release/README.md`) : éditeur ~0,7 s, arbre ~0,94 s, frappe p95 ≤ 5 ms sur 318 Ko, tags dès l'ouverture, watcher ~430 ms, recherche ~100 ms. Les correctifs qui les touchent sont signalés en partie 4.

---

## 1. Doublons

| Constats | Même défaut | Correctif unique à retenir |
|---|---|---|
| **BUG-5 = PERF-1** (+ PERF-2 en partie) | Chaque enregistrement réveille le watcher, qui relance `reconcile_tree` puis `TagIndex::reconcile` (deux listings complets, réécriture de l'instantané, `tree-updated` de 2,3 Mo). **[V]** `main.rs:347-350`, `356-392`. | Un seul mécanisme « écritures propres » (voir 2.2), logique dans `rocksy-core`, plus BUG-5(b) côté UI (ne pas incrémenter `epochN` si l'ensemble des chemins est inchangé). PERF-2 (un seul listing partagé par passe) reste un correctif distinct, complémentaire. |
| **SEC-8 = BUG-11** | `scan_link`/`matching` balaient la fin de ligne pour chaque `[` : quadratique, payé aussi dans `save_file` **[V]** `parse.rs:182-239`. | Un seul correctif dans `tags/parse.rs`. Prendre celui de BUG-11 **avec une borne unique** (une constante partagée par `matching` et par la boucle `]]` de `scan_link`) ; ne pas retenir « sauter jusqu'à la fin de ligne après un `[` non fermé » (voir 6.3). La seconde moitié de SEC-8 (`parse.rs:495-525`, `window_start`/`tag_is_far`, quadratique avec beaucoup de tags sur une ligne) n'est **pas** dans BUG-11 : à garder. L'autre moitié de SEC-8, « sortir l'index du chemin synchrone de `save_file` », contredit PERF-3 : retenir PERF-3. |
| **SEC-12 = BUG-3** | `panic = "abort"` + tantivy qui panique sur un segment endommagé. | BUG-3 (superset : reproduit, + BUG-22, BUG-23). Gravité à aligner sur « Majeur » ; SEC-12 la classe « Faible ». |
| **SEC-18(b) = BUG-17** | Temporaires `.rocksy-tmp` orphelins. | BUG-17. Attention au contrat : c'est une suppression dans le vault (voir 5). |
| **PERF-5 ⊃ PERF-8** | Lancer l'index plus tôt. | PERF-5 (côté Rust). PERF-8 devient sans objet (le rapport de performance le dit lui-même). |
| BUG-2 / BUG-18 / BUG-7 / BUG-1 | Quatre pertes de texte distinctes dans `app.ts` (rechargement, fermeture d'onglet, erreur avalée, fermeture de fenêtre). | Pas doublons. À traiter ensemble (même fonctions), voir lot 4. |
| SEC-3 (UI) / BUG-21 | Les deux concernent `.txt` et fichiers sans extension. | Pas doublons, mais une seule décision : que fait-on d'un `.txt` ? (voir 2.3) |
| SEC-4 / PERF-15 / BUG-14 | Trois mentions du sondage `mtime` toutes les 2 s. | Pas doublons. PERF-15 rejeté (6.3). |
| SEC-15 / PERF-? | `limit` arbitraire. | SEC-15 seul. |

Compatible, rien à fusionner : SEC-5 et SEC-4 (le premier restreint le scope, le second refuse l'accès réseau avant le test de scope, les deux sont nécessaires) ; SEC-9 et SEC-1 (même fichier `canvas.ts`, mais correctifs disjoints) ; SEC-14 et tout le reste.

---

## 2. Correctifs contradictoires ou qui s'annulent

### 2.1 PERF-4 / PERF-3 face à BUG-1, BUG-6, BUG-3 : cohérence de l'index si l'app est tuée

**Verdict : cohérent, à condition de traiter trois détails.**

Ce que j'ai vérifié **[V]** (`tags/index.rs:441-600`, `search/engine.rs:240-440`) : `reconcile` ne saute une note que si les **deux** magasins connaissent la même paire (taille, mtime) : `in_index` (le `.tags`, en mémoire puis persisté) **et** `in_search` (`engine.meta(rel)`, le manifeste du moteur). Si l'un est en retard, la note est relue, et la lecture ne met à jour que le magasin en retard (`index_current[i]`). Ce cas (« l'application a été tuée entre les deux écritures ») est déjà prévu par le code. Conséquence pour un `kill` avec PERF-3 + PERF-4 appliqués :

- `.tags` en retard de ≤ 60 s, tantivy en retard de ≤ 2 s : les notes enregistrées pendant ces fenêtres sont relues au lancement suivant. Coût borné : une lecture par note concernée (~155 ms sur Drive, en parallèle sur 16 threads). Pas d'index faux.
- Les deux dans des états différents : chaque magasin est réparé indépendamment. Pas d'état intermédiaire dangereux.
- BUG-3 (index tantivy endommagé, reconstruction) ne change rien à cette logique : après suppression du dossier `search/`, `in_search` vaut faux partout, tout est relu une fois (comme aujourd'hui).

Les trois détails :

1. **Le manifeste du moteur est mis à jour dans `upsert`, avant le commit** **[V]** (`engine.rs`, `upsert` : `manifest.insert` puis `dirty = true`). Avec PERF-3 (commit différé), tout événement qui jette le « writer » sans commit perd des opérations tout en laissant le manifeste affirmer qu'elles sont indexées : `reconcile` ne relit alors jamais ces notes dans la session (la recherche reste périmée jusqu'au prochain lancement, qui recharge le manifeste depuis le disque). Trois correctifs libèrent ou remplacent le writer : **PERF-11** (libérer après 30 s), **BUG-22** (`*guard = None` après échec) et **BUG-3** (désactiver/recréer le moteur après panique). Règle commune à imposer : avant de libérer un writer, committer ; si le commit échoue, recharger le manifeste depuis l'état committé (`load_manifest`). Sans cela BUG-22 et PERF-3 se contredisent.
2. **`flush_tags` ne committe pas le moteur** **[V]** (`main.rs:422-432` : il annule, joint, puis `state.tags.save`). C'est sans importance aujourd'hui, car chaque mise à jour committe de façon synchrone. Avec PERF-3, `flush_tags` doit committer le moteur ; avec BUG-6 (attente de 2 s au plus), ce commit doit avoir lieu **même si le `join` expire**, sur le thread appelant, pas dans le thread de build.
3. **Le test existant `tests/vault_index.rs:512-519` (« A save: searchable when the call returns », puis `search_docs == 6`) et `vault_index.rs:282` formalisent le contrat actuel** : la recherche voit le texte dès le retour de `update_file_with_meta`. PERF-3 le change en « sous 2 s ». Ces tests doivent être modifiés **dans le même lot** (ajouter un appel `flush`/`commit` explicite dans l'API, ou une variante `update_file_with_meta_deferred`), et `CONTRACT.md` (« `save_file` met à jour les trois sans relire le disque ») doit dire que la recherche suit avec un retard borné. Observable : on enregistre puis on lance une recherche dans la seconde qui suit, et on ne trouve pas le nouveau mot. À accepter explicitement.

Point secondaire : PERF-4 (persistance au plus toutes les 60 s) et le contrat actuel (« quelques secondes après un enregistrement », `CONTRACT.md` ligne 35) : la phrase du contrat est à changer.

### 2.2 PERF-1 (ignorer les écritures propres) face à BUG-4, BUG-14 et au suivi externe

**Verdict : compatible, sans nouvelle classe de changement raté, mais le mécanisme doit être précisé.**

- Peut-on rater un vrai changement externe arrivé juste après un enregistrement ? Le filtre proposé compare `(chemin, taille, mtime)` au `stat` **au moment du lot** (≥ 400 ms après). Un écrivain externe qui modifie le fichier après notre `rename` change le mtime (NTFS : 100 ns ; Drive : ms) et le lot repart par le chemin normal. Rater un changement exigerait même taille **et** même mtime, c'est-à-dire un outil qui préserve le mtime. Ce trou existe déjà partout dans l'app : le poller (`main.rs:442-451`, `watched.get == known`), `expectedMtimeMs` de `save_file`, et `reconcile` (documenté « un contenu modifié sans changement de taille ni de mtime n'est pas vu », `index.rs:439`). PERF-1 n'ajoute pas de classe d'erreur.
- BUG-4 (écriture externe écrasée pendant les tentatives de `rename`) est indépendant : l'écrivain externe perdu l'est avant toute écriture propre enregistrée. Mais les deux touchent `save_file` côté coquille : l'enregistrement de l'écriture propre doit se faire **après** le succès de `save_file`, jamais sur erreur `conflict`.
- BUG-14 (fichier supprimé) : le `stat` échoue, la paire ne correspond pas, le lot repart par le chemin normal ; PERF-1 ne masque pas une suppression.

Précisions à imposer, sinon régressions **[V]** sur le code actuel :

1. `RefreshRequest` ne porte pas les chemins (`main.rs:50-54`) et le worker **fusionne** les demandes (`main.rs:361-364`). Il faut fusionner aussi les ensembles de chemins ; un lot mixte (écriture propre + autre chemin) repart intégralement par le chemin normal.
2. Comparer les chemins avec `path_key` (insensible à la casse sur Windows), pas en chaîne brute : le watcher peut rapporter une casse différente de celle de `absolute_string`.
3. Une création de fichier par `save_file` (nouvelle note) n'est pas dans l'arbre : le lot doit être traité normalement (ou l'entrée ajoutée à l'arbre en mémoire). L'UI n'a aujourd'hui aucune commande « nouvelle note », mais `save_file` sans `expectedMtimeMs` crée un fichier.
4. Durée de vie de l'enregistrement : consommé une fois ou expirant après quelques secondes ; sinon un changement externe identique ultérieur est ignoré.
5. Réutiliser `watched` (déjà `chemin → mtime`, mis à jour par `save_file`, `main.rs:121`) plutôt qu'une seconde table ; il faut ajouter la taille.
6. Mettre à jour l'instantané d'arbre (taille, mtime de l'entrée) sinon le prochain listing réel (focus, autre lot) trouvera l'entrée « différente » (`same_entries` compare taille et mtime, `tree.rs:393-400`) et émettra un `tree-updated` inutile.
7. **Mettre cette logique dans `rocksy-core`** (par exemple une structure `OwnWrites` consultée par une fonction pure) : `src-tauri` est hors workspace et ne se compile pas sous Linux **[V]** (CLAUDE.md), donc le worker n'a aucun test unitaire.

BUG-8 (déplacement de dossier : `markdown = 0`) doit être évalué **avant** le filtre PERF-1 : un chemin sans extension ou un dossier n'est jamais une écriture propre.

### 2.3 PERF-5 et PERF-12 face à SEC-3/SEC-4, et SEC-3 face aux usages hors vault

**PERF-5 : la lecture anticipée doit passer par les mêmes contrôles et faire les mêmes effets de bord que `read_file`.** Aujourd'hui la commande `read_file` fait deux choses **[V]** (`main.rs:105-110`) : lire (`core::read_file`) et enregistrer `(chemin → mtime)` dans `watched`. Si l'UI saute `read_file` grâce à `firstFile`, la note active n'est plus suivie pour les changements externes tant que `set_session` ne passe pas (200 ms plus tard, et avec le mtime **courant**, pas celui lu : fenêtre de course). À corriger dans PERF-5 : le thread anticipé fait le même `watched.insert`. Il doit aussi appeler la même fonction d'autorisation que SEC-3 (une seule fonction `authorize_read(path)`), initialiser `tags_job` (sinon `save_file` n'alimente pas l'index, `main.rs:124-130`), et, s'il lance `load_persisted` + `open_search`, il émet `tags-ready`/`index-ready` **avant** que l'UI ait appelé `listen` (`wireEvents` est après le premier rendu, `main.ts:56-59`) : événements perdus. Il faut soit rejouer l'état (commande `index_state`, ou valeur dans `startup_info`), soit n'anticiper que la lecture de la note et le snapshot d'arbre, en laissant `build_tags` charger l'index. Je recommande la seconde option (gain essentiel de PERF-5 = la lecture de la note ; l'index est déjà prêt ~0,4 s après navigation **[D]**).

**PERF-12** (retirer le premier `metadata` de `read_file`) : à ne pas faire. Gain non mesuré, et sous Windows `File::open` sur un dossier échoue en `PermissionDenied`, donc le test `files.rs:202 read_rejects_non_utf8_and_missing_and_directory` (erreur `invalid` pour un dossier) passerait sous Linux et casserait sous Windows **[D]**. De plus BUG-21 (limite de taille) a besoin de `meta.len()` avant la lecture, et SEC-3/SEC-10 modifient les mêmes fonctions.

**SEC-3 face aux usages hors vault** (la fonction « ouvrir n'importe quel Markdown », CLAUDE.md) :

| Usage | Source du chemin | Casse-t-il avec « allowlist (a) vault enregistré + (b) fichiers ouverts explicitement » ? |
|---|---|---|
| Argument de ligne de commande | Rust (`files_from_args`) | Non, si on alimente l'ensemble côté Rust. Garder les chemins UNC (un `.md` sur un partage réseau est un double-clic légitime) : refuser UNC seulement pour les chemins venus de l'UI. |
| Second lancement (instance unique) | Rust (`emit_open_files`) | Non, même condition. |
| Glisser-déposer | Rust (`on_window_event`, `main.rs:517-524`) | Non, même condition. |
| `pick_file` | Rust | Non. |
| **Restauration de session** | `session.json`, relu par l'UI | **Oui** : l'ensemble est vide au lancement. Il faut l'alimenter depuis `load_session` au démarrage (côté Rust), et `set_session` ne doit retenir que des onglets déjà autorisés (sinon une UI compromise s'ajoute elle-même un chemin). |
| **Clic sur un lien relatif depuis un fichier hors vault** (`openTarget`, `app.ts:771-772`) | UI | **Oui** : le chemin est calculé par l'UI, la coquille ne peut pas distinguer ce clic d'un script. L'allowlist (b) casserait la navigation entre fichiers hors vault. Parade : pour la lecture, autoriser les extensions texte (`.md`, `.markdown`, `.txt`) sous le dossier de tout fichier déjà autorisé ; réserver l'allowlist stricte à l'écriture. |
| Images d'une note hors vault | UI (`asset:`) | Concerne SEC-5, pas `read_file`. |
| Fichiers du vault | UI | Non (chemin sous un vault enregistré). `list_vaults` relit `vaults.json` à chaque appel **[V]** (`vaults.rs:34`, `vault_for_path`) : mettre le registre en cache dans `AppState`, invalidé par `add_vault`/`remove_vault`. |

Coût de vérification au démarrage : une comparaison de préfixes sur une liste de quelques vaults, quelques microsecondes une fois le registre en cache. Sans cache, une lecture de `vaults.json` (dans `%APPDATA%`, profil potentiellement itinérant) par appel : à éviter sur le chemin critique.

Autres points SEC-3 **[V]/[D]** :

- La règle « interdire la création de fichier hors vault » ne peut pas être dans `rocksy-core::save_file` : le test `files.rs:231 save_creates_new_file_but_not_directories` crée un fichier dans un dossier temporaire sans vault. La mettre dans la coquille.
- « Refuser `\\?\` et `\\.\` » ne doit s'appliquer qu'au chemin **d'entrée**. `resolve_target` canonicalise le dossier parent et renvoie un chemin `\\?\C:\…` sous Windows (`files.rs:100`) ; appliquer le refus au chemin résolu casserait tous les enregistrements.
- Règle d'extension d'écriture (`.md`/`.markdown`) : l'UI traite `.txt` et les fichiers sans extension comme des notes éditables (`app.ts:384`, `kindOf`) et `pick_file` propose `txt`. Avec la règle de SEC-3, l'enregistrement automatique d'un `.txt` échouerait en boucle (bandeau d'erreur à chaque frappe, et BUG-7 programmerait des nouvelles tentatives). Décision à prendre en lot 0 : soit `.txt` reste enregistrable (règle d'écriture : `.md`, `.markdown`, `.txt`), soit il s'ouvre en lecture seule (ce qui rejoint BUG-21).
- Les scénarios de test qui appellent `T.open(<fichier hors vault>)` ou `T.inv('save_file', …)` sur un chemin non ouvert par un moyen « explicite » échoueront ; `tools/app/helpers.js` (`T.open`) et `is-w1.json:299` sont à passer en revue.

### 2.4 SEC-2 (CSP stricte) face à ce que fait réellement l'UI

**Verdict : la CSP proposée, telle quelle, casse l'affichage.** Raison **[V]** dans les sources :

- `tauri-codegen-2*/src/context.rs:77-80` : à la compilation, `inject_nonce_token` ajoute `nonce="__TAURI_STYLE_NONCE__"` à chaque `<style>` d'un fichier HTML (`dist/index.html` en contient un, le `<style>` de `html, body {…}`, `dist/index.html:8-10`).
- `tauri-2.12.7/src/manager/mod.rs:96-103` et `replace_csp_nonce` (l. 123-150) : à l'exécution, Tauri remplace le jeton par un nonce aléatoire **et l'ajoute à `style-src`** (avec `'self'` et les hash éventuels).
- Conséquence selon CSP niveau 3 **[D]** : dès qu'un nonce ou un hash figure dans la liste, `'unsafe-inline'` est ignoré, y compris pour les attributs `style=""` (`style-src-attr` retombe sur `style-src`).

Ce que la CSP cassée empêcherait :

| Élément | Preuve | Effet |
|---|---|---|
| Lignes de l'arbre virtualisé : `style="top:…px;padding-left:…"` injecté par `innerHTML` | `sidebar.ts:338`, `344`, `503` **[V]** | Toutes les lignes en `top: auto` : arbre et arbre des tags illisibles. |
| Mermaid (Pele) : `<svg … style="max-width:100%;--_bg:…;--_fg:…">` | Rendu Pele 1.13.0 testé ici : 3 attributs `style` pour un graphe de 2 nœuds, 0 balise `<style>` **[M]** ; injecté par `body.innerHTML = r.svg` (`livepreview.ts:319`) | Diagrammes sans couleurs (les variables CSS `--_bg`, `--_l`… pilotent tout). |
| CodeMirror : feuilles de style injectées par `style-mod` (`document.createElement("style")`, sans nonce) | `node_modules/style-mod/src/style-mod.js:125`, option `nonce` ; `EditorView.cspNonce` existe (`@codemirror/view` l. 9044) mais n'est pas utilisé **[V]** | Éditeur sans thème de base. |
| Cellules de tableau `style="text-align:…"` | `livepreview.ts:257` **[V]** | Alignement perdu. |
| Squelette de la barre latérale `style="width:…"` | `app.ts:852` **[V]** | Cosmétique. |
| `el.style.cssText = …`, `el.style.x = …`, `setProperty` | `canvas.ts:96`, `app.ts:850`, `livepreview.ts:131` **[V]** | **Non touchés** : le CSSOM n'est pas soumis à CSP. |

Ce qui ne casse pas : imports dynamiques de Vite (`script-src 'self'`, `modulePreload: false` ; aucun `eval`, `new Function`, `Worker`, `createObjectURL` ni `url()` dans `dist/` **[V]**) ; `blob:` (jamais utilisé) ; `data:` (images `![](data:…)`, déjà dans `img-src`) ; images du vault par `asset:`/`http://asset.localhost` (dans `img-src`) ; IPC (`connect-src ipc: http://ipc.localhost`, que Tauri n'ajoute pas lui-même, la CSP proposée les liste correctement) ; événements Tauri : affirmation de l'audit de performance **vérifiée** (`tauri-2.12.7/src/event/mod.rs:194-206`, `emit_js_script` insère le payload dans un script évalué par `webview.eval`), mais ce `eval` est hôte (`ExecuteScript` de WebView2, hors CSP de la page) **[D]**, il n'a pas besoin de `'unsafe-eval'` ; le script d'initialisation de `withGlobalTauri` est injecté de la même façon.

Correctif utilisable (au choix, à tester avec les scénarios Windows) :
- (A) la plus simple : dans `tauri.conf.json`, `app.security.dangerousDisableAssetCspModification: ["style-src"]` avec `style-src 'self' 'unsafe-inline'`. Le risque résiduel d'injection de style est faible puisque `img-src` et `connect-src` sont fermés.
- (B) plus stricte : `style-src 'self'` (Tauri ajoute son nonce), `style-src-attr 'unsafe-inline'` (directive séparée, non affectée par le nonce), et `EditorView.cspNonce.of(<nonce lu sur l'élément <style>>)` dans l'éditeur. Demande un changement de code.

SEC-2 doit être appliquée **en dernier** (lot 5) : toute régression d'affichage vient de l'UI finale, et SEC-1 la rend moins urgente (défense en profondeur).

### 2.5 SEC-5 (scope `asset:` limité au vault) face aux images hors vault et aux embeds

**Verdict : compatible si le scope est étendu avant la première requête d'image et couvre le dossier du fichier hors vault.**

- `resolveImage` hors vault renvoie le chemin relatif au fichier (`resolve.ts:134-139`, test `logic.test.ts:84`) ou un chemin absolu (`D:\pics\a.png`, même test). Seul le dossier du fichier ouvert est devinable côté Rust ; les images absolues hors de ce dossier cesseraient de s'afficher (régression visible, probablement acceptable : l'audit propose lui-même `allow_file` pour les images « à côté »). Il faut faire `allow_directory(dirname(fichier), recursive)` à chaque ouverture hors vault (dans `read_file`, dans le thread anticipé de PERF-5, dans `emit_open-files`).
- Ordre : le scope doit être étendu **avant** que l'UI ne demande l'image : à faire dans `read_file`/`open_vault`/setup (côté Rust), pas dans une commande séparée appelée ensuite par l'UI. `allow_directory` échappe correctement les motifs glob **[V]** (`tauri-2.12.7/src/scope/fs.rs:483-493`), donc un vault dont le chemin contient `[` ou `*` est géré.
- Coût : `is_allowed` canonicalise le chemin à chaque requête (`fs.rs:419-480`), comme avec `**` aujourd'hui ; `push_pattern` canonicalise le dossier ajouté (`fs.rs:140-148`), un `stat` sur Drive (non mesuré) qu'il vaut mieux faire hors du chemin critique (thread de PERF-5).
- Embeds de canvas (`canvasHost.assetUrl`, `app.ts:785-794`) : mêmes dossiers, rien de plus. SEC-9 (refuser `..` hors racine) va dans le même sens.
- `forbid **/.obsidian/**` : compatible, aucune image utile dans `.obsidian`.

### 2.6 SEC-7 (index vers le dossier local) face aux index existants et à `ROCKSY_DATA_DIR`

- **Index existants** : sous `%APPDATA%\dev.rocksy.app\{tags,search,trees}` **[V]** (`tree.rs:246`, `persist.rs:47`, `engine.rs:53`). Si le code lit désormais `%LOCALAPPDATA%`, il ne trouve rien : instantané d'arbre absent (donc listing synchrone ; sur Drive à froid 57 s contre l'objectif de 3 s), index des tags à reconstruire, recherche à reconstruire (72 Mo pour x20). Parade : au démarrage, **avant** le chargement de l'index (donc avant PERF-5), `rename` des trois dossiers de l'ancien vers le nouvel emplacement s'ils existent (O(1) sur le même volume, sinon les supprimer). À un seul utilisateur (Alec), le risque réel est limité, mais le premier lancement après mise à jour serait sinon lent sur le vrai vault.
- **`ROCKSY_DATA_DIR`** (tests et mesures) : doit fixer les deux dossiers au même endroit, sinon les scénarios qui listent le dossier de données (`is-close.json`, étape `P7_dataDirAfterClose`) et `measure-auto.ps1` (`$DataDir\timings.jsonl`) changent de sens. `timings.jsonl` et `.corrupt` : rester où ils sont, ou documenter.
- **API du cœur** : toutes les fonctions prennent un seul `data_dir` (`lib.rs` : « Every function that persists Rocksy's own data takes the application data directory »). Plus simple : la coquille passe le dossier **local** à toutes les fonctions d'index/arbre/recherche et le dossier roaming seulement à `list_vaults`/`add_vault`/`session`. `remove_vault` (qui, pour la purge de SEC-7, doit connaître les deux) change de signature ; `vaults.rs` a ses tests. `add_vault` refuse un vault qui contient `data_dir` (`vaults.rs:76-85`) : tester aussi le dossier local.
- Gravité : la menace « profil itinérant » est une hypothèse (le rapport le dit). Le correctif reste peu coûteux, mais pas urgent ; la purge dans `remove_vault` est la partie la plus utile.
- Documentation : `out/release/README.md:65,70` et `install.ps1:11` citent `%APPDATA%\dev.rocksy.app` (français, à mettre à jour avec `-Purge`).

### 2.7 BUG-3 (`panic = "unwind"` + `catch_unwind`) face à la taille, au profil release et à PERF-14

- **Taille [M]** : `rocksy-cli` en release, mêmes options, `panic = "abort"` : 5 928 624 octets (égal au chiffre de PERF-14) ; `panic = "unwind"` : 6 663 480 octets, soit **+735 KB (+12,4 %)** sous Linux. PERF-14 mesure −11,8 % pour `lto = "fat"` + `codegen-units = 1` : les deux effets se compensent presque. Sous Windows gnullvm (x86_64) les tables de déroulement existent déjà même en `abort` (SEH), donc l'écart devrait y être plus petit **[D]** ; non mesuré. `rocksy.exe` n'a pas été recompilé.
- **Deux profils à garder alignés** : `Cargo.toml:16` (racine, ne vaut que pour `rocksy-cli`) et `src-tauri/Cargo.toml:34` (workspace propre).
- **Régression latente à traiter dans le même correctif [V]** : `tree.rs:214` (`h.join().unwrap_or_default()`) et `tree.rs:148` (`read_one_dir(...).unwrap_or_default()`). En `abort`, une panique dans un thread de listing tue le processus ; en `unwind`, `join` renvoie `Err` et le listing devient **partiel sans erreur** : `reconcile` y voit des notes supprimées (`drop_file`), `reconcile_tree` écrit un instantané partiel. À transformer en `Err(ApiError)` (aucun résultat partiel). Même vigilance pour les autres `thread::scope` du cœur (`read_stream`) et pour `queue.lock().unwrap()` (`tree.rs:150`) qui propage l'empoisonnement.
- **Commandes Tauri `async`** : une panique dans une commande ne répond jamais à la promesse JS **[D]** ; d'où l'intérêt de `catch_unwind` autour du corps des commandes qui touchent tantivy, avec réponse d'erreur.
- **Les tests tournent déjà en `unwind`** (profil `test`) : le test `damaged_search_index_on_reopen` des preuves (hors dépôt, `~/rocksy-scratch/audit-bugs/tests/proofs.rs`) doit être porté dans le dépôt avant le correctif.
- Recommandation : PERF-14 uniquement pour la construction publiée, après BUG-3, et constater la taille réelle de `rocksy.exe`.

### 2.8 PERF-6 (alléger l'arbre) face à BUG-5, BUG-13, BUG-15 et au contrat

- **PERF-6(a)** (ne plus sérialiser `size`/`mtimeMs`) : l'UI ne s'en sert pas **[V]** (`grep` sur `ui/src` : seuls `types.ts:15` et `scale.json` les mentionnent). Compatible avec tout. Garder les champs en mémoire dans le cœur (`same_entries` compare taille et mtime, `tree.rs:393`, l'instantané binaire les stocke). Changement de contrat : type `TreeEntry`.
- **PERF-6(b)** (`tree-updated` ne porte plus l'arbre mais `{root, version}`) : compatible avec BUG-15 (il porte déjà la racine, et `tree-updated` est déjà filtré par racine, `app.ts:1319-1321`), mais ajoute un aller-retour et une condition de course (version demandée entre deux événements) et casse `ui/tools/scenarios/scale.json` (scénario `treeUpdateKeepsScroll`, qui émet un `tree-updated` complet via le mock) ainsi que le mock. Après PERF-1/BUG-5, l'événement devient rare : gain faible. **Ne retenir que (a)** ; (c) hors de proportion.
- PERF-6 et BUG-5(b) : mêmes lignes (`setTree`, `epochN`) dans `app.ts`.
- Rôle de `fromSnapshot` : l'UI l'affiche (`sidebar.ts:293`, « instantané, vérification… ») ; une version sans charge utile le perdrait.

### 2.9 BUG-1 (interception de la fermeture) face à BUG-6, BUG-18, instance unique, `flush_tags`

Enchaînement cible : `CloseRequested` → `prevent_close` → événement `close-requested` → l'UI enregistre tout et attend → commande `quit` → `RunEvent::Exit` → `flush_tags` (2 s au plus, BUG-6) → processus terminé.

- **BUG-18 d'abord** (garder la promesse de l'enregistrement en vol dans `doc`) : sans cela, `close-requested` ne sait pas attendre un `save` déjà parti (`save()` retourne tout de suite si `doc.saving`, `app.ts:604`), et BUG-1 quitterait pendant l'écriture.
- **UI gelée** : si le fil principal de la WebView est bloqué (SEC-6 : 30 s de gel mesuré ; un rendu Mermaid long), `close-requested` n'est jamais traité et la fenêtre devient impossible à fermer. Prévoir un garde-fou côté Rust (par exemple `app.exit(0)` après 5 s sans `quit`). Non prévu par BUG-1.
- **`confirm()` bloquant** : BUG-1 propose un `confirm` si l'enregistrement échoue ou si l'enregistrement automatique est désactivé. Le scénario piloté par CDP `is-close.json`, qui ferme la fenêtre pendant une indexation, y échappera tant qu'aucun document n'est modifié ; ajouter un scénario avec document modifié.
- **Idempotence** : un second `CloseRequested` pendant l'attente (clic répété) doit être ignoré ; `beforeunload` conservé comme filet ne doit pas lancer un deuxième `save` concurrent (il est déjà neutralisé par `doc.saving`).
- **Instance unique** : pendant l'attente le processus a encore sa fenêtre ; un second lancement émet `open-files` dans une fenêtre qui va se fermer (fichier perdu, comme décrit par BUG-6). Fenêtre de 2 s au pire, à accepter ou à atténuer en gardant le chemin à ouvrir pour la prochaine session.
- **`flush_tags`** : voir 2.1(2) (commit du moteur) ; et à vérifier que `quit` passe par `app.exit(0)` pour déclencher `RunEvent::Exit` (pas `std::process::exit`).
- Tauri `Focused`/refresh worker : `refresh_on_focus` ne doit pas relancer une passe pendant la fermeture.

### 2.10 SEC-6 / BUG-10 / BUG-11 / SEC-8 (regex et parseurs linéaires) face aux tests et au comportement

- **SEC-6** (`syntax.ts:131`, regex de `INLINE`) : le correctif proposé `\[([^\]\n]+)\]\(([^()\s]*)(?:\s[^()]*)?\)` interdit `(` dans la destination et le titre. Le comportement actuel s'arrêtait déjà au premier `)` ; le nouveau refuse en plus `[x](a(b).md)` (plus de lien du tout au lieu d'un lien tronqué). Acceptable, mais le test `logic.test.ts:176-184` (`renderInlineHtml`) doit recevoir ce cas, plus un test de durée (4 000 caractères `[a](` : borne large, < 100 ms). Ajouter le plafond de 2 000 caractères par cellule. Le test est possible sous Node (fonction pure) ; un test de `mountCanvas` ne l'est pas (environnement vitest `node`, pas de DOM).
- **BUG-11/SEC-8** : voir 6.3 pour le piège de « sauter après un `[` non fermé ». Les tests existants à relancer : `tags/link_tests.rs`, `tags/tests.rs` (grammaire, tags dans les liens), `tests/contract.rs`. Test à ajouter : lignes de 100 Ko de `[`, `[[`, `[a](` avec une borne de temps, et un cas de non-régression `[ voir [a](b.md)` (un `[` isolé suivi d'un vrai lien doit encore donner le lien).
- **BUG-10** (`search/highlight.rs:132`, mémoriser la fin de la 3e ligne retenue) : indépendant, comportement de la recherche inchangé ; tests : `highlight.rs` (tests unitaires l. 246) et `vault_index.rs` ; ajouter une ligne de 500 Ko.
- Les quatre corrections sont dans des fichiers disjoints (`syntax.ts`, `parse.rs`, `highlight.rs`) : parallélisables.

### 2.11 Autres frictions détectées

- **BUG-4 et BUG-7** modifient la même fonction (`atomic.rs` `rename_with_retry`/`write_atomic`). BUG-7 allonge la fenêtre de tentatives à ~2 s : cela rend BUG-4 plus important (plus de temps pour qu'un écrivain externe termine). Mais `write_data_file` (index, instantanés, session) passe par le même `write_atomic` : une fenêtre de 2 s retarderait aussi les persistances de sortie (budget de 2 s de BUG-6). Paramétrer le budget : long pour les fichiers de l'utilisateur, court pour `data_dir`.
- **PERF-2** (un seul listing partagé) face à **BUG-13** (le worker se bloque sur `build_lock`) : fusionner l'arbre et l'index dans un même thread aggraverait BUG-13 pendant la première indexation (minutes sur Drive). Séquencer « arbre d'abord (émettre), index ensuite avec le même listing » ; BUG-13 (ne rafraîchir que l'arbre si un build est en cours, `try_lock`) reste nécessaire.
- **PERF-2 + PERF-1 + PERF-6** veulent tous un « arbre du dernier listing en mémoire » : une seule structure dans `AppState` (arbre partagé + compteur de version), pas trois.
- **PERF-7** (`links_generation`) face à PERF-3/PERF-4 : compatible (`is_dirty` utilise `generation`, qu'on ne touche pas).
- **PERF-10** (filtre de tags en JS) dépend de **BUG-15** : mettre la liste de tags en cache à chaque `tags-ready` sans filtrer par vault empoisonnerait le cache après un changement de vault. Faire BUG-15 d'abord. BUG-15 oublie aussi `tags-progress`, qui n'a pas de racine non plus.
- **BUG-5(b)** (pas d'`epochN++` si les chemins sont inchangés) et **PERF-6** touchent `setTree` ; **SEC-3 UI**, **BUG-21**, **BUG-12** touchent `kindOf`/`load`/`reload` (le calcul de `eol` est dupliqué dans `load` `app.ts:464` et `reload` `app.ts:646`).
- **SEC-1** (canvas) : les correctifs de SEC-1, SEC-6 (partie canvas) et SEC-9 sont dans `canvas.ts`/`app.ts` ; BUG-19 (`removePane`) dans `app.ts`. Un seul agent.
- **BUG-9** (numéroter les demandes d'ouverture) et **BUG-16** (`close_vault`) et **PERF-5** modifient tous `open_vault`/`build_tags`/`AppState` : même zone.
- **SEC-14** (réduire `core:default`) : compatible avec l'UI actuelle, qui n'appelle que des commandes de l'app et `event.listen` ; `dialog:default` est inutile côté JS (les dialogues sont ouverts par `pick_file`/`pick_folder` en Rust). À tester en lançant l'app.

---

## 3. Zones de code touchées par plusieurs correctifs

« Un agent » = ne pas confier à deux agents en parallèle.

| Zone | Constats qui la modifient | Parallélisable ? |
|---|---|---|
| `src-tauri/src/main.rs` : commandes `read_file`, `save_file`, `startup_info`, `set_session`, `open_vault`, `build_tags`, `search`/`tag_fragments`, `open_external`, `setup`, `invoke_handler` | SEC-3, SEC-4, SEC-5, SEC-7, SEC-15, SEC-18(a), PERF-1, PERF-2, PERF-5, PERF-8, PERF-6, BUG-1, BUG-3, BUG-9, BUG-15, BUG-16, BUG-17, SEC-12 | **Un agent, en sous-étapes séquentielles** (3a à 3d, partie 6) |
| `main.rs` : worker de rafraîchissement, `watch_current_vault`, `refresh_on_focus` | PERF-1, PERF-2, PERF-6, BUG-5, BUG-8, BUG-13, BUG-15 (payloads), PERF-15 | idem |
| `main.rs` : `spawn_file_watcher` (poller) | BUG-14, PERF-15, SEC-4 | idem |
| `main.rs` : `spawn_tags_saver`, `flush_tags`, `RunEvent::Exit`, `on_window_event` | PERF-4, PERF-3, BUG-1, BUG-6, BUG-3 | idem |
| `tags/index.rs`, `tags/store.rs`, `search/engine.rs` | PERF-3, PERF-4, PERF-7, PERF-11, BUG-3, BUG-6, BUG-13, BUG-22, BUG-23, SEC-12, SEC-7 (chemins) | **Un agent** (lot 2) |
| `tree.rs` | PERF-1 (entrée d'instantané), PERF-2 (`reconcile_tree` renvoie l'arbre), PERF-6a, BUG-6 (`list_tree` annulable), BUG-3 (`unwrap_or_default` l. 148/214), SEC-15 (plafond des chemins au décodage), SEC-7 | Un agent (avec le lot 2 ou 3b) |
| `files.rs`, `atomic.rs` | BUG-4, BUG-7 (partie Rust), BUG-17, BUG-21, SEC-3 (règle d'extension), SEC-10, PERF-12 (rejeté) | **Un agent**, parallèle aux autres zones du cœur |
| `tags/parse.rs` | SEC-8, BUG-11 | seul sur ce fichier |
| `search/highlight.rs` | BUG-10 | seul |
| `watch.rs` | BUG-20 (petit) ; PERF-1 si l'on y met la structure d'écritures propres | indépendant |
| `vaults.rs`, `session.rs`, `lib.rs` | SEC-7 (deux dossiers, purge), SEC-3 (cache du registre) | avec 3a |
| `ui/src/app.ts` : `save`, `reload`, `load`, `closeDoc`, `removePane`, `beforeunload`, `onFileChanged` | BUG-1, BUG-2, BUG-7, BUG-12, BUG-14, BUG-18, BUG-19, BUG-21 | **Un agent** |
| `app.ts` : `kindOf`, `openTarget`, `resolveImage`, `canvasHost`, `openFiles` | SEC-3 (UI), SEC-4, SEC-9, BUG-21 | même agent |
| `app.ts` : `openVault`, `closeVault`, `setTree`, `wireEvents`, `epochN` ; `main.ts` | PERF-5, PERF-6, PERF-8, PERF-9, BUG-5(b), BUG-15, BUG-16 | même agent |
| `ui/src/sidebar.ts` | PERF-6 (UI), PERF-7 (UI), PERF-10, BUG-15 (`onTagsReady`), CSP (`style=` l. 338/344/503) | un agent, parallèle à `app.ts` une fois le contrat figé |
| `ui/src/linkspanel.ts` | PERF-7 (UI) | avec `sidebar.ts` |
| `ui/src/canvas.ts` | SEC-1, SEC-6 (cartes), SEC-9, BUG-19 (`dispose`) | un agent (parallèle à `app.ts` sauf le point BUG-19) |
| `ui/src/editor/syntax.ts`, `livepreview.ts` | SEC-6, CSP (attributs `style`), BUG-5(b) (`ImageWidget.eq`) | un agent |
| `ui/src/resolve.ts`, `paths.ts` | SEC-4 (UNC), SEC-9 | un agent (petit) |
| `ui/src/backend.ts`, `types.ts`, `mock/*` | tout changement de contrat (partie 5) | **à faire d'un bloc après le contrat** |
| `src-tauri/tauri.conf.json`, `capabilities/default.json` | SEC-2, SEC-5, SEC-14 | un agent, en dernier |
| `Cargo.toml` (racine) et `src-tauri/Cargo.toml` : profils | BUG-3, SEC-12, PERF-14 | un agent |
| `out/release/install.ps1`, `out/release/README.md` | SEC-7, SEC-11, SEC-18 | indépendant |
| `CONTRACT.md` | voir partie 5 | **une seule édition** |
| `ui/tools/app/*.json`, `ui/tools/scenarios/*.json` | tout correctif visible (scénarios à adapter) | par lot |

---

## 4. Risques de régression par correctif important

Les tests de la colonne « Couvert par » ont été relus. Rappel : `src-tauri` n'a **aucun** test automatique sous Linux ; ses seules protections sont les scénarios Windows (`ui/tools/app/*.json`, `drive.py`) et les mesures.

| Correctif | Ce qui peut casser | Couvert par | À ajouter avant de corriger |
|---|---|---|---|
| **SEC-1** (canvas typé, SVG par DOM) | Rendu des arêtes (courbes, flèches), position des nœuds ; coordonnées légitimes en chaîne numérique (`"x": "120"`) devenant 0 | scénario `ui/tools/app/it-canvas.json`, `scenarios/…` (visuel) ; **aucun test unitaire** | Exporter une fonction pure `normalizeCanvas(json)` et la tester sous Node (x,y,width,height non numériques, `fromSide` hostile) ; décider du traitement des chaînes numériques (`Number()` strict) ; scénario CDP avec le canvas piégé du rapport |
| **SEC-2** (CSP) | Voir 2.4 : arbre, Mermaid, éditeur, tableaux | scénarios Windows seulement | Tous les scénarios après changement ; vérifier avec la console qu'aucune violation CSP n'est journalisée (ajouter `securitypolicyviolation` → `log_timing`) |
| **SEC-3** (autorisation des chemins) | Restauration de session, liens entre fichiers hors vault, `.txt`, glisser-déposer, scénarios `T.open` ; `\\?\` sur le chemin résolu | `files.rs` tests (création de fichier, `.obsidian`/`.canvas`) ; **rien côté coquille** | Déplacer la décision dans une fonction pure du cœur (`authorize(path, registry, allowed_set)`) avec tests : vault, hors vault explicite, session restaurée, UNC d'entrée refusé, `\\?\` résolu accepté |
| **SEC-4** (UNC) | Fichier `.md` ouvert depuis un partage réseau (argument) ; image `//` et `\\` | `logic.test.ts:71-88` (images, `D:\pics\a.png`) | Cas UNC dans `resolveImage`/`isAbsolute`, `openTarget` ; test Rust sur `authorize` (UNC UI refusé, UNC argument accepté) |
| **SEC-5** (scope `asset`) | Images hors vault (relatives et absolues), images dans un vault à chemin spécial, démarrage (`stat` sur Drive) | scénarios `it-preview.json`, `it-x20.json` (images) | Scénario : note hors vault avec image relative ; vault avec `[` dans le nom ; mesure `allow_directory` sur `G:` (Alec) |
| **SEC-6 / BUG-11 / SEC-8 / BUG-10** | Liens non reconnus (nouvelle grammaire) ; liens plus longs que la borne ; positions `line/col` ; tags « dans un lien » | `logic.test.ts:176-184`, `tags/tests.rs`, `tags/link_tests.rs`, `tests/contract.rs`, `highlight.rs` | Tests de durée et de non-régression de 2.10 |
| **SEC-7** (dossier local) | Reconstruction complète au premier lancement (57 s d'arbre sur Drive à froid) ; scripts de mesure ; `remove_vault` | `persist_tests.rs`, `tree.rs` tests (prennent `data_dir`) ; `is-close.json` liste le dossier | Test de migration (anciens dossiers déplacés) ; `ROCKSY_DATA_DIR` fixe les deux dossiers |
| **BUG-1** (fermeture) | Fenêtre impossible à fermer si l'UI est gelée ; perte du fichier du second lancement ; blocage sur `confirm` en test | `is-close.json` (fermeture pendant indexation) | Scénario : document modifié + fermeture ; scénario : UI occupée, garde-fou de 5 s |
| **BUG-2** (rechargement) | Faux conflits « externe » ; rechargement manqué si l'utilisateur tape | **aucun** | Scénario CDP : taper pendant un `readFile` retardé (mock avec délai) |
| **BUG-3** (unwind) | Listing partiel (2.7) ; commande qui ne répond jamais ; taille +12 % [M] | tests du cœur (en `unwind` déjà) ; `persist_tests` `damaged_*` | Porter `damaged_search_index_on_reopen` dans le dépôt ; test « panique dans un thread de listing → `Err` » |
| **BUG-4** | `before_rename` appelé plusieurs fois (coût d'un `stat`), faux `conflict` | `files.rs` : `save_detects_conflict_and_keeps_disk_content`, `failed_save_removes_its_temporary_file` | Test de `rename_with_retry` avec un échec injecté (fermeture) qui vérifie que le contrôle de mtime est refait à chaque essai |
| **BUG-5 / PERF-1** | Changement externe raté (2.2) ; nouveau fichier créé par `save_file` non listé ; arbre périmé | `tests/watch.rs:215 atomic_save_reports_only_the_saved_note` ; scénario `1337_watcher` (« enregistrement → arbre mis à jour ») | Tests de la structure d'écritures propres (lot mixte, casse, expiration, nouveau fichier, suppression) dans le cœur ; scénario : enregistrement puis écriture externe de même taille |
| **PERF-3** | La recherche ne voit plus le nouveau texte immédiatement ; manifeste/writer (2.1) ; `search_docs` après enregistrement | `vault_index.rs:512-519` et `:282`, `persist_tests.rs:403-435` (à modifier) | Test : mise à jour sans commit, `flush`, puis recherche ; test : tuer entre les deux, `reconcile` relit la note ; mesure `save_file` sur 318 Ko (61-68 ms actuels) |
| **PERF-4** | Relectures au lancement suivant (bornées) ; texte du contrat | `persist_tests.rs` (`is_dirty`, `save`) | Test : kill simulé après N enregistrements, `reconcile.reread == N` |
| **PERF-5** | Note active non suivie (`watched`) ; événements d'index perdus ; contournement des contrôles SEC-3 ; repli `read_file` | `is-auto.json` + `measure-auto.ps1` (mesure `editor-ready`) | Scénarios : lancement avec argument, avec session, avec fichier absent, avec fichier non UTF-8 (l'erreur doit rester lisible) |
| **PERF-6a** | Aucune fonctionnelle ; forme JSON | `contract.rs` (arbre), `scale.json` | Mettre à jour mock et type |
| **PERF-7** | Panneau de liens périmé si la comparaison échoue | `link_tests.rs`, `vault_index.rs` (liens après enregistrement l. 282-290) | Test : enregistrement sans changement de liens → graphe identique (même `Arc`) ; avec changement → reconstruit |
| **BUG-6** | Perte de la dernière persistance si le `join` expire ; lecture annulable → listing partiel traité comme complet | `index.rs` tests d'annulation (`tags/tests.rs` ~l. 783) | `list_tree` annulé doit renvoyer `Err`/`None`, jamais un arbre partiel |
| **BUG-12** (fins de ligne majoritaires) | Un fichier mixte reste modifié à l'enregistrement : le contrat « fins de ligne préservées » n'est pas tenu | **aucun** | Voir 6.3 ; test UI sur fichier mixte |
| **BUG-14** | Faux « supprimé » lors d'une indisponibilité passagère de Drive | **aucun** | Exiger deux échecs de `stat` consécutifs avant l'événement |
| **BUG-17** (nettoyage des temporaires) | Suppression dans le vault (règle dure du contrat) ; suppression d'un temporaire d'un autre processus Rocksy | `tests/no_unwanted_writes.rs` (vault inchangé octet par octet) | Exemption explicite (motif `.*.<pid>-*.rocksy-tmp`, pid différent, âge > 1 h, dossiers déjà listés) et test dédié |
| **PERF-10** | Règle de tri divergente entre cœur et JS | `logic.test.ts` « tag tree » | Test de parité sur les mêmes entrées |

Mesures à refaire après les lots 2 à 4 (Windows, par Alec) : `measure-auto.ps1` (x20 puis vrai vault) pour `editor-ready`, `tree-ready` ; scénario de frappe (p95 ≤ 5 ms, 318 Ko) ; `it-save.json`/`is-w1.json` (`save_file` 61-68 ms avant, doit baisser avec PERF-3) ; scénario watcher (~430 ms) ; recherche (~100 ms).

---

## 5. Changements de contrat (`CONTRACT.md`), à faire en une seule fois

Modifier le contrat d'abord, puis les trois côtés (règle du fichier). Regroupement :

**5.1 Types et charges utiles**
- `StartupInfo.firstFile?: FileContent | null` (PERF-5), consommé une seule fois ; l'UI garde `read_file` en repli.
- `TreeEntry` : `size` et `mtimeMs` retirés de la forme IPC (PERF-6a). Ne pas changer `tree-updated` en `{root, version}` (6b rejeté).
- `tags-ready`, `index-ready` et `tags-progress` : ajouter `root` (BUG-15).
- `IndexReady` : un indicateur d'échec du moteur de recherche (BUG-3/BUG-22), par exemple `searchError?: boolean`, et la valeur de `searchReady` après désactivation.
- `file-changed` : sens de `mtimeMs: 0` = fichier disparu, ou nouvel événement `file-removed` (BUG-14).
- `FileContent` : indicateur de lecture seule ou plafond (BUG-21 : erreur `invalid` au-delà de 50 Mo, lecture seule au-delà de 5 Mo).
- `ApiError` : aucune nouvelle `kind` ; les refus de SEC-3/SEC-4 sont `invalid`.

**5.2 Commandes et événements**
- Nouvelle commande `close_vault` (BUG-16).
- Nouvelle commande `quit` et nouvel événement `close-requested` (BUG-1).
- Éventuelle commande `index_state` si PERF-5 anticipe l'index (non recommandé, 2.3).

**5.3 Règles de comportement**
- `read_file`/`save_file`/`set_session`/`open_vault` : chemins acceptés (SEC-3), refus UNC venant de l'UI (SEC-4), règle d'extension d'écriture (`.md`, `.markdown`, `.txt` à décider), refus de créer un fichier hors vault.
- Scope du protocole `asset` (SEC-5) : vaults ouverts et dossiers des fichiers hors vault.
- `search` et `tag_fragments` : `limit` plafonné à 1000 (SEC-15).
- Persistance de l'index : « quelques secondes après un enregistrement » devient « au plus toutes les 60 s, après une réconciliation, et à la fermeture » (PERF-4) ; recherche à jour « sous 2 s » après un enregistrement (PERF-3).
- Suivi des changements externes : l'enregistrement de l'app ne relance ni listing ni `tree-updated` (PERF-1) ; un lot qui contient un dossier ou un chemin sans extension déclenche aussi la réconciliation des tags (BUG-8) ; pendant une indexation initiale seul l'arbre est rafraîchi (BUG-13) ; `file-changed` à la suppression (BUG-14).
- Fermeture : séquence `close-requested` → enregistrement → `quit` ; délai maximal de 2 s pour l'index (BUG-6).
- `open_external` : `mailto:` sans paramètre `attach` (SEC-18a).
- Fins de ligne : la phrase « préservées » devient « préservées si le fichier est homogène, majoritaire sinon, avec avertissement » (BUG-12), à décider (6.3).

**5.4 Règles dures à amender**
- « Données propres à Rocksy… `data_dir` » : deux dossiers (roaming pour `vaults.json` et `session.json`, local pour index, instantanés, recherche) (SEC-7), `ROCKSY_DATA_DIR` fixant les deux.
- « La seule écriture dans un vault est `save_file` sur un `.md` » : ajouter l'exception « suppression de temporaires `.rocksy-tmp` orphelins » (BUG-17/SEC-18b), ou renoncer à ce nettoyage dans le vault (le laisser à un outil ou à Alec) ; dans les deux cas amender `tests/no_unwanted_writes.rs` en conséquence.

---

## 6. Ordre d'application recommandé

### 6.1 Lots

**Lot 0 : fondations (aucun changement de comportement), 1 jour**
- Porter dans le dépôt les preuves des audits sous forme de tests qui échouent ou sont ignorés : ReDoS de `syntax.ts` (SEC-6), `parse.rs` (BUG-11/SEC-8), `highlight.rs` (BUG-10), index tantivy endommagé (BUG-3), canvas hostile (fonction pure `normalizeCanvas`).
- Décisions à prendre par Alec : `.txt` enregistrable ou lecture seule ; destin du nettoyage de temporaires dans le vault ; fins de ligne majoritaires ou lecture seule des fichiers mixtes ; recherche « sous 2 s » acceptée ; option A ou B pour la CSP.
- Descendre dans `rocksy-core` la logique aujourd'hui dans `main.rs` : autorisation des chemins (SEC-3/SEC-4), écritures propres (PERF-1). Fonctions pures testables.
- Parallèle à l'intérieur : oui (tests et refactorings dans des fichiers disjoints).

**Lot 1 : correctifs indépendants, en parallèle (fichiers disjoints)**
- 1a. `canvas.ts`, `syntax.ts`, `resolve.ts`, `paths.ts` : SEC-1, SEC-6, SEC-4 (partie UI, parties de SEC-9 hors `app.ts`).
- 1b. `tags/parse.rs` : SEC-8 + BUG-11 ; `search/highlight.rs` : BUG-10 (deux agents possibles).
- 1c. `files.rs`, `atomic.rs` : BUG-4, BUG-7 (Rust), BUG-21, SEC-10, SEC-3 (règle d'extension), BUG-17 (selon la décision du lot 0).
- 1d. `install.ps1`, README : SEC-11, SEC-16, SEC-17 ; `watch.rs` : BUG-20.
- Doit attendre le lot 0 pour les tests de preuve.

**Lot 2 : moteur d'index, un agent**
- BUG-3 (+ `unwrap_or_default` de `tree.rs`, profil `unwind`, `catch_unwind`), BUG-22, BUG-23, PERF-3, PERF-4, PERF-7, SEC-12 (clos par BUG-3), BUG-13 (`try_lock`), BUG-6 (partie cœur : `list_tree` annulable, `flush`), SEC-15 (plafond du décodage de l'instantané). PERF-11 reporté.
- Règle imposée : tout remplacement ou libération du writer committe d'abord ou recharge le manifeste (2.1).
- Doit attendre le lot 1c ? Non. Doit précéder le lot 3 (le commit de sortie et l'annulation sont utilisés par `flush_tags`).

**Lot 3 : contrat puis coque `main.rs`, séquentiel**
- 3-0. Écrire `CONTRACT.md` en entier (partie 5), mettre à jour `types.ts`, `backend.ts`, `mock/*` (un seul agent, petit).
- 3a. Données et autorisation : SEC-7 (deux dossiers, migration, purge), SEC-3, SEC-4 (Rust), SEC-5, SEC-14, SEC-18a.
- 3b. Session de vault : PERF-2, PERF-5 (lecture de la note seulement), BUG-9, BUG-16, BUG-6 (partie coque), PERF-8 supprimé.
- 3c. Worker et poller : PERF-1/BUG-5, BUG-8, PERF-6a, BUG-15 (payloads), BUG-14 (Rust).
- 3d. Fermeture : BUG-1 (Rust), `flush_tags` (commit du moteur, 2 s au plus).
- Pas de parallélisme à l'intérieur (même fichier, mêmes structures `AppState`) ; peut tourner en parallèle de la partie 1a/1b déjà terminées et du lot 4 une fois 3-0 fait.

**Lot 4 : interface, parallélisable par fichier une fois 3-0 terminé**
- 4a. `app.ts` + `main.ts` (un seul agent) : BUG-18 → BUG-1 (UI) → BUG-2, BUG-7, BUG-12, BUG-14 (UI), BUG-19, BUG-21, SEC-3 (UI), SEC-9, PERF-5 (UI), BUG-5(b), BUG-15, BUG-16, PERF-9.
- 4b. `sidebar.ts` + `linkspanel.ts` : PERF-7 (UI), PERF-10 (après BUG-15), PERF-6 (UI).
- 4c. `livepreview.ts` : BUG-5(b) (`ImageWidget.eq`) si séparé d'`app.ts`.
- Attendre : 3-0 (contrat, types, mock) ; PERF-10 attend BUG-15.

**Lot 5 : durcissement et validation**
- SEC-2 (CSP, après stabilisation de l'UI), SEC-14 (si pas fait en 3a), PERF-14 pour le build publié.
- Exécution complète des scénarios Windows et des mesures du rapport de performance (« reste à mesurer »), par Alec pour tout ce qui touche `G:`.

### 6.2 Dépendances strictes

BUG-18 avant BUG-1 ; BUG-15 avant PERF-10 ; lot 2 avant 3d ; contrat avant tout le lot 4 ; migration SEC-7 avant PERF-5 ; BUG-7 et BUG-4 dans le même changement ; SEC-2 en dernier.

### 6.3 Constats faux, exagérés, ou au correctif mauvais

- **SEC-2 : la CSP proposée est incomplète** (2.4). Correctif à reprendre.
- **SEC-3 : l'allowlist (b) est trop étroite** pour la navigation hors vault et la session ; la règle de création de fichier ne peut pas vivre dans le cœur ; le refus de `\\?\` ne doit pas s'appliquer au chemin canonicalisé (2.3).
- **SEC-12 : doublon de BUG-3, gravité sous-évaluée** (la panique est reproduite, 3 cas sur 66, et le processus meurt à chaque ouverture du vault).
- **SEC-7 : gravité « Moyenne » surévaluée** pour un poste personnel (l'impact itinérant est une hypothèse, le rapport le reconnaît). Le correctif est utile (purge, dossier local) mais exige une migration ; pas bloquant pour une diffusion.
- **SEC-10, SEC-11(a)** : hypothèses non testées ; faible priorité, correctifs peu coûteux.
- **BUG-11 : « sauter directement après un `[` non fermé » est faux sémantiquement** si l'on saute jusqu'à la fin de ligne ou jusqu'à la fin de l'ouvrant : `[ voir [a](b.md)` contient un vrai lien après un `[` jamais fermé (le premier `[` ne se ferme pas parce que son compteur de profondeur reste à 1, le second se ferme). Seules les bornes (une constante partagée) ou une mise en correspondance par pile en un seul passage sont sûres.
- **BUG-12 : le correctif n'honore pas le contrat.** Choisir la fin de ligne majoritaire réécrit quand même les lignes minoritaires d'un fichier mixte ; la préservation exacte demande de ne pas normaliser (ou de refuser l'édition). Le texte du contrat doit refléter ce qui sera réellement fait.
- **PERF-6(b) et (c)** : coût et risque (course, scénario `scale.json`, mock) disproportionnés une fois PERF-1/BUG-5 appliqués ; ne retenir que (a).
- **PERF-8** : sans objet une fois PERF-5 en place (et PERF-5 lui-même doit se limiter à la lecture de la note, 2.3).
- **PERF-11** : gain de 25 à 60 Mo sur un processus dont WebView2 pèse ~400 Mo ; risque réel avec PERF-3/BUG-22 (opérations non committées perdues, manifeste faux). À reporter.
- **PERF-12** : non mesuré, touche du code de sécurité, régression Windows possible sur l'erreur « dossier » (2.3). Ne pas faire.
- **PERF-14** : correct (−11,8 %), mais annule presque le surcoût de BUG-3 (+12,4 % mesuré sous Linux) ; le gain de démarrage n'est pas mesurable. Optionnel, pour le build publié.
- **PERF-15** : contradictoire avec BUG-14 (le poller est ce qui détecte une suppression) et non mesuré ; ne pas faire tant que le watcher de Drive n'est pas validé.
- **PERF-4 « sous verrou de lecture bloque `save_file` »** : exact mais l'encodage dure ~19 ms ; sans enjeu.
- **BUG-5 « deux listings toutes les 2-3 s »** : exact, mais sur le code actuel le worker limite déjà à une passe par seconde (`main.rs:389`) ; l'ordre de grandeur est correct.
- **SEC-6** : la gravité « Moyenne » est justifiée (30 s de gel mesuré), mais le gel gêne aussi BUG-1 (fermeture impossible), à lier.
- Les « points sains » des trois audits que j'ai recoupés dans le code se confirment : ordre des verrous, `touched.keys`, écriture atomique, règles `.obsidian`/`.canvas`.

---

## Annexe : ce qui a été vérifié

Lu en entier : `main.rs`, `files.rs`, `atomic.rs`, `tags/index.rs` (réconciliation, persistance, mises à jour, graphe), `search/engine.rs` (ouverture, manifeste, upsert, commit), `tree.rs` (listing, instantané, `reconcile_tree`), `vaults.rs`, `session.rs`, `lib.rs`, `watch.rs` (début), `ui/src/main.ts`, `backend.ts`, extraits d'`app.ts` (sauvegarde, rechargement, fermeture, ouverture de vault, événements), `resolve.ts`, `paths.ts`, `index.html`, `dist/index.html`, configuration Tauri et profils Cargo, tests `ui/tests/*.test.ts` et noms de tests du cœur.

Sources de dépendances consultées : `tauri-2.12.7/src/event/mod.rs`, `manager/mod.rs`, `scope/fs.rs`, `tauri-utils-2.10.4/src/html.rs`, `tauri-codegen-2*/src/context.rs`, `style-mod`, `@codemirror/view`, Pele 1.13.0 (rendu d'un graphe d'essai).

Mesures faites ici : tailles de `rocksy-cli` en `abort` et en `unwind` (5 928 624 contre 6 663 480 octets) ; présence d'attributs `style` dans la sortie de Pele (3 pour un graphe de 2 nœuds). Compilations dans le dossier temporaire de session, hors dépôt ; `git status` inchangé par rapport au départ (`out/suivi/etat.js` modifié et `out/audits/` non suivi existaient déjà).

Non vérifié : tout comportement Windows, WebView2 ou Google Drive (CSP effective en `rocksy.exe`, `ExecuteScript` hors CSP, `canonicalize` sur `G:`, noms 8.3), `rocksy.exe` jamais lancé, aucun scénario `drive.py` exécuté.
