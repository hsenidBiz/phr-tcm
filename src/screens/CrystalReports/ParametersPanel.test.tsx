// The parameters panel: a field per parameter, only filled values go to the
// preview, and a refused value is shown with nothing reloaded.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ParametersPanel } from "./ParametersPanel";

const base = {
  prompt_text: null,
  optional_prompt: false,
  allow_multiple_values: false,
  allow_null_value: false,
  choices: [],
  saved: [],
};
const CYCLE = {
  ...base,
  name: "@cycle",
  value_kind: "Number",
  prompt_text: "Cycle",
  choices: [{ value: "42", description: "FY24" }],
};
const FROM = { ...base, name: "@from", value_kind: "Date", optional_prompt: true };

function mock(list: unknown[], draw: (values: unknown) => void = () => {}) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "crystal_parameters") return list;
    if (cmd === "crystal_draw") return draw(args) ?? null;
    return undefined;
  });
  return calls;
}

afterEach(() => clearMocks());

test("it lists each parameter with its prompt and pick list", async () => {
  mock([CYCLE, FROM]);
  render(<ParametersPanel path="a.rpt" onPreviewed={() => {}} />);
  const cycle = await screen.findByRole("combobox", { name: "@cycle" });
  expect(screen.getByText("Cycle")).toBeInTheDocument();
  fireEvent.click(cycle);
  expect(screen.getByRole("option", { name: "42 - FY24" })).toBeInTheDocument();
  const from = screen.getByRole("textbox", { name: "@from" });
  expect(from).toHaveAttribute("placeholder", "YYYY-MM-DD");
  expect(screen.getByText("Optional")).toBeInTheDocument();
});

test("preview sends only filled values, then reloads", async () => {
  const calls = mock([CYCLE, FROM]);
  const onPreviewed = vi.fn();
  render(<ParametersPanel path="a.rpt" onPreviewed={onPreviewed} />);
  fireEvent.click(await screen.findByRole("combobox", { name: "@cycle" }));
  const box = screen.getByPlaceholderText("Search…");
  fireEvent.change(box, { target: { value: "42" } });
  fireEvent.keyDown(box, { key: "Enter" });
  fireEvent.click(screen.getByRole("button", { name: "Preview with these values" }));
  await waitFor(() => expect(onPreviewed).toHaveBeenCalledTimes(1));
  const draw = calls.find((c) => c.cmd === "crystal_draw");
  expect(draw?.args).toEqual({ values: [{ name: "@cycle", value: "42" }] });
});

test("a refused value is shown and nothing reloads", async () => {
  const onPreviewed = vi.fn();
  mock([CYCLE, FROM], () => {
    throw "A.rpt cannot be previewed with these values: parameter @cycle: `x` is not a number.";
  });
  render(<ParametersPanel path="a.rpt" onPreviewed={onPreviewed} />);
  fireEvent.change(await screen.findByRole("textbox", { name: "@from" }), { target: { value: "x" } });
  fireEvent.click(screen.getByRole("button", { name: "Preview with these values" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("`x` is not a number");
  expect(onPreviewed).not.toHaveBeenCalled();
});

test("a report without parameters shows no panel", async () => {
  const calls = mock([]);
  const { container } = render(<ParametersPanel path="a.rpt" onPreviewed={() => {}} />);
  await waitFor(() => expect(calls.some((c) => c.cmd === "crystal_parameters")).toBe(true));
  await waitFor(() => expect(container).toBeEmptyDOMElement());
});
