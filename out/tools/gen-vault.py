#!/usr/bin/env python3
"""
Generateur de vault Obsidian synthetique, calque sur les KPIs de out/data/vault-kpis.json.

Banc de test pour le core Rust : demarrage, arbre, index, reconciliation, a 1x, 2x, 5x, 10x, 20x
le volume actuel (488 notes, 465 pieces jointes, 94 dossiers par "bloc").

Principes
  * Deterministe : meme --seed + meme --scale = memes fichiers (hors dates, voir --mtime-burst).
  * Stable par prefixe : le vault est decoupe en blocs de 488 notes / 465 pieces jointes.
    Les fichiers d'un bloc ne dependent que de la seed et du numero de bloc, jamais du --scale.
    Donc relancer avec une echelle plus grande dans le MEME dossier n'ajoute que le delta, et les
    fichiers existants sont ignores (sauf --force). Genere le 20x une fois, garde-le.
    Pour revenir a une echelle plus petite : genere dans un autre dossier (c'est quasi instantane
    jusqu'a 5x) ; --scale plus petit dans un dossier existant n'efface rien.
  * Aucune donnee personnelle : noms, textes et liens sont fabriques.
  * Dependances : Python 3.9+, bibliotheque standard uniquement.

Contenu genere (version 2)
  * Tags hierarchiques (2 a 3 niveaux, vocabulaire fabrique, distribution de Zipf + longue traine locale
    a chaque bloc) places en paragraphe, element de liste avec enfants, titre, ligne de tableau,
    citation/callout, et frontmatter (liste YAML). Pieges qui ne sont PAS des tags : #123, # dans une URL,
    dans du code en ligne, dans un bloc de code, dans un wikilink [[note#titre]].
  * Blocs Mermaid (flowchart, sequenceDiagram, classDiagram, stateDiagram-v2, erDiagram, gantt, pie,
    mindmap, wardley-beta) dans ~5 % des notes, callouts (certains repliables), tableaux alignes, blocs de
    code (js, python, rust, bash, json), taches, listes imbriquees, wikilinks avec alias et ancres
    (ancre [[note#Résumé]] : beaucoup de notes ont un titre "Résumé").
  * Pieces jointes .png : vrais PNG valides (32x32, remplis jusqu'a la taille voulue par un chunk prive).
    Embeds ![[Pasted image ....png]] et ![alt](../_ASSETS/....png) relatifs.
  * Bloc 0 uniquement : note pire cas 01_WORK/__DAILY NOTES SYNTH.md (~319 Ko, journal date), active dans
    .obsidian/workspace.json ; deux canvas JSON Canvas : 01_WORK/Canvas simple.canvas (6 noeuds) et
    01_WORK/Canvas dense.canvas (~150 noeuds, groupes, aretes etiquetees, couleurs).

Historique
  * v2 : le contenu de TOUS les fichiers .md change par rapport a la v1 (nouveaux elements, nouveau flux
    aleatoire), la note 0 est renommee "__DAILY NOTES ACTIVE" -> "__DAILY NOTES SYNTH", les PNG sont valides,
    les .canvas aleatoires sont des JSON Canvas valides. Les tailles restent calibrees (le budget d'octets de
    chaque note inclut les elements ajoutes). A regenerer dans un dossier neuf (ou avec --force).
  * v1 : version initiale.

Exemples
  python3 out/tools/gen-vault.py --out /data/vaults/x1  --scale 1
  python3 out/tools/gen-vault.py --out /data/vaults/x20 --scale 20
  python3 out/tools/gen-vault.py --out /data/vaults/x20 --scale 20 --attach-factor 1.0   # tailles reelles (~2,3 Go)
  python3 out/tools/gen-vault.py --out /data/vaults/x5  --stats-only
"""
import argparse
import base64
import bisect
import datetime
import json
import math
import os
import random
import re
import statistics
import sys
import time
import unicodedata
import zlib
from pathlib import Path

VERSION = 2
BASE_NOTES = 488
BASE_ATT = 465
FOLDERS_PER_BLOCK = 94          # dossiers par bloc, racines comprises (base : 94, profondeur max 5)
MAX_DEPTH = 5

# (dossier racine, poids en nombre de notes dans le vault de reference)
TOPS = [("01_WORK", 186), ("PERMANENT", 87), ("MEETINGS", 71), ("PERSONAL", 33), ("00_INBOX", 31),
        ("WRITE", 30), ("TASKS", 24), ("AI", 13), ("TEMPLATES", 5), ("LLM", 3)]
TOP_NAMES = [t for t, _ in TOPS]
TOP_WEIGHTS = [w for _, w in TOPS]

# pieces jointes : (extension, poids) d'apres la repartition reelle
ATT_EXT = [(".png", 420), (".pdf", 10), (".canvas", 10), (".jpg", 8), (".docx", 7), (".gdoc", 5),
           (".excalidraw", 2), (".excalidrawlib", 2), (".odt", 1)]
ATT_EXT_TOTAL = sum(w for _, w in ATT_EXT)
# taille moyenne reelle en octets (kpis : bytes / count)
ATT_REAL_AVG = {".png": 274_067, ".pdf": 471_017, ".canvas": 11_336, ".jpg": 564_267, ".docx": 29_599,
                ".gdoc": 197, ".excalidraw": 32_526, ".excalidrawlib": 28_108, ".odt": 3_171_329}

WORDS = ["projet", "réunion", "équipe", "note", "idée", "étude", "plan", "stratégie", "budget", "revue",
         "atelier", "synthèse", "carte", "backlog", "décision", "architecture", "données", "client",
         "roadmap", "risque", "processus", "bilan", "veille", "outil", "format", "cadre", "modèle", "audit",
         "qualité", "livrable", "chantier", "périmètre", "séance", "arbitrage", "hypothèse", "scénario"]
FM_KEYS = ["tags", "aliases", "created", "modified", "type", "status", "project", "author", "source",
           "priority", "area", "due", "related", "cssclasses", "title", "description", "id", "version", "lang"]
GLOBAL_TAGS = ["projet", "idee", "todo", "meeting", "archive", "ref", "perso", "travail", "revue", "plan"] + \
              [f"theme{n:02d}" for n in range(50)]

DAILY_BYTES = 319_000           # la note active du vault de reference
EXCAL_RATIO = 16 / BASE_NOTES


def rng(*parts):
    return random.Random(":".join(str(p) for p in parts))


# ---- contenu riche (v2) -------------------------------------------------------------------------
TAG_ROOTS = ["projet", "client", "domaine", "equipe", "statut", "theme", "outil", "lieu", "cycle", "source",
             "priorite", "format"]
TAG_LEAVES = ["alpha", "beta", "gamma", "revue", "cadrage", "bilan", "suivi", "audit", "atelier", "recette",
              "pilote", "decision", "risque", "backlog", "jalon", "retro"]
DAYS_FR = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"]
CALLOUT_KINDS = ["note", "warning", "tip", "info", "example", "quote", "success", "question"]
MERMAID_TYPES = ["flowchart", "sequenceDiagram", "classDiagram", "stateDiagram-v2", "erDiagram", "gantt",
                 "pie", "mindmap", "wardley-beta"]
CODE_LANGS = ["js", "python", "rust", "bash", "json"]
DAILY_START = datetime.date(2024, 1, 1)


def make_png(r, n):
    """PNG 32x32 valide (bandes colorees), complete a ~n octets par un chunk prive ancillaire 'ruSt'."""
    def chunk(t, d):
        return len(d).to_bytes(4, "big") + t + d + (zlib.crc32(t + d) & 0xFFFFFFFF).to_bytes(4, "big")
    c1 = [r.randrange(40, 230) for _ in range(3)]
    c2 = [r.randrange(40, 230) for _ in range(3)]
    k = r.randrange(2, 8)
    raw = bytearray()
    for y in range(32):
        raw.append(0)
        for x in range(32):
            raw += bytes(c1 if ((x + y) // k) % 2 else c2)
    ihdr = (32).to_bytes(4, "big") * 2 + bytes([8, 2, 0, 0, 0])
    idat = chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    iend = chunk(b"IEND", b"")
    head = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
    pad = n - len(head) - len(idat) - len(iend) - 12
    mid = chunk(b"ruSt", r.randbytes(pad)) if pad > 0 else b""
    return head + mid + idat + iend


class Content:
    """Generation du contenu riche : tags, Mermaid, callouts, tableaux, code, canvas."""

    def _taglib(self):
        if getattr(self, "_tl", None) is None:
            r = rng(self.seed, "taglib")
            lib = []
            for root in TAG_ROOTS:
                for _ in range(r.randint(6, 14)):
                    child = r.choice(self.vocab)
                    if len(child) < 3:
                        child += "ko"
                    lib.append(f"{root}/{child}")
                    if r.random() < 0.55:
                        for leaf in r.sample(TAG_LEAVES, r.randint(2, 5)):
                            lib.append(f"{root}/{child}/{leaf}")
            r.shuffle(lib)
            cum, acc = [], 0.0
            for k in range(len(lib)):
                acc += 1.0 / (k + 1) ** 1.05
                cum.append(acc)
            self._tl = (lib, cum)
        return self._tl

    def pick_tag(self, r, block, flat_ok=True):
        """Tag hierarchique (zipf sur la bibliotheque globale, longue traine locale au bloc) ou, parfois, plat."""
        lib, cum = self._taglib()
        if flat_ok and r.random() < 0.12:
            return r.choice(GLOBAL_TAGS)
        t = lib[min(len(lib) - 1, bisect.bisect_left(cum, r.random() * cum[-1]))]
        if r.random() < 0.10:
            t = "/".join(t.split("/")[:2]) + f"/b{block:03d}x{r.randrange(25)}"
        return t

    # -- petits blocs -----------------------------------------------------------------------------
    def _wiki(self, r, i, block):
        stem = self.note_stem(self.pick_note(r, i, block))
        k = r.random()
        if k < 0.35:
            return f"[[{stem}|{r.choice(WORDS)}]]"
        if k < 0.7:
            return f"[[{stem}#Résumé]]"
        if k < 0.85:
            return f"[[{stem}#Résumé|{r.choice(WORDS)}]]"
        return f"[[{stem}]]"

    def _sentence(self, r, i, block, ntags=0, nlinks=0):
        bits = [self.words(r, r.randint(4, 9))]
        for _ in range(ntags):
            bits.append("#" + self.pick_tag(r, block))
            bits.append(self.words(r, r.randint(2, 6)))
        for _ in range(nlinks):
            bits.append(self._wiki(r, i, block))
            bits.append(self.words(r, r.randint(1, 4)))
        return " ".join(bits)

    def f_tag_para(self, r, i, block, fp):
        return self._sentence(r, i, block, r.randint(1, 3), int(r.random() < 0.25)) + "\n"

    def f_tag_list(self, r, i, block, fp):
        out = [f"- {self._sentence(r, i, block, 1)}"]
        for _ in range(r.randint(1, 3)):
            out.append(f"  - {self.words(r, r.randint(3, 7))}")
            if r.random() < 0.3:
                out.append(f"    - {self.words(r, r.randint(2, 5))} #{self.pick_tag(r, block)}")
        out.append("- " + self.words(r, 4))
        return "\n".join(out) + "\n"

    def f_tag_heading(self, r, i, block, fp):
        return f"{'#' * r.randint(2, 4)} {r.choice(WORDS).capitalize()} {self.words(r, 2)} #{self.pick_tag(r, block)}\n"

    def f_table(self, r, i, block, fp, tag=False):
        al = [r.choice([":---", "---:", ":---:", "---"]) for _ in range(3)]
        rows = ["| Élément | État | Valeur |", "| " + " | ".join(al) + " |"]
        for _ in range(r.randint(2, 6)):
            c3 = f"#{self.pick_tag(r, block)}" if tag and r.random() < 0.5 else str(r.randint(1, 999))
            rows.append(f"| {r.choice(self.vocab)} | {r.choice(['ok', 'à faire', 'bloqué'])} | {c3} |")
        if tag:
            rows[-1] = rows[-1][:-1] + f"#{self.pick_tag(r, block)} |"
        return "\n".join(rows) + "\n"

    def f_table_tag(self, r, i, block, fp):
        return self.f_table(r, i, block, fp, tag=True)

    def f_callout(self, r, i, block, fp, tag=False):
        kind = r.choice(CALLOUT_KINDS)
        fold = r.choice(["", "", "-", "+"]) if r.random() < 0.5 else ""
        if kind == "info" and r.random() < 0.5:
            fold = "-"
        out = [f"> [!{kind}]{fold} {r.choice(WORDS).capitalize()} {self.words(r, 2)}"]
        for k in range(r.randint(1, 3)):
            t = " #" + self.pick_tag(r, block) if tag and k == 0 else ""
            out.append(f"> {self.words(r, r.randint(5, 12))}{t}")
        return "\n".join(out) + "\n"

    def f_callout_tag(self, r, i, block, fp):
        if r.random() < 0.4:
            return f"> {self._sentence(r, i, block, 1)}\n"
        return self.f_callout(r, i, block, fp, tag=True)

    def f_code(self, r, i, block, fp):
        lang = r.choice(CODE_LANGS)
        v, w = r.choice(self.vocab), r.choice(self.vocab)
        n = r.randint(1, 99)
        body = {
            "js": f"// {w}\nconst {v} = items.filter((x) => x.size > {n});\nconsole.log({v}.length);",
            "python": f"# {w}\ndef {v}(values):\n    return sum(v for v in values if v > {n})\n\nprint({v}([1, {n}, 200]))",
            "rust": f"// {w}\nfn {v}(xs: &[u32]) -> u32 {{\n    xs.iter().filter(|&&x| x > {n}).sum()\n}}",
            "bash": f"# {w}\nfor f in *.md; do\n  echo \"$f\"\ndone | sort > {v}.txt",
            "json": json.dumps({"name": v, "count": n, "tags": [w], "enabled": r.random() < 0.5}, indent=2),
        }[lang]
        return f"```{lang}\n{body}\n```\n"

    def f_tasks(self, r, i, block, fp):
        out = []
        for _ in range(r.randint(2, 5)):
            out.append(f"- [{'x' if r.random() < 0.4 else ' '}] {self.words(r, r.randint(3, 6))}")
            if r.random() < 0.4:
                out.append(f"    - [{'x' if r.random() < 0.4 else ' '}] {self.words(r, 3)}")
        return "\n".join(out) + "\n"

    def f_nested(self, r, i, block, fp):
        out = []
        for _ in range(r.randint(2, 4)):
            out.append(f"- {self.words(r, r.randint(2, 5))}")
            for _ in range(r.randint(0, 2)):
                out.append(f"\t- {self.words(r, r.randint(2, 4))}")
                if r.random() < 0.3:
                    out.append(f"\t\t- {self.words(r, 2)}")
        out.append(f"1. {self.words(r, 3)}\n2. {self.words(r, 3)}")
        return "\n".join(out) + "\n"

    def f_links(self, r, i, block, fp):
        return self._sentence(r, i, block, 0, r.randint(1, 3)) + "\n"

    def f_resume(self, r, i, block, fp):
        return f"## Résumé\n\n{self.words(r, r.randint(8, 20))}\n"

    def f_traps(self, r, i, block, fp):
        stem = self.note_stem(self.pick_note(r, i, block))
        k = r.randint(0, 3)
        if k == 0:
            return f"Voir le ticket #{r.randint(100, 999)} et https://example.com/guide#section pour le détail.\n"
        if k == 1:
            return "Le préprocesseur utilise `#define VALEUR` et `#include` en ligne.\n"
        if k == 2:
            return f"Voir [[{stem}#Résumé]] ou [[{stem}#Titre de section|{r.choice(WORDS)}]] (étape #{r.randint(1, 20)}).\n"
        return "```bash\n#!/bin/bash\n# commentaire #pas-un-tag\necho \"#pas-un-tag\"\n```\n"

    def f_img_md(self, r, i, block, fp):
        j = self.pick_png_index(r, block)
        if j is None:
            return None
        name, ext = self.att_name(j)
        sub = self.att_folder(j, ext)
        rel = "../" * len(fp) + "/".join(sub + (name,))
        return f"![{r.choice(WORDS)}]({rel.replace(' ', '%20')})\n"

    def f_mermaid(self, r, i, block, fp):
        kind = MERMAID_TYPES[(i // 14) % len(MERMAID_TYPES)]      # types equilibres, independants du --scale
        return "```mermaid\n" + self.mermaid(r, kind) + "\n```\n"

    def mermaid(self, r, kind):
        w = lambda: unicodedata.normalize("NFKD", r.choice(self.vocab)).encode("ascii", "ignore").decode().capitalize()
        n = r.randint(3, 5)
        if kind == "flowchart":
            out = [f"flowchart {r.choice(['LR', 'TD'])}", f"  A[{w()}] --> B{{{w()} ?}}"]
            out += [f"  B -->|oui| C[{w()}]", f"  B -->|non| D([{w()}])", "  D --> A"]
            return "\n".join(out)
        if kind == "sequenceDiagram":
            a, b = w(), w()
            return (f"sequenceDiagram\n  participant A as {a}\n  participant B as {b}\n  A->>B: {w()}\n"
                    f"  B-->>A: {w()}\n  Note over A,B: {w()}\n  loop {w()}\n    A->>B: ping\n  end")
        if kind == "classDiagram":
            a, b, c = w(), w(), w()
            return (f"classDiagram\n  class {a} {{\n    +String nom\n    +int total\n    +lancer() void\n  }}\n"
                    f"  class {b} {{\n    +calculer() int\n  }}\n  {a} <|-- {b}\n  {a} \"1\" --> \"*\" {c} : contient")
        if kind == "stateDiagram-v2":
            a, b = w(), w()
            return (f"stateDiagram-v2\n  [*] --> {a}\n  {a} --> {b} : {r.choice(WORDS)}\n"
                    f"  {b} --> {a} : retour\n  {b} --> [*]")
        if kind == "erDiagram":
            a, b = w().upper(), w().upper()
            return (f"erDiagram\n  {a} ||--o{{ {b} : contient\n  {a} {{\n    int id PK\n    string nom\n  }}\n"
                    f"  {b} {{\n    int id PK\n    int {a.lower()}_id FK\n  }}")
        if kind == "gantt":
            return (f"gantt\n  title {w()} {w()}\n  dateFormat YYYY-MM-DD\n  section {w()}\n"
                    f"  Etape un :a1, 2026-01-05, {r.randint(5, 15)}d\n  Etape deux :a2, after a1, {r.randint(3, 10)}d\n"
                    f"  section {w()}\n  Etape trois :a3, after a2, {r.randint(3, 10)}d")
        if kind == "pie":
            return f"pie title {w()}\n" + "\n".join(f'  "{w()}" : {r.randint(5, 60)}' for _ in range(n))
        if kind == "mindmap":
            out = ["mindmap", f"  root(({w()}))"]
            for _ in range(n - 1):
                out.append(f"    {w()}")
                for _ in range(r.randint(1, 2)):
                    out.append(f"      {r.choice(self.vocab)}")
            return "\n".join(out)
        # wardley-beta
        names = [w() for _ in range(4)]
        out = ["wardley-beta", f"title {w()}", f"anchor {names[0]} [0.95, 0.65]"]
        for k, nm in enumerate(names[1:], 1):
            out.append(f"component {nm} [{0.9 - k * 0.2:.2f}, {r.uniform(0.2, 0.9):.2f}]")
        out += [f"{names[0]} -> {names[1]}", f"{names[1]} -> {names[2]}", f"{names[2]} -> {names[3]}",
                f"evolve {names[3]} 0.90"]
        return "\n".join(out)

    FEATURES = [("f_tag_para", 0.55), ("f_tag_list", 0.30), ("f_tag_heading", 0.25), ("f_table_tag", 0.20),
                ("f_callout_tag", 0.20), ("f_traps", 0.25), ("f_callout", 0.35), ("f_table", 0.25),
                ("f_code", 0.25), ("f_tasks", 0.30), ("f_nested", 0.30), ("f_links", 0.15),
                ("f_resume", 0.40), ("f_img_md", 0.12)]

    def rich_blocks(self, r, i, block, fp, target):
        """Liste de blocs (chaines) a inserer ; leur taille totale reste sous ~65 % du budget de la note."""
        if target < 600:
            return []
        scale = min(2.2, (target / 2500) ** 0.4)
        budget = target * 0.65
        out, used = [], 0
        feats = list(self.FEATURES)
        r.shuffle(feats)
        if target >= 1200 and i % 14 == 5:          # ~5 % des notes (celles d'au moins 1200 o)
            feats.append(("f_mermaid", 1.0))
        for name, p in feats:
            if r.random() < min(0.95, p * scale):
                b = getattr(self, name)(r, i, block, fp)
                if b is None:
                    continue
                sz = len(b.encode())
                if used + sz > budget:
                    continue
                out.append(b)
                used += sz
        return out

    # -- note pire cas ---------------------------------------------------------------------------
    def daily_entry(self, r, i, block, day):
        fp = ("01_WORK",)
        out = [f"## {day.isoformat()} ({DAYS_FR[day.weekday()]})\n"]
        out.append(f"- {self._sentence(r, i, block, r.randint(0, 2), 1)}\n")
        out.append(self.f_tag_list(r, i, block, fp))
        out.append(self.f_tasks(r, i, block, fp))
        out.append(self.words(r, r.randint(40, 120)) + "\n")
        n = day.toordinal()
        if n % 11 == 0:
            out.append(self.f_table(r, i, block, fp, tag=r.random() < 0.5))
        if n % 9 == 0:
            out.append(self.f_code(r, i, block, fp))
        if n % 17 == 0:
            out.append(self.f_callout(r, i, block, fp, tag=True))
        if n % 5 == 0:
            out.append(self.f_links(r, i, block, fp))
        if n % 23 == 0:
            out.append(self.f_traps(r, i, block, fp))
        return "\n".join(out)

    def render_daily(self, r, i, block, head_len):
        target = DAILY_BYTES - head_len
        out, size, day = ["## Résumé\n\nJournal synthétique, note pire cas.\n"], 0, DAILY_START
        while True:
            e = self.daily_entry(r, i, block, day)
            sz = len(e.encode()) + 1
            if size + sz > target:
                break
            out.append(e)
            size += sz
            day += datetime.timedelta(days=1)
        return "\n".join(out)

    # -- canvas (bloc 0) -------------------------------------------------------------------------
    def _canvas_targets(self, r, k_notes, k_png):
        notes = []
        for t in range(1, min(self.N, BASE_NOTES)):
            if not self.is_excal(t):
                notes.append("/".join(self.note_folder(t) + (self.note_stem(t) + ".md",)))
            if len(notes) >= k_notes:
                break
        notes.insert(0, "/".join(self.note_folder(0) + (self.note_stem(0) + ".md",)))
        pngs = []
        for j in range(min(self.A, BASE_ATT)):
            name, ext = self.att_name(j)
            if ext == ".png":
                pngs.append("/".join(self.att_folder(j, ext) + (name,)))
            if len(pngs) >= k_png:
                break
        return notes, pngs

    def canvas_simple(self):
        r = rng(self.seed, "canvas", "simple")
        notes, pngs = self._canvas_targets(r, 2, 1)
        nodes = [
            {"id": "s1", "type": "text", "text": f"# Idée centrale\n\n{self.words(r, 8)} #projet/alpha/revue",
             "x": 0, "y": 0, "width": 300, "height": 140, "color": "4"},
            {"id": "s2", "type": "text", "text": f"- {self.words(r, 4)}\n- {self.words(r, 4)}",
             "x": 400, "y": -100, "width": 260, "height": 120},
            {"id": "s3", "type": "file", "file": notes[0], "x": 400, "y": 80, "width": 400, "height": 300},
            {"id": "s4", "type": "file", "file": notes[1] if len(notes) > 1 else notes[0],
             "x": -420, "y": 40, "width": 300, "height": 200},
            {"id": "s5", "type": "text", "text": self.words(r, 10), "x": 0, "y": 260, "width": 300, "height": 100,
             "color": "1"},
        ]
        if pngs:
            nodes.append({"id": "s6", "type": "file", "file": pngs[0], "x": -420, "y": -260, "width": 256,
                          "height": 256})
        else:
            nodes.append({"id": "s6", "type": "text", "text": self.words(r, 5), "x": -420, "y": -260,
                          "width": 256, "height": 100})
        edges = [
            {"id": "e1", "fromNode": "s1", "fromSide": "right", "toNode": "s2", "toSide": "left"},
            {"id": "e2", "fromNode": "s1", "fromSide": "right", "toNode": "s3", "toSide": "left", "label": "détail"},
            {"id": "e3", "fromNode": "s4", "fromSide": "right", "toNode": "s1", "toSide": "left"},
            {"id": "e4", "fromNode": "s1", "fromSide": "bottom", "toNode": "s5", "toSide": "top", "toEnd": "none"},
            {"id": "e5", "fromNode": "s6", "fromSide": "bottom", "toNode": "s4", "toSide": "top"},
        ]
        return json.dumps({"nodes": nodes, "edges": edges}, indent="\t", ensure_ascii=False).encode()

    def canvas_dense(self, n_nodes=150):
        r = rng(self.seed, "canvas", "dense")
        notes, pngs = self._canvas_targets(r, 40, 15)
        groups, nodes, edges = [], [], []
        ng = 6
        per = n_nodes // ng
        cols = 5
        cw, ch, gap = 280, 160, 40
        gw = cols * (cw + gap) + gap
        gh = math.ceil(per / cols) * (ch + gap) + gap + 60
        ids = []
        for g in range(ng):
            gx, gy = (g % 3) * (gw + 120), (g // 3) * (gh + 120)
            groups.append({"id": f"g{g}", "type": "group", "label": f"Groupe {g + 1} {r.choice(WORDS)}",
                           "x": gx, "y": gy, "width": gw, "height": gh, "color": str(g % 6 + 1)})
            for k in range(per):
                nid = f"n{g}_{k}"
                ids.append(nid)
                x = gx + gap + (k % cols) * (cw + gap)
                y = gy + 60 + gap + (k // cols) * (ch + gap)
                kind = r.random()
                nd = {"id": nid, "x": x, "y": y, "width": cw, "height": ch}
                if kind < 0.55:
                    tag = f" #{self.pick_tag(r, 0)}" if r.random() < 0.4 else ""
                    nd.update(type="text", text=f"**{self.words(r, 2)}**\n\n{self.words(r, r.randint(5, 18))}{tag}")
                elif kind < 0.8 and notes:
                    nd.update(type="file", file=r.choice(notes))
                elif kind < 0.92 and pngs:
                    nd.update(type="file", file=r.choice(pngs))
                else:
                    nd.update(type="link", url=f"https://example.com/{r.choice(self.vocab)}/{r.randint(1, 99)}")
                if r.random() < 0.35:
                    nd["color"] = str(r.randint(1, 6)) if r.random() < 0.7 else f"#{r.randrange(0x1000000):06x}"
                nodes.append(nd)
        sides = ["top", "right", "bottom", "left"]
        for e in range(200):
            a, b = r.sample(ids, 2)
            ed = {"id": f"e{e}", "fromNode": a, "fromSide": r.choice(sides), "toNode": b,
                  "toSide": r.choice(sides)}
            if r.random() < 0.55:
                ed["label"] = r.choice(WORDS)
            if r.random() < 0.3:
                ed["color"] = str(r.randint(1, 6))
            if r.random() < 0.1:
                ed["toEnd"] = "none"
            edges.append(ed)
        return json.dumps({"nodes": groups + nodes, "edges": edges}, indent="\t", ensure_ascii=False).encode()


class Gen(Content):
    def __init__(self, a):
        self.seed = a.seed
        self.N = a.notes if a.notes is not None else round(BASE_NOTES * a.scale)
        self.A = a.attachments if a.attachments is not None else round(BASE_ATT * a.scale)
        self.flat_assets = not a.no_flat_assets
        self.attach_factor = a.attach_factor
        self.burst = a.mtime_burst
        self.now = time.time()
        r = rng(self.seed, "vocab")
        syl = ["ba", "ko", "ri", "ten", "lu", "mar", "sol", "vi", "dan", "pe", "cor", "ni", "tal", "ou",
               "eux", "ment", "ion", "aire", "ac", "el", "ré", "dé", "con", "pro"]
        vocab = set()
        while len(vocab) < 600:
            vocab.add("".join(r.choices(syl, k=r.randint(1, 4))))
        self.vocab = sorted(vocab)
        self.avg_word = sum(len(w.encode()) for w in self.vocab) / len(self.vocab) + 1.07
        self._folders = {}

    # ---- structure : dossiers d'un bloc -------------------------------------------------------
    def block_folders(self, block):
        """Liste des chemins (tuples) d'un bloc : racines + sous-dossiers propres au bloc."""
        if block not in self._folders:
            r = rng(self.seed, "folders", block)
            paths = [(t,) for t in TOP_NAMES]
            for k in range(FOLDERS_PER_BLOCK - len(TOP_NAMES)):
                top = r.choices(TOP_NAMES, TOP_WEIGHTS)[0]
                cands = [p for p in paths if p[0] == top and len(p) < MAX_DEPTH]
                parent = r.choice(cands)
                paths.append(parent + (f"{r.choice(WORDS)} {block:03d}-{k:02d}",))
            by_top = {}
            for p in paths:
                by_top.setdefault(p[0], []).append(p)
            self._folders[block] = (paths, by_top)
        return self._folders[block]

    # ---- notes ----------------------------------------------------------------------------------
    def is_excal(self, i):
        return i != 0 and rng(self.seed, "kind", i).random() < EXCAL_RATIO

    def note_stem(self, i):
        if i == 0:
            return "__DAILY NOTES SYNTH"
        r = rng(self.seed, "name", i)
        pre = "__" if r.random() < 0.03 else ""
        stem = f"{pre}{r.choice(WORDS).capitalize()} {r.choice(WORDS)} {i:05d}"
        return stem + (".excalidraw" if self.is_excal(i) else "")

    def note_folder(self, i):
        if i == 0:
            return ("01_WORK",)
        _, by_top = self.block_folders(i // BASE_NOTES)
        r = rng(self.seed, "nfolder", i)
        return r.choice(by_top[r.choices(TOP_NAMES, TOP_WEIGHTS)[0]])

    def note_bytes(self, i):
        if i == 0:
            return DAILY_BYTES
        b = int(2536 * math.exp(rng(self.seed, "size", i).gauss(0, 1.29)))
        return max(80, min(b, 330_000))

    # ---- pieces jointes --------------------------------------------------------------------------
    def att_ext(self, j):
        x = rng(self.seed, "aext", j).random() * ATT_EXT_TOTAL
        acc = 0
        for ext, w in ATT_EXT:
            acc += w
            if x < acc:
                return ext
        return ".png"

    def att_name(self, j):
        ext = self.att_ext(j)
        block, k = divmod(j, BASE_ATT)
        if ext in (".png", ".jpg"):
            return f"Pasted image {block:03d}{k:04d}{ext}", ext
        return f"Doc {rng(self.seed, 'aname', j).choice(WORDS)} {block:03d}-{k:04d}{ext}", ext

    def att_folder(self, j, ext):
        block = j // BASE_ATT
        if ext in (".png", ".jpg"):
            return ("_ASSETS",) if self.flat_assets else ("_ASSETS", f"{block:03d}")
        paths, _ = self.block_folders(block)
        return rng(self.seed, "afolder", j).choice(paths)

    def att_size(self, ext, r):
        avg = ATT_REAL_AVG[ext] * self.attach_factor
        return max(64, int(avg * math.exp(r.gauss(-0.4, 0.9))))

    def pick_png_index(self, r, block):
        lo, hi = block * BASE_ATT, min((block + 1) * BASE_ATT, self.A)
        if hi <= lo:
            return None
        for _ in range(25):
            j = r.randrange(lo, hi)
            if self.att_ext(j) == ".png":
                return j
        return None

    def pick_png(self, r, block):
        j = self.pick_png_index(r, block)
        return None if j is None else self.att_name(j)[0]

    # ---- contenu ------------------------------------------------------------------------------
    def words(self, r, n):
        ws = r.choices(self.vocab, k=n)
        out, cap = [], True
        for w in ws:
            if cap:
                w = w.capitalize()
                cap = False
            out.append(w)
            if r.random() < 0.07:
                out[-1] += "."
                cap = True
        return " ".join(out)

    def links_for(self, r, i, block):
        """Lignes de liens (wikilinks et embeds) pour la note i."""
        lines = []
        if i == 0:
            n_total, n_wiki_forced = 148, 128
        elif r.random() < 0.32:
            n_total, n_wiki_forced = 1 + int(r.expovariate(1 / 3.6)), 0
        else:
            return lines
        for n in range(n_total):
            embed = (n >= n_wiki_forced) and r.random() < 0.51
            if embed:
                tgt = self.pick_png(r, block) if r.random() < 0.95 else None
                if tgt is None:
                    tgt = self.note_stem(self.pick_note(r, i, block))
                lines.append(f"![[{tgt}]]")
            elif r.random() < 0.087:
                lines.append(f"- [[Missing {block:03d}-{r.randrange(40)}]]")
            else:
                stem = self.note_stem(self.pick_note(r, i, block))
                alias = f"|{r.choice(WORDS)}" if r.random() < 0.2 else ""
                lines.append(f"- [[{stem}{alias}]]")
        return lines

    def pick_note(self, r, i, block):
        if r.random() < 0.10:
            t = r.randrange(0, min(30, self.N))            # hubs
        else:
            t = block * BASE_NOTES + r.randrange(BASE_NOTES)
        t = min(t, self.N - 1)
        return t if t != i else (t + 1 if (t + 1) % BASE_NOTES else t - 1)

    def render_note(self, i):
        r = rng(self.seed, "note", i)
        r2 = rng(self.seed, "rich", i)
        block = i // BASE_NOTES
        fp = self.note_folder(i)
        target = self.note_bytes(i)
        if self.is_excal(i):
            size = int(r.uniform(3_000, 45_000))
            payload = base64.b64encode(r.randbytes(int(size * 0.75))).decode()
            return ("---\nexcalidraw-plugin: parsed\ntags: [excalidraw]\n---\n"
                    "==Switch to EXCALIDRAW VIEW in the MORE OPTIONS menu of this document.==\n\n"
                    "# Excalidraw Data\n\n## Text Elements\n"
                    f"{self.words(r, 20)} ^abc123\n\n## Drawing\n```compressed-json\n{payload}\n```\n%%\n").encode()
        parts = []
        # frontmatter : 96 % des notes, 1 a 9 cles
        if i == 0 or r.random() < 0.96:
            nk = max(1, min(9, round(r.gauss(5.4, 2))))
            keys = r.sample(FM_KEYS, nk)
            fm = ["---"]
            for k in keys:
                if k == "tags":
                    tl = [self.pick_tag(r2, block) for _ in range(r2.randint(1, 4))]
                    if r2.random() < 0.5:
                        fm.append("tags: [" + ", ".join(tl) + "]")
                    else:
                        fm.append("tags:\n" + "\n".join(f"  - {t}" for t in tl))
                elif k in ("created", "modified", "due"):
                    fm.append(f"{k}: 20{r.randint(24, 26)}-{r.randint(1, 12):02d}-{r.randint(1, 28):02d}")
                elif k == "aliases":
                    fm.append(f"aliases: [{r.choice(WORDS)}]")
                else:
                    fm.append(f"{k}: {r.choice(WORDS)}")
            fm.append("---\n")
            parts.append("\n".join(fm))
        # sections : titres, taches, tags, liens (leur taille est deduite du budget du corps)
        n_head = min(475, int(target / 500 * r.uniform(0.5, 1.5)))
        heads = [f"{'#' * r.randint(1, 4)} {r.choice(WORDS).capitalize()} {self.words(r, 2)}\n"
                 for _ in range(n_head)]
        extras = list(self.links_for(r, i, block))
        if i != 0:
            extras += self.rich_blocks(r2, i, block, fp, target)
        if r.random() < 0.30:
            for _ in range(1 + int(r.expovariate(1 / 5.0))):
                extras.append(f"- [{'x' if r.random() > 0.86 else ' '}] {self.words(r, 6)}")
        n_tags = int(r.expovariate(1 / 2.0))
        if n_tags:
            extras.append(" ".join("#" + (r.choice(GLOBAL_TAGS) if r.random() < 0.6 else f"b{block}-t{r.randrange(60)}")
                                   for _ in range(n_tags)))
        if i == 0:
            head = "".join(parts) + "## Liens\n\n" + "\n".join(extras) + "\n\n"
            return (head + self.render_daily(r2, i, block, len(head.encode()))).encode("utf-8")
        overhead = sum(len(p.encode()) for p in parts) + sum(len(h.encode()) for h in heads) + \
            sum(len(x.encode()) + 1 for x in extras)
        words_total = max(3, int((target - overhead) / self.avg_word))
        sections = max(1, n_head)
        per = max(1, words_total // sections)
        blocks = [(heads[s] if heads else "") + self.words(r, per) + "\n" for s in range(sections)]
        for x in extras:
            blocks.insert(r.randrange(len(blocks) + 1), x + "\n")
        parts.append("\n".join(blocks))
        return "".join(parts).encode("utf-8")

    def render_att(self, j, ext):
        r = rng(self.seed, "att", j)
        n = self.att_size(ext, r)
        if ext == ".png":
            return make_png(r, n)
        if ext == ".jpg":
            return b"\xff\xd8\xff\xe0" + r.randbytes(n)
        if ext == ".pdf":
            return b"%PDF-1.4\n" + r.randbytes(n)
        if ext in (".docx", ".odt"):
            return b"PK\x03\x04" + r.randbytes(n)
        if ext == ".gdoc":
            return json.dumps({"url": "https://docs.google.com/document/d/synthetic",
                               "doc_id": "synthetic", "email": "user@example.com"}).encode()
        if ext == ".canvas":
            m = max(1, n // 200)
            nodes = [{"id": f"n{k}", "type": "text", "text": self.words(r, 5), "x": (k % 8) * 260,
                      "y": (k // 8) * 120, "width": 240, "height": 100} for k in range(m)]
            edges = [{"id": f"e{k}", "fromNode": f"n{k}", "toNode": f"n{k + 1}"} for k in range(m - 1)]
            return json.dumps({"nodes": nodes, "edges": edges}).encode()
        # .excalidraw / .excalidrawlib
        return json.dumps({"type": "excalidraw", "version": 2, "elements": [],
                           "blob": base64.b64encode(r.randbytes(int(n * 0.75))).decode()}).encode()


# ---- ecriture -----------------------------------------------------------------------------------
class Writer:
    def __init__(self, root, force, now, burst, seed):
        self.root, self.force, self.now, self.burst, self.seed = root, force, now, burst, seed
        self.made = set()
        self.written = self.skipped = 0
        self.bytes = 0

    def put(self, rel, data_fn):
        path = self.root.joinpath(*rel)
        if path.exists() and not self.force:
            self.skipped += 1
            return
        d = path.parent
        if d not in self.made:
            d.mkdir(parents=True, exist_ok=True)
            self.made.add(d)
        data = data_fn()
        path.write_bytes(data)
        r = rng(self.seed, "mtime", "/".join(rel))
        age = r.uniform(0, 22 * 86400) if r.random() < self.burst else r.uniform(22 * 86400, 730 * 86400)
        t = self.now - age
        os.utime(path, (t, t))
        self.written += 1
        self.bytes += len(data)


def write_obsidian_config(w, g):
    cfg = {
        "app.json": {"attachmentFolderPath": "_ASSETS", "newFileLocation": "folder",
                     "newFileFolderPath": "00_INBOX", "alwaysUpdateLinks": True, "readableLineLength": True},
        "core-plugins.json": {k: True for k in ["file-explorer", "global-search", "graph", "backlink", "canvas",
                                                "outgoing-link", "tag-pane", "properties", "page-preview",
                                                "command-palette", "outline", "word-count", "bookmarks"]},
        "community-plugins.json": [],
        "appearance.json": {"theme": "obsidian", "baseFontSize": 14},
        "community-plugins (conflict 2026-01-01-00-00-00).json": [],
    }
    first = "/".join(g.note_folder(0) + (g.note_stem(0) + ".md",))
    leaf = {"id": "leaf0", "type": "leaf", "state": {"type": "markdown", "state": {"file": first}}}
    cfg["workspace.json"] = {
        "main": {"id": "main0", "type": "split", "direction": "vertical",
                 "children": [{"id": "tabs0", "type": "tabs", "children": [leaf]}]},
        "active": "leaf0", "lastOpenFiles": [first]}
    for name, obj in cfg.items():
        w.put((".obsidian", name), lambda o=obj: json.dumps(o, indent=2).encode())


def write_dot_noise(w, g, n):
    r = rng(g.seed, "noise")
    pkgs = max(1, n // 40)
    exts = [".js", ".ts", ".map", ".cjs", ".cts"]
    for k in range(n):
        pkg = f"pkg{k % pkgs:03d}"
        sub = "dist" if k % 2 else "src"
        w.put((".opencode", "node_modules", pkg, sub, f"f{k:05d}{exts[k % 5]}"),
              lambda: ("// synthetic\n" + g.words(r, r.randint(50, 500))).encode())
    for k in range(32):
        w.put((".trash", f"deleted {k:02d}.md"), lambda: b"deleted note\n")


def generate(a):
    root = Path(a.out)
    g = Gen(a)
    w = Writer(root, a.force, g.now, g.burst, g.seed)
    t0 = time.time()
    root.mkdir(parents=True, exist_ok=True)
    print(f"Echelle {a.scale}x : {g.N} notes, {g.A} pieces jointes, "
          f"{math.ceil(max(g.N / BASE_NOTES, g.A / BASE_ATT))} bloc(s), seed '{g.seed}' -> {root}")
    if a.dry_run:
        return
    write_obsidian_config(w, g)
    for i in range(g.N):
        stem = g.note_stem(i)
        w.put(g.note_folder(i) + (stem + ".md",), lambda i=i: g.render_note(i))
        if (i + 1) % 2000 == 0:
            print(f"  notes {i + 1}/{g.N}", flush=True)
    for j in range(g.A):
        name, ext = g.att_name(j)
        w.put(g.att_folder(j, ext) + (name,), lambda j=j, e=ext: g.render_att(j, e))
        if (j + 1) % 2000 == 0:
            print(f"  pieces jointes {j + 1}/{g.A}", flush=True)
    if g.N > 0:
        w.put(("01_WORK", "Canvas simple.canvas"), g.canvas_simple)
        w.put(("01_WORK", "Canvas dense.canvas"), g.canvas_dense)
    if a.dot_noise > 0:
        write_dot_noise(w, g, a.dot_noise)
    dt = time.time() - t0
    manifest = {"generator": "gen-vault.py", "version": VERSION, "seed": g.seed, "scale": a.scale,
                "notes": g.N, "attachments": g.A, "attachFactor": g.attach_factor, "flatAssets": g.flat_assets,
                "dotNoise": a.dot_noise, "mtimeBurst": g.burst,
                "lastRun": {"written": w.written, "skipped": w.skipped, "bytes": w.bytes,
                            "seconds": round(dt, 1)}}
    md = root / ".rocksy-synth"
    md.mkdir(exist_ok=True)
    (md / "manifest.json").write_text(json.dumps(manifest, indent=2))
    print(f"Ecrits : {w.written}, ignores (deja presents) : {w.skipped}, {w.bytes / 1e6:.1f} Mo en {dt:.1f} s")


# ---- statistiques (meme perimetre que vault-kpis.ps1) ------------------------------------------
RE_WIKI = re.compile(r"(!?)\[\[([^\]|#^\r\n]+)(?:[#^][^\]|\r\n]*)?(?:\|[^\]\r\n]*)?\]\]")


def stats(root):
    root = Path(root)
    files, dirs = [], 0
    stack = [root]
    while stack:
        d = stack.pop()
        with os.scandir(d) as it:
            for e in it:
                if e.name.startswith("."):
                    continue
                if e.is_dir(follow_symlinks=False):
                    dirs += 1
                    stack.append(Path(e.path))
                else:
                    files.append(Path(e.path))
    mds = [f for f in files if f.suffix.lower() == ".md"]
    names = {f.stem.lower() for f in mds} | {f.name.lower() for f in files if f.suffix.lower() != ".md"}
    sizes = sorted(f.stat().st_size for f in mds)
    wiki = emb = unres = words = fm = nolinks = 0
    for f in mds:
        t = f.read_text(encoding="utf-8", errors="replace")
        words += len(t.split())
        fm += t.startswith("---")
        n = 0
        for m in RE_WIKI.finditer(t):
            n += 1
            if m.group(1):
                emb += 1
            else:
                wiki += 1
            tgt = m.group(2).strip().lower()
            tgt = tgt[:-3] if tgt.endswith(".md") else tgt
            if tgt not in names:
                unres += 1
        nolinks += n == 0
    pct = lambda p: sizes[min(len(sizes) - 1, math.ceil(p * len(sizes)) - 1)] if sizes else 0
    total = sum(f.stat().st_size for f in files)
    print("--- Statistiques (hors dossiers a point) ---")
    print(f"fichiers {len(files)} (md {len(mds)}, autres {len(files) - len(mds)}), dossiers {dirs}, {total / 1e6:.1f} Mo")
    print(f"md : somme {sum(sizes) / 1e6:.2f} Mo, mediane {int(statistics.median(sizes)) if sizes else 0} o, "
          f"p95 {pct(0.95)} o, max {sizes[-1] if sizes else 0} o, mots {words}")
    print(f"frontmatter {fm}, wikilinks {wiki}, embeds {emb}, non resolus {unres}, notes sans lien sortant {nolinks}")
    print("(reference 1x : 953 fichiers, 488 md, 94 dossiers, md 2,94 Mo, mediane 2536 o, p95 20999 o, "
          "378172 mots, 468 frontmatter, 362 wiki, 381 embeds, 65 non resolus, 330 sans lien)")
    print_rich_stats(mds)
    big = max(mds, key=lambda f: f.stat().st_size) if mds else None
    if big is not None:
        print(f"note pire cas : {big.relative_to(root)} ({big.stat().st_size} o, hors total median ci-dessus)")
    canv = sorted(f for f in files if f.suffix == ".canvas")
    nc = []
    for f in canv:
        try:
            d = json.loads(f.read_text(encoding="utf-8"))
            nc.append((f.name, len(d["nodes"]), len(d["edges"])))
        except Exception as e:  # noqa
            nc.append((f.name, "INVALIDE", str(e)))
    named = [x for x in nc if x[0].startswith("Canvas ")]
    print(f".canvas : {len(canv)} fichiers ; nommes : {named}")
    pngs = [f for f in files if f.suffix == ".png"]
    ok = sum(1 for f in pngs if f.read_bytes()[:8] == b"\x89PNG\r\n\x1a\n" and f.read_bytes()[-12:-8] == b"\x00\x00\x00\x00"
             and f.read_bytes()[-8:-4] == b"IEND")
    print(f"png : {len(pngs)} fichiers, {ok} avec signature + IEND corrects")

# ---- statistiques enrichies (v2) ---------------------------------------------------------------
RE_TAG = re.compile(r"(?<![\w/&#])#([^\W\d][\w\-]*(?:/[\w\-]+)*)")
RE_STRIP = re.compile(r"`[^`\n]*`|https?://\S+|\[\[[^\]\n]*\]\]|\]\([^)\n]*\)")
RE_LIST = re.compile(r"^(\s*)(?:[-*+]|\d+\.)\s")
RE_CALLOUT = re.compile(r"^> \[!(\w+)\]([+-]?)")
RE_TABSEP = re.compile(r"^\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?\s*$")
RE_IMG_WIKI = re.compile(r"!\[\[([^\]|#]+\.(?:png|jpg))")
RE_IMG_MD = re.compile(r"!\[[^\]]*\]\(([^)]+\.(?:png|jpg))\)")
MM_KINDS = ["flowchart", "sequenceDiagram", "classDiagram", "stateDiagram-v2", "erDiagram", "gantt", "pie",
            "mindmap", "wardley-beta"]


def frontmatter_tags(text):
    if not text.startswith("---\n"):
        return [], 0
    end = text.find("\n---", 4)
    if end < 0:
        return [], 0
    fm = text[4:end].split("\n")
    tags = []
    for k, ln in enumerate(fm):
        if ln.startswith("tags:"):
            rest = ln[5:].strip()
            if rest.startswith("["):
                tags += [t.strip() for t in rest.strip("[]").split(",") if t.strip()]
            else:
                for l2 in fm[k + 1:]:
                    if l2.startswith("  - "):
                        tags.append(l2[4:].strip())
                    else:
                        break
    return tags, end


def scan_note(text, acc):
    """Alimente acc (compteurs) pour une note : tags par emplacement, Mermaid, callouts, tableaux..."""
    ftags, _ = frontmatter_tags(text)
    for t in ftags:
        acc["tags"].append((t.lower(), "frontmatter"))
    body = text
    if text.startswith("---\n"):
        e = text.find("\n---", 4)
        body = text[e + 4:] if e >= 0 else text
    lines = body.split("\n")
    fence = None
    prev_table = False
    for k, ln in enumerate(lines):
        st = ln.lstrip()
        if fence is None and st.startswith("```"):
            fence = st[3:].strip()
            if fence == "mermaid":
                nxt = lines[k + 1].strip().split(" ")[0] if k + 1 < len(lines) else ""
                acc["mermaid"][nxt] = acc["mermaid"].get(nxt, 0) + 1
            elif fence:
                acc["code"][fence] = acc["code"].get(fence, 0) + 1
            continue
        if fence is not None:
            if st.startswith("```"):
                fence = None
            continue
        m = RE_CALLOUT.match(ln)
        if m:
            acc["callouts"][m.group(1)] = acc["callouts"].get(m.group(1), 0) + 1
            acc["callouts_fold"] += bool(m.group(2))
        if RE_TABSEP.match(ln.strip()) and k > 0 and lines[k - 1].lstrip().startswith("|"):
            acc["tables"] += 1
        for mm in RE_LIST.finditer(ln):
            mt = re.match(r"^\s*(?:[-*+]|\d+\.)\s\[([ xX])\]", ln)
            if mt:
                acc["tasks_done" if mt.group(1) != " " else "tasks_open"] += 1
            if len(mm.group(1)) > 0:
                acc["nested_items"] += 1
        acc["img_wiki"] += len(RE_IMG_WIKI.findall(ln))
        acc["img_md"] += len(RE_IMG_MD.findall(ln))
        acc["anchors"] += len(re.findall(r"\[\[[^\]|]+#[^\]|]+(?:\|[^\]]*)?\]\]", ln))
        acc["aliases"] += len(re.findall(r"\[\[[^\]|#]+(?:#[^\]|]+)?\|[^\]]+\]\]", ln))
        clean = RE_STRIP.sub(" ", ln)
        acc["numeric_hash"] += len(re.findall(r"(?<![\w/&#])#\d+\b", clean))
        found = RE_TAG.findall(clean)
        if not found:
            continue
        if st.startswith("|"):
            loc = "tableau"
        elif st.startswith(">"):
            loc = "citation/callout"
        elif re.match(r"^#{1,6}\s", st):
            loc = "titre"
        elif RE_LIST.match(ln):
            ind = len(RE_LIST.match(ln).group(1))
            nxt = next((l for l in lines[k + 1:] if l.strip()), "")
            nm = RE_LIST.match(nxt)
            loc = "liste avec enfants" if nm and len(nm.group(1)) > ind else "liste sans enfant"
        else:
            loc = "paragraphe"
        for t in found:
            acc["tags"].append((t.lower(), loc))


def print_rich_stats(files_md):
    acc = {"tags": [], "mermaid": {}, "code": {}, "callouts": {}, "callouts_fold": 0, "tables": 0,
           "tasks_done": 0, "tasks_open": 0, "nested_items": 0, "img_wiki": 0, "img_md": 0,
           "anchors": 0, "aliases": 0, "numeric_hash": 0}
    notes_mm = 0
    for f in files_md:
        t = f.read_text(encoding="utf-8", errors="replace")
        before = sum(acc["mermaid"].values())
        scan_note(t, acc)
        notes_mm += sum(acc["mermaid"].values()) > before
    tags = acc["tags"]
    distinct = {t for t, _ in tags}
    hier = [t for t, _ in tags if "/" in t]
    by_loc = {}
    for t, loc in tags:
        by_loc[loc] = by_loc.get(loc, 0) + 1
    freq = {}
    for t, _ in tags:
        freq[t] = freq.get(t, 0) + 1
    top = sorted(freq.items(), key=lambda kv: -kv[1])[:5]
    depth = {}
    for t in distinct:
        depth[t.count("/") + 1] = depth.get(t.count("/") + 1, 0) + 1
    print("--- Elements enrichis ---")
    print(f"tags : {len(tags)} occurrences, {len(distinct)} distincts, plats {len(tags) - len(hier)}, "
          f"hierarchiques {len(hier)} ; distincts par profondeur {dict(sorted(depth.items()))}")
    print("tags par emplacement : " + ", ".join(f"{k} {v}" for k, v in sorted(by_loc.items(), key=lambda kv: -kv[1])))
    print("tags les plus frequents : " + ", ".join(f"{t} ({n})" for t, n in top)
          + f" ; singletons {sum(1 for n in freq.values() if n == 1)}")
    print(f"pieges '#123' (hors tags) : {acc['numeric_hash']}")
    mm = acc["mermaid"]
    print(f"mermaid : {sum(mm.values())} blocs dans {notes_mm} notes ({100 * notes_mm / max(1, len(files_md)):.1f} % des md) ; "
          + ", ".join(f"{k} {mm.get(k, 0)}" for k in MM_KINDS))
    print(f"callouts : {sum(acc['callouts'].values())} ({acc['callouts_fold']} repliables) : "
          + ", ".join(f"{k} {v}" for k, v in sorted(acc["callouts"].items())))
    print(f"tableaux : {acc['tables']} ; blocs de code : {sum(acc['code'].values())} ("
          + ", ".join(f"{k} {v}" for k, v in sorted(acc["code"].items())) + ")")
    print(f"taches : {acc['tasks_open']} ouvertes, {acc['tasks_done']} faites ; elements de liste imbriques {acc['nested_items']}")
    print(f"embeds images : {acc['img_wiki']} wikilink + {acc['img_md']} markdown ; ancres [[n#t]] {acc['anchors']} ; wikilinks avec alias {acc['aliases']}")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--out", required=True, help="dossier du vault a creer ou completer")
    p.add_argument("--scale", type=float, default=1.0, help="multiplicateur de volume (1, 2, 5, 10, 20...)")
    p.add_argument("--seed", default="rocksy")
    p.add_argument("--notes", type=int, help="force le nombre de notes (remplace 488 x scale)")
    p.add_argument("--attachments", type=int, help="force le nombre de pieces jointes (remplace 465 x scale)")
    p.add_argument("--attach-factor", type=float, default=0.03,
                   help="taille des pieces jointes par rapport au reel (defaut 0.03 ; 1.0 = ~2,3 Go a 20x)")
    p.add_argument("--no-flat-assets", action="store_true",
                   help="range les images dans _ASSETS/<bloc> au lieu d'un seul dossier plat")
    p.add_argument("--dot-noise", type=int, default=3664,
                   help="fichiers dans .opencode/node_modules (a ignorer par le walker), 0 pour desactiver")
    p.add_argument("--mtime-burst", type=float, default=0.66,
                   help="part des fichiers modifies dans les 22 derniers jours (defaut 0.66, comme le vault reel)")
    p.add_argument("--force", action="store_true", help="reecrit les fichiers deja presents")
    p.add_argument("--dry-run", action="store_true", help="affiche le plan sans ecrire")
    p.add_argument("--stats-only", action="store_true", help="ne genere rien, calcule les stats d'un vault")
    p.add_argument("--no-stats", action="store_true", help="saute les statistiques finales")
    a = p.parse_args()
    if a.scale <= 0:
        sys.exit("--scale doit etre > 0")
    if not a.stats_only:
        generate(a)
    if not a.dry_run and not a.no_stats:
        stats(a.out)


if __name__ == "__main__":
    main()
