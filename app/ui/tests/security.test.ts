// Regression tests for the 2026-11-23 security audit (SEC-1, SEC-4, SEC-6, SEC-9).
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import * as canvas from "../src/canvas";
import * as paths from "../src/paths";
import { FileIndex, resolveImage } from "../src/resolve";
import * as syntax from "../src/editor/syntax";
import { renderInlineHtml } from "../src/editor/syntax";

// ---------------------------------------------------------------- minimal recording DOM (vitest runs in Node)

class FakeEl {
  children: FakeEl[] = [];
  attrs = new Map<string, string>();
  style: Record<string, string> = {};
  dataset: Record<string, string> = {};
  className = "";
  textContent = "";
  title = "";
  src = "";
  alt = "";
  draggable = true;
  clientWidth = 800;
  clientHeight = 600;
  classList = { add: () => undefined, remove: () => undefined };
  constructor(
    readonly tag: string,
    readonly ns: string | null,
    private log: DomLog,
  ) {}
  set innerHTML(html: string) {
    this.log.html.push(html);
  }
  insertAdjacentHTML(_where: string, html: string) {
    this.log.adjacent.push(html);
  }
  setAttribute(k: string, v: string) {
    this.attrs.set(k, String(v));
  }
  appendChild(c: FakeEl) {
    this.children.push(c);
    return c;
  }
  append(...cs: FakeEl[]) {
    this.children.push(...cs);
  }
  replaceChildren(...cs: FakeEl[]) {
    this.children = cs;
  }
  querySelector() {
    return new FakeEl("span", null, this.log);
  }
  addEventListener() {}
  removeEventListener() {}
  getBoundingClientRect() {
    return { left: 0, top: 0, width: 800, height: 600 };
  }
  walk(visit: (el: FakeEl) => void) {
    visit(this);
    for (const c of this.children) c.walk(visit);
  }
}
interface DomLog {
  html: string[];
  adjacent: string[];
}

const g = globalThis as Record<string, unknown>;
let log: DomLog;
beforeEach(() => {
  log = { html: [], adjacent: [] };
  g.document = {
    createElement: (tag: string) => new FakeEl(tag, null, log),
    createElementNS: (ns: string, tag: string) => new FakeEl(tag, ns, log),
  };
  g.window = { setTimeout, clearTimeout, addEventListener: () => undefined, removeEventListener: () => undefined };
});
afterEach(() => {
  delete g.document;
  delete g.window;
});

function mount(data: unknown) {
  const calls = { assetUrl: [] as string[], readText: [] as string[] };
  const host = new FakeEl("div", null, log);
  const dispose = canvas.mountCanvas(host as unknown as HTMLElement, JSON.stringify(data), {
    assetUrl: (rel) => (calls.assetUrl.push(rel), "asset://" + rel),
    open: () => undefined,
    readText: (rel) => (calls.readText.push(rel), Promise.resolve(null)),
  });
  dispose();
  return { host, calls };
}

// Payloads of the audit proof (~/rocksy-scratch/audit-securite/canvas-xss.html).
const XSS = `0"/><img src=x onerror="document.title='XSS-'+typeof window.__TAURI__;document.body.setAttribute('data-pwned','1')">`;
const CSS_INJECTION = "0px;background-image:url(http://tracker.example/p.png)";
const hostile = {
  nodes: [
    { id: "a", type: "text", text: "hello <img src=x onerror=alert(1)>", x: XSS, y: 0, width: 100, height: 50 },
    { id: "b", type: "text", text: "world", x: 300, y: 0, width: 100, height: 50 },
    { id: "c", type: "text", text: "css", x: 0, y: CSS_INJECTION, width: CSS_INJECTION, height: 50 },
    { id: "d", type: "group", label: "<script>alert(2)</script>", x: 0, y: 200, width: 100, height: 50, color: 'red" onload="alert(3)' },
    { id: "e", type: "link", url: '"><svg onload=alert(4)>', x: 0, y: 300, width: 100, height: 50 },
    { id: XSS, type: "text", text: "id", x: 600, y: 0, width: 100, height: 50 },
  ],
  edges: [
    { id: "e1", fromNode: "a", toNode: "b", fromSide: "left", toSide: "left" },
    { id: "e2", fromNode: "b", toNode: "d", fromSide: XSS, toSide: XSS, color: XSS, toEnd: XSS, fromEnd: XSS, label: "</text><script>alert(5)</script>" },
    { id: "e3", fromNode: XSS, toNode: "b" },
  ],
};

describe("SEC-1: hostile canvas", () => {
  it("never builds markup from canvas data", () => {
    const { host } = mount(hostile);
    // No HTML string parsing of the edges at all, and what is still set through innerHTML is escaped.
    expect(log.adjacent).toEqual([]);
    for (const html of log.html) {
      expect(html).not.toMatch(/<img|<script|<svg[^>]*onload|onerror\s*=\s*"/i);
      expect(html).not.toContain(XSS);
    }
    const allowed = new Set(["div", "span", "button", "img", "h2", "svg", "defs", "marker", "path", "text"]);
    let paths = 0;
    host.walk((el) => {
      expect(allowed.has(el.tag)).toBe(true);
      for (const [k, v] of el.attrs) {
        expect(k).not.toMatch(/^on/i);
        expect(v).not.toMatch(/[<>"]/);
        if (k === "d") {
          expect(v).toMatch(/^[MCz0-9eE+\-. ]+$/);
          paths++;
        }
        if (k === "stroke") expect(v).toMatch(/^#[0-9a-f]{3,8}$/i);
        if (k === "marker-end" || k === "marker-start") expect(v).toBe("url(#cv-arrow)");
      }
      for (const k of ["left", "top", "width", "height"]) if (k in el.style) expect(el.style[k]).toMatch(/^-?\d+(\.\d+)?px$/);
      for (const k of Object.keys(el.style)) expect(el.style[k]).not.toMatch(/url\(|;/i);
      expect(el.style.cssText).toBeUndefined();
    });
    // Marker path + the two edges whose endpoints are valid nodes ("a" was dropped: e1 goes with it).
    expect(paths).toBe(3);
  });

  it("normalizes nodes and edges to typed values", () => {
    const m = canvas.normalizeCanvas(JSON.stringify(hostile));
    expect(m.nodes.map((n) => n.id)).toEqual(["b", "d", "e", XSS]);
    for (const n of m.nodes) for (const v of [n.x, n.y, n.width, n.height]) expect(Number.isFinite(v)).toBe(true);
    expect(m.nodes[1].color).toBe("");
    expect(m.edges).toHaveLength(2);
    expect(m.edges[0]).toMatchObject({ fromNode: "b", toNode: "d", fromSide: undefined, toSide: undefined, color: "#6b7a99", arrowEnd: true, arrowStart: false });
    expect(m.dropped).toBe(3);
  });

  it("keeps legitimate canvases intact", () => {
    const m = canvas.normalizeCanvas(
      JSON.stringify({
        nodes: [
          { id: "n1", type: "text", text: "T", x: -120, y: 40.5, width: 250, height: 60, color: "4" },
          { id: "n2", type: "file", file: "Notes/Plan.md", x: "300", y: 0, width: 400, height: 400, color: "#aabbcc" },
          { id: "g", type: "group", label: "G", x: 0, y: 0, width: 10, height: 10 },
          { id: "odd", type: "inconnu", x: 1e12, y: 0, width: 1, height: 1 },
          { id: "bad", type: "text", x: null, y: 0, width: 1, height: 1 },
          { id: "nan", type: "text", x: "1e5", y: 0, width: 1, height: 1 },
        ],
        edges: [
          { id: "e", fromNode: "n1", toNode: "n2", fromSide: "right", toSide: "left", toEnd: "none", fromEnd: "arrow", label: "L", color: "2" },
          { id: "dangling", fromNode: "n1", toNode: "absent" },
        ],
      }),
    );
    expect(m.nodes.map((n) => [n.id, n.type, n.x])).toEqual([
      ["n1", "text", -120],
      ["n2", "file", 300],
      ["g", "group", 0],
      ["odd", "text", 1e7],
    ]);
    expect(m.nodes[0].color).toBe("#6fcf79");
    expect(m.nodes[1]).toMatchObject({ file: "Notes/Plan.md", color: "#aabbcc" });
    expect(m.edges).toEqual([{ fromNode: "n1", toNode: "n2", fromSide: "right", toSide: "left", arrowEnd: false, arrowStart: true, label: "L", color: "#f0a04b" }]);
    expect(m.dropped).toBe(3);
    expect(canvas.edgeGeometry(m.nodes[0], m.nodes[1], m.edges[0])).toEqual({ d: "M130 70.5 C215.48239584850205 70.5 214.51760415149795 200 300 200", labelX: 215, labelY: 129.25 });
    expect(canvas.normalizeCanvas("").nodes).toEqual([]);
    expect(canvas.normalizeCanvas("null").nodes).toEqual([]);
    expect(canvas.normalizeCanvas('{"nodes":"x","edges":[1,null]}')).toEqual({ nodes: [], edges: [], dropped: 2 });
    expect(() => canvas.normalizeCanvas("{")).toThrow();
  });
});

describe("SEC-9: canvas file references stay under their base folder", () => {
  const escapes = ["../../../Users/aleca/Documents/x.md", "a/../../x.md", "..\\..\\x.png", "/etc/passwd.md", "C:\\Users\\x.md", "C:/Users/x.png", "\\\\srv\\share\\x.png", "//srv/share/x.md", "a/.. /../x.md", "a/.../x.md", "x.md:stream", ".."];
  it("refuses paths that climb out, are absolute or name a drive", () => {
    for (const p of escapes) expect(paths.safeRelPath(p), p).toBeNull();
    expect(paths.safeRelPath("Notes/Plan.md")).toBe("Notes/Plan.md");
    expect(paths.safeRelPath("Notes\\img\\..\\a.png")).toBe("Notes/a.png");
    expect(paths.safeRelPath("./a/..b/c.md")).toBe("a/..b/c.md");
  });
  it("never hands an escaping path to the host (no read, no image load, no click target)", () => {
    const nodes = escapes.map((file, i) => ({ id: "f" + i, type: "file", file, x: i * 10, y: 0, width: 100, height: 50 }));
    nodes.push({ id: "ok1", type: "file", file: "Notes/Plan.md", x: 0, y: 100, width: 100, height: 50 });
    nodes.push({ id: "ok2", type: "file", file: "_ASSETS/./schema.png", x: 0, y: 200, width: 100, height: 50 });
    const { host, calls } = mount({ nodes, edges: [] });
    expect(calls.readText).toEqual(["Notes/Plan.md"]);
    expect(calls.assetUrl).toEqual(["_ASSETS/schema.png"]);
    const openTargets: string[] = [];
    host.walk((el) => {
      if (el.dataset.open != null) openTargets.push(el.dataset.open);
    });
    expect(openTargets).toEqual(["Notes/Plan.md"]);
  });
});

describe("SEC-4: network paths are never resolved as loadable", () => {
  const index = new FileIndex([{ path: "_ASSETS/schema.png", isDir: false }]);
  const inVault = { fileAbs: "C:\\Vault\\Notes\\Idée.md", vaultRoot: "C:\\Vault", index };
  const outside = { fileAbs: "C:\\n\\x.md", vaultRoot: null, index: null };
  const network = [
    "//attaquant.example/s/a.png",
    "\\\\attaquant\\s\\a.png",
    "\\\\?\\C:\\Windows\\a.png",
    "\\\\?\\UNC\\attaquant\\s\\a.png",
    "\\\\.\\pipe\\a.png",
    "/\\attaquant/s/a.png",
    "\\/attaquant/s/a.png",
    "file://///attaquant/s/a.png",
    "file:////attaquant/s/a.png",
    "file://attaquant/s/a.png",
    "FILE:\\\\attaquant\\s\\a.png",
    "%5C%5Cattaquant%5Cs%5Ca.png",
    "%2F%2Fattaquant/s/a.png",
    "<//attaquant/s/a.png>",
  ];
  it("resolveImage refuses UNC, device and file://host sources", () => {
    for (const src of network) {
      expect(resolveImage(src, inVault), src).toEqual({ kind: "missing" });
      expect(resolveImage(src, outside), src).toEqual({ kind: "missing" });
    }
  });
  it("still resolves local absolute paths and file: URLs", () => {
    expect(resolveImage("D:\\pics\\a.png", outside)).toEqual({ kind: "local", abs: "D:\\pics\\a.png" });
    expect(resolveImage("file:///D:/pics/a.png", outside)).toEqual({ kind: "local", abs: "D:/pics/a.png" });
    expect(resolveImage("file:///tmp/a.png", { fileAbs: "/tmp/n/x.md", vaultRoot: null, index: null })).toEqual({ kind: "local", abs: "/tmp/a.png" });
    expect(resolveImage("/tmp/a.png", { fileAbs: "/tmp/n/x.md", vaultRoot: null, index: null })).toEqual({ kind: "local", abs: "/tmp/a.png" });
    expect(resolveImage("schema.png", inVault)).toEqual({ kind: "local", abs: "C:\\Vault\\_ASSETS\\schema.png" });
  });
  it("isNetworkPath / isAbsolute", () => {
    for (const p of ["\\\\srv\\share\\a.md", "//srv/share/a.md", "\\\\?\\C:\\a.md", "/\\srv\\a.md"]) {
      expect(paths.isNetworkPath(p), p).toBe(true);
      // Used by openTarget for a clicked link outside a vault: a network target must not be taken as is.
      expect(paths.isAbsolute(p), p).toBe(false);
    }
    for (const p of ["C:\\a\\b.md", "c:/a/b.md", "/home/a/b.md", "\\a\\b.md"]) {
      expect(paths.isNetworkPath(p), p).toBe(false);
      expect(paths.isAbsolute(p), p).toBe(true);
    }
    expect(paths.isAbsolute("a/b.md")).toBe(false);
  });
});

describe("SEC-6: inline rendering stays fast on hostile cells", () => {
  const timed = (text: string) => {
    const t = performance.now();
    const html = renderInlineHtml(text);
    return { ms: performance.now() - t, html };
  };
  it("renders an 8 000 character cell in under 100 ms, as escaped plain text", () => {
    for (const unit of ["[a](", "[a](b ", "[a]((", "[[a", "*a ", "`a ", "**a", "==a"]) {
      const text = unit.repeat(Math.ceil(8000 / unit.length));
      const r = timed(text);
      expect(r.ms, unit).toBeLessThan(100);
      expect(r.html).toBe(syntax.escapeHtml(text));
    }
    expect(renderInlineHtml("**g** " + "x".repeat(2000))).toBe("**g** " + "x".repeat(2000));
  });
  it("the link alternative itself is not cubic (cell just under the 2 000 character cap)", () => {
    for (const unit of ["[a](", "[a](b ", "[a]((", "[a](<b "]) {
      const text = unit.repeat(Math.floor(2000 / unit.length));
      expect(text.length).toBeLessThanOrEqual(2000);
      const r = timed(text);
      expect(r.ms, unit).toBeLessThan(100);
      expect(r.html).not.toContain("data-href");
    }
  });
  it("keeps the rendering of Markdown links, with balanced parentheses in the destination", () => {
    const link = (href: string, label: string) => `<span class="cm-lp-link cm-lp-rendered" data-href="${href}">${label}</span>`;
    expect(renderInlineHtml("[t](a.md)")).toBe(link("a.md", "t"));
    expect(renderInlineHtml('voir [**t**](a.md "titre") fin')).toBe("voir " + link("a.md", "<strong>t</strong>") + " fin");
    expect(renderInlineHtml("[x](a(b).md)")).toBe(link("a(b).md", "x"));
    expect(renderInlineHtml("[w](https://fr.wikipedia.org/wiki/Rust_(langage)) ok")).toBe(link("https://fr.wikipedia.org/wiki/Rust_(langage)", "w") + " ok");
    expect(renderInlineHtml('[x](a.md "t (1)") z')).toBe(link("a.md", "x") + " z");
    expect(renderInlineHtml("[x](<a b.md>)")).toBe(link("&lt;a b.md&gt;", "x"));
    // Unbalanced destination: same result as before the fix (cut at the first closing parenthesis).
    expect(renderInlineHtml("[x](a(b.md)")).toBe(link("a(b.md", "x"));
    expect(renderInlineHtml("[x](a b) c)")).toBe(link("a", "x") + " c)");
    // Not links: the text is kept and later marks are still rendered.
    expect(renderInlineHtml("[x]()")).toBe("[x]()");
    expect(renderInlineHtml("[x](a.md")).toBe("[x](a.md");
    expect(renderInlineHtml("[ voir [a](b.md)")).toBe(link("b.md", " voir [a")); // label rule unchanged (as before the fix)
    expect(renderInlineHtml("[x](**g** [a](b.md)")).toBe(link("**g**", "x"));
    expect(renderInlineHtml("[x]( **g**")).toBe("[x]( <strong>g</strong>");
    expect(renderInlineHtml("[[Note]] [a](b.md) ![[i.png]]")).toContain(link("b.md", "a"));
  });
});
