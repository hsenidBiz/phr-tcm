import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    // Playwright owns visual/ (npm run visual); vitest must not try to
    // load @playwright/test specs - that fails the release gate.
    exclude: ["**/node_modules/**", "visual/**"],
  },
});
