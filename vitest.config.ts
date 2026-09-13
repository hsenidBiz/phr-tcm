import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  resolve: {
    // `suiteCases.ts` (query/loading helpers) and `SuiteCases.tsx` (the
    // component) differ only in the case of one letter. NTFS resolves
    // filenames case-insensitively, so an extensionless "./SuiteCases"
    // import tries ".ts" before ".tsx" by default and silently lands on
    // the wrong file on Windows. Trying ".tsx" first fixes that pair
    // without touching any other resolution (it is the only same-name
    // .ts/.tsx pair in the project - see the case-fold check that would
    // catch a new one).
    extensions: [".tsx", ".ts", ".mjs", ".js", ".mts", ".jsx", ".json"],
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
  },
});
