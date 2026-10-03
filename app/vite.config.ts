import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed dev-server port (see src-tauri/tauri.conf.json).
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  build: {
    // WebView2 is Chromium: no need for legacy transforms.
    target: "chrome110",
    sourcemap: false,
  },
});
