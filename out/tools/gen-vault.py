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

Exemples
  python3 out/tools/gen-vault.py --out /data/vaults/x1  --scale 1
  python3 out/tools/gen-vault.py --out /data/vaults/x20 --scale 20
  python3 out/tools/gen-vault.py --out /data/vaults/x20 --scale 20 --attach-factor 1.0   # tailles reelles (~2,3 Go)
  python3 out/tools/gen-vault.py --out /data/vaults/x5  --stats-only
"""
import argparse
import base64
import json
import math
import os
import random
import re
import statistics
import sys
import time
from pathlib import Path

VERSION = 1
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


class Gen:
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
            return "__DAILY NOTES ACTIVE"
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

    def pick_png(self, r, block):
        lo, hi = block * BASE_ATT, min((block + 1) * BASE_ATT, self.A)
        if hi <= lo:
            return None
        for _ in range(25):
            j = r.randrange(lo, hi)
            name, ext = self.att_name(j)
            if ext == ".png":
                return name
        return None

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
        return t if t != i else (t + 1) % self.N

    def render_note(self, i):
        r = rng(self.seed, "note", i)
        block = i // BASE_NOTES
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
                    fm.append("tags: [" + ", ".join(r.sample(GLOBAL_TAGS, r.randint(1, 3))) + "]")
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
        if r.random() < 0.30:
            for _ in range(1 + int(r.expovariate(1 / 5.0))):
                extras.append(f"- [{'x' if r.random() > 0.86 else ' '}] {self.words(r, 6)}")
        n_tags = int(r.expovariate(1 / 2.0))
        if n_tags:
            extras.append(" ".join("#" + (r.choice(GLOBAL_TAGS) if r.random() < 0.6 else f"b{block}-t{r.randrange(60)}")
                                   for _ in range(n_tags)))
        overhead = sum(len(p) for p in parts) + sum(len(h) for h in heads) + sum(len(x) + 1 for x in extras)
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
            return b"\x89PNG\r\n\x1a\n" + r.randbytes(n)
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
            nodes = [{"id": f"n{k}", "type": "text", "text": self.words(r, 5), "x": k * 40, "y": 0,
                      "width": 200, "height": 60} for k in range(max(1, n // 200))]
            return json.dumps({"nodes": nodes, "edges": []}).encode()
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
