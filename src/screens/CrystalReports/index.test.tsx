// The Crystal Reports screen: the pages of the report an assistant is
// working on, its notes in plain words, and the button that hands the file
// to Crystal Reports.
//
// It reads three commands and one event, so the tests mock exactly those.
// jsdom cannot draw a PDF, so pdf.js itself is mocked: what is asserted is
// what the screen asks of it (the bytes, one canvas per page at the zoom
// shown, cancelling and destroying what it no longer needs), never pixels.
// IntersectionObserver and ResizeObserver are stand-ins too: by default
// every page reports itself in view as soon as it is watched, and a test
// that needs to scroll says which pages are near.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import CrystalReports from "./index";

/** A stand-in for pdf.js: every document it opens, and every page it draws. */
const pdfjs = vi.hoisted(() => {
  type Viewport = { width: number; height: number; scale: number };
  type Render = {
    canvas: HTMLCanvasElement;
    /** The page index the canvas shows (its data-page). */
    page: number;
    viewport: Viewport;
    cancel: ReturnType<typeof vi.fn>;
    /** Fails a drawing that hangs. */
    fail: (e: unknown) => void;
  };
  type Task = {
    opts: { data: Uint8Array; useWasm?: boolean };
    destroy: ReturnType<typeof vi.fn>;
    open: (pages: number) => void;
    fail: (e: unknown) => void;
  };
  const state = {
    /** Pages in the next document opened; null leaves it loading until the test opens it. */
    pages: 3 as number | null,
    /** When set, drawing a page fails with this. */
    renderError: null as unknown,
    /** When true, drawing a page never finishes (unless the test fails it). */
    renderHangs: false,
    /** Every page's size in points. */
    sheet: { w: 600, h: 800 },
    tasks: [] as Task[],
    renders: [] as Render[],
    reset() {
      state.pages = 3;
      state.renderError = null;
      state.renderHangs = false;
      state.sheet = { w: 600, h: 800 };
      state.tasks = [];
      state.renders = [];
    },
  };
  const page = () => ({
    getViewport: ({ scale }: { scale: number }) => ({
      width: state.sheet.w * scale,
      height: state.sheet.h * scale,
      scale,
    }),
    render: ({ canvas, viewport }: { canvas: HTMLCanvasElement; viewport: Viewport }) => {
      let fail: (e: unknown) => void = () => {};
      const promise = state.renderHangs
        ? new Promise<void>((_, rej) => {
            fail = rej;
          })
        : state.renderError
          ? Promise.reject(state.renderError)
          : Promise.resolve();
      const r: Render = { canvas, page: Number(canvas.dataset.page), viewport, cancel: vi.fn(), fail };
      state.renders.push(r);
      return { promise, cancel: r.cancel };
    },
  });
  const getDocument = vi.fn((opts: Task["opts"]) => {
    let open!: (d: unknown) => void;
    let fail!: (e: unknown) => void;
    const promise = new Promise((res, rej) => {
      open = res;
      fail = rej;
    });
    const task: Task = {
      opts,
      destroy: vi.fn(() => Promise.resolve()),
      open: (pages) => open({ numPages: pages, getPage: async () => page(), destroy: vi.fn() }),
      fail,
    };
    state.tasks.push(task);
    if (state.pages !== null) task.open(state.pages);
    return { promise, destroy: task.destroy };
  });
  return { state, getDocument, GlobalWorkerOptions: { workerSrc: "" } };
});

vi.mock("pdfjs-dist", () => ({ getDocument: pdfjs.getDocument, GlobalWorkerOptions: pdfjs.GlobalWorkerOptions }));

/** IntersectionObserver: which pages are near the view. `auto` puts every watched page in view. */
const nearby = { auto: true, observers: [] as MockIntersection[] };
class MockIntersection {
  readonly watched = new Set<Element>();
  constructor(
    readonly callback: IntersectionObserverCallback,
    readonly options?: IntersectionObserverInit,
  ) {
    nearby.observers.push(this);
  }
  observe(el: Element) {
    this.watched.add(el);
    if (nearby.auto) this.report([el], true);
  }
  unobserve(el: Element) {
    this.watched.delete(el);
  }
  disconnect() {
    this.watched.clear();
  }
  takeRecords() {
    return [];
  }
  report(els: Element[], isIntersecting: boolean) {
    const entries = els.map((target) => ({ target, isIntersecting }) as IntersectionObserverEntry);
    this.callback(entries, this as unknown as IntersectionObserver);
  }
}
/** Brings pages (0-based) near the view, or takes them away. */
function scrollPages(indices: number[], near: boolean) {
  const io = nearby.observers[nearby.observers.length - 1];
  const els = [...io.watched].filter((el) => indices.includes(Number((el as HTMLElement).dataset.page)));
  act(() => io.report(els, near));
}

/** ResizeObserver: the test says when the page area changed size. */
const resizes = { callbacks: [] as ResizeObserverCallback[] };
class MockResize {
  constructor(callback: ResizeObserverCallback) {
    resizes.callbacks.push(callback);
  }
  observe() {}
  unobserve() {}
  disconnect() {}
}

const PATH = "C:\\Reports\\Goals\\PMSV10_GoalAlignCascade.rpt";
const PDF_TEXT = "%PDF-1.4 a pretend report";
const PDF = btoa(PDF_TEXT);

function current(over: Record<string, unknown> = {}, missing = false) {
  return {
    preview: { path: PATH, page_count: 3, diagnostics: ["A chart on page 2 is not drawn."], live: null, ...over },
    missing,
  };
}

type Answers = { current?: unknown; pdf?: unknown; open?: () => void };

/** Mocks the three commands; `answers` is read on every call, so a test can change what they say. */
function mockCrystal(answers: Answers) {
  const calls: string[] = [];
  mockIPC(
    (cmd) => {
      calls.push(cmd);
      if (cmd === "crystal_current") return answers.current ?? null;
      if (cmd === "crystal_preview_pdf") return answers.pdf ?? null;
      if (cmd === "crystal_open_in_designer") return answers.open?.() ?? null;
      return undefined;
    },
    { shouldMockEvents: true },
  );
  return calls;
}

const pages = () => screen.queryAllByRole("img", { name: /^Page \d+ of \d+$/ });
const header = () => screen.getByRole("region", { name: "Report" }).querySelector("header") as HTMLElement;
/** The zoom shown in the controls ("Zoom" is there for screen readers only). */
const zoomLabel = () =>
  within(screen.getByRole("group", { name: "Page view" })).getByText(
    (_, el) => el?.tagName === "SPAN" && /^Zoom \d+%$/.test(el.textContent ?? ""),
  );
const sheet = (i: number) => document.querySelector(`canvas[data-page="${i}"]`) as HTMLCanvasElement;
/** Visible to a person: nothing on the way up is `hidden`. */
function isVisible(el: Element) {
  for (let e: Element | null = el; e; e = e.parentElement) if ((e as HTMLElement).hidden) return false;
  return true;
}

beforeEach(() => {
  pdfjs.state.reset();
  pdfjs.getDocument.mockClear();
  nearby.auto = true;
  nearby.observers = [];
  resizes.callbacks = [];
  vi.stubGlobal("IntersectionObserver", MockIntersection);
  vi.stubGlobal("ResizeObserver", MockResize);
});

afterEach(() => {
  clearMocks();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("the_screen_says_when_there_is_nothing_to_show", async () => {
  mockCrystal({ current: null, pdf: null });
  render(<CrystalReports />);

  expect(await screen.findByText("No report open. Ask your AI assistant to open one.")).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Pages" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Open in Crystal Reports" })).not.toBeInTheDocument();
});

test("it_shows_the_open_report_and_refreshes_on_change", async () => {
  const answers: Answers = { current: current(), pdf: PDF };
  const calls = mockCrystal(answers);
  render(<CrystalReports />);

  expect(await screen.findByText("PMSV10_GoalAlignCascade.rpt")).toBeInTheDocument();
  // The header card: the folder is the rest of the path, the full path is on hover.
  const head = header();
  expect(within(head).getByText("C:\\Reports\\Goals")).toBeInTheDocument();
  expect(within(head).getByTitle(PATH)).toBeInTheDocument();
  expect(within(head).getByText("Saved data")).toBeInTheDocument();
  expect(within(head).getByText("3 pages")).toBeInTheDocument();
  // One canvas per page.
  await waitFor(() => expect(pages()).toHaveLength(3));
  expect(pages().every((p) => p.tagName === "CANVAS")).toBe(true);
  expect(calls.filter((c) => c === "crystal_current")).toHaveLength(1);
  expect(calls.filter((c) => c === "crystal_preview_pdf")).toHaveLength(1);

  // The assistant changes the report: the screen reads both again and draws the new PDF.
  answers.current = current({ page_count: 1, diagnostics: [] });
  pdfjs.state.pages = 1;
  await act(async () => {
    await emit("crystal-report-changed", { path: PATH });
  });
  expect(await within(header()).findByText("1 page")).toBeInTheDocument();
  await waitFor(() => expect(pages()).toHaveLength(1));
  expect(screen.getByRole("img", { name: "Page 1 of 1" })).toBeInTheDocument();
  expect(calls.filter((c) => c === "crystal_current")).toHaveLength(2);
  expect(calls.filter((c) => c === "crystal_preview_pdf")).toHaveLength(2);
  // The first document is let go as the second takes its place.
  expect(pdfjs.state.tasks).toHaveLength(2);
  expect(pdfjs.state.tasks[0].destroy).toHaveBeenCalled();
  expect(pdfjs.state.tasks[1].destroy).not.toHaveBeenCalled();
});

test("the_pdf_bytes_go_to_pdfjs_and_its_document_is_let_go_when_the_screen_goes", async () => {
  mockCrystal({ current: current(), pdf: PDF });
  const { unmount } = render(<CrystalReports />);
  await waitFor(() => expect(pages()).toHaveLength(3));

  expect(pdfjs.getDocument).toHaveBeenCalledTimes(1);
  const { opts } = pdfjs.state.tasks[0];
  expect(new TextDecoder().decode(opts.data)).toBe(PDF_TEXT);
  // pdf.js compiles nothing from the PDF (no WebAssembly under this CSP).
  expect(opts.useWasm).toBe(false);
  // The worker is the app's own file, never a CDN or a blob.
  expect(pdfjs.GlobalWorkerOptions.workerSrc).toMatch(/pdf\.worker\.min\.mjs/);
  expect(pdfjs.GlobalWorkerOptions.workerSrc).not.toMatch(/^(https?:|blob:)/);
  unmount();
  expect(pdfjs.state.tasks[0].destroy).toHaveBeenCalled();
});

test("the_page_view_fits_the_width_by_default_and_zooms", async () => {
  // The page area is 448px wide; less its 24px gutters that is 400px, and
  // a 600pt page is 800px at 100%, so Fit width is 50%.
  vi.spyOn(Element.prototype, "clientWidth", "get").mockReturnValue(448);
  mockCrystal({ current: current({ page_count: 1 }), pdf: PDF });
  pdfjs.state.pages = 1;
  render(<CrystalReports />);

  await waitFor(() => expect(zoomLabel()).toHaveTextContent("50%"));
  const fit = screen.getByRole("button", { name: "Fit width" });
  expect(fit).toHaveAttribute("aria-pressed", "true");
  const controls = screen.getByRole("group", { name: "Page view" });
  expect(within(controls).getByText("1 page")).toBeInTheDocument();
  // The zoom is plain text, not a live region announced on every re-fit.
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  await waitFor(() => expect(sheet(0).width).toBe(400));
  expect(sheet(0).style.width).toBe("400px");

  fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
  expect(zoomLabel()).toHaveTextContent("67%");
  expect(fit).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(screen.getByRole("button", { name: "Zoom out" }));
  fireEvent.click(screen.getByRole("button", { name: "Zoom out" }));
  expect(zoomLabel()).toHaveTextContent("33%");

  fireEvent.click(screen.getByRole("button", { name: "100%" }));
  expect(zoomLabel()).toHaveTextContent("100%");
  expect(screen.getByRole("button", { name: "100%" })).toHaveAttribute("aria-pressed", "true");
  await waitFor(() => expect(sheet(0).width).toBe(800));

  fireEvent.click(fit);
  expect(zoomLabel()).toHaveTextContent("50%");
  expect(fit).toHaveAttribute("aria-pressed", "true");
  // Each zoom drew the page again.
  expect(pdfjs.state.renders.length).toBeGreaterThan(1);
});

test("drawing_still_under_way_is_cancelled_by_a_new_zoom_and_when_the_screen_goes", async () => {
  pdfjs.state.renderHangs = true;
  mockCrystal({ current: current(), pdf: PDF });
  const { unmount } = render(<CrystalReports />);
  await waitFor(() => expect(pdfjs.state.renders).toHaveLength(1));
  const first = pdfjs.state.renders[0];
  expect(first.cancel).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
  expect(first.cancel).toHaveBeenCalled();
  await waitFor(() => expect(pdfjs.state.renders).toHaveLength(2));
  const second = pdfjs.state.renders[1];
  unmount();
  expect(second.cancel).toHaveBeenCalled();
  expect(pdfjs.state.tasks[0].destroy).toHaveBeenCalled();
});

test("only_pages_near_the_view_are_drawn_and_a_page_that_leaves_is_let_go", async () => {
  nearby.auto = false;
  mockCrystal({ current: current({ diagnostics: [] }), pdf: PDF });
  render(<CrystalReports />);
  // Every page has its sheet, sized for its place in the scroll, drawn or not.
  await waitFor(() => expect(pages()).toHaveLength(3));
  for (const i of [0, 1, 2]) expect(sheet(i).style.height).toBe(`${Math.round(800 * (96 / 72))}px`);
  // The watcher looks about one screen beyond the view, inside the page area.
  // It is made in an effect after the sheets, so under load it can lag them.
  await waitFor(() => expect(nearby.observers.length).toBeGreaterThan(0));
  const io = nearby.observers[nearby.observers.length - 1];
  expect(io.options?.rootMargin).toBe("100% 0px");
  expect(io.options?.root).toBe(sheet(0).closest("[data-page-area]"));
  expect(pdfjs.state.renders).toHaveLength(0);
  // A page that was never near holds no pixels.
  expect(sheet(2).width).toBe(0);

  scrollPages([0, 1], true);
  await waitFor(() => expect(pdfjs.state.renders.map((r) => r.page)).toEqual([0, 1]));
  expect(sheet(0).width).toBeGreaterThan(0);
  expect(sheet(2).width).toBe(0);

  // Page 1 scrolls far away: its canvas is emptied, page 2 comes near and is drawn.
  scrollPages([0], false);
  scrollPages([2], true);
  await waitFor(() => expect(pdfjs.state.renders.map((r) => r.page)).toEqual([0, 1, 2]));
  expect(sheet(0).width).toBe(0);
  expect(sheet(0).height).toBe(0);
  expect(sheet(1).width).toBeGreaterThan(0);

  // A new zoom draws again only what is near.
  fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
  await waitFor(() => expect(pdfjs.state.renders).toHaveLength(5));
  expect(pdfjs.state.renders.slice(3).map((r) => r.page).sort()).toEqual([1, 2]);
});

test("a_canvas_never_holds_more_than_2_to_the_25_pixels", async () => {
  // A 2000 x 3000 pt sheet at 400% is 10667 x 16000 CSS px - about 170
  // million pixels. The canvas is capped and CSS stretches it to size.
  pdfjs.state.sheet = { w: 2000, h: 3000 };
  pdfjs.state.pages = 1;
  mockCrystal({ current: current({ page_count: 1, diagnostics: [] }), pdf: PDF });
  render(<CrystalReports />);
  await waitFor(() => expect(pdfjs.state.renders).toHaveLength(1));

  for (let i = 0; i < 8; i++) fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
  expect(zoomLabel()).toHaveTextContent("400%");
  expect(screen.getByRole("button", { name: "Zoom in" })).toBeDisabled();
  const canvas = sheet(0);
  await waitFor(() => expect(canvas.width * canvas.height).toBeGreaterThan(0.99 * 2 ** 25));
  expect(canvas.style.width).toBe("10667px");
  expect(canvas.style.height).toBe("16000px");
  expect(canvas.width * canvas.height).toBeLessThanOrEqual(2 ** 25);
  expect(canvas.width * canvas.height).toBeGreaterThan(0.99 * 2 ** 25);
  const last = pdfjs.state.renders[pdfjs.state.renders.length - 1];
  expect(Math.floor(last.viewport.width)).toBe(canvas.width);
});

test("fit_width_follows_the_page_area_as_it_is_resized", async () => {
  const width = vi.spyOn(Element.prototype, "clientWidth", "get").mockReturnValue(448);
  pdfjs.state.pages = 1;
  mockCrystal({ current: current({ page_count: 1, diagnostics: [] }), pdf: PDF });
  render(<CrystalReports />);
  await waitFor(() => expect(zoomLabel()).toHaveTextContent("50%"));

  // The window grows: 848 - 48 = 800px, the page's width at 100%.
  width.mockReturnValue(848);
  act(() => {
    for (const cb of resizes.callbacks) cb([], {} as ResizeObserver);
  });
  expect(zoomLabel()).toHaveTextContent("100%");
  await waitFor(() => expect(sheet(0).width).toBe(800));

  // A zoom of the user's own is kept through a resize.
  fireEvent.click(screen.getByRole("button", { name: "Zoom out" }));
  width.mockReturnValue(448);
  act(() => {
    for (const cb of resizes.callbacks) cb([], {} as ResizeObserver);
  });
  expect(zoomLabel()).toHaveTextContent("90%");
});

test("an_old_document_failing_after_a_newer_pdf_arrived_is_not_blamed_on_it", async () => {
  const answers: Answers = { current: current({ diagnostics: [] }), pdf: PDF };
  mockCrystal(answers);
  pdfjs.state.renderHangs = true;
  render(<CrystalReports />);
  await waitFor(() => expect(pdfjs.state.renders).toHaveLength(1));
  const old = pdfjs.state.renders[0];

  // A newer PDF arrives and is still opening when the old drawing fails
  // (pdf.js rejects it once its document is destroyed).
  answers.current = current({ page_count: 1, diagnostics: [] });
  pdfjs.state.pages = null;
  await act(async () => {
    await emit("crystal-report-changed", { path: PATH });
  });
  await waitFor(() => expect(pdfjs.state.tasks).toHaveLength(2));
  await act(async () => {
    old.fail(new Error("Transport destroyed"));
  });
  expect(screen.queryByText("The preview could not be drawn.")).not.toBeInTheDocument();
  expect(screen.queryByText("Transport destroyed")).not.toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Pages" })).toBeInTheDocument();

  // The newer one opens and draws.
  pdfjs.state.renderHangs = false;
  await act(async () => {
    pdfjs.state.tasks[1].open(1);
  });
  await waitFor(() => expect(pages()).toHaveLength(1));
  expect(screen.queryByText("The preview could not be drawn.")).not.toBeInTheDocument();
});

test("a_document_that_arrives_after_a_newer_one_is_never_drawn", async () => {
  const answers: Answers = { current: current(), pdf: PDF };
  mockCrystal(answers);
  pdfjs.state.pages = null; // the first document is slow to open
  render(<CrystalReports />);
  await waitFor(() => expect(pdfjs.state.tasks).toHaveLength(1));

  answers.current = current({ page_count: 1, diagnostics: [] });
  pdfjs.state.pages = 1;
  await act(async () => {
    await emit("crystal-report-changed", { path: PATH });
  });
  await waitFor(() => expect(pages()).toHaveLength(1));
  expect(pdfjs.state.tasks[0].destroy).toHaveBeenCalled();

  // The old one opens at last: it is ignored.
  await act(async () => {
    pdfjs.state.tasks[0].open(3);
  });
  expect(pages()).toHaveLength(1);
  expect(screen.getByRole("img", { name: "Page 1 of 1" })).toBeInTheDocument();
});

test("a_preview_that_could_not_be_drawn_shows_why_and_no_pages", async () => {
  mockCrystal({
    current: current({ page_count: 0, diagnostics: ["The preview of Goals.rpt could not be drawn: no pages."] }),
    pdf: "",
  });
  render(<CrystalReports />);

  expect(await screen.findByText("The preview could not be drawn.")).toBeInTheDocument();
  // With nothing drawn the notes are the explanation, so they start open.
  expect(screen.getByRole("button", { name: /Other notes/ })).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("The preview of Goals.rpt could not be drawn: no pages.")).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Pages" })).not.toBeInTheDocument();
  expect(pdfjs.getDocument).not.toHaveBeenCalled();
});

test("a_pdf_that_pdfjs_cannot_draw_says_so_with_the_reason_in_the_notes", async () => {
  pdfjs.state.renderError = new Error("Invalid PDF structure.");
  mockCrystal({ current: current({ diagnostics: [] }), pdf: PDF });
  render(<CrystalReports />);

  expect(await screen.findByText("The preview could not be drawn.")).toBeInTheDocument();
  expect(screen.getByText("Invalid PDF structure.")).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Pages" })).not.toBeInTheDocument();
  await waitFor(() => expect(pdfjs.state.tasks[0].destroy).toHaveBeenCalled());
});

test("a_pdf_that_will_not_open_says_so_too", async () => {
  pdfjs.state.pages = null;
  mockCrystal({ current: current({ diagnostics: [] }), pdf: PDF });
  render(<CrystalReports />);
  await waitFor(() => expect(pdfjs.state.tasks).toHaveLength(1));

  await act(async () => {
    pdfjs.state.tasks[0].fail(new Error("The PDF is damaged."));
  });
  expect(await screen.findByText("The preview could not be drawn.")).toBeInTheDocument();
  expect(screen.getByText("The PDF is damaged.")).toBeInTheDocument();
});

test("missing_fields_read_as_one_friendly_card_without_the_raw_errors", async () => {
  mockCrystal({
    current: current({
      diagnostics: [
        "formula error: unknown name: {Command.emp_display_number} (Field12) [Details, bytes 0..32]",
        "formula error: unknown name: {Command.goal_name} (Field13) [Details, bytes 0..20]",
        "formula error: unknown name: {Command.emp_display_number} (Field14) [Group Header #1, bytes 0..32]",
        "font 'Arial' lacks glyph(s) '✓✗' (U+2713 U+2717); rendered from a fallback face",
        "A chart on page 2 is not drawn.",
        "formula error: divide by zero (Formula @Ratio)",
      ],
    }),
    pdf: PDF,
  });
  render(<CrystalReports />);

  const notes = await screen.findByRole("region", { name: "Preview notes" });
  expect(within(notes).getByText("2 fields have no data in this preview.")).toBeInTheDocument();
  expect(
    within(notes).getByText(
      "This report holds no saved rows. Ask your AI assistant for a live preview on the Dev login to fill them.",
    ),
  ).toBeInTheDocument();
  expect(within(notes).getByText("2 symbols drawn with a fallback font (✓ ✗)")).toBeInTheDocument();
  // Nothing raw shows until asked for: the lists are there (so the toggles'
  // aria-controls point at them) but hidden.
  const visibleText = (re: RegExp) => within(notes).queryAllByText(re).filter((el) => isVisible(el));
  expect(visibleText(/formula error/)).toEqual([]);
  expect(within(notes).getByText("A chart on page 2 is not drawn.")).not.toBeVisible();
  expect(within(notes).getByText("Emp display number")).not.toBeVisible();

  const showFields = within(notes).getByRole("button", { name: "Show fields" });
  expect(showFields).toHaveAttribute("aria-expanded", "false");
  const fieldList = document.getElementById(showFields.getAttribute("aria-controls")!);
  expect(fieldList).not.toBeNull();
  expect(fieldList).not.toBeVisible();
  fireEvent.click(showFields);
  expect(within(notes).getByRole("button", { name: "Hide fields" })).toHaveAttribute("aria-expanded", "true");
  expect(fieldList).toBeVisible();
  const field = within(notes).getByText("Emp display number");
  expect(field).toBeVisible();
  expect(field.closest("li")).toHaveAttribute("title", "{Command.emp_display_number}");
  expect(within(notes).getByText("Goal name")).toBeVisible();
  expect(visibleText(/formula error/)).toEqual([]);

  const other = within(notes).getByRole("button", { name: /Other notes/ });
  expect(other).toHaveAttribute("aria-expanded", "false");
  expect(document.getElementById(other.getAttribute("aria-controls")!)).not.toBeVisible();
  fireEvent.click(other);
  expect(other).toHaveAttribute("aria-expanded", "true");
  expect(within(notes).getByText("A chart on page 2 is not drawn.")).toBeVisible();
});

test("one_missing_field_on_a_live_preview_is_worded_for_live_data", async () => {
  mockCrystal({
    current: current({
      live: "Reports dev",
      diagnostics: ["formula error: unknown name: {Command.emp_display_number} (Field12)"],
    }),
    pdf: PDF,
  });
  render(<CrystalReports />);

  const notes = await screen.findByRole("region", { name: "Preview notes" });
  expect(within(notes).getByText("1 field has no data in this preview.")).toBeInTheDocument();
  expect(within(notes).getByText("Your live preview's results did not include these fields.")).toBeInTheDocument();
  expect(within(notes).queryByRole("button", { name: /Other notes/ })).not.toBeInTheDocument();
});

test("a_preview_without_notes_shows_no_notes_card", async () => {
  mockCrystal({ current: current({ diagnostics: [] }), pdf: PDF });
  render(<CrystalReports />);

  await waitFor(() => expect(pages()).toHaveLength(3));
  expect(screen.queryByRole("region", { name: "Preview notes" })).not.toBeInTheDocument();
});

test("live_previews_say_where_the_data_came_from", async () => {
  mockCrystal({ current: current({ live: "Reports dev" }), pdf: PDF });
  render(<CrystalReports />);

  await screen.findByText("PMSV10_GoalAlignCascade.rpt");
  expect(within(header()).getByText("Live data from Reports dev")).toBeInTheDocument();
  expect(screen.queryByText("Saved data")).not.toBeInTheDocument();
});

test("a_report_whose_file_is_gone_says_so_instead_of_showing_a_preview", async () => {
  mockCrystal({ current: current({}, true), pdf: PDF });
  render(<CrystalReports />);

  expect(await screen.findByText("The report file is no longer there.")).toBeInTheDocument();
  expect(screen.getByText("PMSV10_GoalAlignCascade.rpt")).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Pages" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Open in Crystal Reports" })).not.toBeInTheDocument();
  expect(pdfjs.getDocument).not.toHaveBeenCalled();
});

test("open_in_crystal_asks_for_the_file_to_be_opened", async () => {
  const calls = mockCrystal({ current: current(), pdf: PDF });
  render(<CrystalReports />);

  fireEvent.click(await screen.findByRole("button", { name: "Open in Crystal Reports" }));
  await waitFor(() => expect(calls).toContain("crystal_open_in_designer"));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("open_in_crystal_shows_a_refusal", async () => {
  mockCrystal({
    current: current(),
    pdf: PDF,
    open: () => {
      throw "Windows has no program set to open .rpt files. Install Crystal Reports, or open the file from it.";
    },
  });
  render(<CrystalReports />);

  fireEvent.click(await screen.findByRole("button", { name: "Open in Crystal Reports" }));
  const refusal = await screen.findByRole("alert");
  expect(refusal).toHaveTextContent(
    "Windows has no program set to open .rpt files. Install Crystal Reports, or open the file from it.",
  );
  // The preview stays where it was.
  expect(screen.getByRole("region", { name: "Pages" })).toBeInTheDocument();
  await waitFor(() => expect(pages()).toHaveLength(3));
});

test("a_refusal_to_read_the_report_is_shown_in_a_sentence", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "crystal_current" || cmd === "crystal_preview_pdf") throw "Crystal Reports is switched off in Modules";
      return undefined;
    },
    { shouldMockEvents: true },
  );
  render(<CrystalReports />);

  expect(await screen.findByRole("alert")).toHaveTextContent("Crystal Reports is switched off in Modules");
  expect(screen.queryByRole("region", { name: "Pages" })).not.toBeInTheDocument();
});
