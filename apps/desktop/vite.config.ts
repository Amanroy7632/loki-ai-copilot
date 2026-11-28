import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri loads the dev server from devUrl (tauri.conf.json): fixed port, localhost only.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { host: "localhost", port: 1420, strictPort: true },
  build: { target: "es2022", sourcemap: false, outDir: "dist", emptyOutDir: true },
});
