# Watcher de dossier — tests dans l'app réelle

- App : `rocksy.exe` 0.1.2 (WebView2 + cœur Rust), Windows 11, pilotée par le port de débogage, profil isolé.
- Données : copie du vault synthétique 1x sur `C:` (NVMe, **pas Google Drive**). Changements faits depuis PowerShell, hors de l'app.
- Délai = temps entre le début de l'opération et la réception de l'événement dans la page.

| Opération externe | Arbre mis à jour | Tags mis à jour | Délai |
|---|---|---|---|
| Création d'une note avec un tag | oui (1 052 entrées) | 1 note relue, tag trouvé | 432 ms |
| Modification de la note (ajout d'un tag) | oui | 1 relue, tag trouvé | 430 ms |
| Renommage | oui | 1 relue, 1 retirée | 430 ms |
| Suppression | oui (1 051 entrées) | 1 retirée, tags disparus | 424 ms |
| Écriture dans `.obsidian/` et dans un dossier à point | aucun événement | aucun | sans objet |
| Enregistrement depuis l'app | oui | aucune relecture (déjà à jour) | 659 ms après l'appel |
| Rafale : 192 notes réécrites en 228 ms | 1 seule mise à jour | 1 seule passe, 192 relues | 654 ms |
| 10 s sans activité | aucun événement | aucun | 0 ms de CPU pour le processus de l'app |

Le délai d'environ 430 ms est le regroupement (400 ms de silence) plus la vérification.

Le test a tourné pendant qu'une autre fenêtre Rocksy (celle d'Alec) était ouverte, sans interférence : un profil isolé est une instance séparée.

## Mesures du cœur seul (rocksy-cli, Windows, 20x)

Mise en place de l'écoute sur 20 747 entrées : moins de 1 ms. Rafale de 300 réécritures : 1 lot. 30 s au repos : 0 ms de CPU, 4,4 Mo.

## Non vérifié

- Google Drive : on ne sait pas si le lecteur virtuel `G:` émet des notifications. Filet prévu : revérification au retour sur la fenêtre (non testé automatiquement).
- Rafale plus longue que 3 s dans l'app (testée dans le cœur seulement), activité massive dans un dossier à point, lecteur démonté.
