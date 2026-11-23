# Rocksy 0.1.1 — installation et prise en main

Rocksy 0.1.1 est un éditeur Markdown rapide pour Windows 11, qui ouvre n'importe quel `.md` (avec ou sans vault) et lit un vault Obsidian sans le convertir. Ce n'est pas encore le remplaçant d'Obsidian : voir « Limites ».

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

Le script copie l'app dans `%LOCALAPPDATA%\Programs\Rocksy`, crée un raccourci dans le menu Démarrer et déclare Rocksy dans « Ouvrir avec » pour `.md` et `.markdown`. Avec `-AddToPath`, `rocksy fichier.md` fonctionne dans un nouveau terminal.

**Double-clic sur un `.md`** : Windows 11 ne laisse pas un script changer l'application par défaut. Une fois : clic droit sur un `.md` > Ouvrir avec > Choisir une autre application > Rocksy > Toujours.

Désinstaller : `powershell -ExecutionPolicy Bypass -File .\install.ps1 -Uninstall`.

## Utiliser

| Action | Comment |
|---|---|
| Ouvrir un fichier | Argument de ligne de commande, « Ouvrir avec », glisser-déposer, ou Ctrl+P > ouvrir un fichier |
| Second fichier | Un second lancement ouvre un onglet dans la fenêtre existante (instance unique) |
| Ajouter un vault | Ctrl+P > « Ajouter un vault », choisir le dossier |
| Aller à une note | Ctrl+O |
| Commandes | Ctrl+P |
| Enregistrer | Ctrl+S, et enregistrement automatique 2 s après la dernière frappe (désactivable dans la barre d'état) |
| Tags | Panneau des tags : filtrer, cliquer un tag pour voir ses fragments, cliquer un fragment pour ouvrir la note à l'endroit exact |
| Canvas | Lecture seule |

## Nouveautés de la 0.1.1

- Index des tags persisté : après une première indexation, les tags sont disponibles dès l'ouverture du vault. Le panneau des tags indique « index enregistré, vérification en cours… » puis « à jour ».
- Pour passer de la 0.1.0 à la 0.1.1 : remplacer `rocksy.exe` (ou relancer `install.ps1`). Les données de `%APPDATA%\dev.rocksy.app` sont conservées.

## Ce que Rocksy écrit, et où

- Dans un vault ou à côté d'un fichier : **uniquement** le `.md` que tu enregistres (écriture atomique). Jamais dans `.obsidian/`, jamais dans un `.canvas`.
- Ses propres données : `%APPDATA%\dev.rocksy.app\` (liste des vaults, snapshots d'arbre, index des tags, dernière session, journal de timings). L'index des tags contient des extraits de tes notes : ce dossier est à traiter comme tes notes si tu le sauvegardes ou le partages. Supprimer ce dossier remet l'app à zéro sans toucher aux notes.

## Limites de la 0.1

- La **première** indexation des tags d'un vault lit toutes les notes en arrière-plan. Sur Google Drive à froid, cela peut prendre de l'ordre de la minute ; l'éditeur et l'arbre restent utilisables pendant ce temps. Les ouvertures suivantes rechargent l'index enregistré sans lire aucune note, puis ne relisent que les notes dont la taille ou la date a changé.
- Une note modifiée par un autre outil sans changement de taille ni de date n'est pas revue par l'index des tags avant sa prochaine modification.
- Pas de recherche plein texte, de backlinks, de graphe, de renommage de tags, de mise à jour des liens au renommage, ni de création, renommage ou suppression de fichiers depuis l'arbre.
- Pas de mods ni de store. Pas d'import automatique des vaults d'Obsidian (ajout manuel).
- Les modifications externes ne sont détectées que pour les fichiers ouverts (scrutation toutes les 2 s) ; l'arbre se met à jour à l'ouverture du vault.
- UTF-8 uniquement.
- Non testé à ce jour sur le vrai vault Google Drive ni à froid après redémarrage : voir `../spikes/m2-resultats.md`.
