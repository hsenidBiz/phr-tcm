import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, test, vi } from "vitest";
import { ANIMATION_CLASSES, render, WIDE, type SiteHandle } from "./render/layout";
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

  test("markers sit just outside their control's corner from positions.json, in shot percentages", () => {
    mount();
    const m = document.querySelector<HTMLElement>('figure[data-shot="alpha-open"] .marker[data-control="filter"]')!;
    // top-left corner 720,450, the marker centred 9 px (0.75 of its 12 px
    // radius) up and left of it: 711 of 1440, 441 of 900
    expect(m.dataset.anchor).toBe("tl");
    expect(m.style.getPropertyValue("--x")).toBe("49.375%");
    expect(m.style.getPropertyValue("--y")).toBe("49%");
    // its size, as a share of the frame width: 24 of 1440
    expect(m.style.getPropertyValue("--d")).toBe("1.6667");
    expect(m.hasAttribute("data-leader")).toBe(false);
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
    expect(m.style.getPropertyValue("--x")).toBe("48.0435%"); // 230 - 9 of 460
    expect(m.style.getPropertyValue("--y")).toBe("23.75%"); // 180 - 9 of 720

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
    expect(m("save").style.getPropertyValue("--x")).toBe("3.9583%"); // x + w + 9 = 57 of 1440
    expect(m("discard").dataset.anchor).toBe("bl");
    expect(m("discard").style.getPropertyValue("--y")).toBe("5.4444%"); // y + h + 9 = 49 of 900
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
  test("opens light by default, even on a system set to dark, as the app does", () => {
    mockMatchMedia(["(prefers-color-scheme: dark)"]);
    mount();
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.querySelector("button.theme-toggle")?.getAttribute("aria-label")).toBe("Switch to dark theme");
  });

  test("the toggle switches data-theme and every screenshot, both ways", () => {
    mount();
    const html = document.documentElement;
    const imgs = () => [...document.querySelectorAll<HTMLImageElement>("img[data-shot]")].map((i) => i.getAttribute("src"));
    expect(imgs().length).toBeGreaterThan(0);
    expect(imgs().every((s) => s?.startsWith("img/light/"))).toBe(true);

    document.querySelector<HTMLButtonElement>("button.theme-toggle")!.click();
    expect(html.dataset.theme).toBe("dark");
    expect(imgs().every((s) => s?.startsWith("img/dark/"))).toBe(true);
    expect(imgs()).toContain("img/dark/alpha-main.jpg");

    document.querySelector<HTMLButtonElement>("button.theme-toggle")!.click();
    expect(html.dataset.theme).toBe("light");
    expect(imgs().every((s) => s?.startsWith("img/light/"))).toBe(true);
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

// A busy screen split into two groups, on one shot (so it gets an overview).
const grouped: SiteContent = {
  ...fixture,
  screens: [
    ...fixture.screens,
    {
      id: "gamma",
      title: "Gamma Screen",
      group: "Work Manager",
      summary: "Gamma summary text.",
      shots: [{ id: "gamma-main", route: [{ nav: "Gamma" }], alt: "The gamma screen" }],
      groups: [
        { id: "top-card", title: "The top card", summary: "What the top card does." },
        { id: "bottom-card", title: "The bottom card" },
      ],
      controls: [
        { id: "a", shot: "gamma-main", group: "top-card", locate: { text: "A" }, name: "Control A", does: "Does A." },
        { id: "b", shot: "gamma-main", group: "top-card", locate: { text: "B" }, name: "Control B", does: "Does B." },
        { id: "c", shot: "gamma-main", group: "bottom-card", locate: { text: "C" }, name: "Control C", does: "Does C." },
        { id: "d", shot: "gamma-main", group: "bottom-card", locate: { text: "D" }, name: "Control D", does: "Does D." },
      ],
    },
  ],
  positions: {
    ...fixture.positions,
    "gamma-main": main({
      a: { x: 100, y: 100, w: 80, h: 30 },
      b: { x: 200, y: 100, w: 80, h: 30 },
      c: { x: 100, y: 700, w: 100, h: 30 },
      d: { x: 400, y: 720, w: 60, h: 20 },
    }),
  },
  available: [...fixture.available, "gamma-main"],
};

const quietObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
  takeRecords() {
    return [];
  }
};

describe("grouped screens", () => {
  test("show the shot once as an overview whose outlined parts link to the subsections, with no numbered markers", () => {
    mount(grouped);
    const overview = document.querySelector('#gamma figure[data-overview="gamma-main"]')!;
    expect(overview.querySelector(".marker")).toBeNull();
    const links = [...overview.querySelectorAll<HTMLAnchorElement>("a.region")];
    expect(links.map((a) => [a.getAttribute("href"), a.textContent])).toEqual([
      ["#gamma/top-card", "The top card"],
      ["#gamma/bottom-card", "The bottom card"],
    ]);
    // the union of the group's boxes, padded by 10: 90..290 x 90..140 of 1440 x 900
    expect(links[0].style.left).toBe("6.25%");
    expect(links[0].style.width).toBe("13.8889%");
  });

  test("then one subsection per group: a heading at #screen/group, a zoomed crop, markers numbered within the group", () => {
    mount(grouped);
    const subs = [...document.querySelectorAll<HTMLElement>("#gamma section.subsection")];
    expect(subs.map((s) => s.id)).toEqual(["gamma/top-card", "gamma/bottom-card"]);
    expect(subs[0].querySelector("h3")?.textContent).toBe("The top card");
    expect(subs[0].textContent).toContain("What the top card does.");

    const frame = subs[0].querySelector<HTMLElement>(".frame")!;
    expect(frame.classList.contains("is-crop")).toBe(true);
    // 100..280 x 100..130, padded 48, at least 40% of 1440 wide and a quarter as tall
    expect([frame.style.getPropertyValue("--shot-w"), frame.style.getPropertyValue("--shot-h")]).toEqual(["576", "144"]);
    const img = frame.querySelector("img")!;
    expect(img.style.width).toBe("250%"); // 1440 of 576
    expect(img.style.top).toBe("-29.8611%"); // 43 of 144

    const numbers = (i: number) => [...subs[i].querySelectorAll<HTMLElement>(".marker")].map((m) => `${m.dataset.control}:${m.textContent}`);
    expect(numbers(0)).toEqual(["a:1", "b:2"]);
    expect(numbers(1)).toEqual(["c:1", "d:2"]);
    // rows keep their #screen/control anchors
    expect(document.getElementById("gamma/d")?.closest("section.subsection")).toBe(subs[1]);
  });

  test("a #screen/group link scrolls to the subsection and focuses it", () => {
    const spy = vi.spyOn(Element.prototype, "scrollIntoView");
    history.replaceState(null, "", "#gamma/bottom-card");
    mount(grouped);
    const sub = document.getElementById("gamma/bottom-card")!;
    expect(spy.mock.contexts).toContain(sub);
    expect(document.activeElement).toBe(sub);
  });

  test("the sidebar lists a screen's groups under it, and scroll-spy opens and marks them", () => {
    type Entries = { target: Element; isIntersecting: boolean }[];
    const callbacks: ((entries: Entries) => void)[] = [];
    const fire = (entries: Entries) => callbacks.forEach((cb) => cb(entries));
    vi.stubGlobal(
      "IntersectionObserver",
      class extends quietObserver {
        constructor(cb: (entries: Entries) => void) {
          super();
          callbacks.push(cb);
        }
      },
    );
    try {
      mount(grouped);
      const nav = document.getElementById("sidebar")!;
      const subLinks = [...nav.querySelectorAll<HTMLAnchorElement>("a.nav-sublink")];
      expect(subLinks.map((a) => a.getAttribute("href"))).toEqual(["#gamma/top-card", "#gamma/bottom-card"]);
      const screenLi = nav.querySelector('a[data-spy="gamma"]')!.parentElement!;
      expect(screenLi.hasAttribute("data-open")).toBe(false);

      fire([
        { target: document.getElementById("gamma")!, isIntersecting: true },
        { target: document.getElementById("gamma/bottom-card")!, isIntersecting: true },
      ]);
      expect(screenLi.hasAttribute("data-open")).toBe(true);
      expect(subLinks[1].getAttribute("aria-current")).toBe("true");
      expect(subLinks[0].hasAttribute("aria-current")).toBe(false);
    } finally {
      vi.stubGlobal("IntersectionObserver", quietObserver);
    }
  });

  test("search finds a group by its title and jumps to its subsection", () => {
    mount(grouped);
    key(document, { key: "k", ctrlKey: true });
    const input = document.querySelector<HTMLInputElement>('.palette input[role="combobox"]')!;
    input.value = "The bottom card";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    const first = document.querySelector('.palette [role="option"]')!;
    expect(first.textContent).toContain("Section");
    key(input, { key: "Enter" });
    expect(location.hash).toBe("#gamma/bottom-card");
  });
});

describe("page width and stage flow", () => {
  test("a very wide window gets the wide layout (a whole shot at its own size, the list beside it)", () => {
    mockMatchMedia([WIDE]);
    mount();
    expect(document.querySelector<HTMLElement>(".site")!.dataset.layout).toBe("wide");
  });

  test("any other window gets the standard layout", () => {
    mount();
    expect(document.querySelector<HTMLElement>(".site")!.dataset.layout).toBe("standard");
  });

  test("the layout follows the window when it crosses the breakpoint", () => {
    const listeners: (() => void)[] = [];
    let wide = false;
    window.matchMedia = ((query: string) => ({
      get matches() {
        return query === WIDE && wide;
      },
      media: query,
      addEventListener: (_: string, fn: () => void) => {
        if (query === WIDE) listeners.push(fn);
      },
      removeEventListener: () => {},
    })) as unknown as typeof window.matchMedia;
    mount();
    const site = document.querySelector<HTMLElement>(".site")!;
    expect(site.dataset.layout).toBe("standard");
    wide = true;
    listeners.forEach((fn) => fn());
    expect(site.dataset.layout).toBe("wide");
  });

  test("a stage draws its figure no taller than the window allows, and puts its list beside it when both fit, else below", () => {
    const observers: { cb: () => void; el: Element }[] = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        cb: () => void;
        constructor(cb: () => void) {
          this.cb = cb;
        }
        observe(el: Element) {
          observers.push({ cb: this.cb, el });
        }
        unobserve() {}
        disconnect() {}
      },
    );
    try {
      mount();
      const stage = document.querySelector<HTMLElement>('#alpha figure[data-shot="alpha-main"]')!.closest<HTMLElement>(".stage")!;
      expect(stage.dataset.flow).toBe("below"); // no width yet
      const resize = (w: number) => {
        Object.defineProperty(stage, "clientWidth", { value: w, configurable: true });
        observers.filter((o) => o.el === stage).forEach((o) => o.cb());
      };
      // jsdom's window is 768 tall: a figure may be 768 - 116 = 652 tall,
      // so the 1440 x 900 shot is drawn 1043 wide.
      expect(stage.style.getPropertyValue("--fig-w")).toBe("1043");
      resize(1868);
      expect(stage.dataset.flow).toBe("beside");
      resize(1300); // 1043 + 28 + 340 does not fit
      expect(stage.dataset.flow).toBe("below");
      // a taller window lets the shot reach its own size
      const tall = window.innerHeight;
      Object.defineProperty(window, "innerHeight", { value: 1100, configurable: true });
      try {
        window.dispatchEvent(new Event("resize"));
        expect(stage.style.getPropertyValue("--fig-w")).toBe("1440");
        resize(1868); // 1440 + 28 + 340 fits
        expect(stage.dataset.flow).toBe("beside");
        resize(1440);
        expect(stage.dataset.flow).toBe("below");
      } finally {
        Object.defineProperty(window, "innerHeight", { value: tall, configurable: true });
      }
    } finally {
      vi.unstubAllGlobals();
      vi.stubGlobal("IntersectionObserver", quietObserver);
    }
  });
});

describe("full-screen view", () => {
  const expandBtn = () => document.querySelector<HTMLButtonElement>('#alpha figure[data-shot="alpha-main"] .shot-expand')!;
  const viewer = () => document.querySelector<HTMLElement>(".viewer")!;
  const dialog = () => viewer().querySelector<HTMLElement>('[role="dialog"]')!;

  test("the expand button is named for the shot and opens a labelled modal dialog with the shot, its markers and its list", () => {
    mount();
    const btn = expandBtn();
    expect(btn.getAttribute("aria-label")).toBe("Open The alpha screen full screen");
    expect(viewer().hidden).toBe(true);
    btn.click();
    expect(viewer().hidden).toBe(false);
    expect(dialog().getAttribute("aria-modal")).toBe("true");
    const label = document.getElementById(dialog().getAttribute("aria-labelledby")!);
    expect(label?.textContent).toBe("The alpha screen");
    expect([...dialog().querySelectorAll<HTMLElement>(".marker")].map((m) => m.textContent)).toEqual(["1", "2"]);
    expect(dialog().querySelectorAll(".row")).toHaveLength(2);
    // its rows are not the page's anchors
    expect(dialog().querySelector('[id="alpha/save"]')).toBeNull();
    expect(document.documentElement.classList.contains("viewer-open")).toBe(true);
  });

  test("hovering a row in the view spotlights the view's copy of the shot", () => {
    mount();
    expandBtn().click();
    const row = dialog().querySelector<HTMLElement>('.row[data-for="discard"]')!;
    row.dispatchEvent(new MouseEvent("mouseenter"));
    expect(dialog().querySelector<HTMLElement>("figure")!.dataset.active).toBe("discard");
    expect(dialog().querySelector(".caption")?.textContent).toContain("Throws the draft away.");
    // the inline figure is untouched
    expect(document.querySelector<HTMLElement>('#alpha figure[data-shot="alpha-main"]')!.dataset.active).toBeUndefined();
  });

  test("clicking the image opens it too", () => {
    mount();
    document.querySelector<HTMLImageElement>('#alpha figure[data-shot="alpha-main"] img')!.click();
    expect(viewer().hidden).toBe(false);
  });

  test("Escape closes it, unlocks the page and gives focus back to the expand button", () => {
    mount();
    expandBtn().focus();
    expandBtn().click();
    expect(dialog().contains(document.activeElement)).toBe(true);
    key(document.activeElement!, { key: "Escape" });
    expect(viewer().hidden).toBe(true);
    expect(document.documentElement.classList.contains("viewer-open")).toBe(false);
    expect(document.activeElement).toBe(expandBtn());
  });

  test("the close button and a click on the backdrop close it", () => {
    mount();
    expandBtn().click();
    dialog().querySelector<HTMLButtonElement>(".viewer-close")!.click();
    expect(viewer().hidden).toBe(true);
    expect(document.activeElement).toBe(expandBtn());

    document.querySelector<HTMLImageElement>('#alpha figure[data-shot="alpha-main"] img')!.click();
    expect(viewer().hidden).toBe(false);
    viewer().querySelector<HTMLElement>(".viewer-backdrop")!.click();
    expect(viewer().hidden).toBe(true);
    expect(document.activeElement).toBe(expandBtn());
  });

  test("Tab stays inside it, and focus pulled out to the page comes back", () => {
    mount();
    expandBtn().click();
    const stops = [...dialog().querySelectorAll<HTMLElement>("button")];
    const first = stops[0];
    const last = stops[stops.length - 1];
    expect(document.activeElement).toBe(first);
    last.focus();
    key(last, { key: "Tab" });
    expect(document.activeElement).toBe(first);
    key(first, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);
    document.querySelector<HTMLElement>("button.theme-toggle")!.focus();
    expect(dialog().contains(document.activeElement)).toBe(true);
  });

  test("page shortcuts wait while it is open", () => {
    mount();
    expandBtn().click();
    key(document.activeElement!, { key: "k", ctrlKey: true });
    expect(document.querySelector(".palette")?.hasAttribute("hidden")).toBe(true);
  });

  test("under reduced motion it applies no animation classes", () => {
    mockMatchMedia(["(prefers-reduced-motion: reduce)"]);
    mount();
    expandBtn().click();
    expect(dialog().querySelector(ANIMATION_CLASSES.map((c) => `.${c}`).join(","))).toBeNull();
  });

  test("a shot with no image yet has no expand button", () => {
    mount();
    expect(document.querySelector('figure[data-shot="beta-main"] .shot-expand')).toBeNull();
  });
});

describe("full-screen view, continued", () => {
  const viewer = () => document.querySelector<HTMLElement>(".viewer")!;
  const dialog = () => viewer().querySelector<HTMLElement>('[role="dialog"]')!;
  const open = () => document.querySelector<HTMLButtonElement>('#alpha figure[data-shot="alpha-main"] .shot-expand')!.click();

  test("Escape still closes it after a click on the shot left focus on the page body", () => {
    mount();
    open();
    (document.activeElement as HTMLElement).blur();
    expect(document.activeElement).toBe(document.body);
    key(document.body, { key: "Escape" });
    expect(viewer().hidden).toBe(true);
  });

  test("a click on the shot or the list focuses the dialog, never the page", () => {
    mount();
    open();
    expect(dialog().getAttribute("tabindex")).toBe("-1");
  });

  test("a click on the empty space around the shot closes it", () => {
    mount();
    open();
    viewer().querySelector<HTMLElement>(".viewer-stage")!.click();
    expect(viewer().hidden).toBe(true);
  });

  test("the page behind is inert while it is open, and not after", () => {
    mount();
    const shell = document.querySelector<HTMLElement>(".shell")!;
    const topbar = document.querySelector<HTMLElement>(".topbar")!;
    open();
    expect(shell.hasAttribute("inert")).toBe(true);
    expect(topbar.hasAttribute("inert")).toBe(true);
    expect(viewer().hasAttribute("inert")).toBe(false);
    key(document.body, { key: "Escape" });
    expect(shell.hasAttribute("inert")).toBe(false);
    expect(topbar.hasAttribute("inert")).toBe(false);
  });

  test("a hash change (the Back button, a link) closes it", () => {
    mount();
    open();
    history.replaceState(null, "", "#beta");
    window.dispatchEvent(new HashChangeEvent("hashchange"));
    expect(viewer().hidden).toBe(true);
  });

  test("a group's crop opens as that crop, named for its group", () => {
    mount(grouped);
    const btn = document.querySelector<HTMLButtonElement>('[id="gamma/top-card"] .shot-expand')!;
    expect(btn.getAttribute("aria-label")).toBe("Open The top card full screen");
    expect(document.querySelector('[id="gamma/top-card"] ol.controls')?.getAttribute("aria-label")).toBe("Controls in The top card");
    btn.click();
    const frame = dialog().querySelector<HTMLElement>(".frame")!;
    expect(frame.classList.contains("is-crop")).toBe(true);
    expect([frame.style.getPropertyValue("--shot-w"), frame.style.getPropertyValue("--shot-h")]).toEqual(["576", "144"]);
    expect([...dialog().querySelectorAll<HTMLElement>(".marker")].map((m) => `${m.dataset.control}:${m.textContent}`)).toEqual(["a:1", "b:2"]);
    expect(dialog().querySelector(".viewer-context")?.textContent).toBe("Gamma Screen · The top card");
  });
});

describe("overview labels", () => {
  test("a region at the very top of the shot has its label inside it, where the frame cannot cut it", () => {
    const top: SiteContent = {
      ...grouped,
      positions: { ...grouped.positions, "gamma-main": main({ ...grouped.positions["gamma-main"].controls, a: { x: 100, y: 4, w: 80, h: 30 } }) },
    };
    mount(top);
    const [first, second] = [...document.querySelectorAll<HTMLElement>('figure[data-overview="gamma-main"] a.region')];
    expect(first.classList.contains("is-top")).toBe(true);
    expect(second.classList.contains("is-top")).toBe(false);
  });
});
