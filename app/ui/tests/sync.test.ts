// Text safety and contract fidelity (audits of 2026-11-23): closing sequence, reload race, save retries,
// deleted files, mixed line endings, read-only documents, tag filter parity, index events of another vault.
// The document logic (docsync.ts) is driven against the mock backend, which follows the contract.
import { describe, expect, it } from "vitest";
import { Text } from "@codemirror/state";
import { createMock, type MockControl } from "../src/mock/mock";
import type { Backend } from "../src/backend";
import type { ApiError, BackendEvents } from "../src/types";
import { type CloseChoice, type CloseIssue, DocSync, openErrorMessage, retryDelay, saveErrorMessage, type SyncDoc, type Timers } from "../src/docsync";
import { decodeText, encodeText } from "../src/eol";
import { filterTagLibrary, makeTagLibrary } from "../src/tagtree";
import { FileIndex, isNetworkSource, resolveCanvasFile, sameTreePaths } from "../src/resolve";
import { isForVault } from "../src/vaultevents";
import { createEditorState } from "../src/editor/editor";

const ROOT = "C:\\Vaults\\Demo";
const NOTE = ROOT + "\\00__DEMO\\Journal.md";
const NOTE2 = ROOT + "\\00__DEMO\\Callouts.md";
const OUTSIDE = "C:\\Temp\\Notes\\hors-vault.md";

type Doc = SyncDoc & { text: Text };

/** Timers under the control of the test: nothing fires by itself. */
function fakeTimers() {
  let next = 1;
  const pending = new Map<number, { fn: () => void; ms: number }>();
  const timers: Timers = {
    set: (fn, ms) => {
      pending.set(next, { fn, ms });
      return next++;
    },
    clear: (id) => void pending.delete(id),
  };
  return {
    timers,
    delays: () => [...pending.values()].map((p) => p.ms),
    fire: () => {
      const [id, p] = [...pending.entries()][0];
      pending.delete(id);
      p.fn();
    },
  };
}

function setup(opts: { autosave?: boolean; params?: string } = {}) {
  const { backend, control } = createMock(new URLSearchParams(opts.params ?? "tree=40&delay=0"));
  const state = { autosave: opts.autosave ?? true };
  const toasts: string[] = [];
  const clock = fakeTimers();
  const docs: Doc[] = [];
  const sync = new DocSync<Doc>(
    {
      backend,
      autosave: () => state.autosave,
      textOf: (d) => (d.kind === "md" && d.loaded ? d.text : null),
      applyDisk: (d, text) => {
        d.text = Text.of(text.split(/\r\n?|\n/));
      },
      refresh: () => undefined,
      saved: () => undefined,
      toast: (m) => toasts.push(m),
    },
    clock.timers,
  );
  const open = async (path: string): Promise<Doc> => {
    const fc = await backend.readFile(path);
    const dec = decodeText(fc.text);
    const doc: Doc = {
      path,
      name: path.slice(path.lastIndexOf("\\") + 1),
      kind: "md",
      loaded: true,
      readOnly: fc.readOnly,
      error: null,
      saveError: null,
      retries: 0,
      mtimeMs: fc.mtimeMs,
      eol: dec.eol,
      eolMixed: dec.mixed,
      bom: dec.bom,
      dirty: false,
      saving: null,
      conflict: null,
      pendingExternal: null,
      timer: 0,
      text: Text.of(dec.text.split(/\r\n?|\n/)),
    };
    docs.push(doc);
    return doc;
  };
  /** What typing does in the app: a new immutable text, the dirty flag, the autosave timer. */
  const type = (doc: Doc, s: string) => {
    doc.text = doc.text.replace(doc.text.length, doc.text.length, Text.of(s.split("\n")));
    doc.dirty = true;
    sync.scheduleAutosave(doc);
  };
  const asked: { issues: CloseIssue<Doc>[]; canSave: boolean }[] = [];
  const closeHost = (answer: (n: number) => CloseChoice | Promise<CloseChoice>) => ({
    docs: () => docs,
    closeAck: () => backend.closeAck(),
    quit: () => backend.quit(),
    ask: async (issues: CloseIssue<Doc>[], canSave: boolean) => {
      asked.push({ issues, canSave });
      return answer(asked.length);
    },
  });
  return { backend, control, sync, open, type, docs, state, toasts, clock, asked, closeHost };
}

const io = (message = "the file is locked"): ApiError => ({ kind: "io", message });
const tick = (ms = 0) => new Promise((r) => setTimeout(r, ms));

describe("BUG-1: closing sequence", () => {
  it("acknowledges first, saves every modified document, and only then quits", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    const b = await t.open(NOTE2);
    t.type(a, "\nA");
    t.type(b, "\nB");
    t.control.calls.length = 0;
    t.control.saveDelayMs = 15;
    expect(await t.sync.requestClose(t.closeHost(() => "cancel"))).toBe("quit");
    expect(t.asked).toHaveLength(0);
    expect(t.control.calls[0]).toBe("close_ack");
    expect(t.control.calls[t.control.calls.length - 1]).toBe("quit");
    // Both saves had been answered when `quit` was sent: [done, started].
    expect(t.control.savesAtQuit).toEqual([2, 2]);
    expect(t.control.textOf(NOTE)).toMatch(/\nA$/);
    expect(t.control.textOf(NOTE2)).toMatch(/\nB$/);
    expect(a.dirty || b.dirty).toBe(false);
  });

  it("waits for a save that was already in flight", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.type(a, "\nen vol");
    t.control.saveDelayMs = 25;
    const inFlight = t.sync.save(a);
    t.type(a, " puis encore");
    expect(await t.sync.requestClose(t.closeHost(() => "cancel"))).toBe("quit");
    await inFlight;
    expect(t.control.savesAtQuit).toEqual([2, 2]);
    expect(t.control.textOf(NOTE)).toMatch(/en vol puis encore$/);
  });

  it("asks when a save fails; cancelling keeps the application open", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.type(a, "\nimportant");
    t.control.failSaves.push(io());
    expect(await t.sync.requestClose(t.closeHost(() => "cancel"))).toBe("cancelled");
    expect(t.control.quits).toBe(0);
    expect(t.asked).toHaveLength(1);
    expect(t.asked[0].issues.map((i) => [i.doc.path, i.reason])).toEqual([[NOTE, "failed"]]);
    expect(t.asked[0].canSave).toBe(true);
    expect(a.dirty).toBe(true);
    expect(a.text.toString()).toMatch(/important$/);
    // Second attempt: "save and quit" retries, the disk works again.
    expect(await t.sync.requestClose(t.closeHost(() => "save"))).toBe("quit");
    expect(t.control.quits).toBe(1);
    expect(t.control.textOf(NOTE)).toMatch(/important$/);
  });

  it("asks again when 'save and quit' fails too, and quits without saving only on request", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.type(a, "\nx");
    t.control.failSaves.push(io(), io());
    expect(await t.sync.requestClose(t.closeHost((n) => (n === 1 ? "save" : "discard")))).toBe("quit");
    expect(t.asked).toHaveLength(2);
    expect(t.control.saves).toHaveLength(0);
    expect(t.control.quits).toBe(1);
  });

  it("with autosave off, nothing is written without asking", async () => {
    const t = setup({ autosave: false });
    const a = await t.open(NOTE);
    t.type(a, "\nbrouillon");
    expect(await t.sync.requestClose(t.closeHost(() => "cancel"))).toBe("cancelled");
    expect(t.asked[0].issues[0].reason).toBe("manual");
    expect(t.control.saves).toHaveLength(0);
    expect(t.control.quits).toBe(0);
    expect(await t.sync.requestClose(t.closeHost(() => "save"))).toBe("quit");
    expect(t.control.textOf(NOTE)).toMatch(/brouillon$/);
  });

  it("never overwrites a conflicting document: only 'quit without saving' or 'cancel' are offered", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.type(a, "\nici");
    t.control.touch(NOTE, "# Autre machine\n");
    t.sync.onFileChanged(a, t.control.textOf(NOTE) ? Date.now() + 5 : 0);
    expect(a.conflict).toBe("external");
    expect(await t.sync.requestClose(t.closeHost(() => "cancel"))).toBe("cancelled");
    expect(t.asked[0].canSave).toBe(false);
    expect(t.asked[0].issues[0].reason).toBe("conflict");
    expect(t.control.textOf(NOTE)).toBe("# Autre machine\n");
  });

  it("several close requests in a row: each is acknowledged, one dialog only", async () => {
    const t = setup({ autosave: false });
    const a = await t.open(NOTE);
    t.type(a, "\nx");
    let answer!: (c: CloseChoice) => void;
    const first = t.sync.requestClose(t.closeHost(() => new Promise<CloseChoice>((r) => (answer = r))));
    await tick(5);
    expect(await t.sync.requestClose(t.closeHost(() => "discard"))).toBe("busy");
    expect(await t.sync.requestClose(t.closeHost(() => "discard"))).toBe("busy");
    expect(t.asked).toHaveLength(1);
    expect(t.control.closeAcks).toBe(3);
    expect(t.control.quits).toBe(0);
    answer("cancel");
    expect(await first).toBe("cancelled");
    // The sequence can run again after a cancellation.
    expect(await t.sync.requestClose(t.closeHost(() => "discard"))).toBe("quit");
  });
});

describe("BUG-18: closing a tab while its save is in flight", () => {
  it("waits for the outcome and keeps the text when the save fails", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.type(a, "\nprécieux");
    t.control.saveDelayMs = 20;
    t.control.failSaves.push(io(), io());
    const inFlight = t.sync.save(a);
    expect(a.saving).not.toBeNull();
    // What closeDoc does before deciding anything.
    expect(await t.sync.flush(a)).toBe(false);
    await inFlight;
    expect(t.sync.unsaved(a)).toBe(true);
    expect(a.saveError).toBe("io");
    expect(a.error).toMatch(/Échec de l'enregistrement/);
    expect(a.text.toString()).toMatch(/précieux$/);
  });

  it("reports nothing left when the save in flight succeeds", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.type(a, "\nok");
    t.control.saveDelayMs = 20;
    void t.sync.save(a);
    expect(await t.sync.flush(a)).toBe(true);
    expect(t.control.saves).toHaveLength(1);
  });
});

describe("BUG-2: reload after an external change", () => {
  it("does not overwrite what was typed while the disk was being read", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.control.readDelayMs = 20;
    t.control.touch(NOTE, "# Version du disque\n");
    t.sync.onFileChanged(a, Date.now() + 10);
    await tick(5);
    t.type(a, "\nfrappe pendant la lecture");
    await tick(60);
    expect(a.text.toString()).toMatch(/frappe pendant la lecture$/);
    expect(a.conflict).toBe("external");
    expect(a.dirty).toBe(true);
    expect(t.clock.delays()).toEqual([]); // no autosave over the external version
    expect(t.toasts).toEqual([]);
  });

  it("replaces a clean document and says so", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.control.touch(NOTE, "# Version du disque\n");
    t.sync.onFileChanged(a, Date.now() + 10);
    await tick(20);
    expect(a.text.toString()).toBe("# Version du disque\n");
    expect(a.dirty).toBe(false);
    expect(a.conflict).toBeNull();
    expect(t.toasts).toHaveLength(1);
  });
});

describe("BUG-7: a failed save is retried", () => {
  it("retries io failures with a growing delay while the document stays modified", async () => {
    const t = setup({ autosave: false });
    const a = await t.open(NOTE);
    t.type(a, "\nà ne pas perdre");
    t.control.failSaves.push(io(), io(), io(), io(), io());
    await t.sync.save(a);
    const seen: number[] = [];
    for (let i = 0; i < 5; i++) {
      expect(a.saveError).toBe("io");
      expect(a.error).not.toBeNull(); // the failure stays visible while it lasts
      expect(t.clock.delays()).toHaveLength(1);
      seen.push(t.clock.delays()[0]);
      t.clock.fire();
      await tick(2);
      await t.sync.settle(a);
    }
    expect(seen).toEqual([2000, 5000, 15000, 30000, 30000]);
    expect(a.dirty).toBe(false);
    expect(a.saveError).toBeNull();
    expect(a.error).toBeNull();
    expect(t.clock.delays()).toEqual([]);
    expect(t.control.textOf(NOTE)).toMatch(/à ne pas perdre$/);
    expect([1, 2, 3, 4, 9].map(retryDelay)).toEqual([2000, 5000, 15000, 30000, 30000]);
  });

  it("does not retry invalid nor conflict", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.type(a, "\nx");
    t.clock.fire();
    await t.sync.settle(a);
    t.type(a, "y");
    t.control.failSaves.push({ kind: "invalid", message: "refused" });
    await t.sync.save(a);
    expect(a.saveError).toBe("invalid");
    expect(t.clock.delays()).toEqual([]);
    t.control.touch(NOTE, "# externe\n");
    await t.sync.save(a);
    expect(a.conflict).toBe("save");
    expect(t.clock.delays()).toEqual([]);
    expect(t.control.textOf(NOTE)).toBe("# externe\n");
  });
});

describe("BUG-14: an open file disappears", () => {
  it("is flagged and never recreated silently", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.type(a, "\nmodif");
    t.control.remove(NOTE);
    t.sync.onFileChanged(a, 0, true);
    expect(a.conflict).toBe("deleted");
    expect(t.clock.delays()).toEqual([]);
    await t.sync.save(a); // autosave or Ctrl+S
    expect(t.control.textOf(NOTE)).toBeNull();
    expect(await t.sync.flush(a)).toBe(false);
    // Explicit choice in the banner: "save anyway at this location".
    await t.sync.save(a, true);
    expect(a.conflict).toBeNull();
    expect(t.control.textOf(NOTE)).toMatch(/modif$/);
  });

  it("a save that finds the file gone says deleted, not changed", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.type(a, "\nmodif");
    t.control.remove(NOTE); // the event has not arrived yet
    await t.sync.save(a);
    expect(a.conflict).toBe("deleted");
    expect(t.control.textOf(NOTE)).toBeNull();
  });

  it("a deletion reported during a save is not lost", async () => {
    const t = setup();
    const a = await t.open(NOTE);
    t.type(a, "\nmodif");
    t.control.saveDelayMs = 15;
    const saving = t.sync.save(a);
    t.sync.onFileChanged(a, 0, true);
    await saving;
    expect(a.conflict).toBe("deleted");
  });
});

describe("BUG-12: line endings", () => {
  const roundTrip = (raw: string) => {
    const d = decodeText(raw);
    return encodeText(Text.of(d.text.split(/\r\n?|\n/)), { bom: d.bom, eol: d.eol, eolMixed: d.mixed }).text;
  };

  it("keeps homogeneous files as before (LF, CRLF, lone CR, BOM, no final newline)", () => {
    for (const raw of ["a\nb\n", "a\r\nb\r\n", "a\rb\r", "\uFEFFa\r\nb", "seule ligne", ""]) expect(roundTrip(raw)).toBe(raw);
    expect(decodeText("a\r\nb\r\n").mixed).toBeNull();
    expect(decodeText("a\nb").eol).toBe("\n");
    expect(decodeText("a\r\nb").eol).toBe("\r\n");
  });

  it("a mixed file saved without change is identical byte for byte", () => {
    const raw = "\uFEFFun\r\ndeux\ntrois\rquatre\n\ncinq\r\n";
    expect(decodeText(raw).mixed).not.toBeNull();
    expect(roundTrip(raw)).toBe(raw);
    // One CRLF among LF lines no longer converts the whole file.
    const one = "a\nb\r\nc\nd\n";
    expect(roundTrip(one)).toBe(one);
  });

  it("a line changed in the middle changes that line only", () => {
    const raw = "un\r\ndeux\ntrois\r\nquatre\ncinq\nsix\r\n";
    const d = decodeText(raw);
    expect(d.eol).toBe("\n"); // 3 LF against 3 CRLF: LF on a tie
    const st = { bom: d.bom, eol: d.eol, eolMixed: d.mixed };
    const lines = d.text.split(/\r\n?|\n/);
    lines[2] = "TROIS";
    const out = encodeText(Text.of(lines), st);
    // Only the edited line differs; it takes the majority ending, its neighbours keep theirs.
    expect(out.text).toBe("un\r\ndeux\nTROIS\nquatre\ncinq\nsix\r\n");
    // Lines added in the middle take the majority ending too.
    lines.splice(3, 0, "nouvelle");
    expect(encodeText(Text.of(lines), st).text).toBe("un\r\ndeux\nTROIS\nnouvelle\nquatre\ncinq\nsix\r\n");
    // A line removed: the others keep their endings.
    const fewer = d.text.split(/\r\n?|\n/);
    fewer.splice(1, 1);
    expect(encodeText(Text.of(fewer), st).text).toBe("un\r\ntrois\r\nquatre\ncinq\nsix\r\n");
    // Text appended after the last line.
    const more = d.text.split(/\r\n?|\n/);
    more.push("fin");
    expect(encodeText(Text.of(more), st).text).toBe(raw + "\nfin");
  });

  it("through a save: the written endings become the reference of the next save", async () => {
    const t = setup();
    const raw = "un\r\ndeux\ntrois\ncr seul\rfin\n";
    t.control.setFile(NOTE, raw);
    const a = await t.open(NOTE);
    a.dirty = true;
    await t.sync.save(a);
    expect(t.control.textOf(NOTE)).toBe(raw);
    a.text = Text.of(["un", "deux modifié", "trois", "cr seul", "fin", ""]);
    a.dirty = true;
    await t.sync.save(a);
    expect(t.control.textOf(NOTE)).toBe("un\r\ndeux modifié\ntrois\ncr seul\rfin\n");
    a.text = Text.of(["un", "deux modifié", "trois", "cr seul", "fin !", ""]);
    a.dirty = true;
    await t.sync.save(a);
    expect(t.control.textOf(NOTE)).toBe("un\r\ndeux modifié\ntrois\ncr seul\rfin !\n");
  });
});

describe("SEC-3 / BUG-21: read-only documents and refused files", () => {
  it("the mock answers like the contract: readOnly, invalid paths, size limit, Markdown-only saves", async () => {
    const { backend, control } = createMock(new URLSearchParams("tree=40&delay=0"));
    expect((await backend.readFile(NOTE)).readOnly).toBe(false);
    expect((await backend.readFile(OUTSIDE)).readOnly).toBe(false); // opened explicitly
    expect((await backend.readFile("C:\\Temp\\Notes\\voisin.md")).readOnly).toBe(true); // reached through a link
    expect((await backend.readFile("C:\\Temp\\Notes\\memo.txt")).readOnly).toBe(true); // not Markdown
    expect((await backend.readFile(ROOT + "\\00__DEMO\\Carte.canvas")).readOnly).toBe(true);
    await expect(backend.readFile("C:\\Prive\\secret.md")).rejects.toMatchObject({ kind: "invalid" });
    await expect(backend.readFile("\\\\serveur\\partage\\a.md")).rejects.toMatchObject({ kind: "invalid" });
    await expect(backend.saveFile("C:\\Temp\\Notes\\voisin.md", "x")).rejects.toMatchObject({ kind: "invalid" });
    await expect(backend.saveFile("C:\\Temp\\Notes\\memo.txt", "x")).rejects.toMatchObject({ kind: "invalid" });
    await expect(backend.saveFile(ROOT + "\\note.txt", "x")).rejects.toMatchObject({ kind: "invalid" });
    await expect(backend.openVault("D:\\Ailleurs")).rejects.toMatchObject({ kind: "invalid" });
    await expect(backend.buildTags("D:\\Ailleurs")).rejects.toMatchObject({ kind: "invalid" });
    control.setFile(ROOT + "\\gros.md", "x", 51 * 1024 * 1024);
    const err = await backend.readFile(ROOT + "\\gros.md").catch((e) => e as ApiError);
    expect(err).toMatchObject({ kind: "invalid" });
    expect(openErrorMessage(err as ApiError)).toMatch(/^Fichier trop volumineux/);
    await backend.setSession({ activeVault: "D:\\Ailleurs", tabs: [NOTE, "C:\\Prive\\secret.md"], activeTab: "C:\\Prive\\secret.md" });
    expect(control.sessions[0]).toEqual({ activeVault: null, tabs: [NOTE], activeTab: null });
  });

  it("gives a clear message for a path that is not allowed", async () => {
    const { backend } = createMock(new URLSearchParams("tree=40&delay=0"));
    const err = (await backend.readFile("C:\\Prive\\secret.md").catch((e) => e)) as ApiError;
    expect(openErrorMessage(err)).toMatch(/^Ouverture refusée : ce fichier n'est ni dans un vault enregistré/);
    expect(err.code).toBe("not_allowed");
    const net = (await backend.readFile("\\\\serveur\\partage\\a.md").catch((e) => e)) as ApiError;
    expect(net).toMatchObject({ kind: "invalid", code: "network_path" });
    expect(openErrorMessage(net)).toMatch(/^Chemin réseau refusé/);
    expect(openErrorMessage({ kind: "not_found", message: "x does not exist" })).toMatch(/^Fichier introuvable/);
  });

  it("add_vault only accepts a folder returned by the folder dialog (or a vault already registered)", async () => {
    const { backend } = createMock(new URLSearchParams("tree=40&delay=0"));
    const before = await backend.listVaults();
    // A folder the page made up: refused, nothing registered.
    await expect(backend.addVault("C:\\Users")).rejects.toMatchObject({ kind: "invalid", code: "not_allowed" });
    expect(await backend.listVaults()).toEqual(before);
    await expect(backend.openVault("C:\\Users")).rejects.toMatchObject({ kind: "invalid" });
    // Already registered: no new right, accepted.
    expect(await backend.addVault(ROOT)).toEqual(before);
    // Chosen in the dialog: accepted.
    const picked = (await backend.pickFolder())!;
    const after = await backend.addVault(picked);
    expect(after.map((v) => v.path)).toContain(picked);
  });

  it("recognises errors by their code, whatever the wording of the message", async () => {
    // The message is free text: reworded or translated, the UI reacts the same way.
    expect(openErrorMessage({ kind: "invalid", message: "trop gros", code: "too_large" })).toMatch(/^Fichier trop volumineux/);
    expect(openErrorMessage({ kind: "invalid", message: "refus", code: "network_path" })).toMatch(/^Chemin réseau refusé/);
    expect(openErrorMessage({ kind: "invalid", message: "refus", code: "not_allowed" })).toMatch(/^Ouverture refusée : ce fichier/);
    // And the wording alone decides nothing.
    expect(openErrorMessage({ kind: "invalid", message: "too large, network path, not allowed" })).toMatch(/^Ouverture refusée \(/);
    expect(saveErrorMessage({ kind: "invalid", message: "x", code: "read_only" })).toMatch(/lecture seule/);

    const t = setup();
    const doc = await t.open(NOTE);
    // A save refused because the file is gone: `code: "gone"`, not a phrase of the message.
    t.type(doc, "\nA");
    t.control.failSaves.push({ kind: "conflict", message: "le fichier a disparu", code: "gone" });
    await t.sync.save(doc);
    expect(doc.conflict).toBe("deleted");
    doc.conflict = null;
    t.control.failSaves.push({ kind: "conflict", message: "x no longer exists on disk" });
    await t.sync.save(doc);
    expect(doc.conflict).toBe("save");
  });

  it("a read-only document is never saved, by autosave or on request", async () => {
    const t = setup();
    const a = await t.open("C:\\Temp\\Notes\\voisin.md");
    expect(a.readOnly).toBe(true);
    a.text = Text.of(["forcé"]);
    a.dirty = true;
    t.sync.scheduleAutosave(a);
    expect(t.clock.delays()).toEqual([]);
    t.control.calls.length = 0;
    await t.sync.save(a);
    await t.sync.save(a, true);
    expect(t.control.calls).not.toContain("save_file");
    expect(t.sync.unsaved(a)).toBe(false); // closing never asks about it
  });

  it("the editor state of a read-only document refuses every change, except the content coming from the disk", () => {
    const host = { onUpdate: () => undefined } as never;
    const locked = createEditorState("contenu", "C:\\x\\a.txt", host, true);
    expect(locked.readOnly).toBe(true);
    expect(locked.update({ changes: { from: 0, insert: "X" } }).state.doc.toString()).toBe("contenu");
    expect(locked.update({ changes: { from: 0, to: 7, insert: "disque" }, filter: false }).state.doc.toString()).toBe("disque");
    const free = createEditorState("contenu", "C:\\x\\a.md", host);
    expect(free.readOnly).toBe(false);
    expect(free.update({ changes: { from: 0, insert: "X" } }).state.doc.toString()).toBe("Xcontenu");
  });
});

/** Opens the demo vault in the mock and waits for the index. */
async function indexedVault(params = "tree=400&delay=0&searchdelay=0"): Promise<{ backend: Backend; control: MockControl; events: { [K in keyof BackendEvents]?: BackendEvents[K][] } }> {
  const { backend, control } = createMock(new URLSearchParams(params));
  const events: Record<string, unknown[]> = {};
  for (const name of ["tags-progress", "tags-ready", "index-ready"] as const) backend.on(name, (p) => (events[name] ??= []).push(p));
  await backend.openVault(ROOT);
  await backend.buildTags(ROOT);
  for (let i = 0; i < 400 && !control.searchReady; i++) await tick(5);
  expect(control.searchReady).toBe(true);
  return { backend, control, events };
}

describe("PERF-10: tag filter on the cached list", () => {
  it("gives the same list as tags_list for every query", async () => {
    const { backend } = await indexedVault();
    const lib = makeTagLibrary(await backend.tagsList(""));
    expect(lib.tags.length).toBeGreaterThan(20);
    const queries = ["", "p", "pr", "pro", "proj", "projet", "projet/", "projet/a", "#projet", "##pro", "  Re ", "RE", "idee", "idée", "e", "a", "t", "/", "todo", "demo/apercu", "zzz", "reunion/h", "n/a"];
    for (const q of queries) expect(filterTagLibrary(lib, q), `query "${q}"`).toEqual(await backend.tagsList(q));
  });

  it("ranks like the core: starts with, then a level starts with, then contains; by count, then key", () => {
    const lib = makeTagLibrary([
      { tag: "Travail", count: 9 },
      { tag: "a/b", count: 7 },
      { tag: "abri", count: 5 },
      { tag: "a/B/abc", count: 4 },
      { tag: "cab", count: 4 },
      { tag: "b", count: 3 },
      { tag: "Bac", count: 3 },
      { tag: "xa/a/a", count: 1 },
    ]);
    const names = (q: string) => filterTagLibrary(lib, q).map((t) => t.tag);
    expect(names("")).toHaveLength(8);
    // "b": prefix (b then Bac: same count, key order), level prefix (a/b, a/B/abc), then substring (abri, cab, Travail has none).
    expect(names("b")).toEqual(["b", "Bac", "a/b", "a/B/abc", "abri", "cab"]);
    expect(names("#B")).toEqual(names("b"));
    expect(names("ab")).toEqual(["abri", "a/B/abc", "cab"]);
    // Non-overlapping occurrences, as the core scans them: the second "a/a" overlaps the first one.
    expect(names("a/a")).toEqual(["xa/a/a"]);
    expect(names("é")).toEqual([]); // case-insensitive, not accent-insensitive
    expect(names("TRAV")).toEqual(["Travail"]);
  });
});

describe("BUG-15: index events of another vault", () => {
  it("every index event names its vault, and events of a vault that is not the active one are ignored", async () => {
    const { events } = await indexedVault("tree=60&delay=0&searchdelay=0");
    const all = [...(events["tags-progress"] ?? []), ...(events["tags-ready"] ?? []), ...(events["index-ready"] ?? [])];
    expect(all.length).toBeGreaterThan(2);
    for (const p of all) {
      expect(p.vault).toBe(ROOT);
      expect(isForVault(ROOT, p)).toBe(true);
      expect(isForVault("c:/vaults/demo/", p)).toBe(true); // same folder, other spelling
      expect(isForVault("D:\\Vaults\\Second", p)).toBe(false); // the user switched vault meanwhile
      expect(isForVault(null, p)).toBe(false); // vault closed
    }
    expect(isForVault(ROOT, {})).toBe(false);
  });

  it("close_vault stops the events of that vault (BUG-16)", async () => {
    const { backend, control } = createMock(new URLSearchParams("tree=3000&delay=0"));
    const seen: string[] = [];
    backend.on("tags-ready", (p) => seen.push(p.vault));
    await backend.openVault(ROOT);
    await backend.buildTags(ROOT);
    await backend.closeVault(ROOT);
    await tick(80);
    expect(control.closedVaults).toEqual([ROOT]);
    expect(seen).toEqual([]);
  });
});

describe("PERF-7 / PERF-5: what the mock reports", () => {
  it("linksGeneration moves only when links change", async () => {
    const { backend, control, events } = await indexedVault("tree=60&delay=0&searchdelay=0");
    const last = () => events["index-ready"]![events["index-ready"]!.length - 1].linksGeneration;
    const g0 = last();
    const text = control.textOf(NOTE)!;
    await backend.saveFile(NOTE, text + "\nUne phrase sans lien.\n");
    await tick(50);
    expect(last()).toBe(g0);
    await backend.saveFile(NOTE, text + "\nVoir [[Callouts]].\n");
    await tick(50);
    expect(last()).toBe(g0 + 1);
  });

  it("startup_info carries the first file, identical to read_file", async () => {
    const { backend } = createMock(new URLSearchParams("tree=40&delay=0&open=00__DEMO/Journal.md"));
    const info = await backend.startupInfo();
    expect(info.firstFile).toEqual(await backend.readFile(NOTE));
    const none = createMock(new URLSearchParams("tree=40&delay=0&firstfile=0"));
    expect((await none.backend.startupInfo()).firstFile).toBeNull();
    const fresh = createMock(new URLSearchParams("tree=40&delay=0&start=fresh"));
    expect((await fresh.backend.startupInfo()).firstFile).toBeNull();
  });
});

describe("BUG-5(b): tree updates that list the same files", () => {
  it("detects an unchanged set of paths", () => {
    const a = [
      { path: "Notes", isDir: true },
      { path: "Notes/a.md", isDir: false },
    ];
    expect(sameTreePaths(a, a.map((e) => ({ ...e })))).toBe(true);
    expect(sameTreePaths(a, [...a, { path: "Notes/b.md", isDir: false }])).toBe(false);
    expect(sameTreePaths(a, [a[0], { path: "Notes/A.md", isDir: false }])).toBe(false);
    expect(sameTreePaths(a, [a[0], { path: "Notes/a.md", isDir: true }])).toBe(false);
  });
});

describe("SEC-9 / SEC-4: canvas files and network links", () => {
  const index = new FileIndex([
    { path: "Notes/a.md", isDir: false },
    { path: "_ASSETS/schema.png", isDir: false },
  ]);
  const inVault = { fileAbs: "C:\\Vault\\Cartes\\c.canvas", vaultRoot: "C:\\Vault", index };

  it("a canvas file is resolved by the vault index and stays under the vault root", () => {
    expect(resolveCanvasFile("Notes/a.md", inVault)).toBe("C:\\Vault\\Notes\\a.md");
    expect(resolveCanvasFile("notes/A.md", inVault)).toBe("C:\\Vault\\Notes\\a.md"); // the path of the tree, not the one written
    expect(resolveCanvasFile("Notes/absent.md", inVault)).toBeNull(); // not a file of the vault
    expect(resolveCanvasFile("../Ailleurs/x.md", inVault)).toBeNull();
    expect(resolveCanvasFile("Notes/../../x.md", inVault)).toBeNull();
    expect(resolveCanvasFile("C:\\Windows\\win.ini", inVault)).toBeNull();
    expect(resolveCanvasFile("//serveur/partage/a.png", inVault)).toBeNull();
    expect(resolveCanvasFile("Notes/a.md", { ...inVault, index: null })).toBeNull(); // tree not known yet: nothing is touched
  });

  it("outside a vault, a canvas file stays under the folder of the canvas", () => {
    const outside = { fileAbs: "C:\\Temp\\Notes\\c.canvas", vaultRoot: null, index: null };
    expect(resolveCanvasFile("img/a.png", outside)).toBe("C:\\Temp\\Notes\\img\\a.png");
    expect(resolveCanvasFile("../secret.md", outside)).toBeNull();
    // A canvas outside the open vault does not use the vault index.
    expect(resolveCanvasFile("Notes/a.md", { fileAbs: "C:\\Temp\\c.canvas", vaultRoot: "C:\\Vault", index })).toBe("C:\\Temp\\Notes\\a.md");
  });

  it("recognises network sources of links and images", () => {
    for (const s of ["//serveur/partage/a.md", "\\\\serveur\\partage\\a.md", "<//serveur/p/a.png>", "%5C%5Cserveur%5Cp%5Ca.md", "file://serveur/p/a.md", "file:////serveur/p/a.md", "\\\\?\\C:\\a.md"]) {
      expect(isNetworkSource(s), s).toBe(true);
    }
    for (const s of ["a.md", "dossier/a.md", "C:\\a\\b.md", "/home/a.md", "file:///C:/a/b.md", "https://exemple.test//a", "Note"]) expect(isNetworkSource(s), s).toBe(false);
  });
});
