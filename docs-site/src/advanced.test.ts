// The guide hides Auto Run and API Templates unless the app opened it with
// Enable Advanced Features on; unknown means hidden.

import { describe, expect, test } from "vitest";
import positions from "../shots/positions.json";
import { ADVANCED_GROUPS, ADVANCED_SCREENS, advancedFrom, forFeatures } from "./advanced";
import { intro, recipes, screens } from "./content";
import { render } from "./render/layout";
import type { Positions, SiteContent } from "./types";

const content: SiteContent = { screens, recipes, intro, positions: positions as Positions, available: [] };

describe("the address", () => {
  test("only ?advanced=1 turns it on; anything else, or nothing, is off", () => {
    expect(advancedFrom("?advanced=1")).toBe(true);
    expect(advancedFrom("?x=2&advanced=1")).toBe(true);
    for (const search of ["", "?advanced=0", "?advanced=true", "?advanced", "?other=1", "?%zz"]) {
      expect(advancedFrom(search), search).toBe(false);
    }
  });
});

describe("the content", () => {
  test("every hidden screen and card exists, so the lists cannot go stale", () => {
    for (const id of ADVANCED_SCREENS) expect(screens.some((s) => s.id === id), id).toBe(true);
    for (const [screen, groups] of Object.entries(ADVANCED_GROUPS)) {
      const s = screens.find((x) => x.id === screen);
      expect(s, screen).toBeTruthy();
      for (const g of groups) expect(s!.groups?.some((x) => x.id === g), `${screen}/${g}`).toBe(true);
    }
  });

  test("on, nothing is left out", () => {
    expect(forFeatures(content, true)).toBe(content);
  });

  test("off, the screens, the cards, their controls and their own shots go", () => {
    const shown = forFeatures(content, false);
    const ids = shown.screens.map((s) => s.id);
    for (const id of ADVANCED_SCREENS) expect(ids).not.toContain(id);
    expect(shown.screens.length).toBe(screens.length - ADVANCED_SCREENS.length);
    const bridge = shown.screens.find((s) => s.id === "ai-bridge")!;
    for (const g of ADVANCED_GROUPS["ai-bridge"]) {
      expect(bridge.groups?.some((x) => x.id === g)).toBe(false);
      expect(bridge.controls.some((c) => c.group === g)).toBe(false);
    }
    // Every control left still has its shot.
    for (const s of shown.screens) for (const c of s.controls) expect(s.shots.some((x) => x.id === c.shot), `${s.id}/${c.id}`).toBe(true);
    // Nothing links into what is hidden.
    const hidden = (link?: string) => !!link && ADVANCED_SCREENS.some((id) => link === id || link.startsWith(`${id}/`));
    for (const r of shown.recipes) for (const st of r.steps) expect(hidden(st.link), `${r.id}: ${st.link}`).toBe(false);
    for (const q of shown.intro.quickStart) expect(hidden(q.link), q.label).toBe(false);
  });

  test("off, the page has no Auto Run or API Templates section or contents entry", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const site = render(root, forFeatures(content, false));
    try {
      for (const id of ADVANCED_SCREENS) {
        expect(root.querySelector(`[id="${id}"]`), id).toBeNull();
        expect(root.querySelector(`a[href="#${id}"]`), id).toBeNull();
      }
      expect(root.querySelector('[id="ai-bridge/api-templates"]')).toBeNull();
      expect(root.querySelector('[id="ai-tools"], [id="ai-bridge"]')).not.toBeNull();
    } finally {
      site.destroy();
      root.remove();
    }
  });

  test("on, both sections are there", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const site = render(root, forFeatures(content, true));
    try {
      for (const id of ADVANCED_SCREENS) expect(root.querySelector(`[id="${id}"]`), id).not.toBeNull();
    } finally {
      site.destroy();
      root.remove();
    }
  });
});
