# out/ — livrables de Rocksy

Point d'entrée : [`PLAN.md`](PLAN.md) (plan de reprise, étapes ordonnées).

```
out/
├── PLAN.md                  plan de reprise
├── docs/                    documents de cadrage (français)
│   ├── perimetre-fonctionnel.md
│   ├── m1-plugins-matrix.md
│   └── m2-cahier-des-charges-spikes.md
├── tools/                   scripts réutilisables
│   ├── gen-vault.py         vault synthétique (python3 out/tools/gen-vault.py --out <dir> --scale N)
│   ├── measure-coldstart.ps1  chrono de démarrage (Windows)
│   ├── vault-kpis.ps1       KPI du vault réel (Windows)
│   └── scan-plugin-api.ps1  scan de l'API des plugins (Windows)
├── data/                    mesures brutes du vault réel (données personnelles : chemins, tailles)
│   ├── vault-kpis.json
│   └── plugin-api-usage.json
├── runs/                    mesures de la baseline Obsidian (voir « Runs »)
└── spikes/                  spikes M2
    ├── README.md            conventions des spikes
    ├── m2-resultats.md      synthèse comparative (à créer à la fin des mesures)
    ├── a-tauri-cm6/         spike A : Tauri 2 + CodeMirror 6
    └── b-rune/              spike B : Rune (natif)
```

## Runs (mesures datées)

Chaque session de mesure écrit dans son propre dossier, jamais dans un fichier partagé :

```
<racine>/runs/AAAA-MM-JJ_HHMM_<cas>/
├── coldstart.csv     une ligne par run (T_window, T1, T2, total Obsidian, remarque)
├── meta.json         date, cas, label, exécutable, machine, OS
└── notes.md          (à la main) état du cache Drive, version, observations, captures
```

- Baseline Obsidian : `out/runs/`.
- Un spike : `out/spikes/<spike>/runs/`. On passe `-RunsRoot` au script.
- Un dossier de run n'est jamais modifié après coup. Une nouvelle mesure = un nouveau dossier.
- Les captures d'écran et vidéos vont dans le dossier du run (`captures/`).
- Indiquer dans `notes.md` l'état du cache Drive (froid après redémarrage, ou chaud).
