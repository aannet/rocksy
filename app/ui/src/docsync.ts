// Saving, reloading, external changes and the closing sequence of the open documents.
// No DOM here: the application shell (app.ts) supplies the editor text and repaints; the tests drive this
// module with the mock backend.
import type { Text } from "@codemirror/state";
import type { Backend } from "./backend";
import { toApiError } from "./backend";
import type { ApiError, ApiErrorKind } from "./types";
import { decodeText, encodeText, type Eol, type MixedEol } from "./eol";

/** `save`: refused because the file changed on disk; `external`: changed on disk while modified here; `deleted`: gone from disk. */
export type Conflict = null | "save" | "external" | "deleted";

export interface SyncDoc {
  path: string;
  name: string;
  kind: "md" | "canvas" | "image";
  loaded: boolean;
  /** `save_file` would be refused (non-Markdown extension, or writing not allowed): no editing, no autosave. */
  readOnly: boolean;
  error: string | null;
  /** Kind of the last failed save, for as long as the failure lasts (`io` is retried). */
  saveError: ApiErrorKind | null;
  retries: number;
  mtimeMs: number;
  eol: Eol;
  eolMixed: MixedEol | null;
  bom: boolean;
  dirty: boolean;
  /** The save in flight, if any. Never rejects. */
  saving: Promise<void> | null;
  conflict: Conflict;
  pendingExternal: { mtimeMs: number; deleted: boolean } | null;
  /** Autosave or retry timer. */
  timer: number;
}

export interface SyncHost<D extends SyncDoc> {
  backend: Pick<Backend, "readFile" | "saveFile">;
  autosave(): boolean;
  /** Current content of a Markdown document: an immutable value, replaced by every edit. Null for viewers. */
  textOf(doc: D): Text | null;
  /** Replaces the content with text read from disk (BOM removed). Not an edit: the document does not become dirty. */
  applyDisk(doc: D, text: string): void;
  /** A state shown to the user changed (tab, banner, status bar). */
  refresh(doc: D): void;
  /** A save succeeded and nothing is left to save. */
  saved(doc: D): void;
  toast(message: string): void;
}

export interface Timers {
  set(fn: () => void, ms: number): number;
  clear(id: number): void;
}

export const AUTOSAVE_MS = 2000;
const RETRY_DELAYS_MS = [2000, 5000, 15000, 30000];

/** Delay before the n-th retry (1-based) of a save that failed with an `io` error: 2 s, 5 s, 15 s, then every 30 s. */
export function retryDelay(attempt: number): number {
  return RETRY_DELAYS_MS[Math.min(Math.max(1, attempt), RETRY_DELAYS_MS.length) - 1];
}

/** Message for a file that could not be opened, chosen on the stable `code` of the error. The backend text (English, with the path) is kept as the detail. */
export function openErrorMessage(err: ApiError): string {
  const detail = err.message ? ` (${err.message})` : "";
  if (err.kind === "invalid") {
    if (err.code === "too_large") return `Fichier trop volumineux : Rocksy n'ouvre pas les fichiers de plus de 50 Mo${detail}`;
    if (err.code === "network_path") return `Chemin réseau refusé : ce fichier n'est pas chargé${detail}`;
    if (err.code === "not_allowed") return `Ouverture refusée : ce fichier n'est ni dans un vault enregistré, ni un fichier que vous avez ouvert, ni à côté d'un tel fichier${detail}`;
    return `Ouverture refusée${detail}`;
  }
  if (err.kind === "not_found") return `Fichier introuvable${detail}`;
  if (err.kind === "not_utf8") return `Ce fichier n'est pas en UTF-8 : il n'est pas ouvert (aucune conversion silencieuse)${detail}`;
  return err.message || "Erreur de lecture";
}

export function saveErrorMessage(err: ApiError): string {
  if (err.code === "read_only") return `Enregistrement refusé : ce fichier est en lecture seule (${err.message})`;
  if (err.code === "not_allowed" || err.code === "network_path") return `Enregistrement refusé : ce fichier n'est ni dans un vault enregistré, ni un fichier que vous avez ouvert (${err.message})`;
  if (err.kind === "invalid") return `Enregistrement refusé : ${err.message}`;
  return `Échec de l'enregistrement : ${err.message}`;
}

export type CloseChoice = "save" | "discard" | "cancel";
/** `failed`: the save failed; `conflict`: changed or deleted on disk; `manual`: autosave is off and changes remain. */
export interface CloseIssue<D> {
  doc: D;
  reason: "failed" | "conflict" | "manual";
}

export interface CloseHost<D extends SyncDoc> {
  docs(): D[];
  closeAck(): Promise<void>;
  quit(): Promise<void>;
  /** Application dialog (never `window.confirm`). `canSave` is false when only conflicts remain. */
  ask(issues: CloseIssue<D>[], canSave: boolean): Promise<CloseChoice>;
}

export class DocSync<D extends SyncDoc> {
  private closing = false;

  constructor(
    private host: SyncHost<D>,
    private timers: Timers = { set: (fn, ms) => setTimeout(fn, ms) as unknown as number, clear: (id) => clearTimeout(id) },
  ) {}

  /** True when the document holds text that is not on disk. */
  unsaved(doc: D): boolean {
    return doc.loaded && doc.kind === "md" && doc.dirty && !doc.readOnly;
  }

  cancelTimer(doc: D): void {
    this.timers.clear(doc.timer);
    doc.timer = 0;
  }

  /** Called after each edit. With autosave off, a pending retry of a failed save is left alone. */
  scheduleAutosave(doc: D): void {
    if (doc.readOnly || doc.conflict || !this.host.autosave()) return;
    this.timers.clear(doc.timer);
    doc.timer = this.timers.set(() => void this.save(doc), AUTOSAVE_MS);
  }

  /** Waits for the save in flight, if any. */
  async settle(doc: D): Promise<void> {
    while (doc.saving) await doc.saving;
  }

  /**
   * Saves the document and resolves once the backend has answered (never rejects; the outcome is in
   * `dirty`, `conflict`, `saveError`). A save already in flight is awaited first, then the newer text is saved.
   * `force` writes without the mtime check: "overwrite" after a conflict, or recreating a deleted file.
   */
  save(doc: D, force = false): Promise<void> {
    if (doc.saving) return doc.saving.then(() => this.save(doc, force));
    if (!doc.loaded || doc.kind !== "md" || doc.readOnly) return Promise.resolve();
    if (!force && (!doc.dirty || doc.conflict)) return Promise.resolve();
    const snapshot = this.host.textOf(doc);
    if (!snapshot) return Promise.resolve();
    this.cancelTimer(doc);
    // The text is captured and `saving` is set before this returns: a caller that closes right away sees the save in flight.
    const job = this.write(doc, snapshot, force);
    doc.saving = job;
    this.host.refresh(doc);
    return job;
  }

  private async write(doc: D, snapshot: Text, force: boolean): Promise<void> {
    const enc = encodeText(snapshot, doc);
    let ok = false;
    try {
      const r = await this.host.backend.saveFile(doc.path, enc.text, force ? undefined : doc.mtimeMs);
      ok = true;
      doc.mtimeMs = r.mtimeMs;
      doc.conflict = null;
      doc.error = null;
      doc.saveError = null;
      doc.retries = 0;
      if (enc.mixed) doc.eolMixed = enc.mixed;
      const now = this.host.textOf(doc);
      doc.dirty = !!now && now !== snapshot;
    } catch (e) {
      const err = toApiError(e);
      if (err.kind === "conflict") {
        doc.conflict = err.code === "gone" ? "deleted" : "save";
        doc.saveError = null;
        doc.error = null;
      } else {
        doc.saveError = err.kind;
        doc.error = saveErrorMessage(err);
      }
    } finally {
      doc.saving = null;
    }
    const ext = doc.pendingExternal;
    doc.pendingExternal = null;
    if (ext && (ext.deleted || ext.mtimeMs !== doc.mtimeMs) && !(doc.conflict && !ext.deleted)) this.onFileChanged(doc, ext.mtimeMs, ext.deleted);
    if (ok && !doc.dirty) this.host.saved(doc);
    else if (doc.dirty && !doc.conflict) {
      if (!doc.saveError) this.scheduleAutosave(doc);
      else if (doc.saveError === "io") {
        // Transient failure (file locked by the sync client or an antivirus, disk full): try again, less and less often.
        doc.retries++;
        this.timers.clear(doc.timer);
        doc.timer = this.timers.set(() => void this.save(doc, force), retryDelay(doc.retries));
      }
    }
    this.host.refresh(doc);
  }

  /** Waits for a save in flight, then saves what is still unsaved. True when nothing is left unsaved. */
  async flush(doc: D): Promise<boolean> {
    await this.settle(doc);
    if (this.unsaved(doc) && !doc.conflict) await this.save(doc);
    return !this.unsaved(doc);
  }

  /**
   * Replaces the document with the content on disk. If the text changed while the disk was being read
   * (the user typed), nothing is replaced: the document goes to the `external` conflict instead.
   * Returns true when the document now shows the disk content.
   */
  async reload(doc: D): Promise<boolean> {
    await this.settle(doc);
    const before = this.host.textOf(doc);
    try {
      const fc = await this.host.backend.readFile(doc.path);
      if (this.host.textOf(doc) !== before) {
        doc.conflict = "external";
        this.cancelTimer(doc);
        this.host.refresh(doc);
        return false;
      }
      const dec = decodeText(fc.text);
      doc.bom = dec.bom;
      doc.eol = dec.eol;
      doc.eolMixed = dec.mixed;
      doc.mtimeMs = fc.mtimeMs;
      doc.readOnly = !!fc.readOnly;
      this.host.applyDisk(doc, dec.text);
      doc.dirty = false;
      doc.conflict = null;
      doc.error = null;
      doc.saveError = null;
      doc.retries = 0;
      this.cancelTimer(doc);
      this.host.refresh(doc);
      return true;
    } catch (e) {
      const err = toApiError(e);
      if (err.kind === "not_found") {
        doc.conflict = "deleted";
        this.cancelTimer(doc);
      } else doc.error = `Rechargement impossible : ${err.message}`;
      this.host.refresh(doc);
      return false;
    }
  }

  /** `file-changed`: an open file changed on disk, or disappeared (`deleted`). */
  onFileChanged(doc: D, mtimeMs: number, deleted = false): void {
    if (!doc.loaded) return;
    if (doc.saving) {
      doc.pendingExternal = { mtimeMs, deleted };
      return;
    }
    if (deleted) {
      if (doc.conflict === "deleted") return;
      doc.conflict = "deleted";
      this.cancelTimer(doc);
      this.host.refresh(doc);
      return;
    }
    if (doc.kind === "image") return;
    // A file that comes back after a deletion is compared again, even with the same mtime.
    if (mtimeMs === doc.mtimeMs && doc.conflict !== "deleted") return;
    if (!doc.dirty) {
      void this.reload(doc).then((shown) => {
        if (shown) this.host.toast(`« ${doc.name} » rechargé (modifié sur le disque)`);
      });
    } else {
      doc.conflict = "external";
      this.cancelTimer(doc);
      this.host.refresh(doc);
    }
  }

  /**
   * Closing sequence (`close-requested`): acknowledge at once, save every modified document and wait for each
   * answer, ask when something cannot be saved silently, then `quit`. A request received while a sequence is
   * running is acknowledged and otherwise ignored (no second dialog).
   */
  async requestClose(h: CloseHost<D>, autosave = this.host.autosave()): Promise<"quit" | "cancelled" | "busy"> {
    void h.closeAck().catch(() => undefined);
    if (this.closing) return "busy";
    this.closing = true;
    try {
      let saveAll = autosave;
      for (;;) {
        await Promise.all(h.docs().map((d) => (saveAll ? this.flush(d) : this.settle(d))));
        const issues: CloseIssue<D>[] = h
          .docs()
          .filter((d) => this.unsaved(d))
          .map((doc) => ({ doc, reason: doc.conflict ? "conflict" : doc.saveError ? "failed" : "manual" }));
        if (!issues.length) break;
        const choice = await h.ask(
          issues,
          issues.some((i) => i.reason !== "conflict"),
        );
        if (choice === "cancel") return "cancelled";
        if (choice === "discard") break;
        saveAll = true;
      }
      await h.quit();
      return "quit";
    } finally {
      this.closing = false;
    }
  }
}
