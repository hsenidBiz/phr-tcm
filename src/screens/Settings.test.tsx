import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import Settings from "./Settings";
import { CHANGELOG } from "../lib/changelog";
import { RATE_LEVELS } from "../lib/adoRate";
import { toast } from "../lib/toast";
import { resetExtrasStore, setExtrasUnlocked } from "../lib/extras";
import { WIDE_QUERY } from "../components/settings/useTileLayout";

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

/** The wide layout's media query matches (and reduced motion, if asked). */
function wideWindow({ reduced = false } = {}) {
  window.matchMedia = ((q: string) => ({
    matches: q === WIDE_QUERY || (reduced && q.includes("prefers-reduced-motion")),
    media: q,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}

/**
 * jsdom does no layout and has no Web Animations. This gives every card a
 * place (a column's x, and a row's y by its order in the column) and the
 * history's fold a height, and records the animations played, each one
 * finishing only when the test says so.
 */
function stubMotion({ foldHeight = 0 } = {}) {
  type Fake = { el: Element; opts: KeyframeAnimationOptions; finish: () => void };
  const played: Fake[] = [];
  const proto = Element.prototype as unknown as Record<string, unknown>;
  const hadAnimate = Object.prototype.hasOwnProperty.call(proto, "animate");
  const realAnimate = proto.animate;
  proto.animate = function (this: Element, _k: Keyframe[], opts: KeyframeAnimationOptions = {}) {
    let resolve!: () => void;
    const finished = new Promise<void>((r) => (resolve = r));
    const a = {
      finished,
      onfinish: null as null | (() => void),
      oncancel: null as null | (() => void),
      cancel() {
        resolve();
        a.oncancel?.();
      },
      effect: { getTiming: () => ({ duration: opts.duration, delay: opts.delay ?? 0 }) },
    };
    played.push({
      el: this,
      opts,
      finish: () => {
        resolve();
        a.onfinish?.();
      },
    });
    return a;
  };
  const realRect = Element.prototype.getBoundingClientRect;
  const rect = (left: number, top: number, width: number, height: number) =>
    ({ left, top, width, height, x: left, y: top, right: left + width, bottom: top + height, toJSON: () => ({}) }) as DOMRect;
  Element.prototype.getBoundingClientRect = function (this: Element) {
    if (this.hasAttribute("data-settings-card")) {
      const col = this.parentElement!;
      const inRight = col.querySelector('[data-visual-mask="release-notes"]') != null;
      return rect(inRight ? 600 : 0, [...col.children].indexOf(this) * 100, 400, 90);
    }
    if (this.classList.contains("t-collapse")) return rect(0, 0, 400, foldHeight);
    return realRect.call(this);
  };
  return {
    cards: () => played.filter((p) => p.el.hasAttribute("data-settings-card")),
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

/// Phase order: the cards slide first - one after another - and the history
/// unfolds only once they have landed.
test("Show more slides the cards across in turn, then unfolds the history", async () => {
  wideWindow();
  const motion = stubMotion();
  try {
    mockIPC(() => undefined);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderSettings(qc);
    await screen.findByRole("heading", { name: "Changelog" });

    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    // Moved at once, played back from the right column...
    expect(headings(columns().left)).toEqual(ALL_CARDS);
    const slides = motion.cards();
    expect(slides.map((s) => s.el.getAttribute("data-settings-card"))).toEqual(["updates", "backup", "help"]);
    expect(slides.map((s) => s.opts.delay)).toEqual([0, 70, 140]);
    // ...and the history waits for them.
    await act(async () => {});
    expect(history()).toBeNull();
    expect(screen.getByRole("button", { name: /^Show more/ })).toHaveAttribute("aria-expanded", "false");

    await act(async () => motion.finishAll());
    expect(await screen.findByRole("button", { name: "Show less" })).toHaveAttribute("aria-expanded", "true");
    expect(history()).not.toBeNull();
  } finally {
    motion.restore();
  }
});

/// Show less runs the other way: the history folds, and only after its fold
/// has played do the cards slide back under the changelog.
test("Show less folds the history first, then slides the cards back", async () => {
  wideWindow();
  const motion = stubMotion({ foldHeight: 300 });
  try {
    mockIPC(() => undefined);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderSettings(qc);
    await screen.findByRole("heading", { name: "Changelog" });
    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    await act(async () => motion.finishAll());
    fireEvent.click(await screen.findByRole("button", { name: "Show less" }));

    expect(history()).toBeNull();
    expect(screen.getByRole("button", { name: /^Show more/ })).toHaveAttribute("aria-expanded", "false");
    // A 300px fold plays for about 300ms: a third of the way in, the cards
    // have not moved yet.
    await act(() => new Promise((r) => window.setTimeout(r, 100)));
    expect(headings(columns().left)).toEqual(ALL_CARDS);

    await waitFor(() => expect(headings(columns().right)).toEqual(["Changelog", ...MOVERS]), { timeout: 2000 });
    expect(headings(columns().left)).toEqual(LOOKS);
    const back = motion.cards().slice(-3);
    expect(back.map((s) => s.el.getAttribute("data-settings-card"))).toEqual(["updates", "backup", "help"]);
  } finally {
    motion.restore();
  }
});

/// A second click while a sequence runs is ignored, so the cards and the
/// history cannot end up out of step.
test("a rapid double click leaves the cards and the history in step", async () => {
  wideWindow();
  const motion = stubMotion();
  try {
    mockIPC(() => undefined);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderSettings(qc);
    await screen.findByRole("heading", { name: "Changelog" });

    const more = screen.getByRole("button", { name: /^Show more/ });
    fireEvent.click(more);
    fireEvent.click(more);
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    await act(async () => motion.finishAll());
    await act(async () => motion.finishAll());

    const less = await screen.findByRole("button", { name: "Show less" });
    expect(history()).not.toBeNull();
    expect(headings(columns().left)).toEqual(ALL_CARDS);

    fireEvent.click(less);
    fireEvent.click(screen.getByRole("button", { name: /^Show more/ }));
    await waitFor(() => expect(headings(columns().right)).toEqual(["Changelog", ...MOVERS]));
    await act(async () => motion.finishAll());
    await act(async () => {});
    expect(screen.getByRole("button", { name: /^Show more/ })).toHaveAttribute("aria-expanded", "false");
    expect(history()).toBeNull();
    expect(headings(columns().left)).toEqual(LOOKS);
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

/// The first open after an update writes ~14 MB to disk - the button must
/// disable itself and say "Opening" while that call is in flight, so a
/// second click before it settles cannot open a second tab, then return to
/// normal once the command resolves.
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
