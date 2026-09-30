import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, expect, test, vi } from "vitest";
import Settings from "./Settings";
import { CHANGELOG } from "../lib/changelog";
import { RATE_LEVELS } from "../lib/adoRate";
import { toast } from "../lib/toast";
import { resetExtrasStore, setExtrasUnlocked } from "../lib/extras";
import { TILE_MS, TILE_SAFETY_MS, TILE_STAGGER_MS, WIDE_QUERY } from "../components/settings/useTileLayout";
import { planClose, planOpen } from "../components/settings/tileSchedule";
import { EASE, foldMs } from "../components/ui/collapse";
import { parseEasing } from "../lib/cubicBezier";

vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

const realMatchMedia = window.matchMedia;

afterEach(() => {
  window.matchMedia = realMatchMedia;
  clearMocks();
  vi.clearAllMocks();
  resetExtrasStore();
  localStorage.removeItem("tcm-v2-dev-capture");
});

function renderSettings(qc: QueryClient) {
  return render(
    <QueryClientProvider client={qc}>
      <Settings org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

/// Reporting a bug is one click from the gear: the button sits in the
/// Help & support card beside How To Use and the tour, not behind the Logs
/// panel (it used to sit in the Changelog header).
test("Report a bug opens its dialog straight from Help & support", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  expect(screen.getByRole("button", { name: "Changelog", pressed: true })).toBeInTheDocument();
  const help = screen.getByRole("heading", { name: "Help & support" }).closest("section")!;
  fireEvent.click(within(help).getByRole("button", { name: "Report a bug" }));
  expect(await screen.findByText("Report a bug in this app")).toBeInTheDocument();
});

test("manual update check seeds the [\"update\"] query the App banner reads", async () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  mockIPC((cmd) => {
    if (cmd === "check_update") return "9.9.9";
    if (cmd === "plugin:app|version") return "1.6.0";
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: /Check for updates/ }));
  // The banner in App renders from this cache entry (fetched once at
  // startup with staleTime Infinity) - a manual check must write it too.
  await waitFor(() => expect(qc.getQueryData(["update"])).toBe("9.9.9"));
});

test("up-to-date check clears any stale banner state", async () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  qc.setQueryData(["update"], "0.0.1"); // pretend a stale value
  mockIPC((cmd) => {
    if (cmd === "check_update") return null;
    if (cmd === "plugin:app|version") return "1.6.0";
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: /Check for updates/ }));
  await waitFor(() => expect(qc.getQueryData(["update"])).toBeNull());
});

/// The changelog used to fill the right column. It now opens on the latest
/// version, with the rest behind Show more.
test("the changelog shows the latest version, and Show more unfolds the history", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  expect(await screen.findByRole("heading", { name: "Changelog" })).toBeInTheDocument();
  // No line of description under the title - the heading says it.
  expect(screen.queryByText(/the same notes the post-update popup shows/)).not.toBeInTheDocument();
  expect(screen.getByText(`Version ${CHANGELOG[0].version}`)).toBeInTheDocument();
  expect(screen.queryByText("Version 1.9.0")).not.toBeInTheDocument();

  const more = screen.getByRole("button", { name: `Show more (${CHANGELOG.length - 1} earlier versions)` });
  expect(more).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(more);
  expect(screen.getByText("Version 1.9.0")).toBeInTheDocument();
  expect(screen.getByText("Version 1.7.1")).toBeInTheDocument();

  const less = screen.getByRole("button", { name: "Show less" });
  expect(less).toHaveAttribute("aria-expanded", "true");
  fireEvent.click(less);
  expect(screen.queryByText("Version 1.9.0")).not.toBeInTheDocument();
});

/// Below the wide breakpoint (jsdom's default: no media query matches)
/// Settings is one column: every card in its natural order, then the
/// changelog/log panel on its own.
test("a narrow window lists the setting cards in order, then only the changelog", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  await screen.findByRole("heading", { name: "Changelog" });
  const { left, right } = columns();
  expect(left).not.toBe(right);

  expect(headings(left)).toEqual(ALL_CARDS);
  expect(headings(right)).toEqual(["Changelog"]);
  // No settings in the right column: no switch, and none of the buttons
  // that moved into cards.
  expect(within(right).queryAllByRole("switch")).toHaveLength(0);
  for (const name of ["Report a bug", "Export to file", "Check for updates", "How To Use"]) {
    expect(within(right).queryByRole("button", { name })).not.toBeInTheDocument();
  }

  // The General card carries the tray switches and the request rate.
  const general = screen.getByRole("heading", { name: "General" }).closest("section")!;
  expect(within(general).getByRole("switch", { name: "Keep running in the tray when closed" })).toBeInTheDocument();
  expect(within(general).getByRole("switch", { name: "Start with Windows" })).toBeInTheDocument();
  expect(within(general).getByRole("button", { name: /^Full speed/ })).toBeInTheDocument();

  // Nothing moves on a narrow window: Show more only unfolds the history.
  fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
  expect(screen.getByText("Version 1.9.0")).toBeInTheDocument();
  expect(headings(columns().left)).toEqual(ALL_CARDS);
  expect(headings(columns().right)).toEqual(["Changelog"]);
});

// ---- The wide layout: cards under the changelog, sliding aside ------------

/** The wide layout's media query matches (and reduced motion, if asked).
 * `resize(wide)` crosses the breakpoint, telling whoever listens. */
function wideWindow({ reduced = false } = {}) {
  let wide = true;
  const listeners = new Set<() => void>();
  window.matchMedia = ((q: string) => ({
    get matches() {
      return (wide && q === WIDE_QUERY) || (reduced && q.includes("prefers-reduced-motion"));
    },
    media: q,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: (_: string, cb: () => void) => listeners.add(cb),
    removeEventListener: (_: string, cb: () => void) => listeners.delete(cb),
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
  return {
    resize: (to: boolean) => {
      wide = to;
      for (const cb of [...listeners]) cb();
    },
  };
}

/**
 * jsdom does no layout and has no Web Animations. This gives the screen a
 * small geometry - the left column at x=0 and the right one at x=600, each
 * card 400x90 on a 100px row by its order in its column, the changelog
 * panel 90px tall plus the history's fold (`foldHeight`) while a fold (or
 * its closing copy) is in it, and the cards under it pushed down by that
 * fold - and records the animations played, each one finishing only when
 * the test says so.
 */
function stubMotion({ foldHeight = 0 } = {}) {
  type Fake = {
    el: Element;
    frames: Keyframe[];
    opts: KeyframeAnimationOptions;
    cancelled: boolean;
    finish: () => void;
  };
  const played: Fake[] = [];
  const proto = Element.prototype as unknown as Record<string, unknown>;
  const hadAnimate = Object.prototype.hasOwnProperty.call(proto, "animate");
  const realAnimate = proto.animate;
  proto.animate = function (this: Element, frames: Keyframe[], opts: KeyframeAnimationOptions = {}) {
    let resolve!: () => void;
    const finished = new Promise<void>((r) => (resolve = r));
    const a = {
      finished,
      onfinish: null as null | (() => void),
      oncancel: null as null | (() => void),
      cancel() {
        record.cancelled = true;
        resolve();
        // As a browser does: the cancel event arrives later, not in the call.
        queueMicrotask(() => a.oncancel?.());
      },
      effect: { getTiming: () => ({ duration: opts.duration, delay: opts.delay ?? 0 }) },
    };
    const record: Fake = {
      el: this,
      frames,
      opts,
      cancelled: false,
      finish: () => {
        resolve();
        a.onfinish?.();
      },
    };
    played.push(record);
    return a;
  };
  const realRect = Element.prototype.getBoundingClientRect;
  const rect = (left: number, top: number, width: number, height: number) =>
    ({ left, top, width, height, x: left, y: top, right: left + width, bottom: top + height, toJSON: () => ({}) }) as DOMRect;
  const panel = () => document.querySelector('[data-visual-mask="release-notes"]');
  const folded = () => (panel()?.querySelector(".t-collapse") ? foldHeight : 0);
  Element.prototype.getBoundingClientRect = function (this: Element) {
    if (this.hasAttribute("data-settings-card")) {
      const col = this.parentElement!;
      const inRight = col.contains(panel());
      return rect(inRight ? 600 : 0, [...col.children].indexOf(this) * 100 + (inRight ? folded() : 0), 400, 90);
    }
    if (this === panel()) return rect(600, 0, 400, 90 + folded());
    if (this === panel()?.parentElement) return rect(600, 0, 400, 500);
    if (this.classList.contains("t-collapse")) return rect(600, 90, 400, foldHeight);
    return realRect.call(this);
  };
  return {
    cards: () => played.filter((p) => p.el.hasAttribute("data-settings-card")),
    /** The fold's own grow or shrink (the box, not its fading content). */
    fold: () => played.filter((p) => p.el.classList.contains("t-collapse")),
    /** When a card's animation actually sets it moving: its delay, or for a
     * drawn path the time its first keyframe changes. */
    setsOff: (p: Fake) => {
      if (p.opts.delay !== undefined) return p.opts.delay;
      const xs = p.frames.map((f) => Number(/translate\((-?[\d.e-]+)px/.exec(String(f.transform))?.[1] ?? 0));
      const k = xs.findIndex((x) => Math.abs(x - xs[0]) > 0.5);
      return k <= 0 ? 0 : (p.frames[k - 1].offset as number) * Number(p.opts.duration);
    },
    finishAll: () => played.forEach((p) => p.finish()),
    restore: () => {
      if (hadAnimate) proto.animate = realAnimate;
      else delete proto.animate;
      Element.prototype.getBoundingClientRect = realRect;
    },
  };
}

const ALL_CARDS = ["Appearance", "General", "AI tools", "Updates", "Backup & transfer", "Help & support"];
const LOOKS = ["Appearance", "General", "AI tools"];
const MOVERS = ["Updates", "Backup & transfer", "Help & support"];

/** The two columns: the one the Appearance card is in, and the changelog's. */
function columns() {
  const left = screen.getByRole("heading", { name: "Appearance" }).closest("section")!.parentElement!;
  const right = document.querySelector('[data-visual-mask="release-notes"]')!.parentElement!;
  return { left, right };
}
const headings = (col: HTMLElement) =>
  within(col)
    .getAllByRole("heading", { level: 2 })
    .map((h) => h.textContent);
const history = () => document.getElementById("changelog-history");

/// The approved layout: while the changelog shows only its latest version,
/// Updates, Backup & transfer and Help & support sit under it; opening the
/// history moves them to the foot of the left column, and closing it brings
/// them back.
test("a wide window puts three cards under the changelog, and Show more moves them left", async () => {
  wideWindow();
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  await screen.findByRole("heading", { name: "Changelog" });

  expect(headings(columns().left)).toEqual(LOOKS);
  expect(headings(columns().right)).toEqual(["Changelog", ...MOVERS]);
  // The right column scrolls with the page now: sticky would ride over the
  // cards beneath the panel.
  expect(columns().right.className).not.toMatch(/sticky/);

  fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
  const less = await screen.findByRole("button", { name: "Show less" });
  expect(less).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("Version 1.9.0")).toBeInTheDocument();
  expect(headings(columns().left)).toEqual(ALL_CARDS);
  expect(headings(columns().right)).toEqual(["Changelog"]);
  // Every card keeps its tour anchor wherever it sits.
  expect(columns().left.querySelector('[data-tour="settings-updates"]')).not.toBeNull();
  expect(columns().left.querySelector('[data-tour="settings-backup"]')).not.toBeNull();

  fireEvent.click(less);
  const more = await screen.findByRole("button", { name: /^Show more/ });
  expect(more).toHaveAttribute("aria-expanded", "false");
  await waitFor(() => expect(headings(columns().right)).toEqual(["Changelog", ...MOVERS]));
  expect(headings(columns().left)).toEqual(LOOKS);
  expect(history()).toBeNull();
});

/// One motion, not two: Show more moves the cards and opens the history on
/// the click. The cards set off top-down, and the history's grow is held
/// only until Updates, right under the panel, is clear of the column - far
/// less than the whole slide the first version waited for.
test("Show more moves the cards and grows the history as one motion, Updates first", async () => {
  wideWindow();
  const motion = stubMotion({ foldHeight: 300 });
  try {
    mockIPC(() => undefined);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderSettings(qc);
    await screen.findByRole("heading", { name: "Changelog" });

    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    expect(screen.getByRole("button", { name: "Show less" })).toHaveAttribute("aria-expanded", "true");
    expect(history()).not.toBeNull();
    expect(headings(columns().left)).toEqual(ALL_CARDS);

    const slides = motion.cards();
    expect(slides.map((s) => s.el.getAttribute("data-settings-card"))).toEqual(["updates", "backup", "help"]);
    const starts = slides.map(motion.setsOff);
    expect(starts[0]).toBe(0);
    expect(starts[1]).toBeGreaterThan(starts[0]);
    expect(starts[2]).toBeGreaterThan(starts[1]);
    const grow = motion.fold()[0];
    const hold = Number(grow.opts.delay);
    expect(hold).toBeGreaterThan(0);
    expect(hold).toBeLessThan(TILE_MS);
    expect(grow.opts.fill).toBe("backwards");
    // Exactly the plan for this geometry: a 300px grow on EASE from the
    // panel's foot at 90px, the cards leaving rows 1-3 on the right for
    // rows 3-5 on the left.
    const ms = foldMs(300);
    const ease = parseEasing(EASE);
    const plan = planOpen(
      [100, 200, 300].map((top, i) => ({ from: { left: 600, top }, to: { left: 0, top: 100 * (i + 3) }, width: 400 })),
      600,
      (t) => (t < 0 ? 90 : t < ms ? 90 + 300 * ease(t / ms) : 390),
      ms,
      { ease, tileMs: TILE_MS, staggerMs: TILE_STAGGER_MS, safetyMs: TILE_SAFETY_MS },
    );
    expect(hold).toBeCloseTo(plan.fold, 6);
    starts.forEach((start, i) => expect(start).toBeCloseTo(plan.starts[i], 6));
  } finally {
    motion.restore();
  }
});

/// Show less runs the other way in one motion: the history folds on the
/// click, and the cards come back bottom-up - Help & support, whose spot the
/// shrinking fold frees first, sets off first.
test("Show less folds the history and brings the cards back bottom-up as their spots free", async () => {
  wideWindow();
  const motion = stubMotion({ foldHeight: 300 });
  try {
    mockIPC(() => undefined);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderSettings(qc);
    await screen.findByRole("heading", { name: "Changelog" });
    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    await act(async () => motion.finishAll());
    // The sequence also waits out the held grow on its own clock.
    await act(() => new Promise((r) => window.setTimeout(r, 800)));

    const before = motion.cards().length;
    fireEvent.click(screen.getByRole("button", { name: "Show less" }));
    expect(history()).toBeNull();
    expect(screen.getByRole("button", { name: /^Show more/ })).toHaveAttribute("aria-expanded", "false");
    expect(headings(columns().right)).toEqual(["Changelog", ...MOVERS]);

    const back = motion.cards().slice(before);
    expect(back.map((s) => s.el.getAttribute("data-settings-card"))).toEqual(["updates", "backup", "help"]);
    const [updates, backup, help] = back.map(motion.setsOff);
    expect(help).toBeLessThan(backup);
    expect(backup).toBeLessThan(updates);
    // Exactly the plan for this geometry: the copy shrinks 300px on EASE
    // from the panel's foot at 90px, and the cards return from the left
    // column (rows 3-5) to rows 1-3 on the right. Keyframes are 1/60s apart.
    const ms = foldMs(300);
    const ease = parseEasing(EASE);
    const plan = planClose(
      [300, 400, 500].map((top, i) => ({ from: { left: 0, top }, to: { left: 600, top: 100 * (i + 1) }, width: 400 })),
      600,
      (t) => 90 + 300 * (1 - ease(Math.min(1, t / ms))),
      ms,
      { ease, tileMs: TILE_MS, staggerMs: TILE_STAGGER_MS, safetyMs: TILE_SAFETY_MS },
    );
    [updates, backup, help].forEach((start, i) => expect(Math.abs(start - plan[i])).toBeLessThan(17));
    // Drawn on a path that starts where each card was, in the left column.
    for (const s of back) expect(String(s.frames[0].transform)).toMatch(/^translate\(-600px/);
  } finally {
    motion.restore();
  }
});

/// A second click while the motion runs is ignored, so the cards and the
/// history cannot end up out of step.
test("a rapid double click leaves the cards and the history in step", async () => {
  wideWindow();
  const motion = stubMotion();
  try {
    mockIPC(() => undefined);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderSettings(qc);
    await screen.findByRole("heading", { name: "Changelog" });

    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    // The label flips on the first click; the second lands on Show less.
    fireEvent.click(screen.getByRole("button", { name: "Show less" }));
    await act(async () => {});
    expect(history()).not.toBeNull();
    expect(headings(columns().left)).toEqual(ALL_CARDS);
    await act(async () => motion.finishAll());

    fireEvent.click(screen.getByRole("button", { name: "Show less" }));
    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    await act(async () => {});
    expect(screen.getByRole("button", { name: /^Show more/ })).toHaveAttribute("aria-expanded", "false");
    expect(history()).toBeNull();
    expect(headings(columns().right)).toEqual(["Changelog", ...MOVERS]);
    await act(async () => motion.finishAll());

    // Once it has finished, the next click is taken.
    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    expect(screen.getByRole("button", { name: "Show less" })).toBeInTheDocument();
  } finally {
    motion.restore();
  }
});

/// In dev the app runs under StrictMode, which runs a new fold's grow effect
/// twice - the second time after this screen has spent its plan. The grow
/// must keep the hold it was given, or the history grows across Updates.
test("under StrictMode Show more still holds the grow for Updates", async () => {
  wideWindow();
  const motion = stubMotion({ foldHeight: 300 });
  try {
    mockIPC(() => undefined);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <StrictMode>
        <QueryClientProvider client={qc}>
          <Settings org="acme" project="Web" />
        </QueryClientProvider>
      </StrictMode>,
    );
    await screen.findByRole("heading", { name: "Changelog" });

    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    const grows = motion.fold().filter((f) => String(f.frames[1]?.height ?? "") !== "0px");
    const last = grows[grows.length - 1];
    expect(Number(last.opts.delay)).toBeGreaterThan(0);
    expect(last.opts.fill).toBe("backwards");
  } finally {
    motion.restore();
  }
});

/// Switching to Logs while the cards come back: the fold (and the copy the
/// returning cards were drawn against) goes with the changelog, so the
/// cards settle under the log panel at once instead of finishing a path
/// planned for a layout that is gone.
test("switching to Logs during Show less settles the returning cards at once", async () => {
  wideWindow();
  const motion = stubMotion({ foldHeight: 300 });
  try {
    mockIPC((cmd) => {
      if (cmd === "app_logs") return [];
      if (cmd === "app_log_dir") return "C:\\logs";
      return undefined;
    });
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderSettings(qc);
    await screen.findByRole("heading", { name: "Changelog" });
    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    await act(async () => motion.finishAll());
    await act(() => new Promise((r) => window.setTimeout(r, 800)));

    const before = motion.cards().length;
    fireEvent.click(screen.getByRole("button", { name: "Show less" }));
    const returning = motion.cards().slice(before);
    expect(returning).toHaveLength(3);
    expect(returning.some((r) => r.cancelled)).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Logs" }));
    expect(returning.every((r) => r.cancelled)).toBe(true);
    expect(headings(columns().right)).toEqual(["App log", ...MOVERS]);
  } finally {
    motion.restore();
  }
});

/// Crossing the breakpoint mid-slide: the two-column layout the slide was
/// planned against is gone, so the cards settle in the one column at once.
test("crossing the wide breakpoint mid-slide settles the cards at once", async () => {
  const win = wideWindow();
  const motion = stubMotion();
  try {
    mockIPC(() => undefined);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderSettings(qc);
    await screen.findByRole("heading", { name: "Changelog" });
    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    const slides = motion.cards();
    expect(slides).toHaveLength(3);

    act(() => win.resize(false));
    expect(slides.every((s) => s.cancelled)).toBe(true);
    expect(headings(columns().left)).toEqual(ALL_CARDS);
    expect(headings(columns().right)).toEqual(["Changelog"]);
  } finally {
    motion.restore();
  }
});

/// Reduced motion: no slide and no waiting - the layout and the history
/// change together, on the click.
test("under reduced motion Show more moves the cards and opens the history at once", async () => {
  wideWindow({ reduced: true });
  const motion = stubMotion();
  try {
    mockIPC(() => undefined);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderSettings(qc);
    await screen.findByRole("heading", { name: "Changelog" });

    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    expect(screen.getByRole("button", { name: "Show less" })).toHaveAttribute("aria-expanded", "true");
    expect(history()).not.toBeNull();
    expect(headings(columns().left)).toEqual(ALL_CARDS);

    fireEvent.click(screen.getByRole("button", { name: "Show less" }));
    expect(history()).toBeNull();
    expect(headings(columns().right)).toEqual(["Changelog", ...MOVERS]);
    expect(motion.cards()).toHaveLength(0);
  } finally {
    motion.restore();
  }
});

/// The rule is only "right column unless the changelog is expanded": the
/// app log keeps the cards under it.
test("on a wide window the cards stay under the app log", async () => {
  wideWindow();
  mockIPC((cmd) => {
    if (cmd === "app_logs") return [];
    if (cmd === "app_log_dir") return "C:\\logs";
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  await screen.findByRole("heading", { name: "Changelog" });

  fireEvent.click(screen.getByRole("button", { name: "Logs" }));
  await screen.findByRole("button", { name: "Copy log" });
  expect(headings(columns().right)).toEqual(["App log", ...MOVERS]);
  expect(headings(columns().left)).toEqual(LOOKS);
});

/// Extras, when shown, is always last on the left - the moving cards land
/// above it.
test("the Extras card stays last on the left as the cards come and go", async () => {
  wideWindow();
  mockIPC((cmd) => {
    if (cmd === "set_extras_unlocked") return null;
    if (cmd === "get_extras_unlocked") return true;
  });
  await act(() => setExtrasUnlocked(true));
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  await screen.findByRole("heading", { name: "Extras" });
  expect(headings(columns().left)).toEqual([...LOOKS, "Extras"]);

  fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
  await screen.findByRole("button", { name: "Show less" });
  expect(headings(columns().left)).toEqual([...ALL_CARDS, "Extras"]);
});

/// The request rate is a compact three-way choice: only the chosen level's
/// explanation shows, and picking another level swaps it.
test("the request rate shows only the chosen level's explanation, and switching changes it", async () => {
  localStorage.removeItem("tcm-v2-ado-rate");
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "set_ado_rate_level") calls.push(String((args as { level: string }).level));
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  const [full, balanced, gentle] = RATE_LEVELS;

  expect(screen.getByRole("button", { name: /^Full speed/ })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByText(full.hint)).toBeInTheDocument();
  expect(screen.queryByText(balanced.hint)).not.toBeInTheDocument();
  expect(screen.queryByText(gentle.hint)).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Gentle" }));
  expect(screen.getByRole("button", { name: "Gentle" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: /^Full speed/ })).toHaveAttribute("aria-pressed", "false");
  expect(screen.getByText(gentle.hint)).toBeInTheDocument();
  expect(screen.queryByText(full.hint)).not.toBeInTheDocument();
  expect(screen.queryByText(balanced.hint)).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-ado-rate")).toBe("gentle");
  await waitFor(() => expect(calls).toContain("gentle"));

  fireEvent.click(screen.getByRole("button", { name: "Balanced" }));
  expect(screen.getByText(balanced.hint)).toBeInTheDocument();
  expect(screen.queryByText(gentle.hint)).not.toBeInTheDocument();
  localStorage.removeItem("tcm-v2-ado-rate");
});

/// Machine-wide registration is an explicit opt-in, and this switch is the
/// only place it is granted - the AI Bridge tab reads the same key.
test("the machine-wide AI registration switch persists its choice", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  const sw = await screen.findByLabelText("Allow registering AI tools machine-wide");
  expect(localStorage.getItem("tcm-v2-ai-global-allowed")).toBeNull();
  fireEvent.click(sw);
  expect(localStorage.getItem("tcm-v2-ai-global-allowed")).toBe("on");
  fireEvent.click(sw);
  expect(localStorage.getItem("tcm-v2-ai-global-allowed")).toBeNull();
  localStorage.clear();
});

test("Settings carries no AI Bridge content (it lives in its own tab)", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  await screen.findByRole("heading", { name: "Changelog" });
  // The changelog history may mention "AI Bridge" in release notes - assert
  // the section itself and the old moved-note are gone, not the words.
  expect(screen.queryByText("AI Bridge has moved to its own tab.")).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "AI Bridge" })).not.toBeInTheDocument();
  expect(screen.queryByText("Registered in Claude Code:")).not.toBeInTheDocument();
});

test("the right column switches from the changelog to the app log", async () => {
  mockIPC((cmd) => {
    if (cmd === "app_logs")
      return [
        { at: "2026-07-26 09:00:01", level: "info", message: "Test Case Manager started" },
        { at: "2026-07-26 09:02:20", level: "error", message: "Submit failed for 'X'" },
      ];
    if (cmd === "app_log_dir") return "C:\logs";
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);

  // Changelog is the default panel.
  expect(await screen.findByRole("button", { name: /Show more/ })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Logs" }));
  expect(await screen.findByText("Test Case Manager started")).toBeInTheDocument();
  expect(screen.getByText("Submit failed for 'X'")).toBeInTheDocument();
  // The changelog panel is gone, not merely hidden below.
  expect(screen.queryByRole("button", { name: /Show more/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Copy log" })).toBeInTheDocument();
});

/// Switching between Changelog and Logs plays an entrance on whichever
/// panel is now showing - keyed on the panel so each switch replays it,
/// rather than reusing the same DOM node across a swap.
test("switching panels plays an entrance animation, replayed on each switch", async () => {
  mockIPC((cmd) => {
    if (cmd === "app_logs") return [];
    if (cmd === "app_log_dir") return "C:\\logs";
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);

  await screen.findByRole("button", { name: /Show more/ });
  const changelogPanel = document.querySelector(".t-panel-in")!;
  expect(changelogPanel).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Logs" }));
  await screen.findByRole("button", { name: "Copy log" });
  const logsPanel = document.querySelector(".t-panel-in")!;
  expect(logsPanel).toBeInTheDocument();
  expect(logsPanel).not.toBe(changelogPanel);

  fireEvent.click(screen.getByRole("button", { name: "Changelog" }));
  await screen.findByRole("button", { name: /Show more/ });
  const changelogAgain = document.querySelector(".t-panel-in")!;
  // A fresh node each switch - React remounts on the key change rather than
  // reusing the earlier changelog panel, so the animation replays.
  expect(changelogAgain).not.toBe(changelogPanel);
});

/// The backup file used to carry every hidden dev tool by name; that has no
/// place in copy a user reads.
test("the backup description does not name Auto Run", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  const heading = await screen.findByRole("heading", { name: "Backup & transfer" });
  expect(heading.closest("section")!.textContent).not.toMatch(/Auto Run/);
});

/// Database logins live in Windows Credential Manager, which the backup
/// never reads - the description has to say so, or a person moving
/// machines expects them to arrive.
test("the backup description says database logins are not included", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  const heading = await screen.findByRole("heading", { name: "Backup & transfer" });
  const text = heading.closest("section")!.textContent ?? "";
  expect(text).toMatch(/database logins stay on this computer/i);
  expect(text).not.toMatch(/—/);
});

/// The folder opens from Rust. The frontend used to call the opener plugin
/// itself, and the webview's `opener:default` permission does not include
/// open_path - so the plugin refused and the button only ever showed the
/// error toast.
test("Open log folder asks Rust to open it", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(String(cmd));
    if (cmd === "app_logs") return [];
    if (cmd === "app_log_dir") return "C:\\logs";
    if (cmd === "open_app_log_dir") return { status: "ok", data: null };
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: "Logs" }));
  const open = await screen.findByRole("button", { name: "Open log folder" });
  await waitFor(() => expect(open).toBeEnabled());
  fireEvent.click(open);
  await waitFor(() => expect(calls).toContain("open_app_log_dir"));
  expect(calls.some((c) => c.startsWith("plugin:opener|"))).toBe(false);
});

/// The detailed DB (and later API) statement trail lives apart from the
/// app log - its own folder, opened the same way: only Rust can
/// `open_path`, so this button calls straight through to Rust too.
test("Open activity folder asks Rust to open it", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(String(cmd));
    if (cmd === "app_logs") return [];
    if (cmd === "app_log_dir") return "C:\\logs";
    if (cmd === "open_activity_log_dir") return { status: "ok", data: null };
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: "Logs" }));
  const open = await screen.findByRole("button", { name: "Open activity folder" });
  fireEvent.click(open);
  await waitFor(() => expect(calls).toContain("open_activity_log_dir"));
  expect(calls.some((c) => c.startsWith("plugin:opener|"))).toBe(false);
});

/// The "How To Use" button is one click from Settings, same shape as the
/// other buttons that open something from Rust.
test("How To Use opens the help site", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(String(cmd));
    if (cmd === "open_help") return { status: "ok", data: null };
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: "How To Use" }));
  await waitFor(() => expect(calls).toContain("open_help"));
});

/// Opening finds the downloaded guide on disk (adopting an older install's
/// first) and hands it to the browser - the button must disable itself and
/// say "Opening" while that call is in flight, so a second click before it
/// settles cannot open a second tab, then return to normal once the command
/// resolves.
test("How To Use disables itself and shows Opening while the command is in flight", async () => {
  let resolveOpen: (v: { status: "ok"; data: null }) => void;
  const opened = new Promise<{ status: "ok"; data: null }>((resolve) => {
    resolveOpen = resolve;
  });
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(String(cmd));
    if (cmd === "open_help") return opened;
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);

  const button = screen.getByRole("button", { name: "How To Use" });
  fireEvent.click(button);

  await waitFor(() => expect(calls).toContain("open_help"));
  await waitFor(() => expect(screen.getByRole("button", { name: "Opening" })).toBeDisabled());

  resolveOpen!({ status: "ok", data: null });
  await waitFor(() => expect(screen.getByRole("button", { name: "How To Use" })).toBeEnabled());
});

/// A failure from Rust (the site could not be written or opened) shows as
/// a toast rather than doing nothing.
test("How To Use shows a toast when it fails", async () => {
  mockIPC((cmd) => {
    if (cmd === "open_help") throw "Could not open the help pages. Settings, Logs has the details.";
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: "How To Use" }));
  await vi.waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith("Could not open the help pages. Settings, Logs has the details."),
  );
});

// Default tags are no longer set here - they moved to Manual Entry, where
// they are used, and their tests went with them (ManualEntry.test.tsx).
test("default tags are not offered in Settings any more", () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    return undefined;
  });
  renderSettings(qc);
  expect(screen.queryByLabelText("Default tags")).not.toBeInTheDocument();
});

test("export sends only the app's own localStorage keys to the backend", async () => {
  localStorage.setItem("tcm-v2-theme", "dark");
  localStorage.setItem("someone-elses-key", "x");
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let sent: Record<string, string> | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "plugin:dialog|save") return "C:/tmp/tcm-backup.json";
    if (cmd === "export_app_backup") {
      sent = (args as { localStorage: Record<string, string> }).localStorage;
      return { path: "C:/tmp/tcm-backup.json", keys: 1, files: 3, skipped: [] };
    }
    return undefined;
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: "Export to file" }));
  await waitFor(() => expect(sent).not.toBeNull());
  expect(sent).toEqual({ "tcm-v2-theme": "dark" });
  localStorage.clear();
});

test("import asks for confirmation, applies the backup's keys, and reloads", async () => {
  localStorage.setItem("tcm-v2-stale", "gone-after-import");
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  mockIPC((cmd) => {
    if (cmd === "plugin:dialog|open") return "C:/tmp/tcm-backup.json";
    if (cmd === "import_app_backup")
      return {
        local_storage: { "tcm-v2-theme": "dark" },
        files_restored: 2,
        exported_at: "2026-08-19 10:00:00",
        app_version: "1.20.4",
      };
    return undefined;
  });
  // jsdom's location.reload is not writable directly - swap the object.
  const original = window.location;
  const reload = vi.fn();
  Object.defineProperty(window, "location", {
    configurable: true,
    value: { ...original, reload },
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: "Import from file" }));
  // Nothing is touched until the modal's confirm - the dialog pick alone
  // must not import.
  expect(await screen.findByRole("dialog")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-theme")).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Import and reload" }));
  await waitFor(() => expect(reload).toHaveBeenCalled());
  expect(localStorage.getItem("tcm-v2-theme")).toBe("dark");
  // Replace semantics: a key the backup doesn't carry is removed.
  expect(localStorage.getItem("tcm-v2-stale")).toBeNull();

  Object.defineProperty(window, "location", { configurable: true, value: original });
  localStorage.clear();
});

test("cancelling the import confirmation touches nothing", async () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let imported = false;
  mockIPC((cmd) => {
    if (cmd === "plugin:dialog|open") return "C:/tmp/tcm-backup.json";
    if (cmd === "import_app_backup") {
      imported = true;
      return { local_storage: {}, files_restored: 0, exported_at: "", app_version: "" };
    }
    return undefined;
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: "Import from file" }));
  await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(imported).toBe(false);
});

/// The AI tools section is one switch now: the option to offer a separate
/// database server went with that server.
test("the AI tools section offers only the machine-wide switch", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  const sw = await screen.findByLabelText("Allow registering AI tools machine-wide");
  const section = sw.closest("section")!;
  expect(within(section).getAllByRole("switch")).toHaveLength(1);
  expect(section.textContent).not.toMatch(/database server/i);
});

/// The tour's last three stops are rung on this screen, so the sections it
/// names have to keep their `data-tour` attributes. `tourAnchors.test.ts`
/// only proves the names exist SOMEWHERE in src/; this proves they are on
/// the right sections here, with something in them to ring.
test("the tour's three Settings anchors sit on the sections it names", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { container } = renderSettings(qc);
  await screen.findByRole("button", { name: "Changelog", pressed: true });

  const anchor = (name: string) => {
    const el = container.querySelector(`[data-tour="${name}"]`);
    expect(el, `no [data-tour="${name}"] on the Settings screen`).not.toBeNull();
    return el as HTMLElement;
  };

  // Appearance: the theme swatches AND the accent row, which is what the
  // stop's words promise.
  const theme = anchor("theme");
  expect(theme.textContent).toContain("Appearance");
  expect(within(theme).getByRole("button", { name: "Theme System" })).toBeInTheDocument();
  expect(within(theme).getAllByRole("button", { name: /^Accent / }).length).toBeGreaterThan(1);

  expect(anchor("settings-backup").textContent).toContain("Backup & transfer");
  const updates = anchor("settings-updates");
  expect(updates.textContent).toContain("Updates");
  expect(within(updates).getByRole("button", { name: "Check for updates" })).toBeInTheDocument();
});

/// Field request: colour the log the way VS Code's Log mode does.
test("the app log colours the level tag and the values in each line", async () => {
  mockIPC((cmd) => {
    if (cmd === "app_logs")
      return [
        { at: "2026-09-11 04:11:18", level: "debug", message: "GET dev.azure.com/acme/_apis/testplan/Plans/107281/suites -> 200 in 184 ms" },
        { at: "2026-09-11 04:11:21", level: "error", message: "Submit failed for 'Login works': Azure DevOps returned HTTP 400" },
      ];
    if (cmd === "app_log_dir") return "C:\logs";
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  fireEvent.click(await screen.findByRole("button", { name: "Logs" }));

  expect(await screen.findByText("[error]")).toHaveClass("text-danger");
  expect(screen.getByText("[debug]")).toHaveClass("text-warning/70");
  expect(screen.getByText("dev.azure.com")).toHaveClass("text-accent");
  expect(screen.getByText("107281")).toHaveClass("text-accent");
  expect(screen.getByText("400")).toHaveClass("text-accent");
  // Nothing is lost between the tokens, and the prose stays the text colour.
  const line = screen.getByText("dev.azure.com").parentElement!;
  expect(line.textContent).toBe("GET dev.azure.com/acme/_apis/testplan/Plans/107281/suites -> 200 in 184 ms");
  expect(line.firstElementChild).toHaveTextContent("GET");
  expect(line.firstElementChild).toHaveClass("text-text");
});

// Fix round 1 (Task 3): the Extras section rendered on this machine's own
// unlocked state alone, so a capture taken on an unlocked machine (the
// owner's) would show it. Off, this unlocked machine's ordinary behaviour
// (the section shown) is unchanged.
test("capture mode hides the Extras section on an unlocked machine; off, it shows", async () => {
  mockIPC((cmd) => {
    if (cmd === "set_extras_unlocked") return null;
    // Settings hydrates this machine's switch on mount - answer "unlocked"
    // so that hydration cannot race the direct setExtrasUnlocked() below
    // back to locked.
    if (cmd === "get_extras_unlocked") return true;
  });
  await act(() => setExtrasUnlocked(true));

  localStorage.setItem("tcm-v2-dev-capture", "on");
  const qcOn = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onRender = renderSettings(qcOn);
  await onRender.findByRole("heading", { name: "Changelog" });
  expect(onRender.queryByRole("heading", { name: "Extras" })).not.toBeInTheDocument();
  expect(onRender.queryByRole("button", { name: "Play the dino game" })).not.toBeInTheDocument();
  expect(onRender.queryByRole("button", { name: "Reset to default" })).not.toBeInTheDocument();
  onRender.unmount();

  localStorage.removeItem("tcm-v2-dev-capture");
  const qcOff = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const offRender = renderSettings(qcOff);
  expect(await offRender.findByRole("heading", { name: "Extras" })).toBeInTheDocument();
  expect(offRender.getByRole("button", { name: "Play the dino game" })).toBeInTheDocument();
  expect(offRender.getByRole("button", { name: "Reset to default" })).toBeInTheDocument();
  offRender.unmount();
});

test("Download beta builds is off by default and turning it on checks for updates", async () => {
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:app|version") return "1.26.0";
    if (cmd === "get_app_settings") return { close_to_tray: true, close_notice_shown: true, beta_updates: false };
    if (cmd === "set_beta_updates") {
      calls.push(`beta:${(args as { on: boolean }).on}`);
      return { close_to_tray: true, close_notice_shown: true, beta_updates: (args as { on: boolean }).on };
    }
    if (cmd === "check_update") {
      calls.push("check");
      return { available: null, blocked: null, failed_attempt: null };
    }
    return undefined;
  });
  renderSettings(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  const beta = await screen.findByRole("switch", { name: "Download beta builds" });
  await waitFor(() => expect(beta).toHaveAttribute("aria-checked", "false"));
  expect(screen.queryByText(/You're on a beta build/)).not.toBeInTheDocument();
  fireEvent.click(beta);
  await waitFor(() => expect(calls).toEqual(["beta:true", "check"]));
});

test("a beta build says so, and says it stays when betas are off", async () => {
  mockIPC((cmd) => {
    if (cmd === "plugin:app|version") return "1.26.0-beta.2";
    if (cmd === "get_app_settings") return { close_to_tray: true, close_notice_shown: true, beta_updates: false };
    return undefined;
  });
  renderSettings(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  expect(await screen.findByText(/Version 1\.26\.0-beta\.2 \(beta\)/)).toBeInTheDocument();
  expect(screen.getByText("You're on a beta build. It stays until the next stable release.")).toBeInTheDocument();
});

/// How To Use is downloaded on demand. Settings asks Rust what is on disk
/// when it opens and shows whichever of Download / How To Use / Update
/// Guide fits. Sizes are whole MB (1 MB = 1024 * 1024 bytes), the total
/// rounded up.
const MB = 1024 * 1024;
const GUIDE_SIZE = 31 * MB;

function guideQc() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

test("a_guide_not_yet_downloaded_offers_to_download_it_with_its_size", async () => {
  mockIPC((cmd) => {
    if (cmd === "guide_status") return { state: "NotDownloaded", size: GUIDE_SIZE };
    return undefined;
  });
  renderSettings(guideQc());

  expect(await screen.findByRole("button", { name: "Download How to Use (31 MB)" })).toBeEnabled();
  expect(screen.queryByRole("button", { name: "How To Use" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Update Guide" })).toBeNull();
});

test("the download size is rounded up to a whole MB, and left out when unknown", async () => {
  mockIPC((cmd) => {
    if (cmd === "guide_status") return { state: "NotDownloaded", size: 30 * MB + 1 };
    return undefined;
  });
  const first = renderSettings(guideQc());
  expect(await screen.findByRole("button", { name: "Download How to Use (31 MB)" })).toBeInTheDocument();
  first.unmount();
  clearMocks();

  mockIPC((cmd) => {
    if (cmd === "guide_status") return { state: "NotDownloaded", size: null };
    return undefined;
  });
  renderSettings(guideQc());
  expect(await screen.findByRole("button", { name: "Download How to Use" })).toBeInTheDocument();
});

test("downloading_shows_progress_then_the_guide_is_ready", async () => {
  let resolveDownload!: (v: { status: "ok"; data: null }) => void;
  const download = new Promise<{ status: "ok"; data: null }>((r) => {
    resolveDownload = r;
  });
  let state: "NotDownloaded" | "Ready" = "NotDownloaded";
  const calls: string[] = [];
  mockIPC(
    (cmd) => {
      calls.push(String(cmd));
      if (cmd === "guide_status") return { state, size: GUIDE_SIZE };
      if (cmd === "guide_download") return download;
      return undefined;
    },
    { shouldMockEvents: true },
  );
  renderSettings(guideQc());

  // The live region is there before anything happens, empty, so a screen
  // reader is already listening when the first figure arrives.
  const live = await screen.findByRole("status");
  expect(live).toHaveTextContent(/^$/);
  expect(screen.queryByRole("progressbar")).toBeNull();

  fireEvent.click(await screen.findByRole("button", { name: "Download How to Use (31 MB)" }));
  const busy = await screen.findByRole("button", { name: "Downloading..." });
  expect(busy).toBeDisabled();
  // A bar from the start; no figure on it until the first bytes arrive.
  const bar = await screen.findByRole("progressbar", { name: "Downloading How to Use" });
  expect(bar).not.toHaveAttribute("aria-valuenow");

  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("guide-progress", { received: 12 * MB, total: GUIDE_SIZE });
  });
  expect(await screen.findByText("12 of 31 MB")).toBeInTheDocument();
  expect(screen.getByRole("status")).toBe(live);
  expect(bar).toHaveAttribute("aria-valuenow", "38");
  expect(bar).toHaveAttribute("aria-valuetext", "12 of 31 MB");
  // Whole MB: a part-way byte count rounds down, the total up.
  await act(async () => {
    await emit("guide-progress", { received: 13 * MB - 1, total: 30 * MB + 1 });
  });
  expect(await screen.findByText("12 of 31 MB")).toBeInTheDocument();

  state = "Ready";
  resolveDownload({ status: "ok", data: null });
  expect(await screen.findByRole("button", { name: "How To Use" })).toBeEnabled();
  expect(screen.queryByText(/of 31 MB/)).toBeNull();
  expect(screen.queryByRole("progressbar")).toBeNull();
  expect(screen.getByRole("status")).toHaveTextContent(/^$/);
  expect(calls.filter((c) => c === "guide_status")).toHaveLength(2);
  expect(calls.filter((c) => c === "guide_download")).toHaveLength(1);
});

test("two quick clicks on Download start one download", async () => {
  let resolveDownload!: (v: { status: "ok"; data: null }) => void;
  const download = new Promise<{ status: "ok"; data: null }>((r) => {
    resolveDownload = r;
  });
  const calls: string[] = [];
  mockIPC(
    (cmd) => {
      calls.push(String(cmd));
      if (cmd === "guide_status") return { state: "NotDownloaded", size: GUIDE_SIZE };
      if (cmd === "guide_download") return download;
      return undefined;
    },
    { shouldMockEvents: true },
  );
  renderSettings(guideQc());

  const button = await screen.findByRole("button", { name: "Download How to Use (31 MB)" });
  fireEvent.click(button);
  fireEvent.click(button);
  await screen.findByRole("button", { name: "Downloading..." });
  fireEvent.click(screen.getByRole("button", { name: "Downloading..." }));
  await act(async () => {
    resolveDownload({ status: "ok", data: null });
  });
  expect(calls.filter((c) => c === "guide_download")).toHaveLength(1);
});

test("a_changed_guide_offers_update_guide", async () => {
  let resolveDownload!: (v: { status: "ok"; data: null }) => void;
  const download = new Promise<{ status: "ok"; data: null }>((r) => {
    resolveDownload = r;
  });
  let state: "UpdateAvailable" | "Ready" = "UpdateAvailable";
  mockIPC(
    (cmd) => {
      if (cmd === "guide_status") return { state, size: GUIDE_SIZE };
      if (cmd === "guide_download") return download;
      return undefined;
    },
    { shouldMockEvents: true },
  );
  renderSettings(guideQc());

  expect(await screen.findByRole("button", { name: "How To Use" })).toBeEnabled();
  fireEvent.click(await screen.findByRole("button", { name: "Update Guide" }));

  // Both buttons wait while the update runs.
  expect(await screen.findByRole("button", { name: "Downloading..." })).toBeDisabled();
  expect(screen.getByRole("button", { name: "How To Use" })).toBeDisabled();

  state = "Ready";
  resolveDownload({ status: "ok", data: null });
  await waitFor(() => expect(screen.queryByRole("button", { name: "Update Guide" })).toBeNull());
  expect(screen.getByRole("button", { name: "How To Use" })).toBeEnabled();
});

test("a_failed_download_says_why", async () => {
  const sentence =
    "Could not download How to Use. Check your connection and try again - Settings, Logs has the details.";
  let statusCalls = 0;
  mockIPC(
    (cmd) => {
      if (cmd === "guide_status") {
        statusCalls += 1;
        return { state: "NotDownloaded", size: GUIDE_SIZE };
      }
      if (cmd === "guide_download") throw sentence;
      return undefined;
    },
    { shouldMockEvents: true },
  );
  renderSettings(guideQc());

  fireEvent.click(await screen.findByRole("button", { name: "Download How to Use (31 MB)" }));
  await waitFor(() => expect(toast.error).toHaveBeenCalledWith(sentence));
  // Still not on disk: the button comes back, and the state was asked again.
  expect(await screen.findByRole("button", { name: "Download How to Use (31 MB)" })).toBeEnabled();
  expect(statusCalls).toBe(2);
});

/// Opening can fail after the guide installed fine (no browser, say), so the
/// status is re-asked after a failure as well as after a success.
test("a failed download re-asks the status and shows what is now on disk", async () => {
  let state: "UpdateAvailable" | "Ready" = "UpdateAvailable";
  mockIPC(
    (cmd) => {
      if (cmd === "guide_status") return { state, size: GUIDE_SIZE };
      if (cmd === "guide_download") {
        state = "Ready";
        throw "Could not open the help pages. Settings, Logs has the details.";
      }
      return undefined;
    },
    { shouldMockEvents: true },
  );
  renderSettings(guideQc());

  fireEvent.click(await screen.findByRole("button", { name: "Update Guide" }));
  await waitFor(() => expect(toast.error).toHaveBeenCalled());
  await waitFor(() => expect(screen.queryByRole("button", { name: "Update Guide" })).toBeNull());
  expect(screen.getByRole("button", { name: "How To Use" })).toBeEnabled();
});

/// Offline from the start, or a build that does not answer: How To Use as
/// ever, no toast, no Update Guide.
test("offline_or_unanswered_status_shows_how_to_use", async () => {
  mockIPC((cmd) => {
    if (cmd === "guide_status") throw new Error("no answer");
    return undefined;
  });
  renderSettings(guideQc());

  expect(await screen.findByRole("button", { name: "How To Use" })).toBeEnabled();
  await act(async () => {
    await Promise.resolve();
  });
  expect(screen.queryByRole("button", { name: "Update Guide" })).toBeNull();
  expect(screen.queryByRole("button", { name: /Download How to Use/ })).toBeNull();
  expect(toast.error).not.toHaveBeenCalled();
});

test("How To Use shows the sentence when nothing is installed to open", async () => {
  mockIPC((cmd) => {
    if (cmd === "open_help") throw "Download How to Use from Settings first.";
    return undefined;
  });
  renderSettings(guideQc());

  fireEvent.click(screen.getByRole("button", { name: "How To Use" }));
  await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Download How to Use from Settings first."));
});

/// A click on How To Use can land before Settings has heard what is on disk
/// (it shows How To Use until then). When opening fails, the status is asked
/// again as well as the toast, so the Download button then appears.
test("a failed How To Use re-asks the status and offers Download", async () => {
  let state: "Ready" | "NotDownloaded" = "Ready";
  let statusCalls = 0;
  mockIPC((cmd) => {
    if (cmd === "guide_status") {
      statusCalls += 1;
      return { state, size: GUIDE_SIZE };
    }
    if (cmd === "open_help") {
      state = "NotDownloaded";
      throw "Download How to Use from Settings first.";
    }
    return undefined;
  });
  renderSettings(guideQc());
  await waitFor(() => expect(statusCalls).toBe(1));

  fireEvent.click(screen.getByRole("button", { name: "How To Use" }));
  await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Download How to Use from Settings first."));
  expect(await screen.findByRole("button", { name: "Download How to Use (31 MB)" })).toBeEnabled();
  expect(statusCalls).toBe(2);
});
