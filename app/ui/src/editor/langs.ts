// Code-block languages, each one behind a dynamic import: nothing here is in the startup bundle.
import { LanguageDescription, LanguageSupport, StreamLanguage, type StreamParser } from "@codemirror/language";

function legacy(load: () => Promise<StreamParser<unknown>>): () => Promise<LanguageSupport> {
  return () => load().then((p) => new LanguageSupport(StreamLanguage.define(p)));
}

const languages: LanguageDescription[] = [
  LanguageDescription.of({
    name: "JavaScript",
    alias: ["js", "javascript", "jsx", "mjs", "cjs", "node", "dataviewjs"],
    load: () => import("@codemirror/lang-javascript").then((m) => m.javascript({ jsx: true })),
  }),
  LanguageDescription.of({
    name: "TypeScript",
    alias: ["ts", "typescript", "tsx"],
    load: () => import("@codemirror/lang-javascript").then((m) => m.javascript({ typescript: true, jsx: true })),
  }),
  LanguageDescription.of({ name: "JSON", alias: ["json", "json5", "jsonc"], load: () => import("@codemirror/lang-json").then((m) => m.json()) }),
  LanguageDescription.of({ name: "Python", alias: ["py", "python", "python3"], load: () => import("@codemirror/lang-python").then((m) => m.python()) }),
  LanguageDescription.of({ name: "Rust", alias: ["rs", "rust"], load: () => import("@codemirror/lang-rust").then((m) => m.rust()) }),
  LanguageDescription.of({ name: "YAML", alias: ["yaml", "yml"], load: () => import("@codemirror/lang-yaml").then((m) => m.yaml()) }),
  LanguageDescription.of({ name: "HTML", alias: ["html", "xml", "svg", "vue"], load: () => import("@codemirror/lang-html").then((m) => m.html()) }),
  LanguageDescription.of({ name: "CSS", alias: ["css", "scss", "less"], load: () => import("@codemirror/lang-css").then((m) => m.css()) }),
  LanguageDescription.of({ name: "SQL", alias: ["sql", "mysql", "postgres", "postgresql", "sqlite", "dataview"], load: () => import("@codemirror/lang-sql").then((m) => m.sql()) }),
  LanguageDescription.of({
    name: "Shell",
    alias: ["sh", "bash", "shell", "zsh", "console"],
    load: legacy(() => import("@codemirror/legacy-modes/mode/shell").then((m) => m.shell as StreamParser<unknown>)),
  }),
  LanguageDescription.of({
    name: "PowerShell",
    alias: ["powershell", "ps1", "pwsh"],
    load: legacy(() => import("@codemirror/legacy-modes/mode/powershell").then((m) => m.powerShell as StreamParser<unknown>)),
  }),
  LanguageDescription.of({
    name: "TOML",
    alias: ["toml", "ini"],
    load: legacy(() => import("@codemirror/legacy-modes/mode/toml").then((m) => m.toml as StreamParser<unknown>)),
  }),
];

const byAlias = new Map<string, LanguageDescription>();
for (const l of languages) for (const a of l.alias) byAlias.set(a, l);

/** Finds the language of a fenced code block from its info string (`ts`, `python {1,3}`…). */
export function findCodeLanguage(info: string): LanguageDescription | null {
  const name = /^[^\s{,:]+/.exec(info.trim().toLowerCase());
  return name ? (byAlias.get(name[0]) ?? null) : null;
}

/** Display label for the code block badge. */
export function codeLabel(info: string): string {
  const raw = /^[^\s{,:]+/.exec(info.trim());
  if (!raw) return "";
  if (raw[0].toLowerCase() === "mermaid") return "Mermaid";
  return findCodeLanguage(info)?.name ?? raw[0];
}
