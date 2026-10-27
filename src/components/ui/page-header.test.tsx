import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { PageHeader } from "./page-header";

test("the header is one wrapping line with the heading first", () => {
  const { container } = render(<PageHeader title="12 test cases" />);
  const line = container.firstElementChild!.firstElementChild!;
  expect(line).toHaveClass("flex", "flex-wrap", "items-center", "gap-2");
  const heading = screen.getByRole("heading", { level: 2, name: "12 test cases" });
  expect(line.firstElementChild).toBe(heading);
});

test("titleAs picks the heading level", () => {
  render(<PageHeader title="Plans" titleAs="h3" />);
  expect(screen.getByRole("heading", { level: 3, name: "Plans" })).toBeInTheDocument();
});

test("no refresh or info button unless asked for", () => {
  render(<PageHeader title="T" />);
  expect(screen.queryByRole("button")).toBeNull();
});

test("refresh is a button named Refresh that calls onRefresh and spins while refreshing", () => {
  const onRefresh = vi.fn();
  const { rerender } = render(<PageHeader title="T" onRefresh={onRefresh} />);
  const btn = screen.getByRole("button", { name: "Refresh" });
  expect(btn).toHaveAttribute("title", "Refresh");
  expect(btn.querySelector("svg")).not.toHaveClass("animate-spin");
  fireEvent.click(btn);
  expect(onRefresh).toHaveBeenCalledTimes(1);
  rerender(<PageHeader title="T" onRefresh={onRefresh} refreshing />);
  expect(screen.getByRole("button", { name: "Refresh" }).querySelector("svg")).toHaveClass("animate-spin");
});

test("refreshLabel renames the refresh button", () => {
  render(<PageHeader title="T" onRefresh={() => {}} refreshLabel="Refresh plans" />);
  expect(screen.getByRole("button", { name: "Refresh plans" })).toBeInTheDocument();
});

test("the info icon is a focusable button whose name and tooltip carry the text", () => {
  render(<PageHeader title="T" info="Outcomes are recorded through the runner." />);
  const btn = screen.getByRole("button", { name: "Outcomes are recorded through the runner." });
  expect(btn).toHaveAttribute("title", "Outcomes are recorded through the runner.");
  expect(btn).not.toHaveAttribute("tabindex", "-1");
});

test("aside sits after the buttons, and actions are pushed to the right", () => {
  render(
    <PageHeader
      title="T"
      onRefresh={() => {}}
      aside={<span>Collecting test cases</span>}
      actions={<button>Export JSON</button>}
    />,
  );
  const aside = screen.getByText("Collecting test cases");
  const refresh = screen.getByRole("button", { name: "Refresh" });
  expect(refresh.compareDocumentPosition(aside) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  const actions = screen.getByRole("button", { name: "Export JSON" }).parentElement!;
  expect(actions).toHaveClass("ml-auto", "flex", "gap-2");
  expect(aside.compareDocumentPosition(actions) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

test("children render as the sub line below the header line", () => {
  render(
    <PageHeader title="T">
      <p>Plan "Sprint 4"</p>
    </PageHeader>,
  );
  const sub = screen.getByText('Plan "Sprint 4"');
  const heading = screen.getByRole("heading");
  expect(heading.compareDocumentPosition(sub) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(sub.closest(".flex-wrap")).toBeNull();
});
