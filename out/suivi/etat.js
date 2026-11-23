// Etat du suivi Rocksy. Ecrit par la session principale uniquement.
window.ROCKSY_ETAT = {
  "maj": "23/11/2026 13:37",
  "resume": "Rocksy 0.1.2 est prêt : C:\\Users\\aleca\\rocksy-test\\release-0.1.2\\rocksy.exe. Nouveauté : le suivi des changements externes. Vérifié dans l'app réelle sur vault synthétique : une note créée, modifiée, renommée ou supprimée par un autre outil met à jour l'arbre et les tags environ 430 ms après ; une rafale de 192 notes donne une seule mise à jour ; les dossiers à point sont ignorés ; 0 CPU au repos. Pas vérifié sur Google Drive, où il n'est pas certain que les notifications arrivent (filet : revérification au retour sur la fenêtre). La 0.1.2 n'est pas commitée. Mes tests ne se mélangent plus à ta fenêtre : un profil de test est maintenant une instance séparée.",
  "actionsAlec": [
    {
      "texte": "1. Passer à la 0.1.2 et vérifier le suivi sur ton vrai vault.",
      "detail": "Ferme Rocksy, lance C:\\Users\\aleca\\rocksy-test\\release-0.1.2\\rocksy.exe. Test le plus utile : Rocksy ouvert sur ton vault, crée ou modifie une note depuis l'explorateur (ou laisse Drive synchroniser un changement venu d'une autre machine) et regarde si l'arbre et le panneau des tags suivent sans rien faire. Si rien ne bouge, clique ailleurs puis reviens sur la fenêtre après 30 s : c'est le filet de sécurité. Dis-moi lequel des deux a marché."
    },
    {
      "texte": "2. Dire si je commite la 0.1.2 (watcher).",
      "detail": "Non commité : tes deux demandes de commit portaient sur le code d'alors."
    },
    {
      "texte": "3. Trois points que je n'ai pas pu tester : glisser-déposer d'un .md sur la fenêtre, dialogues « Ajouter un vault » et « Ouvrir un fichier », clic sur un lien web.",
      "detail": "Ils demandent une action à l'écran. Dis-moi si l'un d'eux ne marche pas."
    },
    {
      "texte": "4. Chronos manuels sur le vrai vault (10 min).",
      "detail": "out\\tools\\measure-coldstart.ps1 -ProcessName rocksy -RunsRoot out\\spikes\\a-tauri-cm6\\runs (3 lancements, F8/F9), une fois à chaud et une fois après redémarrage de Windows (Drive à froid). Même chose sur Obsidian pour la baseline, qui n'a jamais été relevée."
    },
    {
      "texte": "5. Lancer le script KPI étendu sur le vrai vault (75 s, non bloquant).",
      "detail": "powershell -NoProfile -ExecutionPolicy Bypass -File \"\\\\wsl.localhost\\Ubuntu\\home\\alec\\projects\\008-Rocksy\\out\\tools\\vault-kpis.ps1\" -Vault \"G:\\Mon Drive\\MonVault\"  — compteurs uniquement, sortie datée dans out/data/. Si le chemin \\\\wsl.localhost est refusé, copie le script sur C: et ajoute -OutFile."
    }
  ],
  "phases": [
    {
      "id": "e0",
      "titre": "Étape 0 — Cadrage et environnement",
      "statut": "fait",
      "pct": 100,
      "poids": 1,
      "taches": [
        {
          "t": "Lecture du dépôt et du plan",
          "s": "fait"
        },
        {
          "t": "Sonde de l'environnement (WSL + Windows)",
          "s": "fait",
          "note": "Windows : pas de Rust, Node ni Build Tools. WebView2 157 présent. WSL : Node 22, pas de sudo."
        },
        {
          "t": "Rust dans WSL (espace utilisateur) + cibles Windows",
          "s": "fait",
          "note": "cargo 1.100.0"
        },
        {
          "t": "Compilation croisée d'un « hello world » Tauri vers Windows, lancé depuis WSL",
          "s": "fait",
          "note": "Exe de 5,3 Mo + WebView2Loader.dll. Fenêtre en ~0,6 s, 25 Mo de mémoire pour le processus principal. Script : out/tools/build-win.sh"
        },
        {
          "t": "Outil de test de l'app réelle sans capture du bureau (out/tools/cdp.py)",
          "s": "fait",
          "note": "Exécute du JS et capture la page via le port de débogage de WebView2."
        },
        {
          "t": "Verdict de l'étape 0 du spike Rune",
          "s": "fait",
          "note": "PASSE SOUS CONDITIONS, hors 0.1. Éditeur riche et styles mélangés : oui. Frappe sur 319 Ko : aucun commit > 16 ms sur 54 frappes (Linux, headless). Widgets dans le texte (tableau, image) : non. Windows : non vérifié. Détail : out/spikes/b-rune/README.md"
        }
      ]
    },
    {
      "id": "e1",
      "titre": "Étape 1 — Prérequis",
      "statut": "fait",
      "pct": 100,
      "poids": 1,
      "taches": [
        {
          "t": "Générateur enrichi (tags hiérarchiques, Mermaid, callouts, tableaux, embeds)",
          "s": "fait",
          "note": "Version 2. Déterminisme et stabilité par préfixe vérifiés par hash. Note pire cas de 318 Ko, 2 canvas (6 et 156 nœuds). Syntaxe Mermaid et canvas non passés dans un rendu."
        },
        {
          "t": "vault-kpis.ps1 étendu",
          "s": "fait",
          "note": "7 nouvelles sections de compteurs. Testé sur 3 notes fabriquées (attendu = obtenu). Jamais lancé sur le vrai vault."
        },
        {
          "t": "Vaults synthétiques 1x et 20x générés (hors dépôt)",
          "s": "fait",
          "note": "C:\\Users\\aleca\\rocksy-test\\vaults\\x1 (955 fichiers visibles, 488 notes) et x20 (19 062 fichiers visibles, 9 760 notes, 64 Mo de texte). Copies WSL dans ~/rocksy-vaults."
        }
      ]
    },
    {
      "id": "e2",
      "titre": "Étape 2 — Spikes A (Tauri + CM6) et B (Rune)",
      "statut": "fait",
      "pct": 100,
      "poids": 4,
      "taches": [
        {
          "t": "Contrat d'interface cœur / Tauri / UI",
          "s": "fait",
          "note": "out/spikes/a-tauri-cm6/src/CONTRACT.md"
        },
        {
          "t": "Cœur Rust rocksy-core (vault, snapshot, tags, enregistrement atomique)",
          "s": "fait",
          "note": "81 tests passent, clippy propre, test F7 (aucune écriture indésirable) inclus. 3 dépendances seulement."
        },
        {
          "t": "Frontend CodeMirror 6 + Live Preview",
          "s": "fait",
          "note": "7 paliers livrés : Live Preview (F1-F3, callouts), coque, vault et arbre virtualisé, Ctrl+O, palette, tags, canvas en lecture, deux volets, Mermaid via Pele. 39 tests unitaires. JS initial 135 Ko gzip."
        },
        {
          "t": "Assemblage et tests d'intégration dans l'app réelle",
          "s": "fait",
          "note": "Grille remplie dans l'app réelle. 5 corrections d'interface et 2 corrections backend faites en cours de route."
        },
        {
          "t": "Colle Tauri, instance unique, build Windows",
          "s": "fait",
          "note": "Testé dans l'app réelle sous Windows par script : 15 commandes, CRLF et accents préservés à l'enregistrement, conflit détecté, .obsidian et .canvas refusés, second lancement transmis en 65 ms, rien créé à côté d'un fichier hors vault. Reste : test avec la vraie interface."
        },
        {
          "t": "Spike B Rune",
          "s": "partiel",
          "note": "Étape 0 + essai Windows faits. Rune compile et tourne sous Windows : fenêtre de la sonde 319 Ko en 233 ms (médiane de 3, à chaud), premier affichage ~360 ms d'après son journal, frappe scriptée sans commit > 4 ms. Non vérifié : clavier réel, IME, aspect de la fenêtre. Spike complet (tableaux, images, canvas) reporté."
        },
        {
          "t": "Grilles F1 à F9",
          "s": "fait",
          "note": "A : 7 ✅, 2 ⚠️ (F5 volets non restaurés, F6 canvas partiel). B : atteignabilité seulement, d'après l'étape 0."
        }
      ]
    },
    {
      "id": "e3",
      "titre": "Étape 3 — Mesures",
      "statut": "partiel",
      "pct": 70,
      "poids": 1,
      "taches": [
        {
          "t": "Cœur seul sous Windows (rocksy-cli, vault synthétique sur C:)",
          "s": "fait",
          "note": "20x : arbre depuis snapshot 4,6 ms, listing complet 61 ms, recherche de tags ~1 ms, fragments < 1 ms. Index des tags : 0,3 s à chaud, 13 s à la première lecture. Détail : out/spikes/a-tauri-cm6/runs/"
        },
        {
          "t": "Mesures instrumentées de l'app (automatiques, vault synthétique)",
          "s": "fait",
          "note": "5 lancements par cas, médianes : hors vault 318 Ko → éditeur prêt 708 ms ; vault 1x → éditeur 755 ms, arbre 944 ms ; vault 20x → éditeur 680 ms, arbre 939 ms. Pire lancement : 1 036 ms (éditeur), 1 222 ms (arbre). Mémoire : ~30-45 Mo pour l'app + ~370-400 Mo pour WebView2."
        },
        {
          "t": "Latence de frappe et défilement dans l'app réelle",
          "s": "fait",
          "note": "Note de 318 Ko, Live Preview actif : p95 entre 3,8 et 5,0 ms selon l'endroit, max 17 ms (cible proposée 16 ms). Défilement : 16,7 ms entre images, une image à 33,8 ms sur 448."
        },
        {
          "t": "Chronos manuels F8/F9 sur le vrai vault (Alec)",
          "s": "todo",
          "note": "Ne peut être fait que par toi. La baseline Obsidian non plus."
        }
      ]
    },
    {
      "id": "e4",
      "titre": "Étape 4 — Décision de pile",
      "statut": "fait",
      "pct": 100,
      "poids": 1,
      "taches": [
        {
          "t": "out/spikes/m2-resultats.md",
          "s": "fait",
          "note": "Synthèse comparative rédigée : pile A proposée, conditions de mesure, ce qui reste à mesurer."
        },
        {
          "t": "Validation par Alec",
          "s": "fait",
          "note": "Pile Tauri 2 + CodeMirror 6 validée par Alec."
        }
      ]
    },
    {
      "id": "e5",
      "titre": "Étape 5 — Release 0.1",
      "statut": "fait",
      "pct": 100,
      "poids": 4,
      "taches": [
        {
          "t": "Ouvrir n'importe quel .md (CLI, instance unique)",
          "s": "fait",
          "note": "Vu dans l'app : fichier en argument, rien créé à côté, second lancement → onglet en 42 ms. Association de fichiers : script install.ps1 écrit, non exécuté. Glisser-déposer non vérifié."
        },
        {
          "t": "Éditeur Live Preview",
          "s": "fait",
          "note": "Titres, gras, liens, wikilinks, tâches, listes, citations, code, tableaux, images, callouts, frontmatter, Mermaid."
        },
        {
          "t": "Enregistrement atomique, sans diff",
          "s": "fait",
          "note": "LF, CRLF, BOM : seule la ligne modifiée change. Vault de 4 658 fichiers identique après une session."
        },
        {
          "t": "Ajouter un vault, arbre, snapshot",
          "s": "fait",
          "note": "Arbre de 20 747 entrées depuis le snapshot. Dialogue natif de choix du dossier non vérifié."
        },
        {
          "t": "Ctrl+O, onglets, thème sombre",
          "s": "fait",
          "note": "Plus : palette Ctrl+P, deux volets, restauration des onglets."
        },
        {
          "t": "Tags : panneau, fragments, clic → endroit exact",
          "s": "fait",
          "note": "4 199 tags sur le 20x, 9 fragments vérifiés contre le texte. Index non persisté."
        },
        {
          "t": "Build Windows 0.1.0, README d'installation",
          "s": "partiel",
          "note": "C:\\Users\\aleca\\rocksy-test\\release-0.1.0 (exe, DLL, install.ps1, README). Pas d'étiquette git : rien n'est commité."
        },
        {
          "t": "En plus du plan : canvas en lecture, Mermaid, liens externes",
          "s": "partiel",
          "note": "Canvas : extraits bruts dans les cartes de notes, zoom parfois lent sur le canvas dense."
        }
      ]
    },
    {
      "id": "e6",
      "titre": "Étape 6 — Persistance de l'index des tags",
      "statut": "fait",
      "pct": 100,
      "poids": 2,
      "taches": [
        {
          "t": "Cœur : fichier persistant par vault, chargement, réconciliation par taille + mtime",
          "s": "fait",
          "note": "96 tests sous Linux, 93 sous Windows (première exécution réelle des tests du cœur sous Windows). Fichier corrompu ou d'une autre version : ignoré puis reconstruit."
        },
        {
          "t": "App : charger au démarrage, réconcilier en arrière-plan, sauver après enregistrement et à la fermeture",
          "s": "fait",
          "note": "Enregistrement différé de 5 s après une modification, et à la fermeture."
        },
        {
          "t": "Interface : état de l'index (depuis le cache, vérification, à jour)",
          "s": "fait"
        },
        {
          "t": "Tests dans l'app réelle et mesures (1x, 20x)",
          "s": "fait",
          "note": "5 étapes sur une copie du 1x (relance, modification externe, enregistrement depuis l'app puis arrêt brutal). 20x : tags disponibles en 34 à 38 ms, vérification finie en ~95 ms, 0 note relue. Démarrage inchangé (652 ms / 917 ms). Détail : runs/…_persistance-tags."
        },
        {
          "t": "Nouveau build de l'app",
          "s": "fait",
          "note": "0.1.1 dans C:\\Users\\aleca\\rocksy-test\\release-0.1.1"
        },
        {
          "t": "Vérification sur le vrai vault (Drive)",
          "s": "todo",
          "note": "À faire par toi."
        }
      ]
    },
    {
      "id": "e7",
      "titre": "Étape 7 — Watcher de dossier",
      "statut": "fait",
      "pct": 100,
      "poids": 2,
      "taches": [
        {
          "t": "Cœur : écoute récursive, dossiers à point ignorés, regroupement des rafales",
          "s": "fait",
          "note": "112 tests sous Linux, 109 sous Windows. Rafale de 300 réécritures : 1 lot. Repos sur le 20x : 0 CPU, 4,4 Mo."
        },
        {
          "t": "App : à chaque lot, réconcilier l'arbre et les tags ; filet de sécurité",
          "s": "fait",
          "note": "Au plus une passe par seconde. Filet : revérification au retour sur la fenêtre si la dernière date de plus de 30 s (non testé automatiquement)."
        },
        {
          "t": "Tests dans l'app réelle : création, modification, renommage, suppression, rafale",
          "s": "fait",
          "note": "Mise à jour ~430 ms après chaque changement ; rafale de 192 notes en une passe (654 ms) ; rien pour les dossiers à point. Détail : runs/…_watcher."
        },
        {
          "t": "Nouveau build",
          "s": "fait",
          "note": "0.1.2 dans C:\\Users\\aleca\\rocksy-test\\release-0.1.2"
        },
        {
          "t": "Vérification sur Google Drive",
          "s": "todo",
          "note": "À faire par toi : on ne sait pas si le lecteur G: émet des notifications."
        }
      ]
    }
  ],
  "agents": [
    {
      "id": "gen",
      "titre": "Générateur de vault",
      "modele": "Sonnet",
      "statut": "fait",
      "bilan": "1x : 2 885 occurrences de tags (573 distincts), 21 blocs Mermaid, 257 callouts, 220 tableaux. Écart connu : 3,85 Mo de texte par bloc contre 2,94 Mo réels (+31 %, déjà présent avant).",
      "perimetre": "out/tools/gen-vault.py"
    },
    {
      "id": "kpi",
      "titre": "KPI du vrai vault",
      "modele": "Sonnet",
      "statut": "fait",
      "bilan": "Mermaid, callouts, tableaux, tags, embeds, blocs de code, tâches. Sortie datée, n'écrase pas l'ancienne. Limites : fences non fermées et # dans %% ou LaTeX non gérés.",
      "perimetre": "out/tools/vault-kpis.ps1"
    },
    {
      "id": "rune",
      "titre": "Rune : étape 0 (porte d'entrée)",
      "modele": "Fable",
      "statut": "fait",
      "bilan": "Passe sous conditions. Projet en 0.0.1, 200 commits en 2 jours, pas de PR acceptées, mods en Luau. Estimation pour F1-F3 + canvas : 6 à 12 jours.",
      "perimetre": "out/spikes/b-rune/"
    },
    {
      "id": "runewin",
      "titre": "Rune : essai sous Windows",
      "modele": "Fable",
      "statut": "fait",
      "bilan": "Compile du premier coup avec la chaîne croisée. Exe de 31 Mo, 125 Mo de mémoire avec la note de 319 Ko. A installé par accident une toolchain Rust 1.99.1 dans WSL (sans conséquence).",
      "perimetre": "out/spikes/b-rune/"
    },
    {
      "id": "core",
      "titre": "Cœur Rust",
      "modele": "Fable",
      "statut": "fait",
      "bilan": "Livré. Non couvert : commentaires %%, maths, titres setext, watcher de dossier, persistance de l'index des tags.",
      "perimetre": "src/crates/rocksy-core"
    },
    {
      "id": "ui",
      "titre": "Frontend, puis intégration dans l'app réelle",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "src/ui, grille F1-F9",
      "bilan": "Interface livrée puis testée dans l'app réelle : grille F1-F9, 45 captures, 5 corrections d'interface. 40 tests unitaires."
    },
    {
      "id": "persist",
      "titre": "Cœur : persistance des tags",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "src/crates/rocksy-core",
      "bilan": "Livré : chargement de l'index 20x en 25 à 38 ms sous Windows, fichier de 4,4 Mo."
    },
    {
      "id": "watch",
      "titre": "Cœur : watcher de dossier",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "src/crates/rocksy-core",
      "bilan": "Livré avec la crate notify 9 (11 crates en plus). Limite : sous Linux, une surveillance par dossier."
    }
  ],
  "journal": [
    {
      "h": "10:50",
      "type": "info",
      "texte": "Windows n'a ni Rust, ni Node, ni Visual Studio Build Tools. WSL n'a pas sudo (pas de paquets système).",
      "pourquoi": "L'étape 0 du plan (« vérifier la chaîne Windows avec Alec ») aurait bloqué sur une installation manuelle de plusieurs Go."
    },
    {
      "h": "10:52",
      "type": "décision",
      "texte": "Compiler pour Windows depuis WSL (compilation croisée), sans rien installer sous Windows.",
      "pourquoi": "WSL sait lancer des .exe Windows : je peux compiler, lancer et tester l'app sans attendre Alec. Rust est installé dans ~/.cargo et ~/.rustup (espace utilisateur WSL)."
    },
    {
      "h": "10:54",
      "type": "décision",
      "texte": "Pas de worktrees git : les sous-agents travaillent dans des dossiers disjoints du même arbre.",
      "pourquoi": "Le plan interdit de commiter sans demande, et rapatrier des worktrees sans commit est plus risqué que des périmètres de fichiers séparés."
    },
    {
      "h": "10:56",
      "type": "décision",
      "texte": "Un contrat d'interface écrit (CONTRACT.md) fixe les commandes et types entre cœur, Tauri et UI.",
      "pourquoi": "Permet de développer le cœur Rust et le frontend en parallèle sans attendre l'un ou l'autre."
    },
    {
      "h": "10:56",
      "type": "décision",
      "texte": "Le vrai vault (G:) n'est pas touché, même s'il est techniquement joignable via PowerShell. Tests sur vault synthétique uniquement.",
      "pourquoi": "Données personnelles. Les mesures sur le vrai vault restent à faire par Alec."
    },
    {
      "h": "10:58",
      "type": "décision",
      "texte": "Modèles : Sonnet pour le générateur et le script KPI (tâches cadrées), Fable pour le cœur, le Live Preview et l'évaluation de Rune.",
      "pourquoi": "Coût et vitesse là où le risque est faible, modèle le plus fort là où la conception compte."
    },
    {
      "h": "11:00",
      "type": "décision",
      "texte": "Cible de compilation x86_64-pc-windows-gnullvm avec la chaîne llvm-mingw (téléchargée dans ~/rocksy-toolchain, 84 Mo), plutôt que MSVC.",
      "pourquoi": "Ne demande ni Visual Studio ni droits admin. Conséquence : WebView2Loader.dll doit accompagner rocksy.exe. Un build MSVC natif reste possible plus tard si tu installes les Build Tools."
    },
    {
      "h": "11:00",
      "type": "décision",
      "texte": "Dossier de test créé sous Windows : C:\\Users\\aleca\\rocksy-test (exe, vaults synthétiques, captures). Rien d'autre n'est écrit sous Windows.",
      "pourquoi": "L'exe et les vaults de test doivent être sur un disque Windows local pour des mesures réalistes. Supprimable sans risque."
    },
    {
      "h": "11:09",
      "type": "mesure",
      "texte": "Premier build Tauri croisé : 6 min 44 s à froid. Fenêtre visible ~0,6 s après le lancement (page de test vide, relevé par script).",
      "pourquoi": "Mesure indicative sur une page vide, pas un T1."
    },
    {
      "h": "11:11",
      "type": "décision",
      "texte": "Tests de l'app réelle par le port de débogage de WebView2 (JS + capture de la page), pas par capture d'écran du bureau.",
      "pourquoi": "Ne photographie jamais ton écran. Effet visible : une fenêtre Rocksy s'ouvre et se ferme toute seule pendant mes tests."
    },
    {
      "h": "11:11",
      "type": "info",
      "texte": "Tauri 2 stable retenu (Tauri 3 est en alpha).",
      "pourquoi": "Conforme au plan, pas de risque d'API mouvante."
    },
    {
      "h": "11:27",
      "type": "info",
      "texte": "Les heures du journal ont été recalées sur l'horloge de la machine (mes premières entrées étaient en avance de 5 à 35 min).",
      "pourquoi": "Exactitude du journal."
    },
    {
      "h": "11:27",
      "type": "décision",
      "texte": "Écart de calibrage du générateur accepté : +31 % de texte par bloc par rapport au vrai vault, liens 3 fois plus denses.",
      "pourquoi": "Le vault synthétique est plus lourd que le réel : les mesures seront pessimistes, pas optimistes. Non recalibré pour ne pas retarder les spikes."
    },
    {
      "h": "11:35",
      "type": "décision",
      "texte": "Pile de la 0.1 : Tauri 2 + CodeMirror 6. Le spike B (Rune) s'arrête à l'étape 0, complétée par un essai sous Windows.",
      "pourquoi": "Application de la porte d'entrée et du plafond de temps du plan (§6, étapes 0 et 4). Le plan prévoit ta validation : elle est demandée dans « À faire par Alec », sans bloquer le travail."
    },
    {
      "h": "11:35",
      "type": "décision",
      "texte": "Sous-agent supplémentaire : compiler et lancer Rune sous Windows avec la chaîne de compilation croisée.",
      "pourquoi": "Le verdict Rune n'avait aucune preuve Windows. Coût faible (40 min, en parallèle), et la comparaison des deux piles sera plus honnête."
    },
    {
      "h": "11:39",
      "type": "mesure",
      "texte": "Cœur sous Windows, vault 20x sur C: : ouverture de l'arbre depuis le snapshot 4,6 ms ; recherche de tags 1,1 ms ; index des tags 0,3 s à chaud mais 13 s à la première lecture des 9 760 fichiers.",
      "pourquoi": "L'arbre et la recherche tiennent l'échelle. La première indexation des tags sera longue sur Google Drive : elle est en arrière-plan (ne bloque ni T1 ni T2), mais la persistance de l'index devient la priorité n°1 après la 0.1."
    },
    {
      "h": "11:39",
      "type": "décision",
      "texte": "Limite par défaut de 500 fragments par requête de tag côté Tauri.",
      "pourquoi": "Sans limite, un tag très fréquent renvoie 2,6 Mo de JSON (6 521 fragments sur le 20x)."
    },
    {
      "h": "11:39",
      "type": "décision",
      "texte": "Surveillance des fichiers ouverts par scrutation du mtime toutes les 2 s (onglets ouverts seulement), dans la colle Tauri.",
      "pourquoi": "Le watcher de dossier complet (notify, rafales, renommages) est prévu en M3. La scrutation suffit pour détecter qu'un autre outil a modifié une note ouverte."
    },
    {
      "h": "11:39",
      "type": "décision",
      "texte": "Dossier de données de l'app : %APPDATA%\\dev.rocksy.app (liste des vaults, snapshots, session, timings). Variable ROCKSY_DATA_DIR pour isoler mes tests.",
      "pourquoi": "Jamais dans le vault ni dans .obsidian. L'identifiant dev.rocksy.app est provisoire, à revoir avant publication."
    },
    {
      "h": "11:42",
      "type": "mesure",
      "texte": "App réelle sous Windows, vault 20x, appels depuis la page : arbre de 20 747 entrées reçu en 67 ms depuis le snapshot (113 ms par listing complet), note de 318 Ko lue en 11 ms, index des tags prêt en 0,44 s (cache chaud), recherche de tags 5 à 14 ms, 500 fragments en 11 ms.",
      "pourquoi": "Temps aller-retour complets (Rust + JSON + passage vers la page), relevés par script sur C: (pas Google Drive). L'IPC n'est pas un goulot pour la note de 318 Ko."
    },
    {
      "h": "11:42",
      "type": "décision",
      "texte": "Installation sans droits admin par un script (out/release/install.ps1) : copie dans %LOCALAPPDATA%\\Programs\\Rocksy, raccourci, déclaration « Ouvrir avec » pour .md. Pas d'installeur NSIS/MSI en 0.1.",
      "pourquoi": "Un installeur demande des outils en plus et n'apporte rien pour un usage personnel. Limite : Windows 11 interdit à un script de changer l'application par défaut, le choix « Toujours ouvrir avec Rocksy » se fait une fois à la main. Je n'exécute pas ce script moi-même (il écrit dans ton registre utilisateur)."
    },
    {
      "h": "11:42",
      "type": "décision",
      "texte": "Script de mesure automatique (out/tools/measure-auto.ps1) en plus du chrono manuel.",
      "pourquoi": "L'app écrit ses jalons (éditeur prêt, arbre prêt) dans un journal : je peux mesurer sans toi, de façon reproductible. Ces mesures instrumentées ne remplacent pas ton chrono F8/F9 sur le vrai vault."
    },
    {
      "h": "11:44",
      "type": "mesure",
      "texte": "Rune sous Windows : fenêtre en 233 ms avec la note de 319 Ko (Tauri : ~600 ms pour une page vide). Mémoire 125 Mo.",
      "pourquoi": "Rune démarre nettement plus vite, ce qui confirme son intérêt pour plus tard. La décision du jour ne change pas : il lui manque les widgets dans le texte (tableaux, images), estimés à plusieurs jours, et Live Preview F2 est éliminatoire. À toi de voir si un vrai spike B vaut le coup après la 0.1."
    },
    {
      "h": "12:00",
      "type": "mesure",
      "texte": "App réelle, 5 lancements par cas, vault synthétique sur C:, cache chaud : éditeur prêt (note de 318 Ko) en 708 ms hors vault, 755 ms en vault 1x, 680 ms en vault 20x ; arbre prêt en 944 ms (1x) et 939 ms (20x).",
      "pourquoi": "Jalons écrits par l'app et horodatés côté Rust depuis le début du processus. Sous les cibles du spike (T1 ≤ 1 s, T2 ≤ 3 s) en médiane, et pas de dégradation à 20x. Ce ne sont pas des chronos manuels, ni sur Google Drive, ni à froid après redémarrage."
    },
    {
      "h": "12:00",
      "type": "décision",
      "texte": "Mermaid rendu par Pele 1.13 (paquet npm, MIT), chargé à la demande ; canvas en lecture et deux volets inclus dès maintenant.",
      "pourquoi": "Le sous-agent frontend a eu le temps de les faire ; le plan les plaçait après la 0.1. Ils restent à valider dans l'app réelle avant d'être annoncés dans la 0.1."
    },
    {
      "h": "12:00",
      "type": "décision",
      "texte": "Les tests d'intégration sont confiés au sous-agent frontend (qui garde son contexte), avec droit de corriger l'interface mais pas le backend.",
      "pourquoi": "Un défaut backend remonte à la session principale, pour garder une seule main sur le contrat."
    },
    {
      "h": "12:16",
      "type": "problème",
      "texte": "Pendant les tests, des préférences d'interface (enregistrement automatique, repli des dossiers, largeur du panneau) se sont écrites dans le profil WebView2 réel de Rocksy (%LOCALAPPDATA%\\dev.rocksy.app), malgré le dossier de données isolé.",
      "pourquoi": "Corrigé : la fenêtre est créée en code et le profil WebView suit le dossier isolé. Le sous-agent remet les valeurs par défaut. Aucune note ni aucun fichier de vault n'est concerné ; au pire, supprimer ce dossier remet l'interface à zéro."
    },
    {
      "h": "12:16",
      "type": "décision",
      "texte": "Nouvelle commande open_external pour les liens externes, limitée à http, https et mailto.",
      "pourquoi": "window.open ne fait rien sous Tauri. La limite empêche une note de lancer un programme ou un fichier local par un lien. L'ouverture réelle d'un site n'est pas testée automatiquement (cela ouvrirait un onglet dans ton navigateur)."
    },
    {
      "h": "12:16",
      "type": "info",
      "texte": "Premiers résultats d'intégration (annoncés par le sous-agent, rapport complet à venir) : F1, F2, F3, F5, F7, F8 passent dans l'app réelle ; frappe p95 ≤ 4,4 ms sur la note de 318 Ko ; enregistrement LF, CRLF et BOM sans autre octet modifié ; 4 658 fichiers du vault identiques après une session.",
      "pourquoi": "À confirmer par la grille et les captures."
    },
    {
      "h": "12:19",
      "type": "décision",
      "texte": "Suppression de %LOCALAPPDATA%\\dev.rocksy.app (profil WebView2 pollué par les tests).",
      "pourquoi": "Vérifié avant : dossier créé par mes propres tests à 11:10, contenant seulement 6 clés de test, et aucun dossier de données réel (%APPDATA%\\dev.rocksy.app n'existait pas). Tu démarreras donc sur un profil vierge, enregistrement automatique actif par défaut."
    },
    {
      "h": "12:27",
      "type": "info",
      "texte": "Tests d'intégration terminés dans l'app réelle : F1, F2, F3, F4, F7, F8, F9 passent ; F5 et F6 partiels.",
      "pourquoi": "Détail et preuves : out/spikes/a-tauri-cm6/grille.md et runs/2026-11-23_1223_integration."
    },
    {
      "h": "12:27",
      "type": "décision",
      "texte": "La 0.1 inclut canvas en lecture, Mermaid et deux volets, malgré leur statut partiel.",
      "pourquoi": "Ils fonctionnent dans l'app et ne présentent pas de risque pour le vault (aucune écriture dans un .canvas, vérifié octet pour octet). Leurs limites sont listées dans le rapport."
    },
    {
      "h": "12:27",
      "type": "décision",
      "texte": "Pas d'étiquette git 0.1.0, pas de commit.",
      "pourquoi": "Le plan demande une étiquette, mais interdit de commiter sans ta demande. La version 0.1.0 est dans l'exe ; l'étiquette se posera au commit."
    },
    {
      "h": "12:27",
      "type": "décision",
      "texte": "Le code reste dans out/spikes/a-tauri-cm6/src pour l'instant.",
      "pourquoi": "Le déplacer à la racine du dépôt est un choix de structure à faire avec toi, de préférence au moment du premier commit du code."
    },
    {
      "h": "12:48",
      "type": "info",
      "texte": "Alec valide la pile Tauri 2 + CodeMirror 6 et demande la persistance de l'index des tags.",
      "pourquoi": "m2-resultats.md et CLAUDE.md mis à jour."
    },
    {
      "h": "12:48",
      "type": "décision",
      "texte": "Détection des changements par taille + mtime, sans hash ; index stocké dans le dossier de données de l'app (un fichier par vault), jamais dans le vault.",
      "pourquoi": "Lire un fichier est ce qui coûte sur Drive (~155 ms), pas le lister. Limite acceptée : un fichier modifié sans changement de taille ni de date n'est pas revu avant sa prochaine modification."
    },
    {
      "h": "12:48",
      "type": "décision",
      "texte": "Le fichier d'index contient des extraits de notes (pour afficher les fragments sans relire).",
      "pourquoi": "Il reste local, dans %APPDATA%\\dev.rocksy.app, comme le cache de métadonnées d'Obsidian. À savoir si ce dossier est un jour sauvegardé ou partagé."
    },
    {
      "h": "12:48",
      "type": "décision",
      "texte": "Le cœur est confié au sous-agent qui a écrit rocksy-core (repris avec son contexte) ; je garde la colle Tauri, l'interface et les tests de l'app.",
      "pourquoi": "Même répartition que ce matin, une seule main sur le contrat."
    },
    {
      "h": "12:46",
      "type": "info",
      "texte": "Commit 14f80b5 à la demande d'Alec, sur une nouvelle branche release-0.1 (pas sur main).",
      "pourquoi": "Les 18 Mo de captures du run d'intégration sont exclus par .gitignore et restent sur disque. Pas d'étiquette v0.1.0 : le commit contenait déjà un début de persistance."
    },
    {
      "h": "13:10",
      "type": "décision",
      "texte": "Version passée à 0.1.1 pour le build avec persistance ; la 0.1.0 reste dans son dossier.",
      "pourquoi": "Tu utilises déjà la 0.1.0 : deux dossiers distincts évitent toute confusion sur ce que tu testes."
    },
    {
      "h": "13:10",
      "type": "décision",
      "texte": "L'index est enregistré 5 s après une modification et à la fermeture ; à la fermeture, l'app annule l'indexation en cours, l'attend, puis enregistre ce qui a déjà été lu.",
      "pourquoi": "Une première indexation interrompue sur Drive ne repart pas de zéro. Ce cas d'interruption n'a pas pu être provoqué en test (indexation trop rapide sur les vaults synthétiques)."
    },
    {
      "h": "13:10",
      "type": "mesure",
      "texte": "Persistance dans l'app réelle : 1x → tags disponibles en 9 à 11 ms après relance ; 20x → 34 à 38 ms, vérification complète en ~95 ms sans relire de note ; une note modifiée → 1 seule relue.",
      "pourquoi": "Vaults synthétiques sur C:, cache chaud. Sur Drive, la vérification coûtera le temps de lister les fichiers (250 ms mesurés à chaud dans l'audit, 57 s pour le tout premier listing à froid), plus jamais la lecture de toutes les notes."
    },
    {
      "h": "13:10",
      "type": "info",
      "texte": "Constat : %LOCALAPPDATA% et %APPDATA%\\dev.rocksy.app existent de nouveau.",
      "pourquoi": "Ce sont tes propres lancements de l'app (mes tests utilisent des profils isolés). Je n'y touche pas."
    },
    {
      "h": "13:20",
      "type": "info",
      "texte": "Commit bd78554 (0.1.1, persistance des tags) sur release-0.1, à la demande d'Alec.",
      "pourquoi": ""
    },
    {
      "h": "13:22",
      "type": "problème",
      "texte": "Le sous-agent « générateur » restait affiché « en attente d'un shell » depuis 11:11.",
      "pourquoi": "Résidu d'une copie de fichiers lancée en arrière-plan ce matin ; aucun processus ne tournait plus. Tâche arrêtée. Les nouveaux sous-agents ont pour consigne de ne rien laisser en arrière-plan."
    },
    {
      "h": "13:22",
      "type": "décision",
      "texte": "Le watcher n'est qu'un déclencheur : à chaque lot d'événements, l'app relance la vérification par listing (arbre + tags), qui reste la source de vérité. Une vérification périodique sert de filet de sécurité.",
      "pourquoi": "Il n'est pas garanti que le lecteur virtuel de Google Drive émette des notifications fiables, et je ne peux pas le tester. Avec ce choix, un événement manqué retarde la mise à jour au lieu de la perdre."
    },
    {
      "h": "13:22",
      "type": "décision",
      "texte": "Événements des dossiers à point ignorés ; rafales regroupées (400 ms de silence, 3 s au plus).",
      "pourquoi": "79 % des fichiers du vault sont dans des dossiers à point, et le nettoyeur IA réécrit des centaines de fichiers d'un coup."
    },
    {
      "h": "13:22",
      "type": "décision",
      "texte": "Pas de suivi des renommages dans cette étape : un renommage est vu comme une suppression et un ajout (la note renommée est relue une fois).",
      "pourquoi": "Le suivi sert surtout à la mise à jour des liens, qui n'existe pas encore."
    },
    {
      "h": "13:37",
      "type": "décision",
      "texte": "Un lancement avec un profil isolé (variable ROCKSY_DATA_DIR) est une instance séparée : l'instance unique ne s'applique qu'au profil normal.",
      "pourquoi": "Tu avais Rocksy ouvert et mon test s'est arrêté pour ne pas ouvrir ses fichiers dans ta fenêtre. Avec ce changement, mes tests et ton usage ne se croisent plus. L'instance unique du profil normal est inchangée (vérifiée ce matin, pas revérifiée depuis)."
    },
    {
      "h": "13:37",
      "type": "mesure",
      "texte": "Watcher dans l'app réelle (copie du 1x sur C:) : création, modification, renommage, suppression vus en 424 à 432 ms ; rafale de 192 notes : une seule passe, 654 ms ; 0 ms de CPU sur 10 s au repos.",
      "pourquoi": "Je visais 300 notes dans la rafale ; le dossier choisi n'en contenait que 192. La rafale de 300 est testée dans le cœur seul (1 lot)."
    },
    {
      "h": "13:37",
      "type": "décision",
      "texte": "Version 0.1.2 ; la crate notify (et 10 dépendances) s'ajoute au cœur, qui n'en avait que 3.",
      "pourquoi": "C'est la bibliothèque prévue au plan pour le watcher ; l'écrire à la main pour Windows et Linux aurait coûté plus et serait moins sûr."
    }
  ],
  "rapport": {
    "synthese": "Session du 23/11/2026, environ 1 h 40, de 10:50 à 12:30. Le plan prévoyait que tu compiles et chronomètres sous Windows ; comme Windows n'avait aucune chaîne de build, j'ai compilé pour Windows depuis WSL et testé l'app moi-même. Résultat : une app Rocksy 0.1.0 qui tourne sous Windows, vérifiée sur des vaults synthétiques, et un verdict sur Rune. Elle n'a jamais touché ton vrai vault ni Google Drive : c'est la première chose à essayer. Suite de l'après-midi (12:35 à 13:15) : commit du code à ta demande, puis persistance de l'index des tags, livrée en 0.1.1. Puis watcher de dossier, livré en 0.1.2 (13:50 à 14:20).",
    "sections": [
      {
        "titre": "Ce qui a été fait",
        "points": [
          "Chaîne de build : Rust et llvm-mingw dans ton home WSL, compilation croisée vers Windows (out/tools/build-win.sh), sans rien installer sous Windows.",
          "Générateur de vault v2 (tags hiérarchiques, Mermaid, callouts, tableaux, images, note de 318 Ko, 2 canvas) et vaults 1x et 20x ; script KPI étendu (7 familles de compteurs).",
          "Cœur Rust rocksy-core : lecture et enregistrement atomique, registre de vaults, arbre avec snapshot persisté, index des tags avec fragments, session. 3 dépendances.",
          "App Tauri 2 : 16 commandes, instance unique, surveillance des fichiers ouverts, liens externes limités à http, https et mailto.",
          "Interface CodeMirror 6 : Live Preview, onglets, deux volets, arbre virtualisé, Ctrl+O, palette, panneau des tags, canvas en lecture, Mermaid via Pele, thème sombre.",
          "Rune : verdict de l'étape 0 (passe sous conditions, hors 0.1) et essai réussi de compilation et de lancement sous Windows.",
          "Documents : out/spikes/m2-resultats.md, grilles F1-F9, out/release/README.md et install.ps1, CLAUDE.md mis à jour."
        ]
      },
      {
        "titre": "Ajout de l'après-midi : persistance de l'index des tags (0.1.1)",
        "points": [
          "Un fichier d'index par vault dans le dossier de données de l'app, jamais dans le vault ; rechargé à l'ouverture sans lire aucune note, puis vérifié par taille et date.",
          "Testé dans l'app réelle : relance, note modifiée de l'extérieur (1 seule relue), enregistrement depuis l'app suivi d'un arrêt brutal (tag conservé). Sur le 20x : tags disponibles en 34 à 38 ms.",
          "Tests du cœur : 96 sous Linux et, pour la première fois, 93 exécutés réellement sous Windows, tous verts. Démarrage de l'app inchangé.",
          "Non testé : vrai vault, Google Drive, interruption d'une longue première indexation.",
          "Limite : une note modifiée sans changement de taille ni de date n'est pas revue ; le fichier d'index contient des extraits de notes."
        ]
      },
      {
        "titre": "Ajout : suivi des changements externes (0.1.2)",
        "points": [
          "Le dossier du vault est écouté ; à chaque lot de changements, l'app relance la vérification par listing (arbre et tags). Les dossiers à point sont ignorés, les rafales regroupées.",
          "Testé dans l'app réelle : création, modification, renommage, suppression (~430 ms), rafale de 192 notes en une passe, aucun événement pour .obsidian, 0 CPU au repos.",
          "Tests du cœur : 112 sous Linux (relancés par moi), 109 sous Windows d'après le sous-agent.",
          "Non testé : Google Drive (notifications incertaines), le filet de sécurité au retour sur la fenêtre, une rafale de plus de 3 s dans l'app, un lecteur démonté.",
          "Limite : un renommage est vu comme une suppression et un ajout."
        ]
      },
      {
        "titre": "Ce qui a été testé, et comment",
        "points": [
          "Cœur : 81 tests automatiques (règles des tags, blocs, enregistrement atomique, snapshot, aucune écriture indésirable), relancés par moi en fin de session : tous passent. Interface : 40 tests unitaires, tous passent.",
          "App réelle sous Windows, pilotée par le port de débogage de WebView2 (jamais de capture de ton écran) : grille F1 à F9, 7 cas passent, 2 partiels.",
          "Intégrité du vault : 4 658 fichiers identiques (dossiers à point et .obsidian compris) après une session en lecture ; un seul fichier modifié après un enregistrement ; fichiers LF, CRLF et BOM : seule la ligne modifiée change.",
          "Démarrage, 5 lancements par cas : éditeur prêt en 708 ms (hors vault), 755 ms (1x), 680 ms (20x) ; arbre prêt en 944 ms (1x) et 939 ms (20x). Dernier lancement de contrôle : 843 ms et 1 016 ms.",
          "Frappe sur 318 Ko avec Live Preview : p95 de 3,8 à 5,0 ms, max 17 ms. Tags sur 4 199 tags : filtre en 15 à 43 ms, fragments en 56 à 91 ms.",
          "Instance unique : second lancement transmis, onglet actif en 42 ms. Refus d'écrire dans .obsidian et dans un .canvas : vérifié."
        ]
      },
      {
        "titre": "Ce qui n'a PAS été testé",
        "points": [
          "Le vrai vault, Google Drive, le démarrage à froid après redémarrage. Toutes les mesures sont sur vault synthétique, sur C:, cache chaud.",
          "Les chronos manuels T1/T2 et la baseline Obsidian : aucune comparaison chiffrée avec Obsidian n'existe à ce jour.",
          "Glisser-déposer, dialogues natifs de choix de dossier et de fichier, ouverture réelle d'un lien web, install.ps1 (écrit, jamais exécuté), double-clic sur un .md.",
          "Saisie IME, lecteur d'écran, renommage sur un fichier verrouillé par Drive ou un antivirus.",
          "Fidélité de Mermaid et des canvas par rapport à Obsidian ; réouverture du vault dans Obsidian après usage (l'absence de diff est vérifiée par hash, pas dans Obsidian)."
        ]
      },
      {
        "titre": "Limites connues de la 0.1",
        "points": [
          "Première indexation des tags d'un vault : lit toutes les notes (13 s pour 9 760 fichiers jamais lus sur C:, plus long sur Drive). Les suivantes sont instantanées depuis la 0.1.1.",
          "Pas de recherche plein texte, backlinks, graphe, renommage de tags, création ou renommage de fichiers depuis l'arbre, mods, import des vaults d'Obsidian.",
          "Session propre à Rocksy : le workspace.json d'Obsidian n'est pas lu ; la disposition en volets n'est pas restaurée.",
          "Tableau, Mermaid ou callout repliable imbriqué dans une liste ou une citation : affiché en source. Lignes de plus de 6 000 caractères : affichées en source.",
          "Fichier ouvert supprimé par un autre outil : signalé seulement à l'enregistrement. Messages d'erreur du cœur en anglais.",
          "Analyse des tags : commentaires %%, formules et titres soulignés (setext) non exclus ou non reconnus.",
          "WebView2 consomme environ 370 à 400 Mo ; l'exe doit rester accompagné de WebView2Loader.dll.",
          "Sécurité : pas de politique CSP et accès aux fichiers locaux ouvert pour afficher les images. Acceptable en usage personnel, à durcir avant publication."
        ]
      },
      {
        "titre": "Écarts au plan",
        "points": [
          "Compilation et tests faits par moi depuis WSL, pas par toi sous Windows.",
          "Pas de worktrees git : sous-agents dans des dossiers disjoints, avec un contrat d'interface écrit.",
          "Spike B arrêté à l'étape 0 (avec essai Windows), pas de spike complet.",
          "Étape 3 faite en mesures automatiques ; les chronos manuels restent à faire.",
          "Canvas, Mermaid et volets livrés alors qu'ils étaient prévus après la 0.1. Pas d'étiquette git."
        ]
      },
      {
        "titre": "Ce qui a été installé ou créé hors du dépôt",
        "points": [
          "WSL : ~/.cargo et ~/.rustup (Rust, dont une toolchain 1.99.1 installée par accident par un sous-agent), ~/rocksy-toolchain (llvm-mingw), ~/rocksy-vaults, ~/rocksy-scratch (clone de Rune), ~/.cache/rocksy-target.",
          "Windows : C:\\Users\\aleca\\rocksy-test uniquement (exe, release, vaults synthétiques, captures, copies de test). Supprimable.",
          "Le profil WebView2 créé par mes tests dans %LOCALAPPDATA%\\dev.rocksy.app a été supprimé ; il sera recréé à ton premier lancement, avec %APPDATA%\\dev.rocksy.app pour les données de l'app."
        ]
      },
      {
        "titre": "Suite proposée, dans l'ordre",
        "points": [
          "Essai sur le vrai vault et chronos manuels : ils diront si Drive à froid tient les cibles.",
          "Recherche plein texte et backlinks, puis renommage de tags.",
          "Décider de la structure du dépôt et commiter ; alléger les captures.",
          "Avant publication : licence, anonymisation de in/ et out/data/, CSP, identifiant dev.rocksy.app."
        ]
      }
    ]
  }
};
