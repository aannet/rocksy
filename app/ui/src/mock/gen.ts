// Deterministic fabricated content for the mock backend. Nothing here comes from a real vault.

export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return h >>> 0;
}

const WORDS =
  "analyse architecture atelier budget cadrage calendrier client compte contrat décision délai dépendance déploiement document données équipe estimation étape étude exigence feuille fournisseur gouvernance hypothèse incident indicateur interface itération jalon lancement livrable lot mesure migration module objectif option outil périmètre pilote plan planning priorité processus projet prototype qualité question recette référentiel réunion revue risque scénario schéma service session solution sprint stratégie suivi synthèse système tâche test trajectoire usage validation version".split(
    " ",
  );
const VERBS = "préparer valider relire revoir planifier documenter mesurer comparer simplifier livrer tester corriger arbitrer suivre clarifier".split(" ");
const LINKS = "le la les un une des pour avec sans sur dans par vers entre selon avant après pendant".split(" ");
export const TAG_POOL = [
  "projet/alpha",
  "projet/alpha/suivi",
  "projet/beta",
  "projet/beta/risques",
  "projet/gamma",
  "reunion/hebdo",
  "reunion/comite",
  "reunion/atelier",
  "idee",
  "idee/produit",
  "idee/tech",
  "todo",
  "todo/urgent",
  "lecture/article",
  "lecture/livre",
  "decision",
  "decision/archi",
  "perso/sport",
  "perso/voyage",
  "veille/ia",
  "veille/rust",
  "veille/outils",
  "client/nord",
  "client/sud",
  "retro",
  "a-relire",
  "2026/q4",
  "2026/q3",
  "qualité",
  "équipe/recrutement",
];

export class Gen {
  r: () => number;
  constructor(seed: number) {
    this.r = rng(seed);
  }
  int(n: number): number {
    return Math.floor(this.r() * n);
  }
  pick<T>(a: readonly T[]): T {
    return a[this.int(a.length)];
  }
  words(n: number): string {
    let s = "";
    for (let i = 0; i < n; i++) s += (i ? " " : "") + (this.r() < 0.3 ? this.pick(LINKS) : this.pick(WORDS));
    return s;
  }
  sentence(): string {
    const s = this.words(6 + this.int(12));
    return s[0].toUpperCase() + s.slice(1) + ".";
  }
  rich(): string {
    // A sentence with some inline markup.
    const parts: string[] = [];
    const n = 2 + this.int(3);
    for (let i = 0; i < n; i++) {
      const w = this.words(3 + this.int(6));
      const k = this.r();
      if (k < 0.12) parts.push(`**${w}**`);
      else if (k < 0.22) parts.push(`*${w}*`);
      else if (k < 0.28) parts.push(`\`${this.pick(WORDS)}_${this.int(99)}\``);
      else if (k < 0.36) parts.push(`[[${this.noteName()}]]`);
      else if (k < 0.4) parts.push(`[[${this.noteName()}|${this.pick(WORDS)}]]`);
      else if (k < 0.44) parts.push(`==${w}==`);
      else if (k < 0.47) parts.push(`[${this.pick(WORDS)}](https://exemple.test/${this.pick(WORDS)})`);
      else if (k < 0.5) parts.push(`~~${w}~~`);
      else if (k < 0.58) parts.push(`${w} #${this.pick(TAG_POOL)}`);
      else parts.push(w);
    }
    const s = parts.join(" ");
    return s[0] === s[0].toLowerCase() && /[a-zà-ÿ]/.test(s[0]) ? s[0].toUpperCase() + s.slice(1) + "." : s + ".";
  }
  noteName(): string {
    const w = this.pick(WORDS);
    return `${w[0].toUpperCase()}${w.slice(1)} ${this.pick(WORDS)} ${String(this.int(400)).padStart(3, "0")}`;
  }
  list(depth = 0): string {
    let s = "";
    const n = 2 + this.int(4);
    for (let i = 0; i < n; i++) {
      const ind = "\t".repeat(depth);
      const k = this.r();
      if (k < 0.3) s += `${ind}- [${this.r() < 0.4 ? "x" : " "}] ${this.pick(VERBS)} ${this.words(3 + this.int(5))}${this.r() < 0.25 ? " #" + this.pick(TAG_POOL) : ""}\n`;
      else s += `${ind}- ${this.rich()}\n`;
      if (depth < 2 && this.r() < 0.3) s += this.list(depth + 1);
    }
    return s;
  }
  table(): string {
    const cols = 3 + this.int(2);
    let s = "|" + Array.from({ length: cols }, () => " " + this.pick(WORDS) + " ").join("|") + "|\n";
    s += "|" + Array.from({ length: cols }, (_, i) => (i === cols - 1 ? " ---: " : " --- ")).join("|") + "|\n";
    const rows = 2 + this.int(4);
    for (let r = 0; r < rows; r++) s += "|" + Array.from({ length: cols }, (_, i) => (i === cols - 1 ? ` ${this.int(500)} ` : ` ${this.words(1 + this.int(3))} `)).join("|") + "|\n";
    return s;
  }
  code(): string {
    const k = this.int(3);
    if (k === 0) return "```ts\nexport function " + this.pick(WORDS).replace(/[^a-z]/g, "") + "(input: string): number {\n  const n = input.length * " + this.int(9) + ";\n  return n > 10 ? n : 0; // seuil\n}\n```\n";
    if (k === 1) return "```python\ndef " + this.pick(WORDS).replace(/[^a-z]/g, "") + "(items):\n    total = sum(i * 2 for i in items)\n    return {\"total\": total, \"ok\": True}\n```\n";
    return "```bash\ncargo build --release && ls -la target/release | grep rocksy\n```\n";
  }
  callout(): string {
    const type = this.pick(["note", "tip", "warning", "info", "question", "example", "success", "danger"]);
    return `> [!${type}] ${this.words(3)}\n> ${this.sentence()}\n> ${this.rich()}\n`;
  }
  block(): string {
    const k = this.r();
    if (k < 0.42) return this.rich() + " " + this.sentence() + (this.r() < 0.5 ? " " + this.rich() : "") + "\n";
    if (k < 0.75) return this.list();
    if (k < 0.82) return this.table();
    if (k < 0.88) return this.code();
    if (k < 0.94) return this.callout();
    return `> ${this.sentence()}\n> ${this.sentence()}\n`;
  }
}

/** Small note (1 to 3 KB) derived from its path. */
export function smallNote(rel: string): string {
  const g = new Gen(hash(rel));
  const title = rel.slice(rel.lastIndexOf("/") + 1).replace(/\.md$/, "");
  let s = "";
  if (g.r() < 0.9) {
    s += `---\ncreated: 2026-0${1 + g.int(9)}-${String(1 + g.int(27)).padStart(2, "0")}\n`;
    if (g.r() < 0.5) s += `tags: [${g.pick(TAG_POOL)}${g.r() < 0.4 ? ", " + g.pick(TAG_POOL) : ""}]\n`;
    s += `status: ${g.pick(["brouillon", "en-cours", "terminé"])}\n---\n\n`;
  }
  s += `# ${title}\n\n`;
  const sections = 1 + g.int(3);
  for (let i = 0; i < sections; i++) {
    if (i) s += `## ${g.words(2 + g.int(3))}\n\n`;
    const blocks = 1 + g.int(4);
    for (let b = 0; b < blocks; b++) s += g.block() + "\n";
  }
  return s;
}

/** The worst-case note: a long journal of about `targetBytes` of varied Markdown. */
export function bigNote(targetBytes = 320_000): string {
  const g = new Gen(319);
  const out: string[] = ["---\ntitle: Journal de démonstration\ntags: [journal, 2026/q4]\nalias: DAILY\n---\n\n# Journal de démonstration\n\n> [!info] Note fabriquée\n> Ce contenu est généré pour les tests de performance : il ne provient d'aucun vault réel.\n\n"];
  let size = out[0].length;
  let day = 0;
  while (size < targetBytes) {
    const d = new Date(Date.UTC(2026, 10, 23) - day * 86400000);
    let s = `## ${d.toISOString().slice(0, 10)}\n\n`;
    const sections = 1 + g.int(3);
    for (let i = 0; i < sections; i++) {
      s += `### ${g.pick(["Réunion", "Suivi", "Notes", "Décisions", "Idées", "À faire"])} : ${g.words(2 + g.int(3))}\n\n`;
      const blocks = 2 + g.int(5);
      for (let b = 0; b < blocks; b++) s += g.block() + "\n";
    }
    if (day % 9 === 4) s += "![[schema.png|420]]\n\n";
    out.push(s);
    size += s.length;
    day++;
  }
  return out.join("");
}

const FOLDERS = ["00__DEMO", "01__WORK", "02__PROJETS", "03__RESSOURCES", "04__ARCHIVES", "05__PERSO"];

/** Vault-relative file paths (no directories) for a tree of about `count` entries. */
export function treeFiles(count: number): string[] {
  const g = new Gen(42);
  const files: string[] = [];
  const dirs: string[] = [];
  const dirCount = Math.max(8, Math.round(count / 22));
  for (let i = 0; i < dirCount; i++) {
    const top = FOLDERS[1 + (i % (FOLDERS.length - 1))];
    const depth = g.int(3);
    let d = top;
    for (let k = 0; k < depth; k++) d += "/" + g.pick(WORDS) + (k === depth - 1 ? "-" + String(i).padStart(3, "0") : "");
    if (depth === 0) d += "/" + g.pick(WORDS) + "-" + String(i).padStart(3, "0");
    dirs.push(d);
  }
  const n = Math.max(0, count - dirCount * 2);
  for (let i = 0; i < n; i++) {
    const dir = dirs[g.int(dirs.length)];
    const k = g.r();
    const base = g.noteName().replace(/\d+$/, String(i).padStart(5, "0"));
    if (k < 0.52) files.push(`${dir}/${base}.md`);
    else if (k < 0.8) files.push(`_ASSETS/${g.pick(["capture", "schema", "photo", "export"])}-${String(i).padStart(5, "0")}.png`);
    else if (k < 0.92) files.push(`${dir}/${base}.pdf`);
    else if (k < 0.96) files.push(`${dir}/${base}.canvas`);
    else files.push(`${dir}/${base}.xlsx`);
  }
  return files;
}

export function svgImage(label: string, w = 640, h = 320, hue = 215): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${hue},45%,32%)"/><stop offset="1" stop-color="hsl(${hue + 40},50%,20%)"/></linearGradient></defs><rect width="${w}" height="${h}" rx="10" fill="url(#g)"/><g fill="none" stroke="hsla(0,0%,100%,.55)" stroke-width="2"><rect x="${w * 0.1}" y="${h * 0.25}" width="${w * 0.2}" height="${h * 0.22}" rx="8"/><rect x="${w * 0.42}" y="${h * 0.12}" width="${w * 0.2}" height="${h * 0.22}" rx="8"/><rect x="${w * 0.42}" y="${h * 0.56}" width="${w * 0.2}" height="${h * 0.22}" rx="8"/><rect x="${w * 0.73}" y="${h * 0.34}" width="${w * 0.18}" height="${h * 0.22}" rx="8"/><path d="M${w * 0.3} ${h * 0.36} L${w * 0.42} ${h * 0.23} M${w * 0.3} ${h * 0.36} L${w * 0.42} ${h * 0.67} M${w * 0.62} ${h * 0.23} L${w * 0.73} ${h * 0.45} M${w * 0.62} ${h * 0.67} L${w * 0.73} ${h * 0.45}"/></g><text x="${w / 2}" y="${h - 18}" text-anchor="middle" font-family="Segoe UI, sans-serif" font-size="15" fill="hsla(0,0%,100%,.8)">${label}</text></svg>`;
  return "data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg);
}

export const DEMO_LIVE_PREVIEW = `---
title: Live Preview
tags: [demo, demo/apercu]
created: 2026-11-24
---

# Live Preview : éléments en ligne

Un paragraphe avec du **gras**, de l'*italique*, du ~~barré~~, du ==surligné== et du \`code en ligne\`. Un lien externe [vers un site](https://exemple.test/page), un wikilink [[Callouts]], un alias [[Tableaux et code|les tableaux]], une ancre [[Callouts#Repliables]] et un lien cassé [[Note inexistante]].

Les tags se glissent partout : #demo/apercu au milieu d'une phrase, ou en fin de ligne #idee/produit

## Listes et tâches

- Première puce avec un texte assez long pour passer à la ligne et vérifier que le retrait suspendu aligne bien la suite du texte sous le début de l'élément
- Deuxième puce avec **gras** et [[Journal]]
	- Puce imbriquée #projet/alpha
		- Troisième niveau
- [ ] Tâche à faire #todo
- [x] Tâche terminée
	- [ ] Sous-tâche ouverte

1. Élément numéroté
2. Deuxième élément
10. Dixième élément

## Citation et règle

> Une citation sur deux lignes,
> avec de l'*emphase* à l'intérieur.

---

### Titre de niveau 3

#### Titre de niveau 4

Texte final avec un caractère échappé \\*pas d'italique\\* et une URL nue https://exemple.test/chemin.
`;

export const DEMO_BLOCKS = `---
title: Tableaux et code
tags:
  - demo
  - demo/blocs
status: en-cours
---

# Tableaux, code et images

## Tableau

| Jalon | Responsable | État | Charge |
| :--- | :---: | --- | ---: |
| Cadrage | **Alix** | ==terminé== | 3 |
| Prototype | Sam | en cours, voir [[Live Preview]] | 12 |
| Recette | *à définir* | \`bloqué\` | 5 |

## Code

\`\`\`ts
// Coloration chargée à la demande
export function fuzzy(query: string, items: string[]): string[] {
  const q = query.toLowerCase();
  return items.filter((it) => it.toLowerCase().includes(q));
}
\`\`\`

\`\`\`python
def moyenne(valeurs: list[float]) -> float:
    return sum(valeurs) / max(1, len(valeurs))
\`\`\`

\`\`\`mermaid
flowchart LR
  A[Note] --> B{Index}
  B --> C[Tags]
  B --> D[Liens]
\`\`\`

\`\`\`mermaid
sequenceDiagram
  UI->>Coeur: read_file
  Coeur-->>UI: FileContent
\`\`\`

\`\`\`mermaid
flowchart LR
  A[Bloc invalide --> 
\`\`\`

## Images

Embed Obsidian : ![[schema.png]]

Image Markdown relative : ![Photo de test](../_ASSETS/photo.png)

Image avec largeur : ![[photo.png|240]]

Image absente : ![[introuvable.png]]
`;

export const DEMO_CALLOUTS = `# Callouts

> [!note] Note
> Un callout simple avec du **gras** et un tag #demo/callouts

> [!tip] Astuce
> Le marqueur réapparaît quand le curseur entre dans la ligne.

> [!warning] Attention
> - une liste
> - dans un callout

> [!danger]
> Sans titre : le type sert de titre.

> [!question] Une question ?
> Réponse sur une ligne.

> [!success] Réussi
> Tout est vert.

> [!example] Exemple
> \`code\` et [[Live Preview]].

> [!quote] Citation
> « Mesurer avant de décider. »

## Repliables

> [!info]- Replié par défaut
> Ce contenu est masqué tant que le callout est replié.
> Deuxième ligne masquée.

> [!abstract]+ Déplié par défaut
> Ce contenu est visible, et le chevron permet de le replier.

Fin de la note.
`;

export const DEMO_OUTSIDE = `# Fichier hors vault

Ce fichier est ouvert **sans vault** : pas d'arbre, pas d'index, rien créé à côté.

Image relative au fichier : ![Locale](img/local.png)

- [ ] Vérifier l'enregistrement #horsvault/test
- Tag en ligne #horsvault

Texte libre pour la frappe.
`;

export const DEMO_CANVAS = JSON.stringify(
  {
    nodes: [
      { id: "g1", type: "group", x: -40, y: -60, width: 760, height: 420, label: "Cadrage" },
      { id: "n1", type: "text", x: 0, y: 0, width: 260, height: 120, text: "# Objectif\nDémarrage en **moins de 2 s**", color: "5" },
      { id: "n2", type: "file", x: 400, y: 0, width: 280, height: 120, file: "00__DEMO/Live Preview.md" },
      { id: "n3", type: "text", x: 0, y: 200, width: 260, height: 120, text: "Arbre depuis un *snapshot*\n- réconciliation en arrière-plan", color: "4" },
      { id: "n4", type: "file", x: 400, y: 200, width: 280, height: 140, file: "_ASSETS/schema.png" },
      { id: "n5", type: "text", x: 820, y: 90, width: 240, height: 100, text: "Tags par fragments", color: "1" },
      { id: "n6", type: "link", x: 820, y: 260, width: 240, height: 80, url: "https://exemple.test" },
    ],
    edges: [
      { id: "e1", fromNode: "n1", fromSide: "right", toNode: "n2", toSide: "left", label: "édite" },
      { id: "e2", fromNode: "n1", fromSide: "bottom", toNode: "n3", toSide: "top" },
      { id: "e3", fromNode: "n3", fromSide: "right", toNode: "n4", toSide: "left" },
      { id: "e4", fromNode: "n2", fromSide: "right", toNode: "n5", toSide: "left", color: "6" },
      { id: "e5", fromNode: "n5", fromSide: "bottom", toNode: "n6", toSide: "top" },
    ],
  },
  null,
  1,
);
