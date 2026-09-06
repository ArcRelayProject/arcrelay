import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "node:path";

export default defineConfig({
  root: resolve(import.meta.dirname),
  plugins: [svelte()],
  clearScreen: false,
  server: {
    host: "0.0.0.0",
    port: 1420,
    strictPort: true,
    allowedHosts: ["localhost", "127.0.0.1", "terminal.local"],
  },
  build: {
    outDir: resolve(import.meta.dirname, "../dist"),
    emptyOutDir: true,
    sourcemap: false,
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, "index.html"),
        clipboard: resolve(import.meta.dirname, "clipboard.html"),
        trayTransfer: resolve(import.meta.dirname, "tray-transfer.html"),
        permissionGuide: resolve(import.meta.dirname, "permission-guide.html"),
        privacyOverlay: resolve(import.meta.dirname, "privacy-overlay.html"),
      },
    },
  },
});
