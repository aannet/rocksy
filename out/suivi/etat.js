// Etat du suivi Rocksy. Ecrit par la session principale uniquement.
window.ROCKSY_ETAT = {
  "maj": "23/11/2026 22:58",
  "resume": "Rocksy 0.2.1 est prêt : C:\\Users\\aleca\\rocksy-test\\release-0.2.1\\rocksy.exe. Il corrige les constats des trois audits : faille du canvas, CSP stricte, accès aux fichiers limité aux vaults et aux fichiers que tu ouvres, plus de perte de texte à la fermeture, index de recherche endommagé reconstruit, enregistrement 3 à 4 fois plus rapide. Vérifié dans l'app réelle sur vaults synthétiques (C:, cache chaud) : sécurité, fermeture, intégrité du vault (4 658 fichiers identiques), démarrage sans régression nette. Pas vérifié : vrai vault, Google Drive, dialogues natifs, glisser-déposer, second lancement, déplacement réel des index, install.ps1. Suites revérifiées par moi : 244 tests du cœur, 124 tests d'interface. Rien n'est commité.",
  "actionsAlec": [
    {
      "texte": "1. Dire si je commite la 0.2.1 (et si je pose l'étiquette v0.2.1).",
      "detail": "Non commité : 109 fichiers modifiés ou nouveaux, dont les quatre rapports d'audit et le dossier de run."
    },
    {
      "texte": "2. Passer à la 0.2.1 sur ton vrai vault.",
      "detail": "Ferme Rocksy, lance C:\\Users\\aleca\\rocksy-test\\release-0.2.1\\rocksy.exe. Au premier lancement, les index passent de %APPDATA% à %LOCALAPPDATA% sans réindexation (jamais essayé sur ton profil réel : dis-moi si le vault se réindexe quand même). Ne reviens pas ensuite à la 0.2.0 : elle réindexerait tout. À regarder : ajout d'un vault par le dialogue, glisser-déposer d'un .md, second lancement qui ouvre un onglet, fermeture juste après une frappe, et si une note dont le nom contient « ~ » suivi d'un chiffre s'enregistre bien."
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
    },
    {
      "id": "e8",
      "titre": "Étape 8 — Recherche plein texte et backlinks",
      "statut": "fait",
      "pct": 100,
      "poids": 3,
      "taches": [
        {
          "t": "Contrat : commandes search, backlinks, outgoing_links, unresolved_links, événement index-ready",
          "s": "fait",
          "note": "CONTRACT.md"
        },
        {
          "t": "Cœur : liens et résolution, moteur de recherche persisté, une seule lecture par note",
          "s": "fait",
          "note": "tantivy 0.27. 147 tests sous Linux, 144 sous Windows. Index plein texte : 63 Mo pour 64 Mo de texte (20x). Une note modifiée est lue une seule fois pour tags, liens et recherche."
        },
        {
          "t": "Interface : panneau de recherche, panneau des liens, liens non résolus",
          "s": "fait",
          "note": "Vu dans Edge contre un backend simulé seulement : recherche à la frappe (Ctrl+Maj+F), extraits surlignés, navigation clavier, ouverture à la bonne ligne ; onglet des liens entrants et sortants ; commande « Liens non résolus » ; état de l'index dans la barre d'état. 57 tests unitaires. Démarrage inchangé."
        },
        {
          "t": "App : branchement des commandes",
          "s": "fait",
          "note": "4 commandes branchées, événement index-ready. Exe : 11,6 Mo (6,7 avant)."
        },
        {
          "t": "Tests dans l'app réelle et mesures (1x, 20x)",
          "s": "fait",
          "note": "7 points vérifiés dans l'app réelle : recherche (86 recherches tapées : médiane 100 ms, p95 116 ms ; 10 résultats sur 10 exacts ; LF, CRLF, BOM, accents), liens (enregistrement, fichier créé de l'extérieur → lien résolu en 0,5 s), liens non résolus, non-régression, vault intact, 16 requêtes inhabituelles sans erreur. Détail : runs/…_integration-recherche-liens."
        },
        {
          "t": "Build 0.2.0",
          "s": "fait",
          "note": "C:\\Users\\aleca\\rocksy-test\\release-0.2.0"
        },
        {
          "t": "Vérification sur le vrai vault (Drive)",
          "s": "todo",
          "note": "À faire par toi."
        }
      ]
    },
    {
      "id": "e9",
      "titre": "Étape 9 — Audits de la 0.2.0 (sécurité, robustesse, performance)",
      "statut": "fait",
      "pct": 100,
      "poids": 2,
      "taches": [
        {
          "t": "Audit de sécurité (injection dans la WebView, commandes Tauri, chemins, données persistées, dépendances)",
          "s": "fait",
          "note": "Fait. 18 constats : 1 critique (reproduit), 3 élevés, 3 moyens. out/audits/2026-11-23_securite.md"
        },
        {
          "t": "Audit de robustesse (plantages, blocages, pertes de données, fuites mémoire, courses)",
          "s": "fait",
          "note": "Fait. 23 constats : 1 bloquant, 6 majeurs, 16 mineurs. Aucune panique du cœur sur 300 000 documents fuzzés, hors index de recherche endommagé. out/audits/2026-11-23_bugs-robustesse.md"
        },
        {
          "t": "Audit de performance (démarrage, échelle, E/S, interface, mémoire)",
          "s": "fait",
          "note": "Fait. 15 recommandations, mesurées sous WSL (ext4), pas sous Windows ni sur Drive. out/audits/2026-11-23_performance.md"
        },
        {
          "t": "Synthèse et tri des constats",
          "s": "fait",
          "note": "Fait. J'ai relu dans le code SEC-1, SEC-2, SEC-5, SEC-6, BUG-1, BUG-2, BUG-3, BUG-4 : le code correspond à ce que décrivent les audits. Les autres constats ne sont pas revérifiés par moi."
        },
        {
          "t": "Analyse de cohérence entre les trois audits (conflits, régressions, ordre des lots)",
          "s": "fait",
          "note": "Sous-session Sonnet, lecture seule. Non relue par moi dans le code. out/audits/2026-11-23_coherence.md"
        }
      ]
    },
    {
      "id": "e10",
      "titre": "Étape 10 — Correctifs des audits",
      "statut": "fait",
      "pct": 100,
      "poids": 5,
      "taches": [
        {
          "t": "Lot 0 : briques testables dans le cœur (contrôle des chemins SEC-3/SEC-4, écritures propres PERF-1/BUG-5)",
          "s": "fait",
          "note": "Fait. access.rs et own_writes.rs, 30 tests. Pas encore branchés dans main.rs (lot 3)."
        },
        {
          "t": "Lot 1a : interface — canvas (SEC-1, SEC-9), regex lente (SEC-6), chemins réseau (SEC-4)",
          "s": "fait",
          "note": "Fait. 11 tests ajoutés (ils échouaient avant correctif) et scénarios Edge avec backend simulé. Cellule de 8 000 caractères : 31 s → moins de 100 ms."
        },
        {
          "t": "Lot 1b : parseurs Rust linéaires (BUG-10, BUG-11, SEC-8)",
          "s": "fait",
          "note": "Fait. 160 Ko de « [ » sur une ligne : 35,5 s → 12 ms ; recherche sur une ligne de 800 Ko : 277 s → 0,11 s (profil debug, WSL)."
        },
        {
          "t": "Lot 1c : enregistrement atomique (BUG-4, BUG-7), limite de taille (BUG-21), extensions enregistrables (SEC-3), SEC-10",
          "s": "fait",
          "note": "Fait. Branches propres à Windows vérifiées par compilation croisée seulement."
        },
        {
          "t": "Lot 1d : install.ps1 (SEC-11), SEC-16, SEC-17, watcher (BUG-20)",
          "s": "fait",
          "note": "Fait. install.ps1 réécrit, vérifié par analyse syntaxique seulement : toujours jamais exécuté. BUG-20 était réel sous Windows."
        },
        {
          "t": "Lot 2 : moteur d'index (BUG-3 unwind, index endommagé, PERF-3, PERF-4, PERF-7, BUG-13)",
          "s": "fait",
          "note": "Fait. 19 tests ajoutés. Mesuré sous WSL sur le vault 20x : mise à jour de l'index à l'enregistrement de la note de 318 Ko 48 ms → 2 ms (commit de 37 à 39 ms reporté en arrière-plan) ; liens entrants après un enregistrement sans changement de lien 27 ms → 0,04 ms. rocksy-cli grossit de 13 % avec unwind."
        },
        {
          "t": "Lot 3 : CONTRACT.md en une fois, puis main.rs (SEC-3/4/5/7, PERF-1/2/5, BUG-1/6/8/9)",
          "s": "fait",
          "note": "Fait. CONTRACT.md réécrit. 239 tests du cœur. Exercé dans un exe de travail en instance isolée : refus d'accès, fermeture en trois temps, aucun listing après un enregistrement de l'app, renommage de dossier réindexé. Compilé seulement : dialogue de fichier, glisser-déposer, second lancement, migration réelle des données."
        },
        {
          "t": "Lot 4 : interface (fermeture BUG-1, rechargement BUG-2, nouvelles tentatives BUG-7, panneaux)",
          "s": "fait",
          "note": "Fait. 102 tests d'interface (34 ajoutés, 7 défauts réintroduits pour contrôle). Backend simulé seulement. Frappe p95 2,9 à 3,8 ms sur 322 Ko (inchangé). Un scénario navigateur (f2) a un sélecteur périmé, à corriger au lot 5."
        },
        {
          "t": "Lot 5 : intégration, add_vault, CSP (SEC-2), build 0.2.1, essais complets dans l'app réelle, mesures 0.2.0 contre 0.2.1",
          "s": "fait",
          "note": "Fait. 0.2.1 construite et essayée dans l'exe : 18 essais réussis, 1 non faisable en instance isolée (refus d'add_vault). CSP sans violation. Run : out/spikes/a-tauri-cm6/runs/2026-11-23_2240_integration-audits/."
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
      "titre": "Interface : intégration recherche et liens dans l'app réelle",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "src/ui, grille F1-F9",
      "bilan": "Intégration recherche et liens faite : 7 points vérifiés, 4 corrections d'interface."
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
    },
    {
      "id": "search",
      "titre": "Cœur : liens et recherche",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "src/crates/rocksy-core",
      "bilan": "Liens, résolution façon Obsidian, recherche tantivy. Requêtes 1 à 12 ms sur le 20x sous Windows (cœur seul)."
    },
    {
      "id": "audit-sec",
      "titre": "Audit de sécurité",
      "modele": "Opus",
      "statut": "fait",
      "perimetre": "lecture seule ; écrit out/audits/",
      "bilan": "1 critique : XSS par un .canvas piégé (reproduite dans Edge). CSP absente, read_file/save_file sans restriction de chemin, images en chemin réseau."
    },
    {
      "id": "audit-bugs",
      "titre": "Audit de robustesse",
      "modele": "Opus",
      "statut": "fait",
      "perimetre": "lecture seule ; écrit out/audits/",
      "bilan": "Perte de texte à la fermeture de la fenêtre ; plantage sur index de recherche endommagé ; pas de fuite franche."
    },
    {
      "id": "audit-perf",
      "titre": "Audit de performance",
      "modele": "Opus",
      "statut": "fait",
      "perimetre": "lecture seule ; écrit out/audits/",
      "bilan": "Chaque enregistrement relance deux listings du vault et renvoie l'arbre entier à la page (110 à 140 ms mesurés sous WSL)."
    },
    {
      "id": "audit-coherence",
      "titre": "Cohérence des trois audits",
      "modele": "Sonnet",
      "statut": "fait",
      "perimetre": "lecture seule ; écrit out/audits/",
      "bilan": "5 doublons, 4 conflits réels, ordre en 6 lots. La CSP proposée par l'audit de sécurité casserait l'interface telle quelle."
    },
    {
      "id": "lot0",
      "titre": "Lot 0 : briques du cœur",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "nouveaux modules du cœur, lib.rs",
      "bilan": "Terminé, suites vertes."
    },
    {
      "id": "lot1a",
      "titre": "Lot 1a : interface",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "ui : canvas.ts, syntax.ts, resolve.ts, paths.ts",
      "bilan": "Terminé, suites vertes."
    },
    {
      "id": "lot1b",
      "titre": "Lot 1b : parseurs",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "cœur : tags/parse.rs, search/highlight.rs",
      "bilan": "Terminé, suites vertes."
    },
    {
      "id": "lot1c",
      "titre": "Lot 1c : fichiers",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "cœur : files.rs, atomic.rs",
      "bilan": "Terminé, suites vertes."
    },
    {
      "id": "lot1d",
      "titre": "Lot 1d : installeur et watcher",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "install.ps1, README, watch.rs",
      "bilan": "Terminé, suites vertes."
    },
    {
      "id": "lot2",
      "titre": "Lot 2 : moteur d'index",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "cœur : tags/index, search/engine, tree.rs, profils release",
      "bilan": "Terminé, 219 tests verts."
    },
    {
      "id": "lot3",
      "titre": "Lot 3 : coquille Tauri et contrat",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "CONTRACT.md, src-tauri, cœur",
      "bilan": "Terminé, suites vertes."
    },
    {
      "id": "lot4",
      "titre": "Lot 4 : interface",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "ui/",
      "bilan": "Terminé, suites vertes."
    },
    {
      "id": "lot5",
      "titre": "Lot 5 : intégration et essais réels",
      "modele": "Fable",
      "statut": "fait",
      "perimetre": "tout le code, build Windows, runs",
      "bilan": "0.2.1 livrée dans release-0.2.1. Cause des notes manquantes à la première indexation établie et corrigée."
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
    },
    {
      "h": "14:25",
      "type": "info",
      "texte": "Commit 45c483d et étiquettes v0.1.2 (watcher) et v0.1.1 (persistance, sur bd78554), à la demande d'Alec.",
      "pourquoi": "Pas d'étiquette v0.1.0 : le premier commit ne correspond pas exactement à l'exe 0.1.0."
    },
    {
      "h": "14:24",
      "type": "décision",
      "texte": "Un seul index par vault : chaque note nouvelle ou modifiée est lue une seule fois et alimente tags, liens et recherche plein texte.",
      "pourquoi": "Sur Google Drive, lire une note est ce qui coûte (~155 ms). Trois index séparés liraient trois fois."
    },
    {
      "h": "14:24",
      "type": "décision",
      "texte": "Moteur de recherche : tantivy comme prévu au plan, mais soumis à une porte d'entrée de 20 min (compile-t-il avec notre chaîne croisée, à quel coût ?). Repli : un index inversé maison.",
      "pourquoi": "Je ne veux pas découvrir en fin de course qu'il ne compile pas pour Windows sans MSVC. Le sous-agent tranche avec preuve et je consigne le résultat."
    },
    {
      "h": "14:24",
      "type": "décision",
      "texte": "Sémantique de recherche fixée : casse et accents ignorés, mots combinés par ET, dernier mot en préfixe (recherche à la frappe), \"expression exacte\", -mot, path:, tag: (avec descendants). Extraits tirés de l'index, sans relire les fichiers.",
      "pourquoi": "La question ouverte du plan (requêtes combinées ET/OU/NON avec path:) était à v1.x par défaut ; path: et tag: en filtres sont peu coûteux une fois l'index là. Le OU n'est pas prévu. À corriger si tu veux autre chose."
    },
    {
      "h": "14:24",
      "type": "décision",
      "texte": "Résolution des liens comme Obsidian : chemin exact, puis suffixe de chemin, puis nom de fichier, sans tenir compte de la casse ; en cas d'ambiguïté, chemin le plus court. Les pièces jointes ont aussi des backlinks.",
      "pourquoi": "Le vault doit se comporter comme dans Obsidian. Les backlinks des pièces jointes préparent le scan des médias orphelins prévu en job."
    },
    {
      "h": "14:24",
      "type": "décision",
      "texte": "Pas de création de note au clic sur un lien non résolu, pas de mise à jour des liens au renommage dans cette étape.",
      "pourquoi": "Ce sont des écritures dans le vault, à traiter à part avec leurs propres tests."
    },
    {
      "h": "14:35",
      "type": "décision",
      "texte": "tantivy 0.27 retenu pour la recherche plein texte.",
      "pourquoi": "Porte d'entrée passée : il compile et s'exécute sous Windows avec la chaîne croisée, sans dépendance C (compression lz4 en Rust). Coût : environ 100 crates en plus et quelques Mo sur l'exe (4,6 Mo pour un exe de test, contre 1 Mo pour l'outil actuel)."
    },
    {
      "h": "14:35",
      "type": "décision",
      "texte": "Contrat complété d'après les retours de l'interface : longueur du terme trouvé (pour le sélectionner), et pas d'erreur tant que la recherche n'est pas prête (résultat vide, searchReady faux).",
      "pourquoi": "Manques relevés par le sous-agent interface en développant contre le backend simulé."
    },
    {
      "h": "14:35",
      "type": "info",
      "texte": "Dans le ruban, la loupe ouvre désormais la recherche dans le vault ; l'ouverture rapide (Ctrl+O) a une autre icône.",
      "pourquoi": "Choix du sous-agent interface, cohérent avec l'usage habituel de la loupe."
    },
    {
      "h": "15:05",
      "type": "mesure",
      "texte": "App réelle, 20x, appels depuis la page. Première indexation complète (tags + liens + recherche) : 2,5 s à chaud. Après relance : tags et liens à 147 ms, recherche à 176 ms après la demande, 0 note relue. Requêtes : 4 à 22 ms aller-retour. Liens sortants de la note de 318 Ko (446 liens) : 6 ms ; backlinks : 2,5 ms.",
      "pourquoi": "Vault synthétique sur C:, cache chaud. Sur Drive, la première indexation sera dominée par la lecture des notes ; elle est sauvegardée toutes les 5 s et reprend où elle s'est arrêtée."
    },
    {
      "h": "15:05",
      "type": "info",
      "texte": "Coûts de la recherche : exe de 6,7 à 11,6 Mo ; index sur disque à peu près de la taille du texte du vault (63 Mo pour le 20x, environ 4 Mo pour un vault de 488 notes) ; mémoire de l'app 75 Mo après relance sur le 20x, 133 Mo juste après une indexation complète ; enregistrement d'une note plus long de 60 à 110 ms.",
      "pourquoi": "L'index stocke le texte des notes pour produire les extraits sans relire les fichiers. Comme l'index des tags, il contient donc tes notes et reste dans %APPDATA%\\dev.rocksy.app."
    },
    {
      "h": "15:05",
      "type": "décision",
      "texte": "Enregistrement : tags, liens et recherche sont mis à jour avant que l'enregistrement rende la main (60 à 110 ms de plus), plutôt qu'en différé.",
      "pourquoi": "Le panneau des liens et la recherche sont justes dès l'enregistrement. L'enregistrement est asynchrone, la frappe ne devrait pas en souffrir ; c'est mesuré dans les tests d'intégration. Repli possible : validation en arrière-plan."
    },
    {
      "h": "15:05",
      "type": "info",
      "texte": "Écarts du cœur au contrat, acceptés : le dernier mot n'est un préfixe que s'il termine la saisie ; une requête faite seulement d'exclusions renvoie vide ; pas de racinisation (« budget » ne trouve pas « budgétaire », mais « budg » le trouve en fin de saisie).",
      "pourquoi": "Comportements raisonnables et documentés ; à revoir à l'usage."
    },
    {
      "h": "15:19",
      "type": "mesure",
      "texte": "Interface dans l'app réelle (20x) : frappe → résultats affichés en 100 ms (médiane), 116 ms (p95), dont 80 ms d'attente voulue, 7 ms d'appel et 12 ms de rendu. Démarrage : éditeur prêt 547 ms, arbre 705 ms (médianes de 5). Frappe sur 318 Ko avec les panneaux ouverts : p95 ≤ 3,4 ms ; pendant des enregistrements : p95 4,1 ms, max 13,9 ms. Ctrl+S : 50 ms sur une petite note (31 à 34 avant), 69 à 109 ms sur la note de 318 Ko. Mémoire de l'app : 57 Mo après relance, 152 Mo après une première indexation.",
      "pourquoi": "Pas de régression du démarrage ni de la frappe ; l'enregistrement coûte 20 à 70 ms de plus, sans effet visible sur la frappe."
    },
    {
      "h": "15:19",
      "type": "info",
      "texte": "Fermeture de l'app pendant une première indexation : testée, mais l'indexation du 20x s'est terminée avant la sortie ; la « reprise » n'a donc pas été observée dans l'app.",
      "pourquoi": "La reprise est testée dans le cœur par simulation. Sur Drive, où l'indexation est lente, c'est ce cas qui comptera : à surveiller chez toi."
    },
    {
      "h": "15:19",
      "type": "info",
      "texte": "Pas d'événement index-ready après un enregistrement : l'info-bulle de l'état de l'index (nombre de liens) n'est rafraîchie qu'au prochain changement externe ou à la relance.",
      "pourquoi": "Sans effet sur les panneaux, qui se rafraîchissent eux-mêmes. Laissé tel quel."
    },
    {
      "h": "15:19",
      "type": "décision",
      "texte": "Le passage à la 0.2.0 reconstruit l'index (format version 2) : toutes les notes sont relues une fois.",
      "pourquoi": "L'ancien index ne contenait pas les liens. Sur Drive, cette relecture unique sera longue ; elle est en arrière-plan."
    },
    {
      "h": "15:28",
      "type": "info",
      "texte": "0.2.0 commitée sur release-0.1 (8db67f6) et étiquette v0.2.0 posée, à la demande d'Alec. Rien de poussé.",
      "pourquoi": "Demande explicite."
    },
    {
      "h": "15:31",
      "type": "décision",
      "texte": "Les trois audits sont en lecture seule : aucun correctif n'est appliqué par les sous-sessions. Elles peuvent écrire des tests jetables hors dépôt pour prouver un constat.",
      "pourquoi": "Trois sessions qui corrigent en parallèle les mêmes fichiers se marcheraient dessus, et je veux vérifier les constats avant de toucher au code."
    },
    {
      "h": "16:00",
      "type": "info",
      "texte": "Les trois audits sont revenus. Aucun correctif appliqué.",
      "pourquoi": "Les correctifs touchent le contrat (chemins acceptés, fermeture, délai de la recherche après enregistrement) : l'ordre et le périmètre sont à valider par Alec."
    },
    {
      "h": "16:00",
      "type": "info",
      "texte": "L'audit de sécurité a interrogé l'API publique OSV pour les dépendances, en n'envoyant que des noms et versions de crates publics, et a lancé Edge sans interface avec un profil temporaire, supprimé ensuite.",
      "pourquoi": "cargo audit n'est pas installé. Aucune donnée du projet ou du vault n'est sortie."
    },
    {
      "h": "16:35",
      "type": "info",
      "texte": "Analyse de cohérence revenue. Toujours aucun correctif appliqué.",
      "pourquoi": "Cinq décisions reviennent à Alec avant le premier lot."
    },
    {
      "h": "16:45",
      "type": "info",
      "texte": "Alec a donné son accord (« go ») pour les lots 0 et 1, avec les cinq recommandations proposées.",
      "pourquoi": "Demande explicite."
    },
    {
      "h": "16:45",
      "type": "décision",
      "texte": "Un .txt ouvert dans Rocksy passe en lecture seule : l'enregistrement est réservé aux .md et .markdown.",
      "pourquoi": "Recommandation proposée à Alec, non contredite. Réduit ce qu'une page compromise peut écrire."
    },
    {
      "h": "16:45",
      "type": "décision",
      "texte": "Fichier aux fins de ligne mixtes : conservé tel quel. Recherche : un texte enregistré peut apparaître avec jusqu'à 2 s de retard. Fermeture : confirmation seulement si l'auto-enregistrement est désactivé ou si un enregistrement a échoué. Rocksy pourra supprimer ses propres temporaires orphelins dans le vault.",
      "pourquoi": "Recommandations proposées à Alec, non contredites. Appliquées dans les lots 2 à 4."
    },
    {
      "h": "16:45",
      "type": "décision",
      "texte": "Les tests de preuve des audits sont portés par l'agent qui corrige chaque défaut, pas dans un lot séparé ; le lot 0 se limite aux deux briques du cœur.",
      "pourquoi": "Un test de preuve échoue tant que le défaut n'est pas corrigé : le porter seul laisserait la suite rouge."
    },
    {
      "h": "16:45",
      "type": "décision",
      "texte": "Cinq agents en parallèle dans le même arbre de travail, sur des fichiers disjoints, chacun avec son propre dossier de compilation.",
      "pourquoi": "Même méthode que les étapes précédentes ; évite les fusions de worktrees."
    },
    {
      "h": "17:45",
      "type": "info",
      "texte": "Les cinq agents ont été interrompus par la limite d'usage vers 16:45, avant toute modification de code, puis relancés à 17:21.",
      "pourquoi": "Aucune perte : ils ont repris d'un état vierge."
    },
    {
      "h": "17:45",
      "type": "décision",
      "texte": "J'ai corrigé moi-même un plantage trouvé par le lot 1b : une espace insécable avant un lien ou un résultat de recherche loin dans une longue ligne faisait paniquer le calcul d'extrait (excerpt.rs). Test ajouté.",
      "pourquoi": "Défaut préexistant, correctif d'une ligne, fichier hors du périmètre de l'agent. Courant en français (espace insécable avant « : »)."
    },
    {
      "h": "17:45",
      "type": "décision",
      "texte": "Contrôle des chemins : la lecture autorisée autour d'une note hors vault sera limitée à son dossier et ses sous-dossiers, pour les images et fichiers texte ; un fichier atteint par un lien depuis une note hors vault reste en lecture seule, y compris après redémarrage ; une note sur un partage réseau s'ouvre et s'enregistre, sans charger ses images ni ses liens.",
      "pourquoi": "Proposé à Alec à 17:35, à appliquer au lot 3 sauf avis contraire. La règle récursive de la brique rendrait tout le profil lisible par la page."
    },
    {
      "h": "17:45",
      "type": "info",
      "texte": "À vérifier par Alec sur le vrai vault quand la version corrigée sera livrée : une note dont le nom contient « ~ » suivi d'un chiffre pourrait être refusée à l'enregistrement si Windows ne canonicalise pas les chemins de Google Drive.",
      "pourquoi": "Garde contre les noms courts 8.3 (SEC-10), non testable hors de G:."
    },
    {
      "h": "18:28",
      "type": "décision",
      "texte": "J'ai fixé les changements de contrat dans out/audits/2026-11-23_contrat-delta.md avant de lancer les lots 3 et 4 en parallèle : fermeture en trois temps (close-requested, close_ack, quit), commande close_vault, champ readOnly, lecture anticipée de la première note, champ vault dans les événements d'index, données d'index déplacées vers le dossier local.",
      "pourquoi": "Le rapport de cohérence demandait que le contrat ne soit modifié qu'une fois ; un document commun permet aux deux agents de travailler en même temps."
    },
    {
      "h": "18:28",
      "type": "décision",
      "texte": "Garde-fou de fermeture : si l'interface ne répond pas dans les 5 s à une demande de fermeture, l'app se ferme quand même.",
      "pourquoi": "Sans cela, une interface gelée rendrait la fenêtre infermable (risque relevé par l'analyse de cohérence)."
    },
    {
      "h": "18:28",
      "type": "décision",
      "texte": "Fins de ligne mixtes : les lignes non modifiées gardent leur fin d'origine ; les lignes modifiées ou ajoutées prennent la fin majoritaire du fichier.",
      "pourquoi": "Seule façon de respecter « conservé tel quel » tout en permettant l'édition."
    },
    {
      "h": "18:28",
      "type": "info",
      "texte": "Le lot 2 a ajouté un contrôle non demandé : les sommes de contrôle de l'index de recherche sont vérifiées une fois par session (11 à 16 ms mesurés sous WSL à 20x).",
      "pourquoi": "Son test a montré qu'un index altéré pouvait répondre faux sans erreur ni plantage. Conservé."
    },
    {
      "h": "18:28",
      "type": "info",
      "texte": "Le lot 2 a retenu qu'un sous-dossier absent ou d'accès refusé reste listé vide, au lieu de faire échouer tout le listing comme le demandait l'analyse de cohérence ; toute autre erreur fait échouer le listing.",
      "pourquoi": "Un dossier protégé en permanence aurait bloqué tout rafraîchissement. Accepté ; à revoir si Drive renvoie ces erreurs de façon passagère."
    },
    {
      "h": "19:03",
      "type": "décision",
      "texte": "add_vault n'acceptera qu'un dossier choisi dans le dialogue natif pendant la session ; il reste libre en instance isolée (ROCKSY_DATA_DIR), pour les essais automatiques.",
      "pourquoi": "Trou relevé par le lot 3 : une page compromise pouvait s'enregistrer un vault et obtenir l'accès complet. L'instance isolée est décidée par celui qui lance le processus, pas par la page."
    },
    {
      "h": "19:03",
      "type": "décision",
      "texte": "Les erreurs portent un code stable (ApiError.code) ; l'interface ne reconnaît plus les erreurs par leur libellé anglais.",
      "pourquoi": "Un changement de libellé côté cœur aurait fait afficher « fichier supprimé » comme un conflit ordinaire."
    },
    {
      "h": "19:03",
      "type": "décision",
      "texte": "La version corrigée s'appellera 0.2.1.",
      "pourquoi": "Correctifs sans nouvelle fonction."
    },
    {
      "h": "19:03",
      "type": "info",
      "texte": "À élucider au lot 5 : lors d'un essai du lot 3, la première indexation d'un vault fraîchement copié a mis 435 notes sur 488 dans l'index de recherche ; la passe suivante a rattrapé, non reproduit ensuite.",
      "pourquoi": "Cause inconnue. Pas de livraison avant de savoir si des notes peuvent être sautées."
    },
    {
      "h": "19:03",
      "type": "info",
      "texte": "Après le premier lancement de la 0.2.1, relancer la 0.2.0 réindexera tout le vault (les index auront changé de dossier), et la 0.2.1 supprimera cette copie au lancement suivant.",
      "pourquoi": "Conséquence du déplacement des index vers %LOCALAPPDATA% (SEC-7). Sans risque pour les notes, mais long sur Drive."
    },
    {
      "h": "22:58",
      "type": "info",
      "texte": "Le lot 5 a été interrompu par la limite d'usage vers 19:30 et relancé à 22:20.",
      "pourquoi": "Travail intact à la reprise."
    },
    {
      "h": "22:58",
      "type": "info",
      "texte": "Cause des notes manquantes dans l'index de recherche établie : juste après la copie d'un vault, la création d'un fichier de l'index de recherche est parfois refusée par Windows ; le moteur perdait alors ce qui n'était pas encore validé. La 0.2.0 a le même défaut, sans aucun signe. La 0.2.1 affiche « N notes à relire » et relance seule l'indexation (2, 5, 15, 30, 60 s).",
      "pourquoi": "Reproduit 9 fois sur 55 sur x1 et 3 fois sur 4 sur x20, toujours juste après une copie massive. L'antivirus est une hypothèse, non observée."
    },
    {
      "h": "22:58",
      "type": "info",
      "texte": "Un défaut plus grave que prévu a été trouvé à l'intégration : au changement de vault, aucun index n'arrivait. Corrigé et vérifié sur huit ordres d'appel.",
      "pourquoi": "Les lots 3 et 4 n'avaient jamais tourné ensemble."
    },
    {
      "h": "22:58",
      "type": "info",
      "texte": "Pendant un essai, un scénario a ouvert une fois https://example.org dans ton navigateur par défaut. Le scénario ne le fait plus.",
      "pourquoi": "Clic scripté sur un lien externe dans it-f8."
    },
    {
      "h": "22:58",
      "type": "décision",
      "texte": "Régression acceptée : la mémoire de travail du processus passe de 60 à 122 Mo au démarrage sur le vault 20x (mémoire privée inchangée, 39 Mo).",
      "pourquoi": "C'est la vérification d'intégrité de l'index de recherche à l'ouverture, qui lit tout l'index. À revoir si elle pèse sur Drive."
    },
    {
      "h": "22:58",
      "type": "info",
      "texte": "Défaut restant connu : le protocole asset: touche un chemin réseau avant de le refuser (1,3 s contre 2 ms). L'interface ne fait jamais cette requête et la CSP bloque les scripts injectés.",
      "pourquoi": "Comportement de Tauri ; pas de parade simple dans ce lot."
    }
  ],
  "rapport": {
    "synthese": "Session du 23/11/2026, environ 1 h 40, de 10:50 à 12:30. Le plan prévoyait que tu compiles et chronomètres sous Windows ; comme Windows n'avait aucune chaîne de build, j'ai compilé pour Windows depuis WSL et testé l'app moi-même. Résultat : une app Rocksy 0.1.0 qui tourne sous Windows, vérifiée sur des vaults synthétiques, et un verdict sur Rune. Elle n'a jamais touché ton vrai vault ni Google Drive : c'est la première chose à essayer. Suite de l'après-midi (12:35 à 13:15) : commit du code à ta demande, puis persistance de l'index des tags, livrée en 0.1.1. Puis watcher de dossier, livré en 0.1.2 (13:50 à 14:20). Enfin recherche plein texte et backlinks, livrés en 0.2.0 (14:30 à 15:20).",
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
        "titre": "Ajout : recherche plein texte et backlinks (0.2.0)",
        "points": [
          "Un index unique par vault : tags, liens et recherche alimentés par une seule lecture de chaque note modifiée. Moteur de recherche tantivy, sans dépendance C.",
          "Recherche à la frappe avec extraits surlignés, casse et accents ignorés, expressions, exclusions, filtres path: et tag:. Liens entrants et sortants, images comprises, résolution comme Obsidian, liste des liens non résolus.",
          "Testé dans l'app réelle : 7 points, dont exactitude des positions (10 sur 10, fichiers LF, CRLF, BOM), mise à jour après enregistrement et après changement externe, vault intact (4 658 fichiers identiques), 16 requêtes inhabituelles sans erreur.",
          "Tests automatiques relancés par moi : 147 pour le cœur, 57 pour l'interface, tous verts. 144 exécutés sous Windows d'après le sous-agent.",
          "Coûts : exe 11,6 Mo, index de recherche de la taille du texte du vault, 20 à 70 ms de plus par enregistrement.",
          "Non testé : vrai vault, Drive, reprise d'une première indexation interrompue dans l'app, défilement de la liste des liens à la molette.",
          "Limites : pas de OU ni de racinisation ; liens dans le frontmatter et liens par référence non extraits ; une seule instance peut mettre à jour la recherche d'un même profil."
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
          "Pas de graphe, de renommage de tags, de création ou renommage de fichiers depuis l'arbre, de mods, d'import des vaults d'Obsidian.",
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
          "Renommage de tags et mise à jour des liens au renommage (premières écritures en masse dans le vault : aperçu, confirmation, annulation).",
          "Décider de la structure du dépôt et commiter ; alléger les captures.",
          "Avant publication : licence, anonymisation de in/ et out/data/, CSP, identifiant dev.rocksy.app."
        ]
      },
      {
        "titre": "Audits de la 0.2.0 (sécurité, robustesse, performance)",
        "points": [
          "Trois sous-sessions Opus en lecture seule.",
          "Sécurité : un .canvas piégé exécute du JavaScript dans la WebView (reproduit dans Edge avec le code de canvas.ts) ; sans CSP et avec read_file/save_file ouverts à tout chemin, cela donne la lecture et l'écriture de n'importe quel fichier de l'utilisateur.",
          "Robustesse : le texte peut être perdu à la fermeture de la fenêtre ; un index de recherche endommagé fait planter l'app à chaque ouverture du vault (reproduit) ; pas de fuite mémoire franche.",
          "Performance : chaque enregistrement relance deux listings complets du vault.",
          "Non vérifié : rien n'a été exécuté dans rocksy.exe ; les mesures de performance sont sous WSL.",
          "Rapports : out/audits/."
        ]
      },
      {
        "titre": "0.2.1 — correctifs des audits",
        "points": [
          "Méthode : trois audits Opus en lecture seule, une analyse de cohérence Sonnet, puis six lots de correctifs par agents sur des fichiers disjoints, sur un contrat commun (out/audits/2026-11-23_contrat-delta.md).",
          "Sécurité : faille du canvas corrigée, CSP stricte, accès de la page limité aux vaults enregistrés et aux fichiers ouverts par l'utilisateur, chemins réseau refusés, enregistrement réservé aux .md, add_vault limité au dialogue natif, permissions Tauri au plus juste.",
          "Pertes de texte : fermeture en trois temps avec enregistrement préalable, rechargement qui n'écrase plus la saisie, nouvelles tentatives d'enregistrement, écriture externe jamais écrasée, fins de ligne mixtes conservées.",
          "Robustesse : index de recherche endommagé reconstruit, indexation annulable, parseurs linéaires, plantage sur espace insécable corrigé, notes manquantes à la première indexation détectées et reprises.",
          "Mesuré dans l'app, 0.2.0 contre 0.2.1, vaults synthétiques sur C: : enregistrement de la note de 318 Ko 105-111 ms → 21-31 ms ; démarrage, frappe, recherche et suivi des changements externes sans écart net ; exe 11,6 → 14,1 Mo.",
          "Tests : 244 dans le cœur (147 avant), 124 dans l'interface (57 avant), 16 scénarios navigateur, 18 essais dans l'exe réel.",
          "Non vérifié : vrai vault, Google Drive, démarrage à froid, dialogues natifs, glisser-déposer, second lancement en profil normal, déplacement réel des index, install.ps1 (réécrit, jamais exécuté).",
          "Non traité : PERF-11, PERF-12, PERF-15, la seconde partie de PERF-6, l'outil de contrôle des dépendances avant release (SEC-17)."
        ]
      }
    ]
  }
};
