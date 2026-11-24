// Index events (`tags-progress`, `tags-ready`, `index-ready`) name the vault they are about.
import { samePath } from "./paths";

/** True when an index event concerns the active vault. Events of a vault that was just left, or without a vault, are ignored. */
export function isForVault(activeVault: string | null, payload: { vault?: unknown }): boolean {
  return !!activeVault && typeof payload.vault === "string" && samePath(payload.vault, activeVault);
}
