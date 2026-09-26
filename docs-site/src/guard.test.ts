// The help site must never name what a normal install does not show, and
// its copy follows the house style. This is the text side, checked on the
// content itself: every word a reader sees (titles, summaries, captions,
// tips, how-tos, alt text, recipes and the hero), and every content source
// file - and on what ships: the built page and every image file name.
//
// Then the shipped site itself: every shot the content uses has an image
// in both themes at the size it was captured at, positions.json places
// every documented control, and the built page is not stale (it carries a
// hash of its sources, recomputed here).

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import positionsJson from "../shots/positions.json";
import { intro, recipes, screens } from "./content";
import { helpSourceHash, readSourceMeta } from "./sourceHash";
import { shotSize, type Positions, type Size } from "./types";

const DOCS = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SHOTS = join(DOCS, "shots");
const HELP = resolve(DOCS, "../src-tauri/help");
const BUILT = join(HELP, "index.html");
const THEMES = ["light", "dark"] as const;
const positions = positionsJson as Positions;

/** Never in the site: hidden features, the dev tooling, the sample data. */
const FORBIDDEN = [/\bauto ?run\b/i, /autorun/i, /konami/i, /\bunlock/i, /\bextras\b/i, /dev panel/i, /demo data/i, /\bgames?\b/i];

/** What a reader sees, with where it is, for a readable failure. */
function visibleText(): { where: string; text: string }[] {
  const out: { where: string; text: string }[] = [];
  const add = (where: string, text: string | undefined) => {
    if (text) out.push({ where, text });
  };
  for (const s of screens) {
    add(`${s.id} title`, s.title);
    add(`${s.id} summary`, s.summary);
    s.tips?.forEach((t, i) => add(`${s.id} tip ${i + 1}`, t));
    for (const how of s.howTo ?? []) {
      add(`${s.id} how-to`, how.title);
      how.steps.forEach((t, i) => add(`${s.id} how-to "${how.title}" step ${i + 1}`, t));
    }
    for (const shot of s.shots) add(`${shot.id} alt`, shot.alt);
    for (const c of s.controls) {
      add(`${s.id}/${c.id} name`, c.name);
      add(`${s.id}/${c.id} does`, c.does);
      c.tips?.forEach((t, i) => add(`${s.id}/${c.id} tip ${i + 1}`, t));
    }
  }
  for (const r of recipes) {
    add(`recipe ${r.id}`, r.title);
    r.steps.forEach((st, i) => add(`recipe ${r.id} step ${i + 1}`, st.text));
  }
  add("intro promise", intro.promise);
  add("intro lead", intro.lead);
  for (const q of intro.quickStart) {
    add(`quick start ${q.label}`, q.label);
    add(`quick start ${q.label} hint`, q.hint);
  }
  return out;
}

describe("help content words", () => {
  const text = visibleText();

  test("there is content to check", () => {
    expect(text.length).toBeGreaterThan(50);
  });

  test("names no hidden feature, dev tooling or sample data", () => {
    const hits = text.filter(({ text: t }) => [...FORBIDDEN, /\bdemo\b/i].some((re) => re.test(t))).map((x) => `${x.where}: ${x.text}`);
    expect(hits).toEqual([]);
  });

  test("uses no em dash", () => {
    const hits = text.filter(({ text: t }) => t.includes("—")).map((x) => `${x.where}: ${x.text}`);
    expect(hits).toEqual([]);
  });

  test("no content source file names a hidden feature", () => {
    const dir = join(dirname(fileURLToPath(import.meta.url)), "content");
    const hits: string[] = [];
    for (const f of readdirSync(dir).filter((n) => n.endsWith(".ts"))) {
      const src = readFileSync(resolve(dir, f), "utf8");
      for (const re of FORBIDDEN) if (re.test(src)) hits.push(`${f}: ${re}`);
    }
    expect(hits).toEqual([]);
  });
});

/** Every .jpg in a folder (none when it does not exist). */
const jpgsIn = (dir: string) => (existsSync(dir) ? readdirSync(dir).filter((f) => f.toLowerCase().endsWith(".jpg")) : []);

/** A JPEG's pixel size, read from its start-of-frame marker. */
function jpegSize(bytes: Buffer): Size | null {
  if (bytes[0] !== 0xff || bytes[1] !== 0xd8) return null;
  let i = 2;
  while (i + 9 < bytes.length) {
    if (bytes[i] !== 0xff) return null;
    const marker = bytes[i + 1];
    if (marker === 0xff) {
      i++; // fill byte
      continue;
    }
    if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
      return { h: bytes.readUInt16BE(i + 5), w: bytes.readUInt16BE(i + 7) };
    }
    i += 2 + bytes.readUInt16BE(i + 2);
  }
  return null;
}

describe("what ships names nothing hidden", () => {
  const WORDS = [...FORBIDDEN, /demo/i];

  test("the built page", () => {
    const html = existsSync(BUILT) ? readFileSync(BUILT, "utf8") : "";
    expect(html.length).toBeGreaterThan(5000);
    expect(WORDS.filter((re) => re.test(html)).map(String)).toEqual([]);
  });

  test("every image file name, captured and built", () => {
    const names = THEMES.flatMap((t) => [...jpgsIn(join(SHOTS, t)), ...jpgsIn(join(HELP, "img", t))].map((f) => `${t}/${f}`));
    expect(names.length).toBeGreaterThan(0);
    expect(names.filter((n) => WORDS.some((re) => re.test(n)))).toEqual([]);
  });
});

describe("the shipped screenshots", () => {
  const shots = screens.flatMap((s) => s.shots);
  const shotIds = new Set(shots.map((s) => s.id));

  test("every shot the content uses has an image in both themes", () => {
    const used = [...shotIds, ...(intro.heroShot ? [intro.heroShot] : [])];
    const missing = THEMES.flatMap((t) => used.filter((id) => !existsSync(join(HELP, "img", t, `${id}.jpg`))).map((id) => `img/${t}/${id}.jpg`));
    expect(missing).toEqual([]);
  });

  test("no image is left over from a shot the content no longer has", () => {
    const orphans = THEMES.flatMap((t) =>
      [join(SHOTS, t), join(HELP, "img", t)].flatMap((dir) =>
        jpgsIn(dir)
          .filter((f) => !shotIds.has(f.slice(0, -4)))
          .map((f) => join(dir, f)),
      ),
    );
    expect(orphans).toEqual([]);
  });

  test("every image is the size its shot is captured at (the runner at its real size)", () => {
    const wrong: string[] = [];
    for (const shot of shots) {
      const want = shotSize(shot);
      for (const t of THEMES) {
        const file = join(HELP, "img", t, `${shot.id}.jpg`);
        if (!existsSync(file)) continue; // named by the test above
        const got = jpegSize(readFileSync(file));
        if (got?.w !== want.w || got?.h !== want.h) wrong.push(`img/${t}/${shot.id}.jpg is ${got?.w}x${got?.h}, want ${want.w}x${want.h}`);
      }
    }
    expect(wrong).toEqual([]);
  });

  test("positions.json places every control of every screen, on its shot, at the shot's size", () => {
    const problems: string[] = [];
    for (const screen of screens) {
      for (const shot of screen.shots) {
        const want = shotSize(shot);
        const entry = positions[shot.id];
        if (!entry) {
          problems.push(`${shot.id}: no entry`);
          continue;
        }
        if (entry.size.w !== want.w || entry.size.h !== want.h) problems.push(`${shot.id}: size ${entry.size.w}x${entry.size.h}, want ${want.w}x${want.h}`);
      }
      for (const control of screen.controls) {
        const entry = positions[control.shot];
        const box = entry?.controls[control.id];
        if (!box) {
          problems.push(`${screen.id}/${control.id}: no box on ${control.shot}`);
          continue;
        }
        // The marker sits at the box's centre, so that is what must be on the shot.
        const cx = box.x + box.w / 2;
        const cy = box.y + box.h / 2;
        const onShot = box.w > 0 && box.h > 0 && cx >= 0 && cy >= 0 && cx <= entry.size.w && cy <= entry.size.h;
        if (!onShot) problems.push(`${screen.id}/${control.id}: box ${JSON.stringify(box)} is not on ${control.shot}`);
      }
    }
    expect(problems).toEqual([]);
  });
});

describe("the built page is up to date", () => {
  test("it was built from the sources as they are now (run `npm run docs:build`)", () => {
    const html = existsSync(BUILT) ? readFileSync(BUILT, "utf8") : "";
    expect(readSourceMeta(html), "the built page carries no source hash").not.toBeNull();
    expect(readSourceMeta(html), "docs-site changed since the last `npm run docs:build`").toBe(helpSourceHash(DOCS));
  });
});
