# Mesures du cœur sous Windows (rocksy-cli, sans interface)

- Binaire : `rocksy-cli.exe` compilé en release depuis WSL (cible `x86_64-pc-windows-gnullvm`), lancé sous Windows 11.
- Données : vaults synthétiques v2 sur `C:` (NVMe local, **pas Google Drive**) : `x1` (1 049 entrées, 488 notes) et `x20` (20 747 entrées, 9 760 notes).
- Mesures automatiques relevées par l'outil lui-même (médianes). Ce ne sont pas des chronos T1/T2.

| Mesure | 1x | 20x |
|---|---|---|
| Listing complet de l'arbre | 4,3 ms | 61 ms |
| Écriture du snapshot | 3,7 ms | 10,6 ms |
| Relecture du snapshot | 0,3 ms | 6,4 ms |
| `open_tree` depuis le snapshot | 0,4 ms | 4,6 ms |
| Réconciliation sans changement | 4,8 ms | 72 ms |
| Sérialisation JSON de l'arbre | 0,2 ms (117 Ko) | 7,3 ms (2,3 Mo) |
| Index des tags, première lecture après extraction des fichiers | non relevé | **13 060 ms** |
| Index des tags, relances suivantes (cache chaud) | 488 ms (1 relevé) | 337 et 319 ms |
| Index des tags, 2 threads, cache chaud | | 1 001 ms |
| `tags_list("")` | | 1,1 ms (max 2,0) |
| `tag_fragments` du tag le plus fréquent, limite 200 | | 0,08 ms (max 0,24) |
| `tag_fragments` sans limite (6 521 fragments, 2,6 Mo de JSON) | | 8,6 ms avec JSON (max 14,2) |

## Lecture

- L'ouverture de l'arbre depuis le snapshot ne dépend presque pas de la taille (4,6 ms à 20x) : conforme à l'exigence d'échelle.
- La première construction de l'index des tags a pris 13 s : les 9 760 fichiers venaient d'être extraits et n'avaient jamais été lus (cause probable : antivirus et cache disque froid ; non vérifié). C'est l'ordre de grandeur à attendre à froid, et Google Drive sera plus lent encore (~155 ms par fichier mesurés dans l'audit). L'index des tags tourne en arrière-plan et ne bloque ni T1 ni T2, mais sa persistance (hors 0.1) sera nécessaire sur le vrai vault.
- Recherche de tags : très en dessous des cibles (50 ms et 100 ms).
