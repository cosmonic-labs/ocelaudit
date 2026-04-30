import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

// During `vite dev`, proxy /api to the wash-dev gateway so the SPA can
// run hot-reloaded against the real backend. In `vite build` the SPA
// only emits static files; the gateway serves them from disk in prod.
export default defineConfig({
  plugins: [preact()],
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8000",
        changeOrigin: true,
      },
      "/healthz": {
        target: "http://127.0.0.1:8000",
      },
    },
  },
  build: {
    outDir: "dist",
    target: "es2022",
    sourcemap: false,
    cssCodeSplit: false,
  },
});
