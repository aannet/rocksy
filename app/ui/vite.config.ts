import { defineConfig } from "vite";

// Relative base: the build must run from Tauri's custom protocol and from file:// (headless tests).
export default defineConfig({
  base: "./",
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    modulePreload: false,
    chunkSizeWarningLimit: 900,
    reportCompressedSize: true,
  },
  // The tests read the cases shared with the core (../testdata); nothing else outside this folder is served.
  server: { fs: { allow: [".", "../testdata"] } },
  test: { environment: "node", include: ["tests/**/*.test.ts"] },
} as any);
