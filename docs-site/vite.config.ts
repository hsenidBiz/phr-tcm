// Builds the How To Use site into src-tauri/help/ - one self-contained
// index.html (script, styles and fonts inlined) plus img/{light,dark}/.
// The app embeds that folder and opens it from disk (file://), where
// Chromium refuses module scripts and there is no network: so the script
// is a classic IIFE, nothing is fetched, and nothing sits beside the page
// but the screenshots.

import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";
import { helpSourceHash, SOURCE_META } from "./src/sourceHash";

const root = dirname(fileURLToPath(import.meta.url)); // docs-site/
const shotsDir = join(root, "shots");
const outDir = resolve(root, "../src-tauri/help");
const THEMES = ["light", "dark"] as const;

const jpgs = (theme: string) => {
  const dir = join(shotsDir, theme);
  return existsSync(dir) ? readdirSync(dir).filter((f) => f.toLowerCase().endsWith(".jpg")) : [];
};

/** `virtual:help-shots` - ids of the shots that have an image in both themes. */
function helpShots(): Plugin {
  const id = "virtual:help-shots";
  return {
    name: "help-shots",
    resolveId: (source) => (source === id ? `\0${id}` : null),
    load(loaded) {
      if (loaded !== `\0${id}`) return null;
      const dark = new Set(jpgs("dark"));
      const both = jpgs("light")
        .filter((f) => dark.has(f))
        .map((f) => f.slice(0, -4))
        .sort();
      return `export default ${JSON.stringify(both)};`;
    },
    // `vite dev`: serve img/<theme>/<shot>.jpg straight from docs-site/shots/.
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const m = /^\/img\/(light|dark)\/([\w.-]+\.jpg)$/.exec((req.url ?? "").split("?")[0]);
        const file = m ? join(shotsDir, m[1], m[2]) : "";
        if (!m || !existsSync(file)) return next();
        res.setHeader("Content-Type", "image/jpeg");
        res.end(readFileSync(file));
      });
    },
  };
}

/** After the bundle is written: make the inlined script classic, stamp the
 *  page with the hash of its sources (the stale-build guard in
 *  guard.test.ts recomputes it), and copy the shots. */
function classicPageAndShots(): Plugin {
  return {
    name: "help-classic-page",
    apply: "build",
    enforce: "post",
    closeBundle() {
      const page = join(outDir, "index.html");
      const html = readFileSync(page, "utf8");
      // vite-plugin-singlefile keeps Vite's `<script type="module" crossorigin>`;
      // the code inside is already an IIFE, so dropping the attributes is all
      // it takes to make it a classic script that runs from file://.
      const classic = html.replace(/<script\b([^>]*)>/gi, (_tag, attrs: string) => {
        const kept = attrs.replace(/\s+type\s*=\s*["']?module["']?/gi, "").replace(/\s+crossorigin(?:\s*=\s*["'][^"']*["'])?/gi, "");
        return `<script${kept}>`;
      });
      if (/type\s*=\s*["']?module/i.test(classic)) throw new Error("help site: a module script survived the build");
      const stamp = `<meta name="${SOURCE_META}" content="${helpSourceHash(root)}" />`;
      const stamped = classic.replace(/<head>/i, (head) => `${head}\n    ${stamp}`);
      if (stamped === classic) throw new Error("help site: no <head> to stamp the source hash into");
      writeFileSync(page, stamped);

      for (const theme of THEMES) {
        const files = jpgs(theme);
        if (!files.length) continue;
        const dest = join(outDir, "img", theme);
        mkdirSync(dest, { recursive: true });
        for (const f of files) copyFileSync(join(shotsDir, theme, f), join(dest, f));
      }
    },
  };
}

export default defineConfig({
  root,
  base: "./",
  publicDir: false,
  // Not `removeViteModuleLoader`: its pattern strips the first
  // `(function(){...})();` in the page - with IIFE output, that is the whole
  // site. modulePreload: false already keeps Vite's loader out.
  plugins: [helpShots(), viteSingleFile(), classicPageAndShots()],
  server: { port: 1430, strictPort: true },
  build: {
    outDir,
    emptyOutDir: true,
    modulePreload: false,
    reportCompressedSize: false,
    rollupOptions: { output: { format: "iife" } },
  },
});
