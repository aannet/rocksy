// Test helpers injected into the real app page by the driver.
(() => {
  const T = (window.T = {
    inv: (c, a) => window.__TAURI__.core.invoke(c, a),
    /** Paths without backslashes in scenario files: T.it("v1", "a.md"), T.ref("x20"), T.vault("01_WORK", "n.md"). */
    it: (...p) => [(window.__ROCKSY_PATHS || {}).it || "C:\\Users\\aleca\\rocksy-test\\it", ...p].join("\\"),
    ref: (...p) => [(window.__ROCKSY_PATHS || {}).vaults || "C:\\Users\\aleca\\rocksy-test\\vaults", ...p].join("\\"),
    vault: (...p) => [rocksy.vaultRoot, ...p].join("\\"),
    view: () => rocksy.focused.view,
    text: () => rocksy.focused.view.state.doc.toString(),
    async addVault(p) {
      rocksy.vaults = await T.inv("add_vault", { path: p });
      rocksy.sidebarOpen = true;
      await rocksy.openVault(p);
      return rocksy.tree ? rocksy.tree.entries.length : -1;
    },
    async open(p) {
      const t0 = performance.now();
      const d = await rocksy.openPath(p);
      return { loaded: !!(d && d.loaded), error: d && d.error, ms: Math.round(performance.now() - t0), name: d && d.name };
    },
    idx(needle, n = 0) {
      const t = T.text();
      let i = -1;
      for (let k = 0; k <= n; k++) i = t.indexOf(needle, i + 1);
      return i;
    },
    /** Scrolls so that the n-th occurrence of `needle` is near the top; cursor is parked at the end of the document. */
    show(needle, n = 0, margin = 90) {
      const v = T.view();
      const i = typeof needle === "number" ? needle : T.idx(needle, n);
      if (i < 0) return -1;
      v.dispatch({ selection: { anchor: v.state.doc.length } });
      v.dispatch({ effects: v.constructor.scrollIntoView(i, { y: "start", yMargin: margin }) });
      return v.state.doc.lineAt(i).number;
    },
    /** Puts the cursor inside the n-th occurrence of `needle` (+offset). */
    cursor(needle, n = 0, offset = 0, center = false) {
      const v = T.view();
      const i = typeof needle === "number" ? needle : T.idx(needle, n);
      if (i < 0) return -1;
      v.dispatch({ selection: { anchor: i + offset } });
      if (center) v.dispatch({ effects: v.constructor.scrollIntoView(i + offset, { y: "center" }) });
      v.focus();
      return v.state.doc.lineAt(i).number;
    },
    lineEnd(lineNo, center = true) {
      const v = T.view();
      const l = v.state.doc.line(Math.max(1, Math.min(v.state.doc.lines, lineNo)));
      v.dispatch({ selection: { anchor: l.to } });
      if (center) v.dispatch({ effects: v.constructor.scrollIntoView(l.to, { y: "center" }) });
      v.focus();
      return l.text.slice(0, 60);
    },
    curLine() {
      const s = T.view().state;
      const l = s.doc.lineAt(s.selection.main.head);
      return { n: l.number, text: l.text.slice(0, 100) };
    },
    imgs() {
      return [...document.querySelectorAll("img.cm-lp-img")].map((i) => ({ w: i.naturalWidth, h: i.naturalHeight, ok: i.complete && i.naturalWidth > 0, src: i.src.slice(0, 70) }));
    },
    count(sel) {
      return document.querySelectorAll(sel).length;
    },
    /** Programmatic scroll: `px` per frame for `frames` frames; returns frame interval statistics. */
    scrollBench(px = 120, frames = 240) {
      return new Promise((res) => {
        const sc = T.view().scrollDOM;
        const dts = [];
        let last = 0;
        let n = 0;
        const start = sc.scrollTop;
        const step = (ts) => {
          if (last) dts.push(ts - last);
          last = ts;
          sc.scrollTop += px;
          if (++n < frames) requestAnimationFrame(step);
          else {
            const s = [...dts].sort((a, b) => a - b);
            const q = (p) => +s[Math.min(s.length - 1, Math.floor(p * s.length))].toFixed(1);
            res({ frames: dts.length, px: sc.scrollTop - start, median: q(0.5), p95: q(0.95), max: q(1), over20ms: dts.filter((d) => d > 20).length, over34ms: dts.filter((d) => d > 34).length });
          }
        };
        requestAnimationFrame(step);
      });
    },
    wait(fn, ms = 8000) {
      return new Promise((res) => {
        const t0 = performance.now();
        const tick = () => {
          let ok = false;
          try {
            ok = !!fn();
          } catch {}
          if (ok) res(Math.round(performance.now() - t0));
          else if (performance.now() - t0 > ms) res(-1);
          else setTimeout(tick, 15);
        };
        tick();
      });
    },
    saveState() {
      const d = rocksy.activeDoc();
      return d ? { name: d.name, dirty: d.dirty, saving: !!d.saving, conflict: d.conflict, error: d.error, saveError: d.saveError, readOnly: d.readOnly, mtime: d.mtimeMs, eol: d.eolMixed ? "mixed" : d.eol === "\r\n" ? "crlf" : d.eol === "\r" ? "cr" : "lf", bom: d.bom, status: document.querySelector(".st-save").textContent, banner: document.querySelector(".banner").hidden ? null : document.querySelector(".banner").textContent.slice(0, 120) } : null;
    },
  });
  return "helpers ok";
})()
