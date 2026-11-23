// Startup order (KPI no. 1 is "note editable"):
//   1. startup_info
//   2. open the file to show (first argFile, else session.activeTab), render the editor, log "editor-ready"
//   3. only then: tree (open_vault), then tags (build_tags), then lazy panels and editor extras.
import "./styles.css";
import { getBackend } from "./backend";
import { App } from "./app";
import { samePath } from "./paths";

declare global {
  interface Window {
    rocksy?: App;
    rocksyReady?: boolean;
  }
}

async function boot(): Promise<void> {
  const backend = await getBackend();
  const info = await backend.startupInfo();
  const app = new App(backend, info);
  window.rocksy = app;

  const session = info.session;
  const first = info.argFiles[0] ?? session?.activeTab ?? null;
  // A file passed on the command line opens alone: the vault is not loaded (fastest path, F8).
  const standalone = info.argFiles.length > 0;
  const vault = !standalone && session?.activeVault && info.vaults.some((v) => samePath(v.path, session.activeVault!)) ? session.activeVault : null;
  if (vault) {
    app.vaultRoot = vault;
    app.reserveSidebar();
  } else if (standalone && session?.activeVault) {
    // Kept in the session so the next ordinary launch restores it; loaded only if one of its tabs is activated.
    app.dormantVault = session.activeVault;
  }
  if (session) for (const p of session.tabs) app.addTab(p);
  for (const p of info.argFiles) app.addTab(p);

  if (first) {
    const doc = await app.openPath(first);
    if (doc && doc.loaded) {
      const sinceNav = Math.round(performance.now());
      app.metrics.editorReadyMs = sinceNav;
      void backend.logTiming("editor-ready", Date.now() - info.processStartMs);
      void backend.logTiming("editor-ready-since-navigation", sinceNav);
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          app.metrics.editorPaintedMs = Math.round(performance.now());
          void backend.logTiming("editor-painted-since-navigation", app.metrics.editorPaintedMs);
        }),
      );
    }
  } else {
    app.focused.show();
  }

  // ---- everything below is non-essential for typing
  app.buildRibbon();
  app.wireEvents();
  app.updateStatus();
  if (vault) void app.openVault(vault);
  const idle = (fn: () => void) => ("requestIdleCallback" in window ? requestIdleCallback(fn, { timeout: 600 }) : setTimeout(fn, 60));
  idle(() => {
    void app.loadExtras().then(() => {
      window.rocksyReady = true;
    });
  });
}

boot().catch((e) => {
  console.error(e);
  const ws = document.getElementById("workspace");
  if (ws) ws.innerHTML = `<div class="empty is-error"><h2>Erreur au démarrage</h2><div>${String(e?.message ?? e).replace(/</g, "&lt;")}</div></div>`;
});
