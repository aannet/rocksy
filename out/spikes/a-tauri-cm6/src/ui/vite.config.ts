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
  test: { environment: "node", include: ["tests/**/*.test.ts"] },
} as any);
