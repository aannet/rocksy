# Spike B — Rune (natif) — grille des cas

État au 2026-11-23, après l'étape 0 seulement (voir `README.md`). **Aucun cas F1 à F9 n'a été testé sous Windows ni sur le vault réel** (l'essai Windows de l'étape 0 ne couvre que la compilation, l'ouverture de la fenêtre et le rendu headless de la sonde). La colonne « Résultat » dit ce qui paraît atteignable d'après le code lu et l'essai headless sous Linux, pas un résultat de test.

Légende : « atteignable » = les briques existent dans Rune ; « à construire » = faisable mais le travail est côté Rocksy ; « non vérifié » = rien ne permet de conclure.

| # | Cas | Résultat | Preuve (capture, run, remarque) |
|---|---|---|---|
| F1 | Live Preview : éléments courants, marqueurs au curseur | Atteignable, partiellement démontré | Titres, gras et marqueurs masqués hors ligne du curseur rendus dans un `TextArea` de 319 Ko, frappe sous 16 ms : `runs/2026-11-23_1129_etape0-linux-headless/captures/probe-319.png`. Italique, liens `[[ ]]`, tâches, listes et citations non faits. Le masquage passe par `Span::size(0.1)`, la navigation clavier à travers les marqueurs masqués n'est pas testée |
| F2 | Live Preview : frontmatter, code, tableau | À construire | Coloration de code disponible (`rune_kit::syntax`). Pas de repli ni de tableau dans un champ de texte : il faut l'architecture par blocs (un champ ou un rendu par bloc, curseur et annulation entre blocs à écrire). Rien d'exécuté |
| F3 | Image embarquée | À construire | Pas de widget dans le flux du texte (seules les pastilles de mention). Image possible comme bloc entre deux champs, avec le rendu d'image de `rune_kit::Markdown`. Rien d'exécuté |
| F4 | Autocomplétion de wikilinks | Atteignable | Popover, liste et `replace_with_pill` existent (lu dans `crates/rune/README.md`). Rien d'exécuté |
| F5 | Onglets, splits, restauration du workspace | Atteignable | `TabStrip` et `SplitView` dans `rune-kit` (lu). La lecture de `workspace.json` est côté Rocksy. Rien d'exécuté |
| F6 | Canvas en lecture seule | À construire | `canvas_input` (zoom, déplacement) et formes vectorielles existent (lu). La visionneuse est à écrire, 1 à 2 jours estimés |
| F7 | Aucune écriture indésirable (hash avant/après) | Non vérifié | Dépend surtout du code Rocksy. Ce que Rune écrit de lui-même sur disque (préférences, caches) n'a pas été examiné |
| F8 | Fichier hors vault, instance unique | Atteignable | Instance unique par tube nommé sous Windows dans `crates/rune/src/open_url/instance/windows.rs` (lu). Rien d'exécuté |
| F9 | Tags : fragments, recherche instantanée | Non vérifié | Relève du cœur Rust et d'une liste virtualisée (`virtual_rows`, lu). Rien d'exécuté |
