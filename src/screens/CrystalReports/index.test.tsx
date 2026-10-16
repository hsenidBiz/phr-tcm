// The Crystal Reports screen: the PDF preview of the report an assistant is
// working on, and the button that hands the file to Crystal Reports.
//
// It reads three commands and one event, so the tests mock exactly those.
// jsdom cannot draw a PDF, so what is asserted is the iframe - its title and
// that it points at a blob: URL made from the PDF - never its contents.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import CrystalReports from "./index";

const PATH = "C:\\Reports\\Goals\\PMSV10_GoalAlignCascade.rpt";
const PDF = btoa("%PDF-1.4 a pretend report");

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

let made: string[];
let revoked: string[];

beforeEach(() => {
  made = [];
  revoked = [];
  URL.createObjectURL = vi.fn(() => {
    const url = `blob:mock-${made.length + 1}`;
    made.push(url);
    return url;
  });
  URL.revokeObjectURL = vi.fn((url: string) => {
    revoked.push(url);
  });
});

afterEach(() => {
  clearMocks();
  vi.restoreAllMocks();
});

test("the_screen_says_when_there_is_nothing_to_show", async () => {
  mockCrystal({ current: null, pdf: null });
  render(<CrystalReports />);

  expect(await screen.findByText("No report open. Ask your AI assistant to open one.")).toBeInTheDocument();
  expect(screen.queryByTitle("Report preview")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Open in Crystal Reports" })).not.toBeInTheDocument();
});

test("it_shows_the_open_report_and_refreshes_on_change", async () => {
  const answers: Answers = { current: current(), pdf: PDF };
  const calls = mockCrystal(answers);
  render(<CrystalReports />);

  const name = await screen.findByText("PMSV10_GoalAlignCascade.rpt");
  expect(name).toBeInTheDocument();
  // The folder is the rest of the path, and the full path is on hover.
  expect(screen.getByText("C:\\Reports\\Goals")).toBeInTheDocument();
  expect(screen.getByTitle(PATH)).toBeInTheDocument();
  expect(screen.getByText("Saved data")).toBeInTheDocument();
  expect(screen.getByText("3 pages")).toBeInTheDocument();
  expect(screen.getByText("A chart on page 2 is not drawn.")).toBeInTheDocument();
  const frame = await screen.findByTitle("Report preview");
  expect(frame.tagName).toBe("IFRAME");
  expect(frame.getAttribute("src")).toMatch(/^blob:/);
  expect(calls.filter((c) => c === "crystal_current")).toHaveLength(1);
  expect(calls.filter((c) => c === "crystal_preview_pdf")).toHaveLength(1);

  // The assistant changes the report: the screen reads both again and draws the new PDF.
  answers.current = current({ page_count: 1, diagnostics: [] });
  await act(async () => {
    await emit("crystal-report-changed", { path: PATH });
  });
  expect(await screen.findByText("1 page")).toBeInTheDocument();
  expect(screen.queryByText("A chart on page 2 is not drawn.")).not.toBeInTheDocument();
  expect(calls.filter((c) => c === "crystal_current")).toHaveLength(2);
  expect(calls.filter((c) => c === "crystal_preview_pdf")).toHaveLength(2);
  await waitFor(() => expect(screen.getByTitle("Report preview").getAttribute("src")).toBe(made[1]));
  // The first PDF's URL is let go as the second takes its place.
  expect(revoked).toEqual([made[0]]);
});

test("the_blob_is_a_pdf_and_is_let_go_when_the_screen_goes", async () => {
  const blobs: Blob[] = [];
  URL.createObjectURL = vi.fn((b: Blob | MediaSource) => {
    blobs.push(b as Blob);
    return "blob:only";
  });
  mockCrystal({ current: current(), pdf: PDF });
  const { unmount } = render(<CrystalReports />);
  await screen.findByTitle("Report preview");

  expect(blobs).toHaveLength(1);
  expect(blobs[0].type).toBe("application/pdf");
  expect(blobs[0].size).toBe("%PDF-1.4 a pretend report".length);
  unmount();
  expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:only");
});

test("live_previews_say_where_the_data_came_from", async () => {
  mockCrystal({ current: current({ live: "Reports dev" }), pdf: PDF });
  render(<CrystalReports />);

  expect(await screen.findByText("Live data from Reports dev")).toBeInTheDocument();
  expect(screen.queryByText("Saved data")).not.toBeInTheDocument();
});

test("a_preview_that_could_not_be_drawn_shows_why_and_no_frame", async () => {
  mockCrystal({
    current: current({ page_count: 0, diagnostics: ["The preview of Goals.rpt could not be drawn: no pages."] }),
    pdf: "",
  });
  render(<CrystalReports />);

  expect(await screen.findByText("The preview could not be drawn.")).toBeInTheDocument();
  expect(screen.getByText("The preview of Goals.rpt could not be drawn: no pages.")).toBeInTheDocument();
  expect(screen.queryByTitle("Report preview")).not.toBeInTheDocument();
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});

test("a_report_whose_file_is_gone_says_so_instead_of_showing_a_preview", async () => {
  mockCrystal({ current: current({}, true), pdf: PDF });
  render(<CrystalReports />);

  expect(await screen.findByText("The report file is no longer there.")).toBeInTheDocument();
  expect(screen.getByText("PMSV10_GoalAlignCascade.rpt")).toBeInTheDocument();
  expect(screen.queryByTitle("Report preview")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Open in Crystal Reports" })).not.toBeInTheDocument();
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
  expect(screen.getByTitle("Report preview")).toBeInTheDocument();
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
  expect(screen.queryByTitle("Report preview")).not.toBeInTheDocument();
});
