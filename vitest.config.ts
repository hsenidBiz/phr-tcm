import { configDefaults, defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    // `.claude/worktrees/` holds whole checkouts of this repository, each
    // with its own node_modules. Without this exclude vitest collected
    // their test files as part of THIS suite, and every one that mounts
    // React failed with "Cannot read properties of null (reading
    // 'useState')" - two copies of React in one run. A worktree's tests
    // belong to that worktree's own run, never to the release gate here.
    // `**/` because a worktree can sit under a subdirectory too: one at
    // `v2/.claude/worktrees/` slipped past a root-anchored `.claude/**`
    // and failed the gate on untouched main with that same React error.
    exclude: [...configDefaults.exclude, "**/.claude/**"],
  },
});
