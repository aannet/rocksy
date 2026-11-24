# Persistance de l'index des tags — tests et mesures dans l'app réelle

- App : `rocksy.exe` 0.1.1 (WebView2 + cœur Rust), Windows 11, pilotée par le port de débogage.
- Données : copie du vault synthétique 1x et vault 20x de référence, sur `C:` (NVMe, **pas Google Drive**), cache chaud. Profils isolés par `ROCKSY_DATA_DIR`.
- Temps relevés dans la page : délai entre l'appel `build_tags` et la réception de `tags-ready`.

## Scénario sur la copie 1x (488 notes)

| Étape | Résultat |
|---|---|
| 1. Première ouverture | 488 notes lues, index prêt en 334 ms, fichier d'index créé (311 Ko) |
| 2. Relance sans changement | tags disponibles en 11 ms depuis l'index enregistré ; vérification finie à 29 ms, 0 note relue, fichier non réécrit |
| 3. Une note modifiée hors de l'app (ajout d'un tag) | tags en 9 ms ; 1 note relue ; le nouveau tag est trouvé ; fichier réécrit |
| 4. Enregistrement d'une note depuis l'app, puis processus tué après 7 s (sans fermeture propre) | tag visible tout de suite dans l'app ; fichier d'index réécrit dans les 7 s |
| 5. Relance | tags en 9 ms, tag de l'étape 4 présent, 0 note relue |

## Vault 20x (9 760 notes, 46 208 occurrences, 4 199 tags)

| Mesure | Valeur |
|---|---|
| Première indexation (cache chaud) | 336 ms, fichier d'index de 4,38 Mo |
| Relance : tags disponibles (3 relances) | 34, 38 et 36 ms |
| Relance : vérification terminée, 0 note relue | 93, 102 et 95 ms |
| Démarrage de l'app (5 lancements, médiane) | éditeur prêt 652 ms, arbre prêt 917 ms : pas de régression |

Interface : après relance, le panneau affiche « 4 199 tags · 46 208 occurrences · 9 760 notes — à jour, aucune note modifiée (68 ms) ».

## Mesures du cœur seul (rocksy-cli, Windows, 20x)

Chargement de l'index 25 à 38 ms (médiane), max 52 ms ; vérification sans changement 42 à 59 ms ; après 50 notes modifiées 107 à 204 ms (50 relues, 9 710 inchangées).

## Non vérifié

- Google Drive et le vrai vault : la première indexation y lira toutes les notes (lent à froid) ; ensuite seule la liste des fichiers est parcourue. L'égalité entre la date vue au listing et celle relevée à l'enregistrement n'est pas vérifiée sur Drive ; au pire une note enregistrée est relue une fois.
- Fermeture de l'app pendant une première indexation longue : le code annule, attend le fil d'indexation et enregistre ce qui a été lu ; ce cas n'a pas été provoqué en test (l'indexation des vaults synthétiques est trop rapide).
