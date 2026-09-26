// Structural checks on the help content, shared by vitest
// (content.test.ts) and the capture script (`npm run docs:shots --
// --validate`, which loads this file through Vite). Returns every problem
// found as a readable line; an empty list means the content is capturable.
//
// What it checks is what the capture script and the site rely on: ids that
// become file names and anchors are unique and safe, every shot can be
// reached (a non-empty route of known steps), and every control is marked
// on a shot of its own screen and can be looked up (a well-formed locate).
// `positionsProblems` checks positions.json against the content.

import { shotSize, type Positions, type Screen } from "./types";

/** Shot ids become `shots/<theme>/<id>.jpg`; screen and control ids become
 *  `#screen/control` anchors. Lower-case kebab-case keeps all three safe. */
const KEBAB = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

const nonEmpty = (v: unknown): v is string => typeof v === "string" && v.trim() !== "";

/** Why a locate cannot be used, or null when it is well-formed. */
export function locateProblem(locate: unknown): string | null {
  if (!locate || typeof locate !== "object") return "has no locate";
  const l = locate as Record<string, unknown>;
  const keys = Object.keys(l).sort().join(",");
  switch (keys) {
    case "name,role":
      return nonEmpty(l.role) && nonEmpty(l.name) ? null : "locate needs a non-empty role and name";
    case "nameRe,role":
      if (!nonEmpty(l.role) || !nonEmpty(l.nameRe)) return "locate needs a non-empty role and nameRe";
      try {
        new RegExp(l.nameRe);
        return null;
      } catch {
        return `locate nameRe is not a valid pattern: ${String(l.nameRe)}`;
      }
    case "label":
    case "text":
    case "testId": {
      const v = l[keys];
      return nonEmpty(v) ? null : `locate ${keys} is empty`;
    }
    default:
      return `locate has an unknown shape {${keys}}`;
  }
}

/** Why a route step cannot be run, or null when it is well-formed. */
function stepProblem(step: unknown): string | null {
  if (!step || typeof step !== "object") return "is not a step";
  const s = step as Record<string, unknown>;
  const keys = Object.keys(s);
  if (keys.length !== 1) return `has an unknown shape {${keys.sort().join(",")}}`;
  const kind = keys[0];
  switch (kind) {
    case "nav":
    case "press":
      return nonEmpty(s[kind]) ? null : `${kind} is empty`;
    case "click":
    case "waitFor":
    case "scrollTo":
      return locateProblem(s[kind]);
    case "runnerWindow":
      return s.runnerWindow === true ? null : "runnerWindow must be true";
    default:
      return `is an unknown step "${kind}"`;
  }
}

const isPixels = (n: unknown) => Number.isInteger(n) && (n as number) > 0;

function duplicates(ids: string[]): string[] {
  const seen = new Set<string>();
  const dup = new Set<string>();
  for (const id of ids) (seen.has(id) ? dup : seen).add(id);
  return [...dup];
}

/** Every problem in the screens, one line each; [] when there are none. */
export function validateContent(content: { screens: Screen[] }): string[] {
  const problems: string[] = [];
  const { screens } = content;

  for (const id of duplicates(screens.map((s) => s.id))) problems.push(`screen id "${id}" is used more than once`);
  // Shot ids share one folder per theme, so they are unique across screens.
  for (const id of duplicates(screens.flatMap((s) => s.shots.map((sh) => sh.id)))) {
    problems.push(`shot id "${id}" is used more than once`);
  }

  for (const screen of screens) {
    const where = `screen "${screen.id}"`;
    if (!KEBAB.test(screen.id)) problems.push(`${where}: id is not lower-case kebab-case`);

    for (const shot of screen.shots) {
      const at = `${where}, shot "${shot.id}"`;
      if (!KEBAB.test(shot.id)) problems.push(`${at}: id is not lower-case kebab-case`);
      if (shot.size !== undefined && !(isPixels(shot.size.w) && isPixels(shot.size.h))) {
        problems.push(`${at}: size must be whole pixels above zero`);
      }
      if (!Array.isArray(shot.route) || shot.route.length === 0) {
        problems.push(`${at}: has no route`);
        continue;
      }
      shot.route.forEach((step, i) => {
        const p = stepProblem(step);
        if (p) problems.push(`${at}: route step ${i + 1} ${p}`);
      });
    }

    for (const id of duplicates(screen.controls.map((c) => c.id))) {
      problems.push(`${where}: control id "${id}" is used more than once`);
    }
    const shotIds = new Set(screen.shots.map((s) => s.id));
    for (const control of screen.controls) {
      const at = `${where}, control "${control.id}"`;
      if (!KEBAB.test(control.id)) problems.push(`${at}: id is not lower-case kebab-case`);
      if (!shotIds.has(control.shot)) problems.push(`${at}: shot "${control.shot}" is not one of this screen's shots`);
      const p = locateProblem(control.locate);
      if (p) problems.push(`${at}: ${p}`);
    }
  }
  return problems;
}

/** Every way positions.json disagrees with the content: an entry for a shot
 *  or control that is not documented, or boxes measured at a different size
 *  than the shot is now taken at (re-capture). A documented control with no
 *  box is not a problem here - the site lists it without a marker. */
export function positionsProblems(screens: Screen[], positions: Positions): string[] {
  const problems: string[] = [];
  const shots = new Map(screens.flatMap((screen) => screen.shots.map((shot) => [shot.id, { screen, shot }] as const)));
  for (const id of Object.keys(positions).sort()) {
    const entry = positions[id];
    const found = shots.get(id);
    if (!found) {
      problems.push(`positions: shot "${id}" is not in the content`);
      continue;
    }
    const want = shotSize(found.shot);
    if (entry?.size?.w !== want.w || entry?.size?.h !== want.h) {
      problems.push(`positions: shot "${id}" was captured at ${entry?.size?.w}x${entry?.size?.h}, the content says ${want.w}x${want.h}`);
    }
    const onShot = new Set(found.screen.controls.filter((c) => c.shot === id).map((c) => c.id));
    for (const control of Object.keys(entry?.controls ?? {}).sort()) {
      if (!onShot.has(control)) problems.push(`positions: shot "${id}" has a box for "${control}", which is not documented on it`);
    }
  }
  return problems;
}
