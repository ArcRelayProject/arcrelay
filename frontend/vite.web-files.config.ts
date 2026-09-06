import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "node:path";

export default defineConfig({
  root: resolve(import.meta.dirname),
  plugins: [svelte()],
  clearScreen: false,
  server: {
    proxy: {
      "/api": {
        target: process.env.WEB_FILES_GATEWAY_TARGET ?? "http://127.0.0.1:8767",
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: resolve(import.meta.dirname, "../../arcrelay-web-gateway/assets"),
    emptyOutDir: true,
    sourcemap: false,
    rollupOptions: {
      input: {
        webFiles: resolve(import.meta.dirname, "web-files.html"),
      },
    },
  },
});
