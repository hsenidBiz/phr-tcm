import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

/**
 * The app's fonts (@fontsource, imported in src/index.css) list every face
 * as woff2 first and woff second. WebView2 is Chromium, which uses the first
 * source it can read - always the woff2 - so the woff files were never
 * fetched, yet each one was copied into dist/ and so embedded in the exe
 * (about 0.7 MB of the frontend's 2.6 MB there, measured October 2026).
 * This drops a face's woff sources when it has a woff2 one.
 *
 * `Once`, not an `AtRule` visitor: Vite's own url() step is a `Once` that
 * runs after the plugins listed here, and it is the step that copies each
 * file it finds into dist/. A visitor runs after every `Once`, which is too
 * late - the woff files would already be emitted.
 */
type CssDecl = { value: string };
type CssRule = { walkDecls: (prop: string, cb: (decl: CssDecl) => void) => void };
const woff2Only = {
  postcssPlugin: "woff2-only",
  Once(root: { walkAtRules: (name: string, cb: (rule: CssRule) => void) => void }) {
    root.walkAtRules("font-face", (rule) => {
      rule.walkDecls("src", (decl) => {
        const sources = decl.value.split(/,(?=\s*url\()/);
        const isWoff = (src: string) => /format\(\s*["']?woff["']?\s*\)/.test(src);
        const kept = sources.filter((src) => !isWoff(src));
        const hasWoff2 = kept.some((src) => /format\(\s*["']?woff2["']?\s*\)/.test(src));
        if (hasWoff2 && kept.length < sources.length) {
          decl.value = kept.map((src) => src.trim()).join(", ");
        }
      });
    });
  },
};

/**
 * Sample-data files under src/dev/ (the demo case video) are imported by
 * src/dev/demo.ts, which only dev builds load, so its code is tree-shaken out
 * of a release. Vite still emits the asset it imports, which would ship in the
 * app. This drops any emitted asset whose source lived under src/dev/, by the
 * bundle's `originalFileNames`, with the `demo-` file name as a fallback for a
 * Vite that does not report them.
 */
const devAssetsOutOfRelease = {
  name: "dev-assets-out-of-release",
  apply: "build" as const,
  generateBundle(_options: unknown, bundle: Record<string, { type: string; name?: string; originalFileNames?: string[] }>) {
    for (const [key, item] of Object.entries(bundle)) {
      if (item.type !== "asset") continue;
      const fromDev = (item.originalFileNames ?? []).some((f) => /(^|[\/])src[\/]dev[\/]/.test(f));
      const byName = (item.name ?? "").startsWith("demo-");
      if (fromDev || byName) delete bundle[key];
    }
  },
};

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss(), devAssetsOutOfRelease],

  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },

  css: { postcss: { plugins: [woff2Only] } },

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
