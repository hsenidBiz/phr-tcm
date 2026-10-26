import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import type { PbiHit } from "../bindings";
import PbiPicker from "./PbiPicker";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function Harness({ project = "Web", perProject = false }: { project?: string; perProject?: boolean }) {
  const [pbi, setPbi] = useState<PbiHit | null>(null);
  return (
    <PbiPicker org="acme" project={project} pbi={pbi} onChange={setPbi} perProject={perProject} />
  );
}

function renderPicker(props: { project?: string; perProject?: boolean } = {}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness {...props} />
    </QueryClientProvider>,
  );
}

const hit = (id: number, title: string, project: string): PbiHit => ({
  id,
  title,
  work_item_type: "Product Backlog Item",
  project,
});

describe("switched off", () => {
  beforeEach(() => localStorage.setItem("tcm-v2-all-projects", "0"));

  test("picking a PBI records it; recents appear on focus", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "search_pbis" && (args as { query: string }).query === "login")
        return [{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }];
    });
    renderPicker();

    const input = screen.getByLabelText("Find PBI");
    fireEvent.change(input, { target: { value: "login" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.click(await screen.findByText(/Login flow/));

    // Chip shown; clear it.
    expect(screen.getByText("Login flow")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Clear PBI"));

    // Empty input focus lists the recent pick.
    fireEvent.focus(screen.getByLabelText("Find PBI"));
    expect(await screen.findByText("Recently used")).toBeInTheDocument();
    expect(screen.getByText(/Login flow/)).toBeInTheDocument();
  });

  test("clicking outside dismisses the results; clicking a result still picks", async () => {
    mockIPC((cmd) => {
      if (cmd === "search_pbis")
        return [{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }];
    });
    render(<div data-testid="elsewhere">background</div>);
    renderPicker();

    const input = screen.getByLabelText("Find PBI");
    fireEvent.change(input, { target: { value: "login" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(await screen.findByText(/Login flow/)).toBeInTheDocument();

    // A press on empty space closes it - the reported bug was that it didn't.
    fireEvent.pointerDown(screen.getByTestId("elsewhere"));
    expect(screen.queryByText(/Login flow/)).not.toBeInTheDocument();

    // Re-open and confirm the dismissal never eats a real selection: the
    // press lands INSIDE, so the click behind it still picks.
    fireEvent.keyDown(input, { key: "Enter" });
    const hit = await screen.findByText(/Login flow/);
    fireEvent.pointerDown(hit);
    fireEvent.click(hit);
    expect(screen.getByLabelText("Clear PBI")).toBeInTheDocument();
  });

  test("Escape still closes the results", async () => {
    mockIPC((cmd) => {
      if (cmd === "search_pbis")
        return [{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }];
    });
    renderPicker();

    const input = screen.getByLabelText("Find PBI");
    fireEvent.change(input, { target: { value: "login" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(await screen.findByText(/Login flow/)).toBeInTheDocument();

    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByText(/Login flow/)).not.toBeInTheDocument();
  });

  /// A finished PBI otherwise squats in the recents until enough newer picks
  /// push it out - the X drops it on the spot, without picking it.
  test("a recent can be removed from the dropdown without picking it", async () => {
    mockIPC(() => undefined);
    localStorage.setItem(
      "tcm-v2-recent-pbis:acme/Web",
      JSON.stringify([
        { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" },
        { id: 43, title: "Checkout", work_item_type: "Product Backlog Item" },
      ]),
    );
    renderPicker();

    fireEvent.focus(screen.getByLabelText("Find PBI"));
    await screen.findByText("Recently used");

    fireEvent.click(screen.getByLabelText("Remove #42 from recent PBIs"));

    // Gone from the list AND from storage; the other survives; nothing was
    // picked (no chip, dropdown still open on the remaining recent).
    expect(screen.queryByText(/Login flow/)).not.toBeInTheDocument();
    expect(screen.getByText(/Checkout/)).toBeInTheDocument();
    expect(screen.queryByLabelText("Clear PBI")).not.toBeInTheDocument();
    expect(JSON.parse(localStorage.getItem("tcm-v2-recent-pbis:acme/Web")!)).toHaveLength(1);
  });

  /// The header search is the shared SearchInput: a 40px box with a magnifier
  /// and a clear button, and it fills the room it is given.
  test("Find PBI is a full-width, 40px search box with a clear button", async () => {
    mockIPC(() => undefined);
    renderPicker();

    const input = screen.getByLabelText("Find PBI");
    expect(input).toHaveClass("h-10", "text-sm", "w-full");
    expect(input.parentElement!.querySelector("svg")).toBeInTheDocument();
    // The picker itself takes the whole width its parent gives it.
    expect(input.closest("div")).toHaveClass("w-full");

    expect(screen.queryByRole("button", { name: "Clear search" })).not.toBeInTheDocument();
    fireEvent.change(input, { target: { value: "login" } });
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect((input as HTMLInputElement).value).toBe("");
    expect(input).toHaveFocus();
  });

  test("the results list is at least as wide as the box, and the picked chip is as tall", async () => {
    mockIPC((cmd) => {
      if (cmd === "search_pbis")
        return [{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }];
    });
    renderPicker();

    const input = screen.getByLabelText("Find PBI");
    fireEvent.change(input, { target: { value: "login" } });
    fireEvent.keyDown(input, { key: "Enter" });
    const hit = await screen.findByText(/Login flow/);
    const list = hit.closest("ul")!.parentElement!;
    expect(list).toHaveClass("w-full", "min-w-[28rem]");

    fireEvent.click(hit);
    expect(screen.getByLabelText("Clear PBI").parentElement).toHaveClass("h-10");
  });

  test("clearing the search with its x reopens the recents list", () => {
    localStorage.setItem(
      "tcm-v2-recent-pbis:acme/Web",
      JSON.stringify([{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }]),
    );
    mockIPC(() => []);
    renderPicker();

    // Typed text, with nothing open (the box was never focused).
    const input = screen.getByLabelText("Find PBI");
    fireEvent.change(input, { target: { value: "abc" } });
    expect(screen.queryByText("Recently used")).not.toBeInTheDocument();

    // The clear button empties the box and focuses it: the recents open, as
    // they do on a plain focus of an empty box.
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(input).toHaveValue("");
    expect(screen.getByText("Recently used")).toBeInTheDocument();
  });

  test("with no project picked the box is disabled and says why", () => {
    mockIPC(() => undefined);
    renderPicker({ project: "" });
    const input = screen.getByLabelText("Find PBI");
    expect(input).toBeDisabled();
    expect(input).toHaveAttribute("placeholder", "Pick a project first");
  });
});

describe("combined", () => {
  test("combined search asks every project and needs no project", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "search_pbis_all") return [hit(42, "Login flow", "HRM")];
    });
    renderPicker({ project: "" });

    const input = screen.getByLabelText("Find PBI");
    expect(input).toBeEnabled();
    expect(input).toHaveAttribute("placeholder", "Find PBI");
    fireEvent.change(input, { target: { value: "login" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(await screen.findByText(/Login flow/)).toBeInTheDocument();

    const all = calls.filter((c) => c.cmd === "search_pbis_all");
    expect(all).toHaveLength(1);
    expect(all[0].args).toEqual({ organization: "acme", query: "login" });
    expect(calls.some((c) => c.cmd === "search_pbis")).toBe(false);
  });

  test("hits from two projects name their project", async () => {
    mockIPC((cmd) => {
      if (cmd === "search_pbis_all")
        return [hit(42, "Login flow", "HRM"), hit(43, "Login report", "Payroll")];
    });
    renderPicker({ project: "" });

    const input = screen.getByLabelText("Find PBI");
    fireEvent.change(input, { target: { value: "login" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByText(/Login flow/);

    expect(screen.getByText("HRM")).toBeInTheDocument();
    expect(screen.getByText("Payroll")).toBeInTheDocument();
  });

  test("hits from one project show no project", async () => {
    mockIPC((cmd) => {
      if (cmd === "search_pbis_all")
        return [hit(42, "Login flow", "HRM"), hit(43, "Login report", "HRM")];
    });
    renderPicker({ project: "" });

    const input = screen.getByLabelText("Find PBI");
    fireEvent.change(input, { target: { value: "login" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByText(/Login flow/);

    expect(screen.queryByText("HRM")).not.toBeInTheDocument();
  });

  test("a per-project picker searches its project even while combined", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "search_pbis") return [hit(42, "Login flow", "Web")];
    });
    renderPicker({ project: "Web", perProject: true });

    const input = screen.getByLabelText("Find PBI");
    fireEvent.change(input, { target: { value: "login" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByText(/Login flow/);

    const one = calls.filter((c) => c.cmd === "search_pbis");
    expect(one).toHaveLength(1);
    expect(one[0].args).toMatchObject({ organization: "acme", project: "Web", query: "login" });
    expect(calls.some((c) => c.cmd === "search_pbis_all")).toBe(false);
  });

  test("combined recents are kept under the org-wide key", async () => {
    const oldList = JSON.stringify([hit(7, "Older", "Web")]);
    localStorage.setItem("tcm-v2-recent-pbis:acme/Web", oldList);
    mockIPC((cmd) => {
      if (cmd === "search_pbis_all") return [hit(42, "Login flow", "HRM")];
    });
    renderPicker({ project: "Web" });

    const input = screen.getByLabelText("Find PBI");
    fireEvent.change(input, { target: { value: "login" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.click(await screen.findByText(/Login flow/));

    const combined = JSON.parse(localStorage.getItem("tcm-v2-recent-pbis:acme/*")!) as PbiHit[];
    expect(combined.map((p) => p.id)).toEqual([42]);
    expect(combined[0].project).toBe("HRM");
    expect(localStorage.getItem("tcm-v2-recent-pbis:acme/Web")).toBe(oldList);
  });

  test("combined recents name their project when they span two", async () => {
    localStorage.setItem(
      "tcm-v2-recent-pbis:acme/*",
      JSON.stringify([hit(42, "Login flow", "HRM"), hit(43, "Payslips", "Payroll")]),
    );
    mockIPC(() => undefined);
    renderPicker({ project: "" });

    fireEvent.focus(screen.getByLabelText("Find PBI"));
    await screen.findByText("Recently used");
    expect(screen.getByText("HRM")).toBeInTheDocument();
    expect(screen.getByText("Payroll")).toBeInTheDocument();
  });
});
