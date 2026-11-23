# Rocksy 0.2.1 — installation et prise en main

Rocksy 0.2.1 est un éditeur Markdown rapide pour Windows 11, qui ouvre n'importe quel `.md` (avec ou sans vault) et lit un vault Obsidian sans le convertir. Ce n'est pas encore le remplaçant d'Obsidian : voir « Limites ».

## Contenu

| Fichier | Rôle |
|---|---|
| `rocksy.exe` | L'application |
| `WebView2Loader.dll` | À garder **à côté** de `rocksy.exe` (l'app ne démarre pas sans) |
| `install.ps1` | Installation pour l'utilisateur courant, sans droits administrateur |

Prérequis : Windows 11 (WebView2 est inclus). Les binaires sont produits par `out/tools/build-win.sh` et déposés dans `C:\Users\<toi>\rocksy-test\bin\` ; ils ne sont pas versionnés.

## Essayer sans installer

```powershell
C:\Users\aleca\rocksy-test\bin\rocksy.exe "C:\chemin\vers\une note.md"
```

## Installer

```powershell
cd C:\Users\aleca\rocksy-test\bin
copy \\wsl.localhost\Ubuntu\home\alec\projects\008-Rocksy\out\release\install.ps1 .
powershell -ExecutionPolicy Bypass -File .\install.ps1 -AddToPath
```

Le script copie l'app dans `%LOCALAPPDATA%\Programs\Rocksy`, crée un raccourci dans le menu Démarrer et ajoute Rocksy à « Ouvrir avec » pour `.md` et `.markdown`, sans toucher aux entrées des autres applications (Obsidian, VS Code…). Il n'écrit que dans le registre de l'utilisateur (`HKCU`), sans droits administrateur.

Avec `-AddToPath`, le dossier est ajouté au PATH utilisateur (une seule fois, même si le script est relancé ; les entrées existantes, y compris celles en `%VARIABLE%`, sont conservées telles quelles) : `rocksy fichier.md` fonctionne dans un nouveau terminal.

**Mettre à jour** : relancer `install.ps1` avec les nouveaux binaires. Si le Rocksy installé est ouvert, le script lui demande de se fermer, comme un clic sur la croix, et attend 10 s (`-CloseTimeoutSec <secondes>` pour changer ce délai). S'il est toujours ouvert, le script s'arrête sans rien modifier : enregistrer (Ctrl+S), fermer Rocksy, relancer le script. `-ForceClose` arrête le processus de force ; les modifications non enregistrées sont alors perdues. Par prudence, enregistrer avant de lancer une mise à jour. Un Rocksy lancé depuis un autre dossier (par exemple `rocksy-test\bin`) n'est pas fermé.

**Double-clic sur un `.md`** : Windows 11 ne laisse pas un script changer l'application par défaut. Une fois : clic droit sur un `.md` > Ouvrir avec > Choisir une autre application > Rocksy > Toujours.

**Désinstaller** : `powershell -ExecutionPolicy Bypass -File .\install.ps1 -Uninstall`. Le script ferme Rocksy de la même façon, puis retire le dossier d'installation, le raccourci, les clés de registre de Rocksy (sa seule entrée dans « Ouvrir avec », pas celles des autres applications) et son entrée du PATH, les autres restant inchangées. Les données de `%APPDATA%\dev.rocksy.app` (liste des vaults, session) et de `%LOCALAPPDATA%\dev.rocksy.app` (index, donc le texte des notes) sont conservées : supprimer ces deux dossiers à la main pour tout effacer.

Le script n'a toujours pas été exécuté sur une machine (0.2.1 comprise) : seules sa syntaxe et sa gestion du PATH ont été vérifiées. À la première utilisation, vérifier le résultat (« Ouvrir avec » sur un `.md`, `rocksy` dans un nouveau terminal).

## Utiliser

| Action | Comment |
|---|---|
| Ouvrir un fichier | Argument de ligne de commande, « Ouvrir avec », glisser-déposer, ou Ctrl+P > ouvrir un fichier |
| Second fichier | Un second lancement ouvre un onglet dans la fenêtre existante (instance unique) |
| Ajouter un vault | Ctrl+P > « Ajouter un vault », choisir le dossier dans le dialogue de Windows |
| Retirer un vault | Sélecteur de vault > « Retirer ce vault de la liste » : le vault disparaît de la liste et ses index sont supprimés ; aucun fichier du vault n'est touché |
| Aller à une note | Ctrl+O |
| Commandes | Ctrl+P |
| Enregistrer | Ctrl+S, et enregistrement automatique 2 s après la dernière frappe (désactivable dans la barre d'état) |
| Rechercher dans le vault | Ctrl+Maj+F ou la loupe : recherche à la frappe, casse et accents ignorés, mots combinés par ET. `"expression exacte"`, `-mot` pour exclure, `path:texte`, `tag:a/b` |
| Liens d'une note | Onglet « Liens » du panneau latéral : liens entrants (backlinks) et sortants de la note active ; Ctrl+P > « Liens non résolus » |
| Tags | Panneau des tags : filtrer, cliquer un tag pour voir ses fragments, cliquer un fragment pour ouvrir la note à l'endroit exact |
| Canvas, `.txt` et autres fichiers non Markdown | Lecture seule (cadenas sur l'onglet) |
| Fermer | La croix enregistre d'abord tout ce qui est modifié. Si un enregistrement est impossible, ou si l'enregistrement automatique est désactivé, Rocksy demande : enregistrer, quitter sans enregistrer ou annuler |

## Nouveautés de la 0.2.1

Version de correction, issue de trois audits (sécurité, robustesse, performance). Pas de nouvelle fonction.

- **Sécurité.** Un canvas ou une note piégés ne peuvent plus exécuter de code dans l'application. La page n'accède qu'aux vaults enregistrés et aux fichiers que tu as ouverts toi-même (ligne de commande, « Ouvrir avec », dialogue, glisser-déposer) ; autour d'un fichier ouvert hors vault, les notes et images voisines sont lisibles mais pas modifiables. Un dossier ne devient un vault que s'il a été choisi dans le dialogue de Windows. Aucune connexion réseau n'est possible depuis la page ; les images distantes et celles d'un partage réseau ne sont pas chargées (un message les remplace).
- **Plus de perte de texte à la fermeture.** Fermer la fenêtre juste après avoir tapé enregistre avant de quitter. Un enregistrement qui échoue (fichier verrouillé par Drive ou un antivirus) est retenté et signalé ; un fichier modifié ou supprimé par un autre outil pendant que tu l'édites n'est jamais écrasé sans te le demander.
- **Lecture seule des fichiers non Markdown.** Rocksy n'enregistre que les `.md` et `.markdown`. Un `.txt`, un canvas ou un fichier atteint par un lien depuis une note hors vault s'ouvre en lecture seule.
- **Fins de ligne.** Un fichier aux fins de ligne mélangées (LF et CRLF) garde les siennes : seules les lignes modifiées changent.
- **Données d'index dans `%LOCALAPPDATA%`.** Les index (qui contiennent le texte des notes) quittent le profil itinérant. Ceux d'une version précédente sont déplacés au premier lancement, sans réindexation.
- **Retirer un vault purge ses index** (arbre, tags, liens, recherche).
- **Index plus sûr.** Un index de recherche endommagé est reconstruit tout seul au lieu de faire planter l'application. Si des notes n'ont pas pu être lues ou indexées (fichier occupé), la barre d'état l'indique (« Index : N notes à relire… ») et Rocksy réessaie de lui-même.
- Enregistrer une grosse note est plus rapide (l'index de recherche est mis à jour en différé, sous 2 s), et un enregistrement ne relance plus de parcours complet du vault.
- L'exe passe de 11,6 à 14,1 Mo.

## Nouveautés de la 0.2.0

- Recherche plein texte dans tout le vault, à la frappe, avec extraits surlignés ; un clic ouvre la note à l'endroit trouvé.
- Liens entrants (backlinks) et sortants de la note active, y compris vers les images ; liste des liens non résolus. La résolution suit les règles d'Obsidian.
- Un seul index par vault : chaque note nouvelle ou modifiée est lue une seule fois pour les tags, les liens et la recherche. Après la première indexation, tout est disponible dès l'ouverture.
- L'exe passe de 6,7 à 11,6 Mo. L'index de recherche occupe sur disque à peu près la taille du texte du vault.

## Nouveautés de la 0.1.2

- Suivi des changements externes : quand un autre outil (Drive, nettoyeur IA, explorateur) crée, modifie, renomme ou supprime des notes pendant que Rocksy est ouvert, l'arbre et les tags se mettent à jour, environ une demi-seconde après sur un disque local. Les rafales sont regroupées et seules les notes modifiées sont relues. Les dossiers à point sont ignorés.
- Filet de sécurité : au retour sur la fenêtre, le vault est revérifié si la dernière vérification date de plus de 30 s (au cas où Google Drive ne signalerait pas tous les changements).

## Nouveautés de la 0.1.1

- Index des tags persisté : après une première indexation, les tags sont disponibles dès l'ouverture du vault. Le panneau des tags indique « index enregistré, vérification en cours… » puis « à jour ».
- Pour changer de version : remplacer `rocksy.exe` (ou relancer `install.ps1`). Les données de l'application sont conservées.

## Ce que Rocksy écrit, et où

- Dans un vault ou à côté d'un fichier : **uniquement** le `.md` ou `.markdown` que tu enregistres (écriture atomique : un fichier temporaire `.<nom>.…rocksy-tmp` dans le même dossier, renommé aussitôt). Jamais dans `.obsidian/`, jamais dans un `.canvas`, jamais dans un autre type de fichier.
- Seule suppression possible dans un vault : à son ouverture, les fichiers temporaires `…rocksy-tmp` laissés par un enregistrement interrompu de Rocksy lui-même, s'ils ont plus d'une heure.
- `%APPDATA%\dev.rocksy.app\` (suit un profil itinérant) : liste des vaults, dernière session, liste des fichiers ouverts hors vault (`opened.json`), journal de timings (`timings.jsonl`, sans contenu de note).
- `%LOCALAPPDATA%\dev.rocksy.app\` (reste sur la machine) : instantanés d'arbre, index des tags et des liens, index de recherche. L'index de recherche contient le texte de tes notes : ce dossier est à traiter comme tes notes si tu le sauvegardes ou le partages.
- Supprimer ces deux dossiers remet l'app à zéro sans toucher aux notes. Retirer un vault de la liste supprime ses index.
- Rien n'est envoyé sur le réseau.

## Limites

- La **première** indexation des tags d'un vault lit toutes les notes en arrière-plan. Sur Google Drive à froid, cela peut prendre de l'ordre de la minute ; l'éditeur et l'arbre restent utilisables pendant ce temps. Les ouvertures suivantes rechargent l'index enregistré sans lire aucune note, puis ne relisent que les notes dont la taille ou la date a changé.
- Une note modifiée par un autre outil sans changement de taille ni de date n'est pas revue par l'index des tags avant sa prochaine modification.
- Pas de graphe, de renommage de tags, de mise à jour des liens au renommage, de création de note depuis un lien non résolu, ni de création, renommage ou suppression de fichiers depuis l'arbre.
- Recherche : pas de OU, pas de racinisation (« budget » ne trouve pas « budgétaire », mais « budg » en fin de saisie le trouve). Le dernier mot saisi n'est un préfixe que s'il termine la requête.
- Pas de mods ni de store. Pas d'import automatique des vaults d'Obsidian (ajout manuel).
- Le suivi des changements externes n'a pas été vérifié sur Google Drive : s'il ne reçoit pas de notification, la mise à jour attend le retour sur la fenêtre ou la réouverture du vault. Un renommage est vu comme une suppression et un ajout.
- UTF-8 uniquement. Fichiers de plus de 50 Mo refusés.
- Revenir à un exe antérieur après avoir lancé la 0.2.1 provoque une réindexation complète : l'ancienne version cherche ses index dans `%APPDATA%`, d'où la 0.2.1 les a déplacés. Rien n'est perdu, mais la première ouverture du vault relit toutes les notes.
- Les images d'une note hors vault ne s'affichent que si elles sont dans le dossier de la note ou en dessous. Les images d'un vault posé sur un partage réseau ne sont pas affichées.
- Juste après la copie ou la synchronisation d'un grand nombre de fichiers, la première indexation peut être ralentie et l'index de recherche complété en deux temps (quelques secondes) : la barre d'état l'indique.
- `install.ps1` n'a jamais été exécuté (voir « Installer »).
- Non testé à ce jour sur le vrai vault Google Drive ni à froid après redémarrage : voir `../spikes/m2-resultats.md`. Les essais de la 0.2.1 (`../spikes/a-tauri-cm6/runs/2026-11-23_*_integration-audits/`) portent sur des vaults synthétiques, sur disque local, en profil isolé : le second lancement (instance unique), les dialogues de Windows, le glisser-déposer et le déplacement des index entre les deux dossiers réels n'y sont pas couverts.
