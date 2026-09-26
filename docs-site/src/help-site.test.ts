import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, test, vi } from "vitest";
import { ANIMATION_CLASSES, render, type SiteHandle } from "./render/layout";
import { score } from "./render/search";
import { captionPlacement, spotStyle } from "./render/shot";
import type { Box, ShotPositions, SiteContent } from "./types";

/** A main-window shot's positions.json entry. */
const main = (controls: Record<string, Box>): ShotPositions => ({ size: { w: 1440, h: 900 }, controls });

// jsdom has no IntersectionObserver and no Element.scrollIntoView. The app's
// src/test-setup.ts stubs both for every test, but these tests must not
// depend on another suite's setup, so they stub them here too.
beforeAll(() => {
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
      takeRecords() {
        return [];
      }
    },
  );
  if (!Element.prototype.scrollIntoView) Element.prototype.scrollIntoView = () => {};
});
afterAll(() => {
  vi.unstubAllGlobals();
});

// A small, self-contained fixture - never the real registry, so these
// tests pin the site's behaviour rather than today's content.
const fixture: SiteContent = {
  intro: {
    promise: "Fixture promise.",
    lead: "Fixture lead.",
    heroShot: "alpha-main",
    quickStart: [
      { label: "Open alpha", hint: "First", link: "alpha" },
      { label: "Somewhere else", hint: "Not on the page", link: "nowhere" },
    ],
  },
  screens: [
    {
      id: "alpha",
      title: "Alpha Screen",
      group: "Test cases",
      summary: "Alpha summary text.",
      shots: [
        { id: "alpha-main", route: [{ nav: "Alpha" }], alt: "The alpha screen" },
        { id: "alpha-open", route: [{ nav: "Alpha" }], alt: "The alpha screen with a panel open" },
      ],
      controls: [
        { id: "save", shot: "alpha-main", locate: { role: "button", name: "Save" }, name: "Save", does: "Keeps the draft." },
        { id: "discard", shot: "alpha-main", locate: { role: "button", name: "Discard" }, name: "Discard", does: "Throws the draft away." },
        { id: "filter", shot: "alpha-open", locate: { label: "Filter" }, name: "Filter box", does: "Narrows the list." },
      ],
      tips: ["Press [[Ctrl+S]] to keep the draft."],
    },
    {
      id: "beta",
      title: "Beta Screen",
      group: "Settings",
      summary: "Beta summary text.",
      shots: [{ id: "beta-main", route: [{ nav: "Beta" }], alt: "The beta screen" }],
      controls: [
        { id: "reset-layout", shot: "beta-main", locate: { role: "button", name: "Reset layout" }, name: "Reset layout", does: "Puts every panel back." },
        { id: "unplaced", shot: "beta-main", locate: { text: "Unplaced" }, name: "Unplaced option", does: "Has no position yet." },
      ],
    },
  ],
  recipes: [{ id: "first-run", title: "Your first run", steps: [{ text: "Open alpha", link: "alpha" }, { text: "Save", link: "alpha/save" }] }],
  positions: {
    "alpha-main": main({ save: { x: 100, y: 50, w: 80, h: 30 }, discard: { x: 200, y: 50, w: 90, h: 30 } }),
    "alpha-open": main({ filter: { x: 720, y: 450, w: 200, h: 40 } }),
    "beta-main": main({ "reset-layout": { x: 1200, y: 800, w: 120, h: 32 } }),
  },
  // beta-main has no image yet: it must get the placeholder frame.
  available: ["alpha-main", "alpha-open"],
};

let root: HTMLElement;
let handle: SiteHandle | undefined;

function mockMatchMedia(matching: string[]) {
  window.matchMedia = ((query: string) => ({
    matches: matching.includes(query),
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}

function mount(content: SiteContent = fixture) {
  handle = render(root, content);
  return handle;
}

function key(target: EventTarget, init: KeyboardEventInit) {
  target.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
}

const originalMatchMedia = window.matchMedia;

beforeEach(() => {
  try {
    localStorage.clear();
  } catch {
    /* storage unavailable is fine */
  }
  history.replaceState(null, "", "/");
  mockMatchMedia([]);
  root = document.createElement("div");
  document.body.appendChild(root);
});

afterEach(() => {
  handle?.destroy();
  handle = undefined;
  root.remove();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-motion");
  window.matchMedia = originalMatchMedia;
  vi.restoreAllMocks();
});

describe("screen sections", () => {
  test("each screen renders a section with its heading and summary", () => {
    mount();
    for (const s of fixture.screens) {
      const section = document.getElementById(s.id);
      expect(section?.tagName).toBe("SECTION");
      expect(section?.querySelector("h2")?.textContent).toBe(s.title);
      expect(section?.textContent).toContain(s.summary);
    }
  });

  test("one annotated figure per shot, one numbered marker per placed control, in on-screen order", () => {
    mount();
    const alpha = document.getElementById("alpha")!;
    const figures = alpha.querySelectorAll("figure[data-shot]");
    expect([...figures].map((f) => f.getAttribute("data-shot"))).toEqual(["alpha-main", "alpha-open"]);

    const main = alpha.querySelector('figure[data-shot="alpha-main"]')!;
    const markers = [...main.querySelectorAll<HTMLElement>(".marker[data-control]")];
    expect(markers.map((m) => m.dataset.control)).toEqual(["save", "discard"]);
    expect(markers.map((m) => m.textContent)).toEqual(["1", "2"]);

    const open = alpha.querySelector('figure[data-shot="alpha-open"]')!;
    expect(open.querySelector('.marker[data-control="filter"]')?.textContent).toBe("1");
  });

  test("markers sit where positions.json puts them, scaled to the rendered width", () => {
    mount();
    const m = document.querySelector<HTMLElement>('figure[data-shot="alpha-open"] .marker[data-control="filter"]')!;
    // anchored on the top-left corner: x 720 of 1440 = 50%, y 450 of 900 = 50%
    expect(m.dataset.anchor).toBe("tl");
    expect(m.style.getPropertyValue("--x")).toBe("50%");
    expect(m.style.getPropertyValue("--y")).toBe("50%");
  });

  test("a narrow window's shot keeps its own size: framed narrow, markers scaled by its pixels", () => {
    const withRunner: SiteContent = {
      ...fixture,
      screens: [
        {
          ...fixture.screens[0],
          shots: [...fixture.screens[0].shots, { id: "alpha-runner", route: [{ runnerWindow: true }], alt: "The runner", size: { w: 460, h: 720 } }],
          controls: [
            ...fixture.screens[0].controls,
            { id: "close", shot: "alpha-runner", locate: { role: "button", name: "Close" }, name: "Close", does: "Closes it." },
          ],
        },
        ...fixture.screens.slice(1),
      ],
      positions: { ...fixture.positions, "alpha-runner": { size: { w: 460, h: 720 }, controls: { close: { x: 230, y: 180, w: 20, h: 20 } } } },
      available: [...fixture.available, "alpha-runner"],
    };
    mount(withRunner);
    const fig = document.querySelector('figure[data-shot="alpha-runner"]')!;
    const frame = fig.querySelector<HTMLElement>(".frame")!;
    expect(frame.classList.contains("is-narrow")).toBe(true);
    expect(frame.style.getPropertyValue("--shot-w")).toBe("460");
    expect(frame.style.getPropertyValue("--shot-h")).toBe("720");
    const img = fig.querySelector("img")!;
    expect([img.getAttribute("width"), img.getAttribute("height")]).toEqual(["460", "720"]);
    const m = fig.querySelector<HTMLElement>('.marker[data-control="close"]')!;
    expect(m.style.getPropertyValue("--x")).toBe("50%"); // 230 of 460
    expect(m.style.getPropertyValue("--y")).toBe("25%"); // 180 of 720

    // The main window's shots are unchanged: full width, 1440 x 900.
    const mainFrame = document.querySelector<HTMLElement>('figure[data-shot="alpha-main"] .frame')!;
    expect(mainFrame.classList.contains("is-narrow")).toBe(false);
    expect(mainFrame.style.getPropertyValue("--shot-w")).toBe("1440");
  });

  test("a control on the frame's left or top edge gets its marker on the far corner, so it is never cut in half", () => {
    const edge: SiteContent = {
      ...fixture,
      positions: {
        ...fixture.positions,
        "alpha-main": main({
          save: { x: 0, y: 300, w: 48, h: 40 }, // the left nav rail
          discard: { x: 400, y: 4, w: 160, h: 36 }, // the top context bar
        }),
        "alpha-open": main({ filter: { x: 2, y: 2, w: 40, h: 40 } }), // the corner
      },
    };
    mount(edge);
    const m = (id: string) => document.querySelector<HTMLElement>(`.marker[data-control="${id}"]`)!;
    expect(m("save").dataset.anchor).toBe("tr");
    expect(m("save").style.getPropertyValue("--x")).toBe("3.3333%"); // x + w = 48 of 1440
    expect(m("discard").dataset.anchor).toBe("bl");
    expect(m("discard").style.getPropertyValue("--y")).toBe("4.4444%"); // y + h = 40 of 900
    expect(m("filter").dataset.anchor).toBe("br");
  });

  test("markers and rows are numbered in reading order (top to bottom, then left to right) when positions exist", () => {
    const shuffled: SiteContent = {
      ...fixture,
      screens: [
        {
          ...fixture.screens[0],
          shots: [fixture.screens[0].shots[0]],
          controls: [
            { id: "low", shot: "alpha-main", locate: { text: "Low" }, name: "Low", does: "Sits at the bottom." },
            { id: "right", shot: "alpha-main", locate: { text: "Right" }, name: "Right", does: "Top row, right." },
            { id: "left", shot: "alpha-main", locate: { text: "Left" }, name: "Left", does: "Top row, left, a little taller." },
            { id: "loose", shot: "alpha-main", locate: { text: "Loose" }, name: "Loose", does: "No position yet." },
          ],
        },
      ],
      positions: {
        "alpha-main": main({
          low: { x: 100, y: 700, w: 80, h: 30 },
          right: { x: 900, y: 104, w: 80, h: 32 },
          left: { x: 100, y: 96, w: 80, h: 48 }, // same visual row as "right"
        }),
      },
    };
    mount(shuffled);
    const markers = [...document.querySelectorAll<HTMLElement>('figure[data-shot="alpha-main"] .marker')];
    expect(markers.map((x) => `${x.dataset.control}:${x.textContent}`)).toEqual(["left:1", "right:2", "low:3"]);
    const rows = [...document.querySelectorAll<HTMLElement>("#alpha [data-for]")];
    expect(rows.map((r) => r.dataset.for)).toEqual(["left", "right", "low", "loose"]);
    expect(rows.map((r) => r.querySelector(".row-num")?.textContent)).toEqual(["1", "2", "3", "4"]);
  });

  test("the control list rows are buttons with the name and what it does", () => {
    mount();
    const rows = [...document.querySelectorAll<HTMLButtonElement>('#alpha [data-for]')];
    expect(rows.map((r) => r.tagName)).toEqual(["BUTTON", "BUTTON", "BUTTON"]);
    expect(rows[0].textContent).toContain("Save");
    expect(rows[0].textContent).toContain("Keeps the draft.");
    expect(rows[0].id).toBe("alpha/save");
  });

  test("a control with no position keeps its row but gets no marker", () => {
    mount();
    expect(document.querySelector('#beta .marker[data-control="unplaced"]')).toBeNull();
    const row = document.getElementById("beta/unplaced");
    expect(row?.tagName).toBe("BUTTON");
    expect(row?.textContent).toContain("Unplaced option");
  });

  test("a shot with no image yet shows a placeholder frame instead of an img", () => {
    mount();
    const beta = document.querySelector('figure[data-shot="beta-main"]')!;
    expect(beta.querySelector("img")).toBeNull();
    expect(beta.querySelector(".placeholder")).not.toBeNull();
    const alpha = document.querySelector('figure[data-shot="alpha-main"] img') as HTMLImageElement;
    expect(alpha.getAttribute("alt")).toBe("The alpha screen");
  });

  test("key caps markup renders as kbd elements", () => {
    mount();
    const kbds = [...document.querySelectorAll("#alpha kbd")].map((k) => k.textContent);
    expect(kbds).toEqual(["Ctrl", "S"]);
  });

  test("a quick start step whose section is missing is plain text, not a dead link", () => {
    mount();
    const strip = document.querySelector(".quickstart")!;
    expect(strip.querySelector('a[href="#alpha"]')).not.toBeNull();
    expect(strip.querySelector('a[href="#nowhere"]')).toBeNull();
    expect(strip.textContent).toContain("Somewhere else");
  });

  test("the sidebar groups screens under their group headings in the fixed order", () => {
    mount();
    const nav = document.querySelector("nav.sidebar")!;
    const groups = [...nav.querySelectorAll(".nav-group-title")].map((g) => g.textContent);
    expect(groups).toEqual(["Test cases", "Settings"]);
    expect(nav.querySelector('a[href="#beta"]')?.textContent).toContain("Beta Screen");
  });
});

describe("spotlight", () => {
  test("hovering a row activates its marker and the figure", () => {
    mount();
    const row = document.getElementById("alpha/discard")!;
    row.dispatchEvent(new MouseEvent("mouseenter"));
    const fig = document.querySelector<HTMLElement>('figure[data-shot="alpha-main"]')!;
    expect(fig.dataset.active).toBe("discard");
    expect(fig.querySelector('.marker[data-control="discard"]')?.hasAttribute("data-active")).toBe(true);
    expect(fig.querySelector('.marker[data-control="save"]')?.hasAttribute("data-active")).toBe(false);
    expect(fig.querySelector(".caption")?.textContent).toContain("Throws the draft away.");

    row.dispatchEvent(new MouseEvent("mouseleave"));
    expect(fig.dataset.active).toBeUndefined();
    expect(fig.querySelector('.marker[data-control="discard"]')?.hasAttribute("data-active")).toBe(false);
  });

  test("focusing a row does the same, so the keyboard gets the spotlight too", () => {
    mount();
    const row = document.getElementById("alpha/save") as HTMLButtonElement;
    row.focus();
    const fig = document.querySelector<HTMLElement>('figure[data-shot="alpha-main"]')!;
    expect(fig.dataset.active).toBe("save");
    row.blur();
    expect(fig.dataset.active).toBeUndefined();
  });

  test("hovering a marker spotlights it too", () => {
    mount();
    const marker = document.querySelector<HTMLElement>('.marker[data-control="save"]')!;
    marker.dispatchEvent(new MouseEvent("mouseenter"));
    expect(document.querySelector<HTMLElement>('figure[data-shot="alpha-main"]')!.dataset.active).toBe("save");
  });

  test("a row whose control has no position spotlights nothing", () => {
    mount();
    document.getElementById("beta/unplaced")!.dispatchEvent(new MouseEvent("mouseenter"));
    expect(document.querySelector<HTMLElement>('figure[data-shot="beta-main"]')!.dataset.active).toBeUndefined();
  });

  test("a #screen/control deep link spotlights that control on load", () => {
    history.replaceState(null, "", "#alpha/discard");
    const spy = vi.spyOn(Element.prototype, "scrollIntoView");
    mount();
    expect(document.querySelector<HTMLElement>('figure[data-shot="alpha-main"]')!.dataset.active).toBe("discard");
    expect(spy.mock.contexts).toContain(document.getElementById("alpha/discard"));
  });
});

describe("geometry", () => {
  test("the spotlight rings the control's box with a small margin, in shot percentages", () => {
    expect(spotStyle({ x: 720, y: 450, w: 144, h: 90 })).toEqual({
      left: "calc(50% - 6px)",
      top: "calc(50% - 6px)",
      width: "calc(10% + 12px)",
      height: "calc(10% + 12px)",
    });
  });

  test("a narrow shot's spotlight is in percentages of its own size", () => {
    expect(spotStyle({ x: 230, y: 360, w: 46, h: 72 }, { w: 460, h: 720 })).toEqual({
      left: "calc(50% - 6px)",
      top: "calc(50% - 6px)",
      width: "calc(10% + 12px)",
      height: "calc(10% + 12px)",
    });
  });

  test("a narrow shot's caption scales by its own size", () => {
    // 460 x 720 shot in a 230 x 360 frame: half scale. Box 200..260 x 100..140
    // = 100..130 x 50..70 frame; centre 115 - 50 = 65; below: 70 + 14 = 84.
    const p = captionPlacement({ x: 200, y: 100, w: 60, h: 40 }, { w: 230, h: 360 }, { w: 100, h: 40 }, { w: 460, h: 720 });
    expect(p).toEqual({ left: 65, top: 84, side: "below" });
  });

  // A 720 x 450 frame is the shot at half scale: shot pixels / 2 = frame pixels.
  const frame = { w: 720, h: 450 };
  const cap = { w: 200, h: 60 };

  test("a caption sits centred below a control that has room below it", () => {
    // box 600..700 x 100..140 shot = 300..350 x 50..70 frame; centre 325 - 100; top 70 + 14
    expect(captionPlacement({ x: 600, y: 100, w: 100, h: 40 }, frame, cap)).toEqual({ left: 225, top: 84, side: "below" });
  });

  test("a caption flips above a control with no room below", () => {
    // box y 800..860 shot = 400..430 frame; above: 400 - 14 - 60 = 326
    expect(captionPlacement({ x: 600, y: 800, w: 100, h: 60 }, frame, cap)).toEqual({ left: 225, top: 326, side: "above" });
  });

  test("a caption near a side edge shifts inside the frame instead of being clipped", () => {
    expect(captionPlacement({ x: 0, y: 100, w: 40, h: 40 }, frame, cap).left).toBe(8);
    expect(captionPlacement({ x: 1400, y: 100, w: 40, h: 40 }, frame, cap).left).toBe(720 - 200 - 8);
  });

  test("a caption with no room above or below still stays inside the frame", () => {
    const tall = { w: 200, h: 400 };
    const p = captionPlacement({ x: 600, y: 400, w: 100, h: 100 }, frame, tall);
    expect(p.top).toBeGreaterThanOrEqual(8);
    expect(p.top + tall.h).toBeLessThanOrEqual(450 - 8);
  });
});

describe("contents drawer (narrow windows)", () => {
  const NARROW = "(max-width: 960px)";

  test("closed, it is inert - out of the Tab order and the accessibility tree", () => {
    mockMatchMedia([NARROW]);
    mount();
    expect(document.getElementById("sidebar")!.hasAttribute("inert")).toBe(true);
  });

  test("opening moves focus to its first link; closing hands focus back to the toggle", () => {
    mockMatchMedia([NARROW]);
    mount();
    const toggle = document.querySelector<HTMLButtonElement>("button.menu-btn")!;
    const sidebar = document.getElementById("sidebar")!;
    toggle.focus();
    toggle.click();
    expect(sidebar.hasAttribute("inert")).toBe(false);
    expect(document.activeElement).toBe(sidebar.querySelector("a"));
    key(document.activeElement!, { key: "Escape" });
    expect(sidebar.hasAttribute("inert")).toBe(true);
    expect(document.activeElement).toBe(toggle);
  });

  test("on a wide window the sidebar is never inert", () => {
    mount();
    expect(document.getElementById("sidebar")!.hasAttribute("inert")).toBe(false);
  });
});

describe("deep links", () => {
  test("a malformed hash does not throw", () => {
    history.replaceState(null, "", "#50%");
    expect(() => mount()).not.toThrow();
  });
});

describe("search", () => {
  test("fuzzy score prefers exact, then prefix, then substring, then a subsequence; no match is 0", () => {
    const exact = score("reset layout", "Reset layout");
    const prefix = score("reset", "Reset layout");
    const inner = score("layout", "Reset layout");
    const loose = score("rstlay", "Reset layout");
    expect(exact).toBeGreaterThan(prefix);
    expect(prefix).toBeGreaterThan(inner);
    expect(inner).toBeGreaterThan(loose);
    expect(loose).toBeGreaterThan(0);
    expect(score("zzz", "Reset layout")).toBe(0);
  });

  test("Ctrl+K opens the palette; typing a control name lists it first; Enter jumps to and flashes it", () => {
    const spy = vi.spyOn(Element.prototype, "scrollIntoView");
    mount();
    key(document, { key: "k", ctrlKey: true });
    const input = document.querySelector<HTMLInputElement>('.palette input[role="combobox"]')!;
    expect(input).not.toBeNull();
    expect(document.activeElement).toBe(input);

    input.value = "Reset layout";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    const first = document.querySelector('.palette [role="option"]')!;
    expect(first.textContent).toContain("Reset layout");
    expect(first.getAttribute("aria-selected")).toBe("true");

    key(input, { key: "Enter" });
    expect(location.hash).toBe("#beta/reset-layout");
    const row = document.getElementById("beta/reset-layout")!;
    expect(row.classList.contains("flash")).toBe(true);
    expect(spy.mock.contexts).toContain(row);
    expect(document.querySelector<HTMLElement>('figure[data-shot="beta-main"]')!.dataset.active).toBe("reset-layout");
    expect(document.querySelector(".palette")?.hasAttribute("hidden")).toBe(true);
  });

  test("search covers screens, tips and recipes too, and Escape closes it", () => {
    mount();
    key(document, { key: "k", ctrlKey: true });
    const input = document.querySelector<HTMLInputElement>('.palette input[role="combobox"]')!;
    const find = (q: string) => {
      input.value = q;
      input.dispatchEvent(new Event("input", { bubbles: true }));
      return [...document.querySelectorAll('.palette [role="option"]')].map((o) => o.textContent ?? "");
    };
    expect(find("Beta Screen")[0]).toContain("Beta Screen");
    expect(find("keep the draft").some((t) => t.includes("Ctrl+S to keep the draft"))).toBe(true);
    expect(find("first run")[0]).toContain("Your first run");
    key(input, { key: "Escape" });
    expect(document.querySelector(".palette")?.hasAttribute("hidden")).toBe(true);
  });

  test("arrow keys move the selection", () => {
    mount();
    key(document, { key: "k", ctrlKey: true });
    const input = document.querySelector<HTMLInputElement>('.palette input[role="combobox"]')!;
    input.value = "a";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    key(input, { key: "ArrowDown" });
    const options = document.querySelectorAll('.palette [role="option"]');
    expect(options[1].getAttribute("aria-selected")).toBe("true");
    expect(input.getAttribute("aria-activedescendant")).toBe(options[1].id);
  });
});

describe("theme", () => {
  test("follows the system by default, and the toggle switches data-theme and every screenshot", () => {
    mockMatchMedia(["(prefers-color-scheme: dark)"]);
    mount();
    const html = document.documentElement;
    expect(html.dataset.theme).toBe("dark");
    const imgs = () => [...document.querySelectorAll<HTMLImageElement>("img[data-shot]")].map((i) => i.getAttribute("src"));
    expect(imgs().length).toBeGreaterThan(0);
    expect(imgs().every((s) => s?.startsWith("img/dark/"))).toBe(true);

    document.querySelector<HTMLButtonElement>("button.theme-toggle")!.click();
    expect(html.dataset.theme).toBe("light");
    expect(imgs().every((s) => s?.startsWith("img/light/"))).toBe(true);
    expect(imgs()).toContain("img/light/alpha-main.jpg");
  });

  test("the choice is remembered", () => {
    mount();
    expect(document.documentElement.dataset.theme).toBe("light");
    document.querySelector<HTMLButtonElement>("button.theme-toggle")!.click();
    handle!.destroy();
    root.replaceChildren();
    mount();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  test("storage that throws does not break the page", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    mount();
    document.querySelector<HTMLButtonElement>("button.theme-toggle")!.click();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});

describe("reduced motion", () => {
  const anyAnimated = () => document.querySelector(ANIMATION_CLASSES.map((c) => `.${c}`).join(","));

  test("with motion allowed, animation classes are applied (so the next test means something)", () => {
    mount();
    expect(anyAnimated()).not.toBeNull();
  });

  test("prefers-reduced-motion applies no animation classes, even after a search jump", () => {
    mockMatchMedia(["(prefers-reduced-motion: reduce)"]);
    mount();
    expect(document.documentElement.dataset.motion).toBe("reduce");
    expect(anyAnimated()).toBeNull();

    key(document, { key: "k", ctrlKey: true });
    const input = document.querySelector<HTMLInputElement>('.palette input[role="combobox"]')!;
    input.value = "Save";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    key(input, { key: "Enter" });
    expect(location.hash).toBe("#alpha/save");
    expect(anyAnimated()).toBeNull();
  });
});
