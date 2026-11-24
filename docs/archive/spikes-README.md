# Spikes M2

Cahier des charges : [`../docs/m2-cahier-des-charges-spikes.md`](../docs/m2-cahier-des-charges-spikes.md).
Plan : [`../PLAN.md`](../PLAN.md) étapes 3 à 6.

## Conventions

- Un dossier par spike, avec le même squelette :
  ```
  <spike>/
  ├── README.md     comment construire et lancer (Windows), état d'avancement
  ├── src/          code du spike
  ├── grille.md     grille des cas F1 à F9 (✅ / ⚠️ / ❌ + preuve)
  └── runs/         mesures datées AAAA-MM-JJ_HHMM_<cas>/ (voir ../README.md)
  ```
- Un spike est jetable : il répond à une question, il ne devient pas le produit tel quel.
- Les artefacts de build (`target/`, `node_modules/`, `dist/`, binaires) ne sont pas versionnés.
- Aucun contenu de note ni valeur de `data.json` du vault réel dans le dépôt.
- La synthèse comparative va dans `m2-resultats.md` (ici), avec les médianes et la grille de chaque spike.
