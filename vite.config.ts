import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],

  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
    // `src/screens/ManageCases/suiteCases.ts` (query/loading helpers) and
    // `SuiteCases.tsx` (the component) differ only in the case of their
    // first letter. NTFS resolves filenames case-insensitively, so an
    // extensionless "./SuiteCases" import tries ".ts" before ".tsx" by
    // default and silently lands on the wrong file on Windows. Trying
    // ".tsx" first fixes that pair without touching any other resolution
    // (it is the only same-name .ts/.tsx pair in the project).
    extensions: [".tsx", ".ts", ".mjs", ".js", ".mts", ".jsx", ".json"],
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
