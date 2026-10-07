// `node scripts/measure-text-centre.mjs`: measures how far each label's
// text sits from the vertical centre of its box, by counting ink rows in a
// screenshot - the only measurement that sees what the eye sees. Font
// maths and DOM boxes cannot: CSS centres the font's line box, not the
// glyphs, and where the glyphs land in it depends on the font's metrics,
// the font size and the screen's scale.
//
// For each target (scripts/measure-text-centre.targets.json) it:
//   1. walks the docs-site shot route the target names (the same routes
//      `npm run docs:shots` uses, run through the same helpers), plus any
//      extra steps, so the element is on screen;
//   2. at each device scale (1 and 1.5 by default, set with
//      Emulation.setDeviceMetricsOverride at 1440x900 and cleared after)
//      takes a PNG of just the element with Page.captureScreenshot + clip;
//   3. takes the element's inner box (inside its border) and the columns
//      its text covers (a Range over its text nodes, so an icon beside the
//      text and the border are never counted), leaving out the columns of
//      glyphs that reach past the capitals or below the baseline by design
//      (g, p, y, and the lowercase ascenders b, d, h, k, l, t, which are
//      taller than a capital in Fira Sans) - the question is whether the
//      capitals and the baseline sit centred;
//   4. finds the rows holding ink: pixels moved clearly from the box's own
//      background (the most common colour among those columns) towards
//      the text's colour - so a round pill's anti-aliased rim is not ink;
//   5. reports the space above and below the ink in device px and the
//      offset, (above - below) / 2: negative rides HIGH, positive LOW.
//      Rotated text (a transform or a vertical writing mode) is measured
//      the same way along its own axis, "above" being the side the tops of
//      its letters face. "desc" is the space left under ALL the ink,
//      descenders included - below zero would mean a clipped descender.
//      "fine" is the offset to a fraction of a pixel, from how dark the edge
//      rows are. "ref" says what the top of the ink is: "cap" when a
//      capital or digit is among the measured glyphs, "x-height" for an
//      all-lowercase label, whose ink is lower than a capital's by design;
//      for those "vs caps" is the fine offset less that design drop (half
//      of cap height minus x-height, read from the font with a canvas), so
//      it reads on the same scale as a label with capitals.
//
// The app must be running with `npm run docs:dev` (CDP on 9333), signed
// out. Like docs:shots it switches the app to sample data, capture mode and
// a theme through localStorage, so it snapshots every entry first (also to
// a temp file) and puts every one back at the end, then reloads.
//
//   --only a,b        measure just these target names
//   --themes a,b      app theme ids (default light,graphite)
//   --scales a,b      device scales (default 1,1.5); "native" alone measures
//                     the window as it is, at its own size and scale
//   --out file.md     also write the table to this file
//   --targets file    a different targets file
//   --clips dir       also save each element's clip PNG there, to look at
//   --restore         put the settings back from an interrupted run
//
// A target: { "name", "shot": <docs shot id>, "steps"?: [route steps],
// "locate": <docs locate>, "nth"?: n, "label"?: <what the table calls the
// text, when the text itself must not be printed>, "before"?: <JS expression run in the
// app after its reload, before the route - e.g. to raise a sample
// notification the sample data does not have> }. The locate must reach the box
// whose centre is in question (the pill, the button), not its text span.

import { existsSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as sleep } from "node:timers/promises";
import { chromium } from "playwright-core";
import { CDP_URL, KEYS, MAIN_SIZE, SETTLE_MS, locatorFor, passStorage, planIsEmpty, restorePlan, runStep, visibleOnly } from "./docs-shots-lib.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const BACKUP = join(tmpdir(), "tcm-measure-text-centre-backup.json");
const RESTORE_FLUSH_MS = 10_000;

function parseArgs(argv) {
  const out = { only: null, themes: ["light", "graphite"], scales: [1, 1.5], out: null, targets: join(repo, "scripts", "measure-text-centre.targets.json"), restore: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const list = () => argv[++i].split(",").map((s) => s.trim()).filter(Boolean);
    if (a === "--only") out.only = list();
    else if (a === "--themes") out.themes = list();
    else if (a === "--scales") out.scales = list().map((v) => (v === "native" ? v : Number(v)));
    else if (a === "--out") out.out = argv[++i];
    else if (a === "--targets") out.targets = argv[++i];
    else if (a === "--restore") out.restore = true;
    else if (a === "--clips") out.clips = argv[++i];
    else throw new Error(`Unknown option ${a}`);
  }
  return out;
}

async function loadShots() {
  const { createServer } = await import("vite");
  const server = await createServer({
    configFile: false,
    root: join(repo, "docs-site"),
    appType: "custom",
    logLevel: "error",
    server: { middlewareMode: true, hmr: false, ws: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
  });
  try {
    const { screens } = await server.ssrLoadModule("/src/content/index.ts");
    return new Map(screens.flatMap((s) => s.shots.map((shot) => [shot.id, shot])));
  } finally {
    await server.close();
  }
}

const readStorage = (page) =>
  page.evaluate(() => {
    const out = {};
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k !== null) out[k] = localStorage.getItem(k) ?? "";
    }
    return out;
  });

const applyPlan = (page, plan) =>
  page.evaluate(({ set, remove }) => {
    for (const [k, v] of Object.entries(set)) localStorage.setItem(k, v);
    for (const k of remove) localStorage.removeItem(k);
  }, plan);

/** Puts `snapshot` back from a same-origin page that runs none of the app
 *  (Vite's client script), so nothing the app does can write over it. */
async function putBack(page, snapshot, homeUrl) {
  await page.goto(new URL("/@vite/client", homeUrl).href, { waitUntil: "load" }).catch(() => {});
  for (let attempt = 0; attempt < 10; attempt++) {
    const left = restorePlan(snapshot, await readStorage(page));
    if (planIsEmpty(left)) break;
    await applyPlan(page, left);
    await sleep(300);
  }
  const left = restorePlan(snapshot, await readStorage(page));
  if (!planIsEmpty(left)) throw new Error(`could not put back: ${[...Object.keys(left.set), ...left.remove].join(", ")}`);
  await page.goto(homeUrl, { waitUntil: "load" });
  console.log("Saving the restored settings...");
  await sleep(RESTORE_FLUSH_MS);
  unlinkSync(BACKUP);
}

// ------------------------------------------------------------- measuring

/** Geometry of the box and its text, in CSS px, read in the page. */
function readGeometry(el) {
  // Glyphs that reach past the cap height or below the baseline by design:
  // descenders, lowercase ascenders (taller than a capital in Fira Sans),
  // the dotted i and j, and brackets. Left out of the centring rows.
  const OFF_CAP = /[gjpqyQbdfhklti,;()[\]{}|_@$/\\]/;
  const r = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  const inner = {
    top: r.top + parseFloat(cs.borderTopWidth),
    bottom: r.bottom - parseFloat(cs.borderBottomWidth),
    left: r.left + parseFloat(cs.borderLeftWidth),
    right: r.right - parseFloat(cs.borderRightWidth),
  };
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  const keep = [];
  const all = [];
  let angle = 0;
  let capRef = false;
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (!n.textContent.trim()) continue;
    const parent = n.parentElement;
    const pr = parent.getBoundingClientRect();
    if (pr.width < 2 || pr.height < 2) continue; // screen-reader-only text
    // The text's own rotation: transforms from it up to the box, or a
    // vertical writing mode.
    for (let p = parent; p && p !== el.parentElement; p = p.parentElement) {
      const pcs = getComputedStyle(p);
      if (pcs.transform && pcs.transform !== "none") {
        const m = new DOMMatrix(pcs.transform);
        angle += Math.round((Math.atan2(m.b, m.a) * 180) / Math.PI);
      }
      if (pcs.rotate && pcs.rotate !== "none") angle += Math.round(parseFloat(pcs.rotate));
      if (p === parent && pcs.writingMode.startsWith("vertical")) angle += 90;
    }
    const transform = getComputedStyle(parent).textTransform;
    const text = n.textContent;
    for (let i = 0; i < text.length; i++) {
      if (!text[i].trim()) continue;
      const range = document.createRange();
      range.setStart(n, i);
      range.setEnd(n, i + 1);
      const cr = range.getBoundingClientRect();
      if (!cr.width && !cr.height) continue;
      const box = { top: cr.top, bottom: cr.bottom, left: cr.left, right: cr.right };
      all.push(box);
      const shownUpper = transform === "uppercase" || (transform === "capitalize" && (i === 0 || !text[i - 1].trim()));
      const ch = shownUpper ? text[i].toUpperCase() : text[i];
      if (!OFF_CAP.test(ch)) keep.push(box);
      if (/[A-Z0-9]/.test(ch)) capRef = true;
    }
  }
  angle = ((angle % 360) + 360) % 360;
  // For an all-lowercase label: how far below a capital's centre the
  // x-height's centre sits by design in this font, in CSS px, so the
  // offset can also be read as if the label had capitals.
  let xDrop = 0;
  const first = document.createTreeWalker(el, NodeFilter.SHOW_TEXT).nextNode();
  if (first) {
    const ctx = new OffscreenCanvas(1, 1).getContext("2d");
    ctx.font = getComputedStyle(first.parentElement).font;
    xDrop = (ctx.measureText("H").actualBoundingBoxAscent - ctx.measureText("x").actualBoundingBoxAscent) / 2;
  }
  return {
    xDrop,
    fg: first ? getComputedStyle(first.parentElement).color : "",
    rect: { x: r.x, y: r.y, w: r.width, h: r.height },
    // The box's parent: its height, and where the box sits in it, show
    // whether a change moved the neighbours.
    parentH: el.parentElement.getBoundingClientRect().height,
    topInParent: r.top - el.parentElement.getBoundingClientRect().top,
    inner,
    keep,
    all,
    angle,
    capRef,
    label: (el.innerText || el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 40),
  };
}

/** Ink analysis of the PNG `b64` (a clip whose top-left is device pixel
 *  (ox, oy)), run in the page so no image library is needed. */
async function analyse({ b64, ox, oy, dsf, geo }) {
  const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
  const bmp = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
  const cv = new OffscreenCanvas(bmp.width, bmp.height);
  const ctx = cv.getContext("2d");
  ctx.drawImage(bmp, 0, 0);
  const { data, width, height } = ctx.getImageData(0, 0, bmp.width, bmp.height);
  // Everything below is in the text's own frame: "rows" run across the
  // letters, top of the letters first. Rotation swaps screen axes.
  const rotated = geo.angle === 90 || geo.angle === 270;
  const toDev = (v, axis) => v * dsf - (axis === "x" ? ox : oy);
  const inner = {
    x0: toDev(geo.inner.left, "x"),
    x1: toDev(geo.inner.right, "x"),
    y0: toDev(geo.inner.top, "y"),
    y1: toDev(geo.inner.bottom, "y"),
  };
  // Cross axis (the one centring is about) and inline axis, in device px.
  const cross = rotated ? [inner.x0, inner.x1] : [inner.y0, inner.y1];
  const spans = (boxes) =>
    boxes.map((b) => (rotated ? [Math.round(toDev(b.top, "y")), Math.round(toDev(b.bottom, "y"))] : [Math.round(toDev(b.left, "x")), Math.round(toDev(b.right, "x"))]));
  const pixel = (line, along) => {
    const [x, y] = rotated ? [line, along] : [along, line];
    if (x < 0 || y < 0 || x >= width || y >= height) return null;
    const i = (y * width + x) * 4;
    return [data[i], data[i + 1], data[i + 2]];
  };
  const lineFrom = Math.ceil(cross[0]);
  const lineTo = Math.floor(cross[1]) - 1;
  const alongSet = (sp) => {
    const s = new Set();
    for (const [a, b] of sp) for (let v = Math.max(a, 0); v < b; v++) s.add(v);
    return [...s];
  };
  // A glyph's ink can reach past its own advance box (the lower bowl of a
  // g under the n before it), so each measured glyph's columns are taken
  // one device pixel in from either side.
  const keepAlong = alongSet(spans(geo.keep).map(([a, b]) => (b - a >= 4 ? [a + 1, b - 1] : [a, b])));
  const allAlong = alongSet(spans(geo.all));
  // The background: the most common colour in the inner box over the
  // text's columns.
  const counts = new Map();
  for (let l = lineFrom; l <= lineTo; l++)
    for (const a of allAlong) {
      const p = pixel(l, a);
      if (!p) continue;
      const k = p.join(",");
      counts.set(k, (counts.get(k) ?? 0) + 1);
    }
  const bg = [...counts.entries()].sort((a, b) => b[1] - a[1])[0][0].split(",").map(Number);
  // The text's own colour, resolved by painting it (computed colours can
  // be oklch). Ink is a pixel moved from the background TOWARDS that
  // colour: the anti-aliased rim of a round pill blends the background
  // with what is outside it instead, and is not mistaken for a letter.
  const swatch = new OffscreenCanvas(1, 1).getContext("2d");
  swatch.fillStyle = geo.fg || "#000";
  swatch.fillRect(0, 0, 1, 1);
  const fg = [...swatch.getImageData(0, 0, 1, 1).data.slice(0, 3)];
  const v = [fg[0] - bg[0], fg[1] - bg[1], fg[2] - bg[2]];
  const vv = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
  const diff =
    vv > 900
      ? (p) => Math.max(0, ((p[0] - bg[0]) * v[0] + (p[1] - bg[1]) * v[1] + (p[2] - bg[2]) * v[2]) / vv)
      : (p) => Math.max(Math.abs(p[0] - bg[0]), Math.abs(p[1] - bg[1]), Math.abs(p[2] - bg[2])) / 255;
  let maxDiff = 0;
  for (let l = lineFrom; l <= lineTo; l++)
    for (const a of allAlong) {
      const p = pixel(l, a);
      if (p) maxDiff = Math.max(maxDiff, diff(p));
    }
  // A row holds ink when one of its pixels is clearly ink: 35% of the
  // strongest in the box, so a faint anti-aliased fringe is not counted
  // and the rows match what the eye calls the letter.
  const threshold = Math.max(0.08, maxDiff * 0.35);
  const rowStrength = (l, along) => {
    let m = 0;
    for (const a of along) {
      const p = pixel(l, a);
      if (p) m = Math.max(m, diff(p));
    }
    return m;
  };
  // The letters are one run of ink rows (a gap of up to two rows allowed,
  // for an accent or a split stroke). A stray row apart from it - the
  // anti-aliased rim of a small round badge, which can blend towards the
  // text's colour - is not part of the label, so only the run holding the
  // most ink is kept.
  const inkLines = (along) => {
    const runs = [];
    for (let l = lineFrom; l <= lineTo; l++) {
      const st = rowStrength(l, along);
      if (st < threshold) continue;
      const run = runs[runs.length - 1];
      if (run && l - run.lines[run.lines.length - 1] <= 3) {
        run.lines.push(l);
        run.ink += st;
      } else runs.push({ lines: [l], ink: st });
    }
    return runs.sort((x, y) => y.ink - x.ink)[0]?.lines ?? [];
  };
  const keepCols = keepAlong.length ? keepAlong : allAlong;
  const keepLines = inkLines(keepCols);
  const allLines = inkLines(allAlong);
  if (!keepLines.length) return { error: "no ink found" };
  // Rotated by 90 degrees clockwise the letters' tops face screen RIGHT, so
  // "above" is the far end of the cross axis; at 270 it is the near end.
  const first = keepLines[0];
  const last = keepLines[keepLines.length - 1] + 1;
  const near = first - cross[0];
  const far = cross[1] - last;
  const topsFaceFar = geo.angle === 90;
  const above = topsFaceFar ? far : near;
  const below = topsFaceFar ? near : far;
  // The same edges to a fraction of a pixel: a partly covered edge row
  // (and the faint row just outside it) counts by how dark it is. Whole
  // rows can only say "within half a pixel"; this says where in that half.
  const cover = (l) => (l < lineFrom || l > lineTo ? 0 : Math.min(1, rowStrength(l, keepCols) / maxDiff));
  const nearF = first - cross[0] + (1 - cover(first)) - cover(first - 1);
  const farF = cross[1] - last + (1 - cover(last - 1)) - cover(last);
  const aboveF = topsFaceFar ? farF : nearF;
  const belowF = topsFaceFar ? nearF : farF;
  const allNear = allLines[0] - cross[0];
  const allFar = cross[1] - (allLines[allLines.length - 1] + 1);
  const desc = topsFaceFar ? allNear : allFar;
  return { above, below, offset: (above - below) / 2, fine: (aboveF - belowF) / 2, desc, ink: last - first, contrast: maxDiff };
}

async function measure(page, cdp, locator, scales, clipPath) {
  const results = [];
  const el = await locator.elementHandle();
  for (const scale of scales) {
    if (scale === "native") await cdp.send("Emulation.clearDeviceMetricsOverride");
    else await cdp.send("Emulation.setDeviceMetricsOverride", { width: MAIN_SIZE.w, height: MAIN_SIZE.h, deviceScaleFactor: scale, mobile: false });
    const dsf = scale === "native" ? await page.evaluate(() => devicePixelRatio) : scale;
    await sleep(SETTLE_MS);
    const geo = await el.evaluate(readGeometry);
    // A clip that starts on a whole device pixel, so image pixel (0, 0) is
    // device pixel (ox, oy) exactly.
    const ox = Math.floor(geo.rect.x * dsf) - 2;
    const oy = Math.floor(geo.rect.y * dsf) - 2;
    const ow = Math.ceil((geo.rect.x + geo.rect.w) * dsf) + 2 - ox;
    const oh = Math.ceil((geo.rect.y + geo.rect.h) * dsf) + 2 - oy;
    const { data } = await cdp.send("Page.captureScreenshot", {
      format: "png",
      clip: { x: ox / dsf, y: oy / dsf, width: ow / dsf, height: oh / dsf, scale: 1 },
    });
    if (clipPath) writeFileSync(`${clipPath}-${dsf}x.png`, Buffer.from(data, "base64"));
    const r = await page.evaluate(analyse, { b64: data, ox, oy, dsf, geo });
    results.push({ dsf, xDrop: geo.xDrop * dsf, label: geo.label, h: geo.rect.h, parentH: geo.parentH, topInParent: geo.topInParent, w: geo.rect.w, angle: geo.angle, capRef: geo.capRef, ...r });
  }
  await cdp.send("Emulation.clearDeviceMetricsOverride").catch(() => {});
  return results;
}

// ------------------------------------------------------------- run

const fmt = (n) => (typeof n === "number" ? (Math.round(n * 100) / 100).toFixed(2) : "-");

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const browser = await chromium.connectOverCDP(CDP_URL, { timeout: 5_000 }).catch(() => null);
  if (!browser) {
    console.error("The app is not reachable on CDP port 9333. Start it with `npm run docs:dev`.");
    return 1;
  }
  const page = browser.contexts().flatMap((c) => c.pages()).find((p) => /^https?:/.test(p.url()) && !p.url().includes("#runner"));
  if (!page) {
    console.error("The app's main window was not found.");
    return 1;
  }
  browser.contexts().forEach((c) => c.setDefaultTimeout(15_000));

  if (args.restore || existsSync(BACKUP)) {
    if (!existsSync(BACKUP)) {
      console.log("Nothing to restore.");
      return 0;
    }
    if (!args.restore) {
      console.error(`An earlier run was interrupted. Run with --restore to put the app's settings back (or delete ${BACKUP}).`);
      return 1;
    }
    const { storage, url } = JSON.parse(readFileSync(BACKUP, "utf8"));
    await putBack(page, storage, url);
    console.log("The app's settings are back as they were.");
    return 0;
  }

  const targets = JSON.parse(readFileSync(args.targets, "utf8")).filter((t) => !args.only || args.only.includes(t.name));
  const shots = await loadShots();
  for (const t of targets) if (!shots.has(t.shot)) throw new Error(`${t.name}: no docs shot "${t.shot}"`);

  const homeUrl = page.url();
  const snapshot = await readStorage(page);
  writeFileSync(BACKUP, JSON.stringify({ at: Date.now(), url: homeUrl, storage: snapshot }));
  const cdp = await page.context().newCDPSession(page);
  const rows = [];
  try {
    for (const theme of args.themes) {
      await applyPlan(page, passStorage(theme));
      if ((await page.evaluate((k) => localStorage.getItem(k), KEYS.demo)) !== "on") {
        const loaded = page.waitForEvent("load", { timeout: 30_000 });
        await page.evaluate(async () => {
          const m = await import("/src/dev/demo.ts");
          setTimeout(() => m.toggleDemoMode(), 0);
        });
        await loaded;
      }
      for (const t of targets) {
        const shot = shots.get(t.shot);
        if (args.scales.includes("native")) await cdp.send("Emulation.clearDeviceMetricsOverride");
        else await cdp.send("Emulation.setDeviceMetricsOverride", { width: MAIN_SIZE.w, height: MAIN_SIZE.h, deviceScaleFactor: 1, mobile: false });
        await page.reload({ waitUntil: "load" });
        await page.getByRole("navigation").first().waitFor({ state: "visible", timeout: 30_000 });
        let target = page;
        try {
          if (t.before) await page.evaluate(t.before);
          for (const step of [...shot.route, ...(t.steps ?? [])]) {
            if ("runnerWindow" in step || "reviewPage" in step) throw new Error("runner and review page routes are not supported");
            target = await runStep({ page: target }, step);
          }
          const loc = visibleOnly(locatorFor(target, t.locate)).nth(t.nth ?? 0);
          await loc.waitFor({ state: "visible", timeout: 10_000 });
          await loc.scrollIntoViewIfNeeded();
          await page.mouse.move(2, 2);
          // Still pictures: a pulsing glow beside a label (the bridge
          // status dot) would otherwise land in some captures and not
          // others. Gone with the next target's reload.
          await page.addStyleTag({ content: "*, *::before, *::after { animation: none !important; transition: none !important; }" });
          await page.waitForLoadState("networkidle").catch(() => {});
          await sleep(SETTLE_MS);
          for (const r of await measure(page, cdp, loc, args.scales, args.clips && join(resolve(args.clips), `${t.name}-${theme}`))) rows.push({ name: t.name, theme, ...r, label: t.label ?? r.label });
        } catch (e) {
          rows.push({ name: t.name, theme, error: String(e.message ?? e).split("\n")[0] });
        }
        const last = rows.filter((r) => r.name === t.name && r.theme === theme);
        for (const r of last) console.log(`${t.name.padEnd(28)} ${theme.padEnd(9)} ${r.error ? r.error : `x${r.dsf}  h=${fmt(r.h)}  above=${fmt(r.above)}  below=${fmt(r.below)}  offset=${fmt(r.offset)}  fine=${fmt(r.fine)}`}`);
      }
    }
  } finally {
    await cdp.send("Emulation.clearDeviceMetricsOverride").catch(() => {});
    await cdp.detach().catch(() => {});
    await putBack(page, snapshot, homeUrl);
    await browser.close().catch(() => {});
  }

  const lines = [
    "| target | label | theme | scale | height (css px) | parent h | top in parent | above | below | offset | fine | desc | rotated | ref | vs caps |",
    "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|",
    ...rows.map((r) =>
      r.error
        ? `| ${r.name} | | ${r.theme} | | | | | | | ${r.error} | | | | | |`
        : `| ${r.name} | ${r.label} | ${r.theme} | ${r.dsf} | ${r.h.toFixed(3)} | ${r.parentH.toFixed(3)} | ${r.topInParent.toFixed(3)} | ${fmt(r.above)} | ${fmt(r.below)} | ${fmt(r.offset)} | ${fmt(r.fine)} | ${fmt(r.desc)} | ${r.angle ? r.angle : ""} | ${r.capRef ? "cap" : "x-height"} | ${r.capRef ? "" : fmt(r.fine - r.xDrop)} |`,
    ),
  ];
  const table = lines.join("\n");
  console.log(`\nDevice px; offset = (above - below) / 2, negative rides high.\n\n${table}`);
  if (args.out) writeFileSync(resolve(args.out), `${table}\n`);
  return rows.some((r) => r.error) ? 1 : 0;
}

process.exitCode = await main().catch((e) => {
  console.error(e?.stack ?? e);
  return 1;
});
